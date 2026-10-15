//! Screen casting. One thread owns the compositor connection and the PipeWire
//! loop, so a frame moves from the compositor to a consumer without leaving it;
//! the D-Bus side talks to it through a [`Caster`].

mod allocator;
mod engine;
mod formats;
mod outputs;
mod params;
mod session;
mod stream;
mod windows;
mod workspaces;

use anyhow::{Context, anyhow};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CastId(pub u64);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutputInfo {
    pub name: String,
    pub description: String,
    pub model: String,
    pub position: (i32, i32),
    pub mode: (i32, i32),
    pub refresh_mhz: i32,
    pub scale: i32,
}

impl OutputInfo {
    /// The size in compositor coordinates, which is what the portal reports.
    pub fn logical_size(&self) -> (i32, i32) {
        let scale = self.scale.max(1);
        (self.mode.0 / scale, self.mode.1 / scale)
    }
}

/// A window the compositor lists, by its `ext-foreign-toplevel-list` identity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowInfo {
    pub identifier: String,
    pub title: String,
    pub app_id: String,
}

/// A workspace the compositor lists through `ext-workspace`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceInfo {
    pub id: String,
    pub name: String,
    /// The output it lives on, when the compositor said.
    pub output: Option<String>,
    pub active: bool,
}

/// Everything that can be shared right now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    pub outputs: Vec<OutputInfo>,
    pub windows: Vec<WindowInfo>,
    pub workspaces: Vec<WorkspaceInfo>,
}

impl Catalog {
    pub fn output(&self, name: &str) -> Option<&OutputInfo> {
        self.outputs.iter().find(|output| output.name == name)
    }

    /// Whether `source` can still be captured.
    pub fn contains(&self, source: &Source) -> bool {
        match source {
            Source::Output(name) => self.output(name).is_some(),
            Source::Window(identifier) => self
                .windows
                .iter()
                .any(|window| window.identifier == *identifier),
            Source::Workspace(id) => self.workspaces.iter().any(|workspace| workspace.id == *id),
        }
    }
}

/// What a cast captures.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Source {
    /// An output, by its `wl_output` name.
    Output(String),
    /// A window, by its `ext-foreign-toplevel-list` identifier.
    Window(String),
    /// A workspace, by its `ext-workspace` id.
    Workspace(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    Hidden,
    Embedded,
}

enum Command {
    Catalog(oneshot::Sender<Catalog>),
    Start {
        id: CastId,
        source: Source,
        cursor: Cursor,
        ready: oneshot::Sender<anyhow::Result<u32>>,
    },
    Switch {
        id: CastId,
        source: Source,
    },
    Stop(CastId),
}

/// The D-Bus side's handle on the cast thread.
#[derive(Clone)]
pub struct Caster {
    commands: pipewire::channel::Sender<Command>,
}

impl Caster {
    /// Starts the cast thread. A cast that ends on its own — the output went
    /// away, the consumer disconnected — is reported on `ended`.
    pub fn spawn(ended: mpsc::UnboundedSender<CastId>) -> anyhow::Result<Self> {
        let (commands, inbox) = pipewire::channel::channel();
        let (started, startup) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("crownos-cast".into())
            .spawn(move || engine::run(inbox, ended, &started))
            .context("spawning the cast thread")?;
        startup
            .recv()
            .context("the cast thread died while starting")??;
        Ok(Self { commands })
    }

    pub async fn catalog(&self) -> anyhow::Result<Catalog> {
        let (reply, answer) = oneshot::channel();
        self.send(Command::Catalog(reply))?;
        answer.await.context("the cast thread is gone")
    }

    /// Starts casting `source` and returns the PipeWire node the consumer
    /// connects to.
    pub async fn start(&self, id: CastId, source: Source, cursor: Cursor) -> anyhow::Result<u32> {
        let (ready, answer) = oneshot::channel();
        self.send(Command::Start {
            id,
            source,
            cursor,
            ready,
        })?;
        answer
            .await
            .context("the capture ended before its stream was ready")?
    }

    /// Points a running cast at another source. The consumer keeps its
    /// stream; only the picture, and perhaps its size, changes.
    pub fn switch(&self, id: CastId, source: Source) -> anyhow::Result<()> {
        self.send(Command::Switch { id, source })
    }

    pub fn stop(&self, id: CastId) {
        if self.send(Command::Stop(id)).is_err() {
            tracing::debug!(?id, "stopping a cast after the cast thread ended");
        }
    }

    fn send(&self, command: Command) -> anyhow::Result<()> {
        self.commands
            .send(command)
            .map_err(|_| anyhow!("the cast thread is gone"))
    }
}
