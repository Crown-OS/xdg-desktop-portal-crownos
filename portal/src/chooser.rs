//! Asks the user what to share — a screen, a window or a workspace — through
//! `crownos-portal-dialog`.

use std::path::PathBuf;
use std::process::Stdio;

use anyhow::Context;
use crownos_config::consent_dialog::{OptionShape, QuestionShape};
use tokio::process::Command;

use crate::cast::{Catalog, OutputInfo, Source, WindowInfo, WorkspaceInfo};
use crate::dbus::options::{SOURCE_MONITOR, SOURCE_WINDOW};
use crate::util::desktop_entry::display_name;

const DIALOG: &str = "crownos-portal-dialog";

/// The source the user picked among those `types` allows, or `None` when
/// they declined.
pub async fn choose_source(
    app_id: &str,
    catalog: &Catalog,
    types: u32,
) -> anyhow::Result<Option<Source>> {
    let sections = sections(catalog, types);
    anyhow::ensure!(
        sections.iter().any(|section| !section.choices.is_empty()),
        "there is nothing to share"
    );
    let app = display_name(app_id);
    let (title, message) = wording(app.as_deref());

    let mut dialog = Command::new(dialog_binary());
    dialog
        .args(["--icon", "screen", "--title", &title, "--message", &message])
        .args(["--cancel", "Cancel", "--confirm", "Share"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .kill_on_drop(true);
    for section in sections
        .iter()
        .filter(|section| !section.choices.is_empty())
    {
        dialog.args(["--section", section.title]);
        for choice in &section.choices {
            dialog
                .arg("--choice")
                .arg(format!("{}={}", encode(&choice.source), choice.label))
                .args(["--detail", &choice.detail]);
        }
    }

    let offered: Vec<&Source> = sections
        .iter()
        .flat_map(|section| &section.choices)
        .map(|choice| &choice.source)
        .collect();
    let status = dialog
        .status()
        .await
        .with_context(|| format!("running {DIALOG}"))?;
    let shape = QuestionShape {
        choices: offered.len() + 1,
        options: Vec::new(),
    };
    let picked = shape
        .decode(status.code())
        .and_then(|answer| offered.get(answer.choice).copied().cloned());
    Ok(picked.filter(|source| catalog.contains(source)))
}

struct Section {
    title: &'static str,
    choices: Vec<Labelled>,
}

/// A source with what the picker and the bar call it.
pub struct Labelled {
    pub source: Source,
    pub label: String,
    pub detail: String,
}

fn sections(catalog: &Catalog, types: u32) -> Vec<Section> {
    let monitors = types & SOURCE_MONITOR != 0;
    let windows = types & SOURCE_WINDOW != 0;
    let mut sections = Vec::with_capacity(3);
    if monitors {
        sections.push(Section {
            title: "Screens",
            choices: screens(catalog),
        });
    }
    if windows {
        sections.push(Section {
            title: "Windows",
            choices: catalog.windows.iter().map(window).collect(),
        });
    }
    if monitors {
        sections.push(Section {
            title: "Workspaces",
            choices: catalog
                .workspaces
                .iter()
                .map(|workspace| self::workspace(catalog, workspace))
                .collect(),
        });
    }
    sections
}

/// Everything that can be shared, labelled, for the bar's switcher.
pub fn everything(catalog: &Catalog) -> Vec<Labelled> {
    sections(catalog, SOURCE_MONITOR | SOURCE_WINDOW)
        .into_iter()
        .flat_map(|section| section.choices)
        .collect()
}

/// What to call a source the user already picked.
pub fn describe(source: &Source, catalog: &Catalog) -> String {
    everything(catalog)
        .into_iter()
        .find(|labelled| labelled.source == *source)
        .map_or_else(|| "Screen".to_owned(), |labelled| labelled.label)
}

fn screens(catalog: &Catalog) -> Vec<Labelled> {
    catalog
        .outputs
        .iter()
        .zip(labels(&catalog.outputs))
        .map(|(output, label)| Labelled {
            source: Source::Output(output.name.clone()),
            label,
            detail: format!("{} × {}", output.mode.0, output.mode.1),
        })
        .collect()
}

fn window(window: &WindowInfo) -> Labelled {
    let app = display_name(&window.app_id).unwrap_or_else(|| window.app_id.clone());
    let label = match window.title.trim() {
        "" => app.clone(),
        title => title.to_owned(),
    };
    Labelled {
        source: Source::Window(window.identifier.clone()),
        label,
        detail: app,
    }
}

fn workspace(catalog: &Catalog, workspace: &WorkspaceInfo) -> Labelled {
    let screen = workspace.output.as_deref().and_then(|name| {
        let index = catalog
            .outputs
            .iter()
            .position(|output| output.name == name)?;
        labels(&catalog.outputs).into_iter().nth(index)
    });
    let mut detail = screen.map_or_else(String::new, |screen| format!("On {screen}"));
    if workspace.active {
        detail.push_str(if detail.is_empty() {
            "Current"
        } else {
            " · current"
        });
    }
    Labelled {
        source: Source::Workspace(workspace.id.clone()),
        label: format!("Workspace {}", workspace.name),
        detail,
    }
}

pub fn encode(source: &Source) -> String {
    match source {
        Source::Output(name) => format!("output:{name}"),
        Source::Window(identifier) => format!("window:{identifier}"),
        Source::Workspace(id) => format!("workspace:{id}"),
    }
}

pub fn decode(text: &str) -> Option<Source> {
    let (kind, value) = text.split_once(':')?;
    let value = value.to_owned();
    match kind {
        "output" => Some(Source::Output(value)),
        "window" => Some(Source::Window(value)),
        "workspace" => Some(Source::Workspace(value)),
        _ => None,
    }
}

/// A yes/no question for [`ask`].
pub struct Question<'a> {
    pub dangerous: bool,
    pub title: &'a str,
    pub message: &'a str,
    pub grant: &'a str,
    pub deny: &'a str,
    /// Dialog flags for extra questions, such as `--checkbox`.
    pub option_arguments: &'a [String],
    /// The extra questions that `option_arguments` asks, in order.
    pub option_shapes: &'a [OptionShape],
}

/// What the user said, with the values of the extra questions when the
/// dialog printed one for each.
pub struct Answered {
    pub granted: bool,
    pub option_values: Option<Vec<String>>,
}

const GRANT: &str = "grant";

/// Whether the user said yes, from the dialog's exit status.
pub async fn ask(question: &Question<'_>) -> anyhow::Result<Answered> {
    let status = Command::new(dialog_binary())
        .args([
            "--icon",
            if question.dangerous {
                "warning"
            } else {
                "access"
            },
        ])
        .args(["--title", question.title, "--message", question.message])
        .args(["--cancel", question.deny])
        .arg("--choice")
        .arg(format!("{GRANT}={}", question.grant))
        .args(question.option_arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .kill_on_drop(true)
        .status()
        .await
        .with_context(|| format!("running {DIALOG}"))?;
    Ok(answered(question.option_shapes, status.code()))
}

/// The answer in an exit status: grant (choice 0) or the cancel button (choice 1), with the
/// option values as the `choices` result reads them, `true`/`false` for a checkbox and the item
/// index for a list. Escape gives no values.
fn answered(option_shapes: &[OptionShape], status: Option<i32>) -> Answered {
    let shape = QuestionShape {
        choices: 2,
        options: option_shapes.to_vec(),
    };
    let Some(answer) = shape.decode(status) else {
        return Answered {
            granted: false,
            option_values: None,
        };
    };
    let option_values = option_shapes
        .iter()
        .zip(answer.option_values)
        .map(|(option, value)| match option {
            OptionShape::Checkbox => (value == 1).to_string(),
            OptionShape::List { .. } => value.to_string(),
        })
        .collect();
    Answered {
        granted: answer.choice == 0,
        option_values: Some(option_values),
    }
}

fn wording(app: Option<&str>) -> (String, String) {
    let subject = app.unwrap_or("The app");
    let title = match app {
        Some(app) => format!("Choose What {app} Can See"),
        None => "Choose What to Share".to_owned(),
    };
    let message = format!(
        "{subject} will see everything in the screen, window or workspace you \
         pick until you stop sharing. You can switch or stop from the bar."
    );
    (title, message)
}

/// One name per output; the model names it unless two outputs share one.
fn labels(outputs: &[OutputInfo]) -> Vec<String> {
    if let [_] = outputs {
        return vec!["Entire Screen".to_owned()];
    }
    let name = |output: &OutputInfo| {
        let model = output.model.trim();
        let unique = !model.is_empty()
            && outputs
                .iter()
                .filter(|other| other.model.trim() == model)
                .count()
                == 1;
        if unique {
            model.to_owned()
        } else {
            output.name.clone()
        }
    };
    outputs.iter().map(name).collect()
}

/// Installed next to the portal; `PATH` for a development build.
fn dialog_binary() -> PathBuf {
    std::env::current_exe()
        .ok()
        .map(|portal| portal.with_file_name(DIALOG))
        .filter(|dialog| dialog.is_file())
        .unwrap_or_else(|| PathBuf::from(DIALOG))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(name: &str, model: &str) -> OutputInfo {
        OutputInfo {
            name: name.to_owned(),
            model: model.to_owned(),
            ..OutputInfo::default()
        }
    }

    #[test]
    fn choices_round_trip_through_the_dialog() {
        for source in [
            Source::Output("DP-1".into()),
            Source::Window("Ab9:z".into()),
            Source::Workspace("crownos-workspace-3".into()),
        ] {
            assert_eq!(decode(&encode(&source)), Some(source));
        }
        assert_eq!(decode("tablet:1"), None);
    }

    #[test]
    fn windows_are_only_offered_when_asked_for() {
        let catalog = Catalog {
            outputs: vec![output("DP-1", "Panel")],
            windows: vec![WindowInfo {
                identifier: "w".into(),
                title: "Notes".into(),
                app_id: "notes".into(),
            }],
            workspaces: Vec::new(),
        };
        let titles = |types| {
            sections(&catalog, types)
                .into_iter()
                .map(|section| section.title)
                .collect::<Vec<_>>()
        };
        assert_eq!(titles(SOURCE_MONITOR), ["Screens", "Workspaces"]);
        assert_eq!(titles(SOURCE_WINDOW), ["Windows"]);
        assert_eq!(describe(&Source::Window("w".into()), &catalog), "Notes");
    }

    #[test]
    fn the_answer_and_the_options_come_from_the_exit_status() {
        use crownos_config::consent_dialog::DialogAnswer;
        let shapes = [OptionShape::Checkbox, OptionShape::List { items: 3 }];
        let shape = QuestionShape {
            choices: 2,
            options: shapes.to_vec(),
        };
        let status = shape.encode(&DialogAnswer {
            choice: 0,
            option_values: vec![1, 2],
        });
        let granted = answered(&shapes, status.map(i32::from));
        assert!(granted.granted);
        assert_eq!(
            granted.option_values,
            Some(vec!["true".to_owned(), "2".to_owned()])
        );
        let cancel_button = shape.encode(&DialogAnswer {
            choice: 1,
            option_values: vec![1, 0],
        });
        let denied = answered(&shapes, cancel_button.map(i32::from));
        assert!(!denied.granted);
        assert_eq!(
            denied.option_values,
            Some(vec!["true".to_owned(), "0".to_owned()])
        );
        let escape = answered(&shapes, Some(1));
        assert!(!escape.granted);
        assert_eq!(escape.option_values, None);
    }
}
