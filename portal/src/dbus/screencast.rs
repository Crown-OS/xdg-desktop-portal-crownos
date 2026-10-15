//! `org.freedesktop.impl.portal.ScreenCast`, version 5.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::{ObjectServer, interface};

use super::control;
use super::options::{
    CURSOR_EMBEDDED, CURSOR_HIDDEN, Remembered, SOURCE_MONITOR, SOURCE_WINDOW,
    SelectSourcesOptions, StartResults, StreamProperties,
};
use super::request::Request;
use super::response::Response;
use super::session::{ActiveCast, Selection, Session, Sessions};
use crate::cast::{CastId, Caster, Catalog, Cursor, Source};
use crate::chooser;
use crate::util::desktop_entry::display_name;

const VERSION: u32 = 5;
/// Workspaces are offered as monitors: they are a screen's worth of picture.
const AVAILABLE_SOURCES: u32 = SOURCE_MONITOR | SOURCE_WINDOW;

type Results = HashMap<String, OwnedValue>;

pub struct ScreenCast {
    caster: Caster,
    sessions: Sessions,
    next_id: AtomicU64,
}

impl ScreenCast {
    pub fn new(caster: Caster, sessions: Sessions) -> Self {
        Self {
            caster,
            sessions,
            next_id: AtomicU64::new(1),
        }
    }

    async fn begin(
        &self,
        handle: &OwnedObjectPath,
        session_handle: &OwnedObjectPath,
        app_id: &str,
        server: &ObjectServer,
    ) -> anyhow::Result<Option<StartResults>> {
        let session = server
            .interface::<_, Session>(session_handle)
            .await
            .context("Start on an unknown session")?;
        let (id, selection) = {
            let session = session.get().await;
            anyhow::ensure!(!session.started, "the session was already started");
            (session.id, session.selection.clone())
        };

        let catalog = self.caster.catalog().await?;
        let remembered = selection
            .restore
            .as_ref()
            .and_then(|remembered| remembered.resolve(&catalog));
        let source = match remembered {
            Some(source) => source,
            None => match self
                .ask(handle, app_id, &catalog, selection.types, server)
                .await?
            {
                Some(source) => source,
                None => return Ok(None),
            },
        };

        let node_id = self
            .caster
            .start(id, source.clone(), selection.cursor)
            .await?;
        session.get_mut().await.started = true;
        tracing::info!(app_id, ?source, node_id, "screen cast started");

        let label = chooser::describe(&source, &catalog);
        let app = display_name(app_id).unwrap_or_else(|| match app_id {
            "" => "An app".to_owned(),
            app_id => app_id.to_owned(),
        });
        self.sessions.set_active(
            id,
            ActiveCast {
                app,
                source: source.clone(),
                label,
            },
        );
        control::announce(server).await;

        let persist = selection.persist_mode != 0;
        Ok(Some(StartResults {
            streams: Some(vec![(node_id, StreamProperties::of(&source, &catalog))]),
            persist_mode: persist.then_some(selection.persist_mode),
            restore_data: persist
                .then(|| Remembered::of(&source, &catalog))
                .flatten()
                .and_then(|remembered| remembered.encode()),
        }))
    }

    async fn ask(
        &self,
        handle: &OwnedObjectPath,
        app_id: &str,
        catalog: &Catalog,
        types: u32,
        server: &ObjectServer,
    ) -> anyhow::Result<Option<Source>> {
        let (request, closed) = Request::new();
        server.at(handle, request).await?;
        let choice = tokio::select! {
            choice = chooser::choose_source(app_id, catalog, types) => choice,
            () = closed.notified() => Ok(None),
        };
        server.remove::<Request, _>(handle).await?;
        choice
    }
}

#[interface(name = "org.freedesktop.impl.portal.ScreenCast")]
impl ScreenCast {
    #[zbus(out_args("response", "results"))]
    async fn create_session(
        &self,
        _handle: OwnedObjectPath,
        session_handle: OwnedObjectPath,
        _app_id: String,
        _options: Results,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> (u32, Results) {
        let id = CastId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let session = Session::new(
            id,
            session_handle.clone(),
            self.caster.clone(),
            self.sessions.clone(),
        );
        match server.at(&session_handle, session).await {
            Ok(_) => {
                self.sessions.insert(id, session_handle);
                (Response::Success.into(), Results::new())
            }
            Err(error) => {
                tracing::warn!(%error, "creating a screen cast session");
                (Response::Failed.into(), Results::new())
            }
        }
    }

    #[zbus(out_args("response", "results"))]
    async fn select_sources(
        &self,
        _handle: OwnedObjectPath,
        session_handle: OwnedObjectPath,
        _app_id: String,
        options: SelectSourcesOptions,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> (u32, Results) {
        let Ok(session) = server.interface::<_, Session>(&session_handle).await else {
            return (Response::Failed.into(), Results::new());
        };
        let types = options.types.unwrap_or(SOURCE_MONITOR) & AVAILABLE_SOURCES;
        session.get_mut().await.selection = Selection {
            cursor: match options.cursor_mode {
                Some(CURSOR_EMBEDDED) => Cursor::Embedded,
                _ => Cursor::Hidden,
            },
            types: if types == 0 { SOURCE_MONITOR } else { types },
            restore: options.restore_data.as_ref().and_then(Remembered::decode),
            persist_mode: options.persist_mode.unwrap_or_default(),
        };
        (Response::Success.into(), Results::new())
    }

    #[zbus(out_args("response", "results"))]
    async fn start(
        &self,
        handle: OwnedObjectPath,
        session_handle: OwnedObjectPath,
        app_id: String,
        _parent_window: String,
        _options: Results,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> (u32, StartResults) {
        match self.begin(&handle, &session_handle, &app_id, server).await {
            Ok(Some(results)) => (Response::Success.into(), results),
            Ok(None) => (Response::Cancelled.into(), StartResults::default()),
            Err(error) => {
                tracing::warn!(app_id, "screen cast failed to start: {error:#}");
                (Response::Failed.into(), StartResults::default())
            }
        }
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn available_source_types(&self) -> u32 {
        AVAILABLE_SOURCES
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn available_cursor_modes(&self) -> u32 {
        CURSOR_HIDDEN | CURSOR_EMBEDDED
    }

    #[zbus(property(emits_changed_signal = "const"), name = "version")]
    fn version(&self) -> u32 {
        VERSION
    }
}
