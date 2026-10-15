//! `org.crownos.ScreenCast1`: what the bar shows while something is being
//! shared, and how it switches or stops it.
//!
//! The bar is trusted UI — the user picks a new source there themselves —
//! so a switch goes straight to the cast without asking again.

use zbus::object_server::SignalEmitter;
use zbus::{ObjectServer, fdo, interface};

use super::session::{self, Sessions};
use crate::cast::{CastId, Caster};
use crate::chooser;

pub const BUS_NAME: &str = "org.crownos.ScreenCast";
pub const PATH: &str = "/org/crownos/ScreenCast";

pub struct Control {
    caster: Caster,
    sessions: Sessions,
}

impl Control {
    pub fn new(caster: Caster, sessions: Sessions) -> Self {
        Self { caster, sessions }
    }
}

/// `(kind, id)`, how a source travels over the bus.
fn kind_and_id(encoded: &str) -> (&str, &str) {
    encoded.split_once(':').unwrap_or(("", encoded))
}

#[interface(name = "org.crownos.ScreenCast1")]
impl Control {
    /// Every running cast: id, the app watching, the source's kind
    /// (`output`, `window` or `workspace`), its id, and its name.
    #[zbus(property)]
    fn casts(&self) -> Vec<(u64, String, String, String, String)> {
        self.sessions
            .active()
            .into_iter()
            .map(|(CastId(id), cast)| {
                let encoded = chooser::encode(&cast.source);
                let (kind, source) = kind_and_id(&encoded);
                (id, cast.app, kind.to_owned(), source.to_owned(), cast.label)
            })
            .collect()
    }

    /// What a cast could switch to: kind, id, name, and a line of detail.
    async fn sources(&self) -> fdo::Result<Vec<(String, String, String, String)>> {
        let catalog = self
            .caster
            .catalog()
            .await
            .map_err(|error| fdo::Error::Failed(format!("{error:#}")))?;
        Ok(chooser::everything(&catalog)
            .into_iter()
            .map(|labelled| {
                let encoded = chooser::encode(&labelled.source);
                let (kind, id) = kind_and_id(&encoded);
                (
                    kind.to_owned(),
                    id.to_owned(),
                    labelled.label,
                    labelled.detail,
                )
            })
            .collect())
    }

    async fn switch(
        &self,
        cast: u64,
        kind: String,
        source: String,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let id = CastId(cast);
        let mut active = self
            .sessions
            .active()
            .into_iter()
            .find(|(running, _)| *running == id)
            .map(|(_, active)| active)
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("no cast {cast} is running")))?;
        let source = chooser::decode(&format!("{kind}:{source}"))
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("unknown source kind {kind}")))?;
        let catalog = self
            .caster
            .catalog()
            .await
            .map_err(|error| fdo::Error::Failed(format!("{error:#}")))?;
        if !catalog.contains(&source) {
            return Err(fdo::Error::InvalidArgs("that source is gone".into()));
        }
        self.caster
            .switch(id, source.clone())
            .map_err(|error| fdo::Error::Failed(format!("{error:#}")))?;
        active.label = chooser::describe(&source, &catalog);
        active.source = source;
        self.sessions.set_active(id, active);
        self.casts_changed(&emitter).await?;
        Ok(())
    }

    async fn stop(&self, cast: u64, #[zbus(object_server)] server: &ObjectServer) {
        let id = CastId(cast);
        self.caster.stop(id);
        session::close(server, &self.sessions, id).await;
    }
}

/// Tells the bar the set of casts changed.
pub async fn announce(server: &ObjectServer) {
    let Ok(control) = server.interface::<_, Control>(PATH).await else {
        return;
    };
    if let Err(error) = control
        .get()
        .await
        .casts_changed(control.signal_emitter())
        .await
    {
        tracing::debug!(%error, "announcing the running casts");
    }
}
