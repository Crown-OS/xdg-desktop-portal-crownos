//! The alert xdg-desktop-portal-crownos asks the user with. It prints the id
//! of the chosen button and exits 0, or exits 1 when the user declines.

mod alert;
mod caution;
mod escape;
mod layout;
mod view;

use std::process::ExitCode;
use std::sync::{Arc, OnceLock};

use crownuikit::config::load_appearance;
use crownuikit::util::inter_font;
use xilem::{EventLoop, WindowId, Xilem};

use alert::Alert;
use view::{Answer, Dialog, app_logic};

const DECLINED: u8 = 1;
const MISUSED: u8 = 2;

fn main() -> ExitCode {
    let alert = match Alert::parse(std::env::args().skip(1)) {
        Ok(alert) => alert,
        Err(error) => {
            eprintln!("crownos-portal-dialog: {error:#}");
            return ExitCode::from(MISUSED);
        }
    };
    load_appearance();

    let answer = Arc::new(OnceLock::new());
    let dialog = Dialog {
        alert,
        answer: Arc::clone(&answer),
        window: WindowId::next(),
        tab: 0,
        picked: None,
    };
    let app = Xilem::new(dialog, app_logic).with_font(inter_font());
    if let Err(error) = app.run_in(EventLoop::with_user_event()) {
        eprintln!("crownos-portal-dialog: {error}");
        return ExitCode::from(MISUSED);
    }
    match answer.get() {
        Some(Answer::Choice(id)) => {
            println!("{id}");
            ExitCode::SUCCESS
        }
        Some(Answer::Cancel) | None => ExitCode::from(DECLINED),
    }
}
