//! One cast: a compositor capture session feeding one PipeWire stream.
//!
//! Every PipeWire buffer is also a slot of the compositor's ring, so a frame is
//! never copied. A buffer is always in exactly one place:
//!
//! - with the consumer, queued on the stream;
//! - held here, dequeued but not attached to the current capture session;
//! - with the compositor, attached and waiting to be rendered into.
//!
//! The compositor only lets a slot be attached once per constraints sequence,
//! so when PipeWire replaces its buffers while some are attached, the capture
//! session is replaced too and the new buffers go to a fresh ring.

use std::ptr::NonNull;
use std::rc::Rc;
use std::time::Duration;

use anyhow::Context;
use crownos_protocols::screencast::v1::client::crownos_screencast_manager_v1::CursorMode;
use crownos_protocols::screencast::v1::client::crownos_screencast_session_v1::{
    self, CrownosScreencastSessionV1,
};
use pipewire::stream::StreamState;
use pipewire::sys::pw_buffer;
use tokio::sync::oneshot;
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1::Flags;

use super::CastId;
use super::engine::Services;
use super::formats::Constraints;
use super::stream::{StreamEvent, VideoStream};
use crate::util::bits::{join_u32, split_u64};

const MAX_SLOTS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owner {
    Consumer,
    Held,
    Compositor,
}

struct Slot {
    raw: NonNull<pw_buffer>,
    wl_buffer: Option<WlBuffer>,
    owner: Owner,
}

impl Drop for Slot {
    fn drop(&mut self) {
        if let Some(wl_buffer) = &self.wl_buffer {
            wl_buffer.destroy();
        }
    }
}

/// A source, resolved to what the compositor's requests name it by.
#[derive(Debug, Clone)]
pub(super) enum Target {
    Output(WlOutput),
    Window(String),
    Workspace(String),
}

struct Capture {
    proxy: CrownosScreencastSessionV1,
    incoming: Constraints,
    constraints: Option<Constraints>,
    attached: [bool; MAX_SLOTS],
    started: bool,
}

impl Capture {
    fn create(services: &Services, id: CastId, target: &Target, cursor: CursorMode) -> Self {
        let (manager, queue) = (&services.manager, &services.queue);
        let proxy = match target {
            Target::Output(output) => manager.create_output_session(output, cursor, queue, id),
            Target::Window(identifier) => {
                manager.create_toplevel_session(identifier.clone(), cursor, queue, id)
            }
            Target::Workspace(workspace) => {
                manager.create_workspace_session(workspace.clone(), cursor, queue, id)
            }
        };
        Self {
            proxy,
            incoming: Constraints::default(),
            constraints: None,
            attached: [false; MAX_SLOTS],
            started: false,
        }
    }

    fn release(&self, index: usize) {
        if let Ok(index) = u32::try_from(index) {
            self.proxy.release(index, 0, 0);
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.proxy.destroy();
    }
}

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Status {
    Running,
    Ended,
}

pub(super) struct CastSession {
    id: CastId,
    target: Target,
    cursor: CursorMode,
    refresh_mhz: u32,
    // Declared before the stream: the compositor lets go of the buffers
    // before PipeWire frees them.
    capture: Capture,
    slots: Vec<Slot>,
    stream: Option<VideoStream>,
    generation: u64,
    ready: Option<oneshot::Sender<anyhow::Result<u32>>>,
}

impl CastSession {
    pub(super) fn new(
        services: &Services,
        id: CastId,
        target: Target,
        refresh_mhz: u32,
        cursor: CursorMode,
        ready: oneshot::Sender<anyhow::Result<u32>>,
    ) -> Self {
        Self {
            capture: Capture::create(services, id, &target, cursor),
            id,
            target,
            cursor,
            refresh_mhz,
            slots: Vec::new(),
            stream: None,
            generation: 0,
            ready: Some(ready),
        }
    }

    /// Captures `target` from now on, into the same stream. The new capture
    /// session takes the buffers over once its constraints arrive, after the
    /// consumer renegotiated if the size changed.
    pub(super) fn switch(&mut self, services: &Services, target: Target, refresh_mhz: u32) {
        self.capture = Capture::create(services, self.id, &target, self.cursor);
        self.target = target;
        self.refresh_mhz = refresh_mhz;
        self.slots
            .iter_mut()
            .filter(|slot| slot.owner == Owner::Compositor)
            .for_each(|slot| slot.owner = Owner::Held);
    }

    pub(super) fn on_capture(
        &mut self,
        services: &Services,
        proxy: &CrownosScreencastSessionV1,
        event: crownos_screencast_session_v1::Event,
    ) -> Status {
        use crownos_screencast_session_v1::Event;

        if *proxy != self.capture.proxy {
            return Status::Running;
        }
        match event {
            Event::Constraints { width, height } => self.capture.incoming.begin(width, height),
            Event::Format { format } => self.capture.incoming.add_format(format),
            Event::Modifier {
                format,
                modifier_hi,
                modifier_lo,
            } => self
                .capture
                .incoming
                .add_modifier(format, join_u32(modifier_hi, modifier_lo)),
            Event::ConstraintsDone => {
                if let Err(error) = self.constraints_done(services) {
                    return self.fail(&error);
                }
            }
            Event::Frame {
                index,
                tv_sec_hi,
                tv_sec_lo,
                tv_nsec,
                ..
            } => {
                let sampled = Duration::new(join_u32(tv_sec_hi, tv_sec_lo), tv_nsec);
                self.frame(index as usize, sampled);
            }
            Event::Stopped { reason } => {
                tracing::info!(cast = ?self.id, ?reason, "the compositor stopped the capture");
                return Status::Ended;
            }
            _ => {}
        }
        Status::Running
    }

    pub(super) fn tick(&mut self, services: &Services) -> Status {
        let Some(stream) = &self.stream else {
            return Status::Running;
        };
        let events = stream.take_events();
        let node_id = stream.node_id();
        let replaced = stream.generation() != self.generation;
        for event in events {
            match event {
                StreamEvent::State(StreamState::Paused) => {
                    if let Some(ready) = self.ready.take() {
                        let _ = ready.send(Ok(node_id));
                    }
                }
                StreamEvent::State(StreamState::Error(message)) | StreamEvent::Failed(message) => {
                    return self.fail(&anyhow::anyhow!(message));
                }
                StreamEvent::State(StreamState::Unconnected) => return Status::Ended,
                StreamEvent::State(_) => {}
            }
        }
        if replaced {
            self.adopt_buffers(services);
        }
        self.collect_returned();
        self.attach_held(services);
        Status::Running
    }

    fn fail(&mut self, error: &anyhow::Error) -> Status {
        tracing::warn!(cast = ?self.id, "{error:#}");
        if let Some(ready) = self.ready.take() {
            let _ = ready.send(Err(anyhow::anyhow!("{error:#}")));
        }
        Status::Ended
    }

    fn constraints_done(&mut self, services: &Services) -> anyhow::Result<()> {
        let constraints = self.capture.incoming.clone();
        let offer = constraints.offer(self.refresh_mhz);
        anyhow::ensure!(
            !offer.formats.is_empty(),
            "the compositor renders no format a PipeWire consumer reads"
        );
        // Every sequence after the first empties the ring.
        self.capture.constraints = Some(constraints);
        self.capture.attached = [false; MAX_SLOTS];
        self.slots
            .iter_mut()
            .filter(|slot| slot.owner == Owner::Compositor)
            .for_each(|slot| slot.owner = Owner::Held);

        match &self.stream {
            Some(stream) => stream.update_offer(offer),
            None => {
                let stream =
                    VideoStream::connect(&services.core, Rc::clone(&services.allocator), offer)?;
                self.stream = Some(stream);
                Ok(())
            }
        }
    }

    fn frame(&mut self, index: usize, sampled: Duration) {
        let (Some(stream), Some(slot)) = (&mut self.stream, self.slots.get_mut(index)) else {
            return;
        };
        if slot.owner != Owner::Compositor {
            return;
        }
        if !stream.is_streaming() {
            self.capture.release(index);
            return;
        }
        let pts = i64::try_from(sampled.as_nanos()).unwrap_or(i64::MAX);
        // Safety: a compositor-owned slot was dequeued and has not been queued
        // since.
        unsafe { stream.queue(slot.raw, pts) };
        slot.owner = Owner::Consumer;
    }

    /// PipeWire replaced its buffers; start over with the new set.
    fn adopt_buffers(&mut self, services: &Services) {
        let Some(stream) = &self.stream else {
            return;
        };
        if self.capture.attached.iter().any(|attached| *attached) {
            self.capture = Capture::create(services, self.id, &self.target, self.cursor);
        }
        self.slots = stream
            .buffers()
            .into_iter()
            .map(|raw| Slot {
                raw,
                wl_buffer: None,
                owner: Owner::Consumer,
            })
            .collect();
        self.generation = stream.generation();
    }

    fn collect_returned(&mut self) {
        let Some(stream) = &self.stream else {
            return;
        };
        while let Some(raw) = stream.dequeue() {
            let Some((index, slot)) = self
                .slots
                .iter_mut()
                .enumerate()
                .find(|(_, slot)| slot.raw == raw)
            else {
                continue;
            };
            if self.capture.attached.get(index).copied().unwrap_or(false) {
                self.capture.release(index);
                slot.owner = Owner::Compositor;
            } else {
                slot.owner = Owner::Held;
            }
        }
    }

    fn attach_held(&mut self, services: &Services) {
        let (Some(stream), Some(constraints)) = (&self.stream, &self.capture.constraints) else {
            return;
        };
        let Some(negotiated) = stream.negotiated() else {
            return;
        };
        if negotiated.size != (constraints.width, constraints.height)
            || !constraints.accepts(negotiated.fourcc, negotiated.modifier)
        {
            return;
        }
        for (index, slot) in self.slots.iter_mut().enumerate().take(MAX_SLOTS) {
            if slot.owner != Owner::Held || self.capture.attached[index] {
                continue;
            }
            let wl_buffer = match &slot.wl_buffer {
                Some(wl_buffer) => wl_buffer,
                None => match import(
                    services,
                    stream,
                    slot.raw,
                    negotiated.size,
                    negotiated.fourcc,
                ) {
                    Ok(wl_buffer) => slot.wl_buffer.insert(wl_buffer),
                    Err(error) => {
                        tracing::warn!(cast = ?self.id, "{error:#}");
                        continue;
                    }
                },
            };
            let Ok(slot_index) = u32::try_from(index) else {
                continue;
            };
            self.capture.proxy.attach_buffer(slot_index, wl_buffer);
            self.capture.attached[index] = true;
            slot.owner = Owner::Compositor;
        }
        if !self.capture.started && self.capture.attached.iter().any(|attached| *attached) {
            self.capture.proxy.start(negotiated.max_fps_mhz);
            self.capture.started = true;
        }
    }
}

fn import(
    services: &Services,
    stream: &VideoStream,
    raw: NonNull<pw_buffer>,
    (width, height): (u32, u32),
    fourcc: u32,
) -> anyhow::Result<WlBuffer> {
    use std::os::fd::AsFd;

    let allocation = stream
        .allocation(raw)
        .context("the buffer has no allocation")?;
    let params = services.dmabuf.create_params(&services.queue, ());
    let [modifier_hi, modifier_lo] = split_u64(allocation.modifier);
    for (index, plane) in (0..).zip(&allocation.planes) {
        params.add(
            plane.fd.as_fd(),
            index,
            plane.offset,
            plane.stride,
            modifier_hi,
            modifier_lo,
        );
    }
    let wl_buffer = params.create_immed(
        i32::try_from(width)?,
        i32::try_from(height)?,
        fourcc,
        Flags::empty(),
        &services.queue,
        (),
    );
    params.destroy();
    Ok(wl_buffer)
}
