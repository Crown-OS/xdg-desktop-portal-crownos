//! The alert xdg-desktop-portal-crownos asks the user with. It prints the id
//! of the chosen button and exits 0, or exits 1 when the user declines.

mod alert;
mod answer;
mod options;
mod view;

use std::process::ExitCode;

use crownui::{
    AppConfig, kit,
    prelude::{Anchor, KeyboardInteractivity, Layer, LayerOptions, TRANSPARENT},
};

use alert::Alert;
use answer::DECLINED;

const NAMESPACE: &str = "crownos-portal-dialog";
const MISUSED: u8 = 2;

fn main() -> ExitCode {
    let alert = match Alert::parse(std::env::args().skip(1)) {
        Ok(alert) => alert,
        Err(error) => {
            eprintln!("{NAMESPACE}: {error:#}");
            return ExitCode::from(MISUSED);
        }
    };
    let layer = LayerOptions::new(NAMESPACE, Anchor::ALL)
        .layer(Layer::Overlay)
        .exclusive_zone(-1)
        .keyboard(KeyboardInteractivity::Exclusive);
    let config = AppConfig::new(alert.title.clone())
        .app_id(NAMESPACE)
        .clear_color(TRANSPARENT)
        .font(kit::inter_font().to_vec(), Some(kit::INTER_FAMILY))
        .layer(layer);
    if let Err(error) = crownui::run(view::root(alert, |reply| reply.deliver()), config) {
        eprintln!("{NAMESPACE}: {error:#}");
        return ExitCode::from(MISUSED);
    }
    ExitCode::from(DECLINED)
}
