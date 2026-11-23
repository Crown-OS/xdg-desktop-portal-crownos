//! What the portal and the compositor read back: the exit status only, as
//! `crownos_config::consent_exit` encodes it. The dialog prints nothing.

use std::process;

use crownos_config::consent_exit::DialogAnswer;
pub use crownos_config::consent_exit::{DECLINED, MISUSED};

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
            Answer::Choice(id) => encode(alert, id, &self.option_values).unwrap_or(MISUSED),
            Answer::Cancel => DECLINED,
        }
    }

    /// Reports the reply and ends the process on the spot, before the overlay
    /// animates out.
    pub fn deliver(self, alert: &Alert) -> ! {
        process::exit(i32::from(self.status(alert)))
    }
}

fn encode(alert: &Alert, id: &str, option_values: &[String]) -> Option<u8> {
    let choice = alert.choices().position(|choice| choice.id == id)?;
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
    use crownos_config::consent_exit::FIRST_ANSWER;

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
        let cancel = Reply {
            answer: Answer::Cancel,
            option_values: Vec::new(),
        };
        assert_eq!(cancel.status(&alert), DECLINED);
        Ok(())
    }
}
