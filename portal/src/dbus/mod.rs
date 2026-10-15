//! The portal's D-Bus face: the implementation interfaces xdg-desktop-portal
//! forwards app requests to.

mod access;
mod control;
pub mod options;
mod request;
mod response;
mod screencast;
mod session;

use anyhow::Context;
use tokio::sync::mpsc;

use crate::cast::{CastId, Caster};
use access::Access;
use control::Control;
use screencast::ScreenCast;
use session::Sessions;

const BUS_NAME: &str = "org.freedesktop.impl.portal.desktop.crownos";
const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";

/// Serves the portal until the bus connection goes away.
pub async fn serve(caster: Caster, ended: mpsc::UnboundedReceiver<CastId>) -> anyhow::Result<()> {
    let sessions = Sessions::default();
    let connection = zbus::connection::Builder::session()?
        .serve_at(
            OBJECT_PATH,
            ScreenCast::new(caster.clone(), sessions.clone()),
        )?
        .serve_at(OBJECT_PATH, Access)?
        .serve_at(control::PATH, Control::new(caster, sessions.clone()))?
        .name(BUS_NAME)?
        .name(control::BUS_NAME)?
        .build()
        .await
        .with_context(|| format!("claiming {BUS_NAME} on the session bus"))?;
    tracing::info!("serving {BUS_NAME}");
    session::close_ended(connection, sessions, ended).await;
    Ok(())
}
