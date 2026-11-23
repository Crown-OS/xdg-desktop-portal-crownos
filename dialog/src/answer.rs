//! What the portal reads back: the chosen button's id on stdout and exit 0,
//! or exit 1 when the user declines. The values of the extra questions come
//! first, one `option VALUE` line each, in both cases.

use std::process;

use crate::options::OUTPUT_PREFIX;

pub const DECLINED: u8 = 1;

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
    /// Reports the reply and ends the process on the spot, before the overlay
    /// animates out.
    pub fn deliver(self) -> ! {
        for value in &self.option_values {
            println!("{OUTPUT_PREFIX}{value}");
        }
        match self.answer {
            Answer::Choice(id) => {
                println!("{id}");
                process::exit(0)
            }
            Answer::Cancel => process::exit(i32::from(DECLINED)),
        }
    }
}
