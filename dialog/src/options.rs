//! Extra questions under the message, such as "Remember this choice":
//!
//! ```text
//! crownos-portal-dialog ... --checkbox "Remember this choice" --initial true \
//!     --list "Show" --item "Titles only" --item "Everything" --initial 1
//! ```
//!
//! `--initial` sets the last `--checkbox` (`true` or `false`) or the last
//! `--list` (the index of an item). Their values go into the exit status with
//! the chosen button; see `crownos_config::consent_dialog`.

use anyhow::{Context, bail, ensure};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionKind {
    Checkbox { initially_on: bool },
    List { items: Vec<String>, initial: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertOption {
    pub label: String,
    pub kind: OptionKind,
}

/// Applies one command-line flag. Returns `false` when `flag` is not an option flag.
pub fn apply_flag(
    options: &mut Vec<AlertOption>,
    flag: &str,
    value: String,
) -> anyhow::Result<bool> {
    match flag {
        "--checkbox" => options.push(AlertOption {
            label: value,
            kind: OptionKind::Checkbox {
                initially_on: false,
            },
        }),
        "--list" => options.push(AlertOption {
            label: value,
            kind: OptionKind::List {
                items: Vec::new(),
                initial: 0,
            },
        }),
        "--item" => match options.last_mut().map(|option| &mut option.kind) {
            Some(OptionKind::List { items, .. }) => items.push(value),
            _ => bail!("--item must follow a --list"),
        },
        "--initial" => match options.last_mut().map(|option| &mut option.kind) {
            Some(OptionKind::Checkbox { initially_on }) => {
                *initially_on = match value.as_str() {
                    "true" => true,
                    "false" => false,
                    _ => bail!("--initial {value} is not true or false"),
                };
            }
            Some(OptionKind::List { initial, .. }) => {
                *initial = value
                    .parse()
                    .with_context(|| format!("--initial {value} is not an item index"))?;
            }
            None => bail!("--initial must follow a --checkbox or a --list"),
        },
        _ => return Ok(false),
    }
    Ok(true)
}

pub fn validate(options: &[AlertOption]) -> anyhow::Result<()> {
    for option in options {
        if let OptionKind::List { items, initial } = &option.kind {
            ensure!(!items.is_empty(), "--list {} has no --item", option.label);
            ensure!(
                *initial < items.len(),
                "--list {} has no item {initial}",
                option.label
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(flags: &[(&str, &str)]) -> anyhow::Result<Vec<AlertOption>> {
        let mut options = Vec::new();
        for (flag, value) in flags {
            ensure!(apply_flag(&mut options, flag, (*value).to_owned())?);
        }
        validate(&options)?;
        Ok(options)
    }

    #[test]
    fn checkboxes_and_lists_take_their_initial_values() -> anyhow::Result<()> {
        let options = parse(&[
            ("--checkbox", "Remember"),
            ("--initial", "true"),
            ("--list", "Show"),
            ("--item", "Titles"),
            ("--item", "Everything"),
            ("--initial", "1"),
        ])?;
        assert_eq!(options[0].kind, OptionKind::Checkbox { initially_on: true });
        assert_eq!(
            options[1].kind,
            OptionKind::List {
                items: vec!["Titles".to_owned(), "Everything".to_owned()],
                initial: 1,
            }
        );
        Ok(())
    }

    #[test]
    fn misplaced_or_invalid_option_flags_are_refused() {
        assert!(parse(&[("--item", "a")]).is_err());
        assert!(parse(&[("--initial", "true")]).is_err());
        assert!(parse(&[("--checkbox", "a"), ("--initial", "yes")]).is_err());
        assert!(parse(&[("--list", "a")]).is_err());
        assert!(parse(&[("--list", "a"), ("--item", "b"), ("--initial", "1")]).is_err());
        assert!(parse(&[("--checkbox", "a"), ("--item", "b")]).is_err());
    }

    #[test]
    fn other_flags_are_left_alone() -> anyhow::Result<()> {
        let mut options = Vec::new();
        assert!(!apply_flag(&mut options, "--choice", "a=b".to_owned())?);
        assert!(options.is_empty());
        Ok(())
    }
}
