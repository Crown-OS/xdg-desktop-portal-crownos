//! What the portal reads back: the chosen button's id on stdout and exit 0,
//! or exit 1 when the user declines.

use std::process;

pub const DECLINED: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Choice(String),
    Cancel,
}

impl Answer {
    /// Reports the answer and ends the process on the spot: xilem 0.4 stops
    /// its event loop only for a window close, never after a button's action.
    pub fn deliver(self) -> ! {
        match self {
            Self::Choice(id) => {
                println!("{id}");
                process::exit(0)
            }
            Self::Cancel => process::exit(i32::from(DECLINED)),
        }
    }
}
