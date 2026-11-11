//! The alert xdg-desktop-portal-crownos asks the user with. It prints the id
//! of the chosen button and exits 0, or exits 1 when the user declines.

mod alert;
mod answer;
mod caution;
mod escape;
mod layout;
mod view;

use std::process::ExitCode;

use crownuikit::config::load_appearance;
use crownuikit::util::inter_font;
use xilem::{EventLoop, WindowId, Xilem};

use alert::Alert;
use answer::DECLINED;
use view::{Dialog, app_logic};

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

    let dialog = Dialog {
        alert,
        window: WindowId::next(),
        tab: 0,
        picked: None,
    };
    let app = Xilem::new(dialog, app_logic).with_font(inter_font());
    if let Err(error) = app.run_in(EventLoop::with_user_event()) {
        eprintln!("crownos-portal-dialog: {error}");
        return ExitCode::from(MISUSED);
    }
    ExitCode::from(DECLINED)
}
