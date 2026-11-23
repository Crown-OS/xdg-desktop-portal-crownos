//! The alert xdg-desktop-portal-crownos and crownpositor ask the user with.
//! Its exit status is the answer (`crownos_config::consent_dialog`). Once its
//! surface shows, it turns `PR_SET_DUMPABLE` off, so no other process of the
//! user can attach to it and answer.

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
use answer::{DECLINED, MISUSED};

const NAMESPACE: &str = "crownos-portal-dialog";

fn main() -> ExitCode {
    let alert = match Alert::parse(std::env::args().skip(1)) {
        Ok(alert) => alert,
        Err(error) => {
            eprintln!("{NAMESPACE}: {error:#}");
            return ExitCode::from(MISUSED);
        }
    };
    if !alert.shape().fits() {
        eprintln!("{NAMESPACE}: too many answers to fit an exit status");
        return ExitCode::from(MISUSED);
    }
    let layer = LayerOptions::new(NAMESPACE, Anchor::ALL)
        .layer(Layer::Overlay)
        .exclusive_zone(-1)
        .keyboard(KeyboardInteractivity::Exclusive);
    let config = AppConfig::new(alert.title.clone())
        .app_id(NAMESPACE)
        .clear_color(TRANSPARENT)
        .font(kit::inter_font().to_vec(), Some(kit::INTER_FAMILY))
        .layer(layer);
    let answered = alert.clone();
    let root = view::root(alert, move |reply| reply.deliver(&answered));
    if let Err(error) = crownui::run(view::after_first_frames(root, forbid_tracing), config) {
        eprintln!("{NAMESPACE}: {error:#}");
        return ExitCode::from(MISUSED);
    }
    ExitCode::from(DECLINED)
}

/// After the compositor has identified the dialog: it read the executable when the dialog
/// connected, before the first frame.
fn forbid_tracing() {
    if let Err(error) =
        rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable)
    {
        eprintln!("{NAMESPACE}: cannot turn PR_SET_DUMPABLE off: {error}");
        std::process::exit(i32::from(MISUSED));
    }
}
