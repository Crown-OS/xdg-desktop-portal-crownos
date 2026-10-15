//! What the dialog shows, as the portal describes it on the command line.
//!
//! A question, with one button per choice:
//!
//! ```text
//! crownos-portal-dialog --icon access --title T --message M \
//!     --choice grant="Allow" --cancel "Don't Allow"
//! ```
//!
//! A picker, when `--confirm` is given: choices grouped into tabbed
//! sections, each with a line of detail, picked and then confirmed.
//!
//! ```text
//! crownos-portal-dialog --icon screen --title T --message M \
//!     --cancel Cancel --confirm Share \
//!     --section Screens --choice output:DP-1="Entire Screen" --detail "3840 × 2160" \
//!     --section Windows --choice window:abc=Inbox --detail Mail
//! ```

use anyhow::{Context, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertIcon {
    Caution,
    Screen,
    Access,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Section {
    pub title: String,
    pub choices: Vec<Choice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    pub icon: AlertIcon,
    pub title: String,
    pub message: String,
    pub sections: Vec<Section>,
    pub cancel: String,
    /// The picker's confirm button; `None` for a question.
    pub confirm: Option<String>,
}

impl Alert {
    pub fn parse(mut args: impl Iterator<Item = String>) -> anyhow::Result<Self> {
        let mut alert = Self {
            icon: AlertIcon::Caution,
            title: String::new(),
            message: String::new(),
            sections: Vec::new(),
            cancel: "Cancel".to_owned(),
            confirm: None,
        };
        while let Some(flag) = args.next() {
            let value = args
                .next()
                .with_context(|| format!("{flag} needs a value"))?;
            match flag.as_str() {
                "--icon" => alert.icon = icon(&value)?,
                "--title" => alert.title = value,
                "--message" => alert.message = value,
                "--cancel" => alert.cancel = value,
                "--confirm" => alert.confirm = Some(value),
                "--section" => alert.sections.push(Section {
                    title: value,
                    choices: Vec::new(),
                }),
                "--choice" => {
                    let (id, label) = value
                        .split_once('=')
                        .with_context(|| format!("--choice {value} is not ID=LABEL"))?;
                    if alert.sections.is_empty() {
                        alert.sections.push(Section::default());
                    }
                    if let Some(section) = alert.sections.last_mut() {
                        section.choices.push(Choice {
                            id: id.to_owned(),
                            label: label.to_owned(),
                            detail: String::new(),
                        });
                    }
                }
                "--detail" => {
                    alert
                        .sections
                        .last_mut()
                        .and_then(|section| section.choices.last_mut())
                        .context("--detail must follow a --choice")?
                        .detail = value;
                }
                _ => bail!("unknown option {flag}"),
            }
        }
        alert.sections.retain(|section| !section.choices.is_empty());
        anyhow::ensure!(
            !alert.sections.is_empty(),
            "an alert needs at least one --choice"
        );
        Ok(alert)
    }

    pub fn choices(&self) -> impl Iterator<Item = &Choice> {
        self.sections.iter().flat_map(|section| &section.choices)
    }
}

fn icon(name: &str) -> anyhow::Result<AlertIcon> {
    match name {
        "warning" => Ok(AlertIcon::Caution),
        "screen" => Ok(AlertIcon::Screen),
        "access" => Ok(AlertIcon::Access),
        _ => bail!("unknown icon {name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> anyhow::Result<Alert> {
        Alert::parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn a_choice_splits_at_the_first_equals_sign() -> anyhow::Result<()> {
        let alert = parse(&["--choice", "DP-1=Share a=b", "--title", "T"])?;
        let choices: Vec<_> = alert.choices().collect();
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].id, "DP-1");
        assert_eq!(choices[0].label, "Share a=b");
        assert_eq!(alert.title, "T");
        assert!(alert.confirm.is_none());
        Ok(())
    }

    #[test]
    fn a_picker_groups_choices_with_details() -> anyhow::Result<()> {
        let alert = parse(&[
            "--confirm",
            "Share",
            "--section",
            "Screens",
            "--choice",
            "output:DP-1=Screen",
            "--detail",
            "1920 × 1080",
            "--section",
            "Empty",
            "--section",
            "Windows",
            "--choice",
            "window:a=Inbox",
        ])?;
        let titles: Vec<_> = alert.sections.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["Screens", "Windows"]);
        assert_eq!(alert.sections[0].choices[0].detail, "1920 × 1080");
        assert_eq!(alert.confirm.as_deref(), Some("Share"));
        Ok(())
    }

    #[test]
    fn a_detail_without_a_choice_is_refused() {
        assert!(parse(&["--detail", "x", "--choice", "a=b"]).is_err());
    }

    #[test]
    fn an_alert_without_choices_is_refused() {
        assert!(parse(&["--title", "T"]).is_err());
    }

    #[test]
    fn a_flag_without_a_value_is_refused() {
        assert!(parse(&["--choice", "a=b", "--title"]).is_err());
    }
}
