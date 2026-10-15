//! The cast thread: the compositor connection and the PipeWire loop, driven
//! from one place so no callback ever runs concurrently with another.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::os::fd::AsRawFd;
use std::rc::Rc;
use std::sync::mpsc::SyncSender;

use anyhow::{Context, anyhow, bail};
use crownos_protocols::screencast::v1::client::crownos_screencast_manager_v1::{
    self, CrownosScreencastManagerV1, CursorMode,
};
use crownos_protocols::screencast::v1::client::crownos_screencast_session_v1::{
    self, CrownosScreencastSessionV1,
};
use pipewire::channel::Receiver;
use pipewire::context::ContextRc;
use pipewire::core::CoreRc;
use pipewire::loop_::Timeout;
use pipewire::main_loop::MainLoopRc;
use pipewire::spa::support::system::IoFlags;
use tokio::sync::mpsc;
use wayland_client::backend::WaylandError;
use wayland_client::event_created_child;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_buffer::{self, WlBuffer};
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1},
    ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1},
    ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
};
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1::{
    self, ZwpLinuxBufferParamsV1,
};
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_dmabuf_v1::{
    self, ZwpLinuxDmabufV1,
};

use super::allocator::Allocator;
use super::outputs::Outputs;
use super::session::{CastSession, Status, Target};
use super::windows::Windows;
use super::workspaces::Workspaces;
use super::{CastId, Catalog, Command, Cursor, Source};

/// `create_immed` and nothing newer.
const DMABUF_VERSION: u32 = 3;
/// Window and workspace sessions.
const SCREENCAST_VERSION: u32 = 2;
const FALLBACK_REFRESH_MHZ: u32 = 60_000;

/// What every cast needs to create its objects.
pub(super) struct Services {
    pub(super) manager: CrownosScreencastManagerV1,
    pub(super) dmabuf: ZwpLinuxDmabufV1,
    pub(super) queue: QueueHandle<Engine>,
    pub(super) core: CoreRc,
    pub(super) allocator: Rc<Allocator>,
}

pub(super) struct Engine {
    services: Services,
    outputs: Outputs,
    windows: Windows,
    workspaces: Workspaces,
    casts: HashMap<CastId, CastSession>,
    ended: mpsc::UnboundedSender<CastId>,
}

/// Runs the thread until the compositor goes away. Setup failures are
/// reported on `started`, so the portal refuses to start without a working
/// compositor and PipeWire.
pub(super) fn run(
    inbox: Receiver<Command>,
    ended: mpsc::UnboundedSender<CastId>,
    started: &SyncSender<anyhow::Result<()>>,
) {
    let setup = setup(ended);
    let (main_loop, _context, connection, mut queue, mut engine) = match setup {
        Ok(parts) => {
            let _ = started.send(Ok(()));
            parts
        }
        Err(error) => {
            let _ = started.send(Err(error));
            return;
        }
    };
    if let Err(error) = drive(&main_loop, &connection, &mut queue, &mut engine, inbox) {
        tracing::error!("the cast thread stopped: {error:#}");
    }
}

type Parts = (
    MainLoopRc,
    ContextRc,
    Connection,
    EventQueue<Engine>,
    Engine,
);

fn setup(ended: mpsc::UnboundedSender<CastId>) -> anyhow::Result<Parts> {
    let main_loop = MainLoopRc::new(None).context("creating the PipeWire loop")?;
    let context = ContextRc::new(&main_loop, None).context("creating the PipeWire context")?;
    let core = context.connect_rc(None).context("connecting to PipeWire")?;

    let connection = Connection::connect_to_env().context("connecting to the compositor")?;
    let (globals, mut queue) = registry_queue_init::<Engine>(&connection)?;
    let handle = queue.handle();
    let manager = globals
        .bind(&handle, 1..=SCREENCAST_VERSION, ())
        .context("the compositor does not offer crownos-screencast-v1 to this client")?;
    // Optional: without them only outputs are offered.
    let _toplevels = globals
        .bind::<ExtForeignToplevelListV1, _, _>(&handle, 1..=1, ())
        .inspect_err(|error| tracing::info!("windows cannot be shared: {error}"));
    let _workspaces = globals
        .bind::<ExtWorkspaceManagerV1, _, _>(&handle, 1..=1, ())
        .inspect_err(|error| tracing::info!("workspaces cannot be shared: {error}"));
    let dmabuf = globals
        .bind(&handle, DMABUF_VERSION..=DMABUF_VERSION, ())
        .context("the compositor does not offer linux-dmabuf")?;

    let mut outputs = Outputs::default();
    globals.contents().with_list(|list| {
        list.iter()
            .filter(|global| global.interface == WlOutput::interface().name)
            .for_each(|global| {
                outputs.bind(globals.registry(), global.name, global.version, &handle)
            });
    });

    let mut engine = Engine {
        services: Services {
            manager,
            dmabuf,
            queue: handle,
            core,
            allocator: Rc::new(Allocator::open()?),
        },
        outputs,
        windows: Windows::default(),
        workspaces: Workspaces::default(),
        casts: HashMap::new(),
        ended,
    };
    queue.roundtrip(&mut engine)?;
    queue.roundtrip(&mut engine)?;
    Ok((main_loop, context, connection, queue, engine))
}

fn drive(
    main_loop: &MainLoopRc,
    connection: &Connection,
    queue: &mut EventQueue<Engine>,
    engine: &mut Engine,
    inbox: Receiver<Command>,
) -> anyhow::Result<()> {
    let pending = Rc::new(RefCell::new(Vec::new()));
    let _inbox = inbox.attach(main_loop.loop_(), {
        let pending = Rc::clone(&pending);
        move |command| pending.borrow_mut().push(command)
    });

    let disconnected = Rc::new(Cell::new(false));
    let fd = connection.backend().poll_fd().as_raw_fd();
    let _wayland = main_loop.loop_().add_io(fd, IoFlags::IN, {
        let connection = connection.clone();
        let disconnected = Rc::clone(&disconnected);
        move |_| {
            let Some(guard) = connection.prepare_read() else {
                return;
            };
            match guard.read() {
                Err(WaylandError::Io(error)) if error.kind() == ErrorKind::WouldBlock => {}
                Err(_) => disconnected.set(true),
                Ok(_) => {}
            }
        }
    });

    loop {
        queue.dispatch_pending(engine)?;
        for command in pending.take() {
            engine.apply(command);
        }
        engine.tick();
        queue.flush()?;
        if disconnected.get() {
            bail!("the compositor closed the connection");
        }
        main_loop.loop_().iterate(Timeout::Infinite);
    }
}

impl Engine {
    fn apply(&mut self, command: Command) {
        match command {
            Command::Catalog(reply) => {
                let _ = reply.send(self.catalog());
            }
            Command::Start {
                id,
                source,
                cursor,
                ready,
            } => {
                let (target, refresh_mhz) = match self.resolve(&source) {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                let cursor = match cursor {
                    Cursor::Hidden => CursorMode::Hidden,
                    Cursor::Embedded => CursorMode::Embedded,
                };
                let cast = CastSession::new(&self.services, id, target, refresh_mhz, cursor, ready);
                self.casts.insert(id, cast);
            }
            Command::Switch { id, source } => {
                let resolved = self.resolve(&source);
                let Some(cast) = self.casts.get_mut(&id) else {
                    return;
                };
                match resolved {
                    Ok((target, refresh_mhz)) => cast.switch(&self.services, target, refresh_mhz),
                    Err(error) => tracing::warn!(?id, "cannot switch the cast: {error:#}"),
                }
            }
            Command::Stop(id) => {
                self.casts.remove(&id);
            }
        }
    }

    fn catalog(&self) -> Catalog {
        let outputs = self.outputs.infos();
        Catalog {
            windows: self.windows.infos(),
            workspaces: self
                .workspaces
                .infos(|proxy| self.outputs.name_of(proxy).map(str::to_owned)),
            outputs,
        }
    }

    /// The compositor's name for `source`, and the frame rate to offer.
    fn resolve(&self, source: &Source) -> anyhow::Result<(Target, u32)> {
        let fastest = self
            .outputs
            .infos()
            .iter()
            .filter_map(|output| u32::try_from(output.refresh_mhz).ok())
            .max()
            .unwrap_or(FALLBACK_REFRESH_MHZ);
        let versioned = || {
            anyhow::ensure!(
                self.services.manager.version() >= SCREENCAST_VERSION,
                "the compositor only captures outputs"
            );
            Ok(())
        };
        match source {
            Source::Output(name) => {
                let tracked = self
                    .outputs
                    .find(name)
                    .ok_or_else(|| anyhow!("no output is named {name}"))?;
                let refresh = u32::try_from(tracked.info.refresh_mhz).unwrap_or(fastest);
                Ok((Target::Output(tracked.proxy.clone()), refresh))
            }
            Source::Window(identifier) => {
                versioned()?;
                Ok((Target::Window(identifier.clone()), fastest))
            }
            Source::Workspace(id) => {
                versioned()?;
                Ok((Target::Workspace(id.clone()), fastest))
            }
        }
    }

    fn tick(&mut self) {
        self.casts.retain(|id, cast| {
            let running = cast.tick(&self.services) == Status::Running;
            if !running {
                let _ = self.ended.send(*id);
            }
            running
        });
    }
}

impl Dispatch<CrownosScreencastSessionV1, CastId> for Engine {
    fn event(
        engine: &mut Self,
        proxy: &CrownosScreencastSessionV1,
        event: crownos_screencast_session_v1::Event,
        id: &CastId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(cast) = engine.casts.get_mut(id) else {
            return;
        };
        if cast.on_capture(&engine.services, proxy, event) == Status::Ended {
            engine.casts.remove(id);
            let _ = engine.ended.send(*id);
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for Engine {
    fn event(
        engine: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } if interface == WlOutput::interface().name => {
                engine.outputs.bind(registry, name, version, queue);
            }
            wl_registry::Event::GlobalRemove { name } => engine.outputs.remove(name),
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, u32> for Engine {
    fn event(
        engine: &mut Self,
        _: &WlOutput,
        event: wl_output::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        engine.outputs.apply(*global, event);
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for Engine {
    fn event(
        engine: &mut Self,
        _: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event {
            engine.windows.add(toplevel);
        }
    }

    event_created_child!(Engine, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for Engine {
    fn event(
        engine: &mut Self,
        proxy: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        engine.windows.apply(proxy, event);
    }
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for Engine {
    fn event(
        engine: &mut Self,
        _: &ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_manager_v1::Event::WorkspaceGroup { workspace_group } => {
                engine.workspaces.add_group(workspace_group);
            }
            ext_workspace_manager_v1::Event::Workspace { workspace } => {
                engine.workspaces.add(workspace);
            }
            _ => {}
        }
    }

    event_created_child!(Engine, ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ExtWorkspaceHandleV1, ()),
    ]);
}

impl Dispatch<ExtWorkspaceGroupHandleV1, ()> for Engine {
    fn event(
        engine: &mut Self,
        proxy: &ExtWorkspaceGroupHandleV1,
        event: ext_workspace_group_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        engine.workspaces.apply_group(proxy, event);
    }
}

impl Dispatch<ExtWorkspaceHandleV1, ()> for Engine {
    fn event(
        engine: &mut Self,
        proxy: &ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        engine.workspaces.apply(proxy, event);
    }
}

impl Dispatch<ZwpLinuxBufferParamsV1, ()> for Engine {
    fn event(
        _: &mut Self,
        _: &ZwpLinuxBufferParamsV1,
        event: zwp_linux_buffer_params_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_linux_buffer_params_v1::Event::Failed = event {
            tracing::warn!("the compositor rejected a cast buffer");
        }
    }
}

macro_rules! ignore_events {
    ($($proxy:ty => $event:ty),+ $(,)?) => {$(
        impl Dispatch<$proxy, ()> for Engine {
            fn event(_: &mut Self, _: &$proxy, _: $event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )+};
}

ignore_events!(
    CrownosScreencastManagerV1 => crownos_screencast_manager_v1::Event,
    ZwpLinuxDmabufV1 => zwp_linux_dmabuf_v1::Event,
    WlBuffer => wl_buffer::Event,
);
