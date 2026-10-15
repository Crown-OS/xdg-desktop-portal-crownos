//! `org.freedesktop.impl.portal.Session`, and the cast each one owns.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::mpsc;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedObjectPath;
use zbus::{Connection, ObjectServer, interface};

use super::control;
use super::options::{Remembered, SOURCE_MONITOR};
use crate::cast::{CastId, Caster, Cursor, Source};

/// What SelectSources asked for, applied at Start.
#[derive(Debug, Clone)]
pub struct Selection {
    pub cursor: Cursor,
    /// The portal's source type bits the app accepts.
    pub types: u32,
    pub restore: Option<Remembered>,
    pub persist_mode: u32,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            cursor: Cursor::Hidden,
            types: SOURCE_MONITOR,
            restore: None,
            persist_mode: 0,
        }
    }
}

/// A cast that is streaming, as the bar shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveCast {
    pub app: String,
    pub source: Source,
    pub label: String,
}

struct Entry {
    path: OwnedObjectPath,
    active: Option<ActiveCast>,
}

/// Which session object each cast belongs to, and what the started ones
/// capture.
#[derive(Clone, Default)]
pub struct Sessions(Arc<Mutex<BTreeMap<CastId, Entry>>>);

impl Sessions {
    fn with<T>(&self, f: impl FnOnce(&mut BTreeMap<CastId, Entry>) -> T) -> T {
        f(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn insert(&self, id: CastId, path: OwnedObjectPath) {
        self.with(|entries| entries.insert(id, Entry { path, active: None }));
    }

    pub fn remove(&self, id: CastId) -> Option<OwnedObjectPath> {
        self.with(|entries| entries.remove(&id).map(|entry| entry.path))
    }

    pub fn set_active(&self, id: CastId, cast: ActiveCast) {
        self.with(|entries| {
            if let Some(entry) = entries.get_mut(&id) {
                entry.active = Some(cast);
            }
        });
    }

    pub fn active(&self) -> Vec<(CastId, ActiveCast)> {
        self.with(|entries| {
            entries
                .iter()
                .filter_map(|(id, entry)| entry.active.clone().map(|cast| (*id, cast)))
                .collect()
        })
    }
}

pub struct Session {
    pub id: CastId,
    pub selection: Selection,
    pub started: bool,
    caster: Caster,
    sessions: Sessions,
    path: OwnedObjectPath,
}

impl Session {
    pub fn new(id: CastId, path: OwnedObjectPath, caster: Caster, sessions: Sessions) -> Self {
        Self {
            id,
            selection: Selection::default(),
            started: false,
            caster,
            sessions,
            path,
        }
    }
}

#[interface(name = "org.freedesktop.impl.portal.Session")]
impl Session {
    async fn close(&self, #[zbus(connection)] connection: &Connection) {
        self.caster.stop(self.id);
        self.sessions.remove(self.id);
        // Not from inside the object's own method call.
        let connection = connection.clone();
        let path = self.path.clone();
        tokio::spawn(async move {
            let server = connection.object_server();
            if let Err(error) = server.remove::<Self, _>(&path).await {
                tracing::debug!(%error, "removing a closed session");
            }
            control::announce(server).await;
        });
    }

    #[zbus(signal)]
    async fn closed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(property(emits_changed_signal = "const"), name = "version")]
    fn version(&self) -> u32 {
        1
    }
}

/// Ends `id`'s session as though the app's stream went away: the app is told
/// with `Closed` and the object disappears.
pub async fn close(server: &ObjectServer, sessions: &Sessions, id: CastId) {
    let Some(path) = sessions.remove(id) else {
        return;
    };
    if let Ok(session) = server.interface::<_, Session>(&path).await
        && let Err(error) = Session::closed(session.signal_emitter()).await
    {
        tracing::warn!(%error, "announcing a closed session");
    }
    if let Err(error) = server.remove::<Session, _>(&path).await {
        tracing::debug!(%error, "removing an ended session");
    }
    control::announce(server).await;
}

/// Closes the session of every cast that ended on its own, so the app learns
/// its stream is gone.
pub async fn close_ended(
    connection: Connection,
    sessions: Sessions,
    mut ended: mpsc::UnboundedReceiver<CastId>,
) {
    let server = connection.object_server();
    while let Some(id) = ended.recv().await {
        close(server, &sessions, id).await;
    }
}
