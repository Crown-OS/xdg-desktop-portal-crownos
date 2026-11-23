//! Asks the user what to share — a screen, a window or a workspace — through
//! `crownos-portal-dialog`.

use std::path::PathBuf;
use std::process::Stdio;

use anyhow::Context;
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
        .stdout(Stdio::piped())
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

    let finished = dialog
        .output()
        .await
        .with_context(|| format!("running {DIALOG}"))?;
    if !finished.status.success() {
        return Ok(None);
    }
    let picked = last_line(&finished.stdout).and_then(decode);
    Ok(picked.filter(|source| catalog.contains(source)))
}

/// The dialog's answer, below anything its toolkit logged to stdout first.
fn last_line(stdout: &[u8]) -> Option<&str> {
    std::str::from_utf8(stdout)
        .ok()?
        .lines()
        .next_back()
        .map(str::trim)
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
    /// How many extra questions `option_arguments` asks.
    pub option_count: usize,
}

/// What the user said, with the values of the extra questions when the
/// dialog printed one for each.
pub struct Answered {
    pub granted: bool,
    pub option_values: Option<Vec<String>>,
}

const GRANT: &str = "grant";
const OPTION_PREFIX: &str = "option ";

/// Whether the user said yes.
pub async fn ask(question: &Question<'_>) -> anyhow::Result<Answered> {
    let finished = Command::new(dialog_binary())
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
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .with_context(|| format!("running {DIALOG}"))?;
    Ok(Answered {
        granted: finished.status.success() && last_line(&finished.stdout) == Some(GRANT),
        option_values: option_values(&finished.stdout, question.option_count),
    })
}

/// The last `count` option lines the dialog printed.
fn option_values(stdout: &[u8], count: usize) -> Option<Vec<String>> {
    let values: Vec<&str> = std::str::from_utf8(stdout)
        .ok()?
        .lines()
        .filter_map(|line| line.strip_prefix(OPTION_PREFIX))
        .collect();
    let first = values.len().checked_sub(count)?;
    Some(
        values[first..]
            .iter()
            .map(|value| value.trim().to_owned())
            .collect(),
    )
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
    fn the_answer_is_read_below_toolkit_logs() {
        let stdout = b"01:52:02.787Z  INFO window created\noutput:eDP-1\n";
        assert_eq!(
            last_line(stdout).and_then(decode),
            Some(Source::Output("eDP-1".into()))
        );
        assert_eq!(last_line(b""), None);
    }

    #[test]
    fn option_values_are_read_above_the_answer() {
        let stdout = b"INFO log\noption true\noption 1\ngrant\n";
        assert_eq!(
            option_values(stdout, 2),
            Some(vec!["true".to_owned(), "1".to_owned()])
        );
        assert_eq!(
            option_values(b"option false\n", 1),
            Some(vec!["false".to_owned()])
        );
        assert_eq!(option_values(b"grant\n", 0), Some(Vec::new()));
        assert_eq!(option_values(b"option true\ngrant\n", 2), None);
    }

    #[test]
    fn a_single_output_is_just_the_screen() {
        assert_eq!(labels(&[output("eDP-1", "0x1234")]), ["Entire Screen"]);
    }

    #[test]
    fn outputs_are_named_by_model_unless_it_is_shared() {
        let outputs = [
            output("DP-1", "U2720Q"),
            output("DP-2", "U2720Q"),
            output("eDP-1", "Panel"),
        ];
        assert_eq!(labels(&outputs), ["DP-1", "DP-2", "Panel"]);
    }
}
