//! What the portal and the compositor read back: the exit status only, as
//! `crownos_config::consent_dialog` encodes it. The dialog prints nothing. The
//! cancel button keeps the values of the extra questions; Escape does not.

use std::process;

use crownos_config::consent_dialog::DialogAnswer;
pub use crownos_config::consent_dialog::{DECLINED, MISUSED};

use crate::alert::Alert;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Choice(String),
    Cancel,
}

/// The answer with the values of the extra questions, in their order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub answer: Answer,
    pub option_values: Vec<String>,
}

impl Reply {
    /// The exit status for this reply to `alert`.
    pub fn status(&self, alert: &Alert) -> u8 {
        match &self.answer {
            Answer::Choice(id) => alert
                .choices()
                .position(|choice| choice.id == *id)
                .and_then(|choice| encode(alert, choice, &self.option_values))
                .unwrap_or(MISUSED),
            Answer::Cancel if !alert.options.is_empty() => {
                encode(alert, alert.choices().count(), &self.option_values).unwrap_or(DECLINED)
            }
            Answer::Cancel => DECLINED,
        }
    }

    /// Reports the reply and ends the process on the spot, before the overlay
    /// animates out.
    pub fn deliver(self, alert: &Alert) -> ! {
        process::exit(i32::from(self.status(alert)))
    }
}

/// The status of `choice` (the cancel button after the others) with these values.
fn encode(alert: &Alert, choice: usize, option_values: &[String]) -> Option<u8> {
    let option_values = option_values
        .iter()
        .map(|value| match value.as_str() {
            "true" => Some(1),
            "false" => Some(0),
            index => index.parse().ok(),
        })
        .collect::<Option<Vec<usize>>>()?;
    alert.shape().encode(&DialogAnswer {
        choice,
        option_values,
    })
}

#[cfg(test)]
mod tests {
    use crownos_config::consent_dialog::FIRST_ANSWER;

    use super::*;

    fn alert(arguments: &[&str]) -> anyhow::Result<Alert> {
        Alert::parse(arguments.iter().map(|argument| (*argument).to_owned()))
    }

    #[test]
    fn a_reply_round_trips_through_the_exit_status() -> anyhow::Result<()> {
        let alert = alert(&[
            "--choice",
            "grant=Allow",
            "--choice",
            "always=Always",
            "--checkbox",
            "Remember",
            "--list",
            "Show",
            "--item",
            "Titles",
            "--item",
            "All",
        ])?;
        let reply = Reply {
            answer: Answer::Choice("always".into()),
            option_values: vec!["true".into(), "1".into()],
        };
        let status = reply.status(&alert);
        assert!(status >= FIRST_ANSWER);
        let decoded = alert.shape().decode(Some(i32::from(status)));
        assert_eq!(
            decoded,
            Some(DialogAnswer {
                choice: 1,
                option_values: vec![1, 1]
            })
        );
        let cancel_button = Reply {
            answer: Answer::Cancel,
            option_values: vec!["true".into(), "0".into()],
        };
        let denied = alert
            .shape()
            .decode(Some(i32::from(cancel_button.status(&alert))));
        assert_eq!(
            denied,
            Some(DialogAnswer {
                choice: 2,
                option_values: vec![1, 0]
            })
        );
        let escape = Reply {
            answer: Answer::Cancel,
            option_values: Vec::new(),
        };
        assert_eq!(escape.status(&alert), DECLINED);
        Ok(())
    }
}
