//! A PipeWire video source whose buffers are GBM dmabufs.
//!
//! The consumer picks a format and modifier from the offer; the stream pins the
//! modifier with a test allocation, then allocates every buffer itself
//! (`ALLOC_BUFFERS`), so the same dmabufs can be handed to the compositor as
//! its ring slots.

use std::cell::{Ref, RefCell};
use std::os::fd::AsRawFd;
use std::ptr::NonNull;
use std::rc::Rc;

use anyhow::{Context, bail};
use pipewire::core::CoreRc;
use pipewire::spa::buffer::DataType;
use pipewire::spa::param::ParamType;
use pipewire::spa::param::format::{FormatProperties, MediaSubtype, MediaType};
use pipewire::spa::param::format_utils::parse_format;
use pipewire::spa::param::video::VideoInfoRaw;
use pipewire::spa::pod::deserialize::PodDeserializer;
use pipewire::spa::pod::{Pod, PodPropFlags};
use pipewire::spa::sys::{
    SPA_CHUNK_FLAG_NONE, SPA_DATA_FLAG_READWRITE, SPA_META_Header, spa_buffer, spa_meta_header,
};
use pipewire::spa::utils::{Choice, ChoiceEnum, Direction, Id};
use pipewire::stream::{StreamFlags, StreamListener, StreamRc, StreamState};
use pipewire::sys::pw_buffer;
use pipewire::{keys, properties::properties};

use super::allocator::{Allocation, Allocator};
use super::formats::{Offer, drm_format};
use super::params;

const STREAM_NAME: &str = "crownos-screencast";
/// A source consumers link to, not a playback stream the session manager
/// routes somewhere.
const MEDIA_CLASS: &str = "Video/Source";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Negotiated {
    pub(super) fourcc: u32,
    pub(super) modifier: u64,
    pub(super) size: (u32, u32),
    pub(super) max_fps_mhz: u32,
}

pub(super) enum StreamEvent {
    State(StreamState),
    Failed(String),
}

struct StreamBuffer {
    raw: NonNull<pw_buffer>,
    allocation: Allocation,
}

/// What the stream's callbacks and its owner share. Callbacks only ever touch
/// this, so nothing they do can re-enter the owner.
struct Shared {
    allocator: Rc<Allocator>,
    offer: Offer,
    negotiated: Option<Negotiated>,
    buffers: Vec<StreamBuffer>,
    generation: u64,
    events: Vec<StreamEvent>,
}

pub(super) struct VideoStream {
    // Dropped before the stream, so no callback outlives it.
    _listener: StreamListener<()>,
    stream: StreamRc,
    shared: Rc<RefCell<Shared>>,
    sequence: u64,
}

impl VideoStream {
    pub(super) fn connect(
        core: &CoreRc,
        allocator: Rc<Allocator>,
        offer: Offer,
    ) -> anyhow::Result<Self> {
        let stream = StreamRc::new(
            core.clone(),
            STREAM_NAME,
            properties! {
                *keys::MEDIA_CLASS => MEDIA_CLASS,
                *keys::NODE_NAME => STREAM_NAME,
            },
        )
        .context("creating the PipeWire stream")?;
        let enum_formats = params::enum_formats(&offer)?;
        let shared = Rc::new(RefCell::new(Shared {
            allocator,
            offer,
            negotiated: None,
            buffers: Vec::new(),
            generation: 0,
            events: Vec::new(),
        }));

        let listener = stream
            .add_local_listener_with_user_data(())
            .state_changed({
                let shared = Rc::clone(&shared);
                move |_, (), _, new| shared.borrow_mut().events.push(StreamEvent::State(new))
            })
            .param_changed({
                let shared = Rc::clone(&shared);
                move |stream, (), id, pod| {
                    let (Some(pod), ParamType::Format) = (pod, ParamType::from_raw(id)) else {
                        return;
                    };
                    let reply = shared.borrow_mut().negotiate(pod);
                    let sent =
                        reply.and_then(|params| Ok(stream.update_params(&mut params.pods())?));
                    if let Err(error) = sent {
                        let failure = StreamEvent::Failed(format!("{error:#}"));
                        shared.borrow_mut().events.push(failure);
                    }
                }
            })
            .add_buffer({
                let shared = Rc::clone(&shared);
                move |_, (), raw| {
                    let mut shared = shared.borrow_mut();
                    if let Err(error) = shared.add_buffer(raw) {
                        shared
                            .events
                            .push(StreamEvent::Failed(format!("{error:#}")));
                    }
                }
            })
            .remove_buffer({
                let shared = Rc::clone(&shared);
                move |_, (), raw| {
                    let mut shared = shared.borrow_mut();
                    shared.buffers.retain(|buffer| buffer.raw.as_ptr() != raw);
                    shared.generation += 1;
                }
            })
            .register()
            .context("listening to the PipeWire stream")?;

        stream
            .connect(
                Direction::Output,
                None,
                StreamFlags::DRIVER | StreamFlags::ALLOC_BUFFERS,
                &mut enum_formats.pods(),
            )
            .context("connecting the PipeWire stream")?;

        Ok(Self {
            _listener: listener,
            stream,
            shared,
            sequence: 0,
        })
    }

    pub(super) fn node_id(&self) -> u32 {
        self.stream.node_id()
    }

    pub(super) fn is_streaming(&self) -> bool {
        self.stream.state() == StreamState::Streaming
    }

    /// Replaces the offer, which makes the consumer negotiate again.
    pub(super) fn update_offer(&self, offer: Offer) -> anyhow::Result<()> {
        let enum_formats = params::enum_formats(&offer)?;
        let mut shared = self.shared.borrow_mut();
        if shared.offer == offer {
            return Ok(());
        }
        shared.offer = offer;
        shared.negotiated = None;
        drop(shared);
        self.stream.update_params(&mut enum_formats.pods())?;
        Ok(())
    }

    pub(super) fn take_events(&self) -> Vec<StreamEvent> {
        std::mem::take(&mut self.shared.borrow_mut().events)
    }

    pub(super) fn generation(&self) -> u64 {
        self.shared.borrow().generation
    }

    pub(super) fn negotiated(&self) -> Option<Negotiated> {
        self.shared.borrow().negotiated
    }

    pub(super) fn buffers(&self) -> Vec<NonNull<pw_buffer>> {
        self.shared
            .borrow()
            .buffers
            .iter()
            .map(|buffer| buffer.raw)
            .collect()
    }

    pub(super) fn allocation(&self, raw: NonNull<pw_buffer>) -> Option<Ref<'_, Allocation>> {
        Ref::filter_map(self.shared.borrow(), |shared| {
            shared
                .buffers
                .iter()
                .find(|buffer| buffer.raw == raw)
                .map(|buffer| &buffer.allocation)
        })
        .ok()
    }

    /// A buffer the consumer is done with, if any.
    pub(super) fn dequeue(&self) -> Option<NonNull<pw_buffer>> {
        // Safety: the stream is connected for as long as `self` lives.
        NonNull::new(unsafe { self.stream.dequeue_raw_buffer() })
    }

    /// Hands a rendered buffer to the consumer.
    ///
    /// # Safety
    ///
    /// `raw` must have come from [`Self::dequeue`] on this stream and not have
    /// been queued since.
    pub(super) unsafe fn queue(&mut self, raw: NonNull<pw_buffer>, pts_ns: i64) {
        self.sequence = self.sequence.wrapping_add(1);
        // Safety: guaranteed by the caller; PipeWire keeps the buffer's data
        // and metadata alive while it is dequeued.
        unsafe {
            let buffer = (*raw.as_ptr()).buffer;
            let chunk = (*(*buffer).datas).chunk;
            (*chunk).flags = SPA_CHUNK_FLAG_NONE as i32;
            if let Some(header) = header(buffer) {
                let header = header.as_ptr();
                (*header).flags = 0;
                (*header).seq = self.sequence;
                (*header).pts = pts_ns;
            }
            self.stream.queue_raw_buffer(raw.as_ptr());
        }
        // A driving stream starts each graph cycle itself; without this the
        // consumer only ever sees the frames queued before it started.
        if self.stream.is_driving()
            && let Err(error) = self.stream.trigger_process()
        {
            tracing::debug!(%error, "triggering a PipeWire cycle");
        }
    }
}

impl Drop for VideoStream {
    fn drop(&mut self) {
        if let Err(error) = self.stream.disconnect() {
            tracing::debug!(%error, "disconnecting the PipeWire stream");
        }
    }
}

impl Shared {
    /// The params that answer the consumer's `Format`: either the format
    /// again with its modifier pinned, or the buffers once it is.
    fn negotiate(&mut self, pod: &Pod) -> anyhow::Result<params::Params> {
        let (media_type, media_subtype) = parse_format(pod)?;
        if media_type != MediaType::Video || media_subtype != MediaSubtype::Raw {
            bail!("the consumer asked for something other than raw video");
        }
        let mut info = VideoInfoRaw::new();
        info.parse(pod)?;
        let Some(format) = self
            .offer
            .formats
            .iter()
            .find(|format| format.video == info.format())
        else {
            bail!("the consumer chose a format that was not offered");
        };
        let fourcc = drm_format(format.video).context("offered format without a fourcc")?;
        let size = (info.size().width, info.size().height);

        let modifier_prop = pod
            .as_object()?
            .find_prop(Id(FormatProperties::VideoModifier.as_raw()))
            .context("the format carries no modifier")?;

        if modifier_prop.flags().contains(PodPropFlags::DONT_FIXATE) {
            let (_, Choice(_, choice)) =
                PodDeserializer::deserialize_from::<Choice<i64>>(modifier_prop.value().as_bytes())
                    .map_err(|_| anyhow::anyhow!("the modifier is not a choice of longs"))?;
            let alternatives: Vec<u64> = match choice {
                ChoiceEnum::None(only) => vec![only as u64],
                ChoiceEnum::Enum {
                    default,
                    alternatives,
                } => std::iter::once(default)
                    .chain(alternatives)
                    .map(|modifier| modifier as u64)
                    .collect(),
                other => bail!("the modifier is offered as {other:?}, not a list"),
            };
            let preferred = self.allocator.prefer_single_plane(fourcc, &alternatives);
            let modifier = self.allocator.allocate(size, fourcc, &preferred)?.modifier;
            tracing::debug!(?alternatives, modifier, "pinned the modifier");
            return params::fixated(format, modifier, &self.offer);
        }

        let modifier = info.modifier();
        let probe = self.allocator.allocate(size, fourcc, &[modifier])?;
        tracing::debug!(fourcc, modifier, ?size, "the consumer fixed the format");
        let max_framerate = info.max_framerate();
        self.negotiated = Some(Negotiated {
            fourcc,
            modifier,
            size,
            max_fps_mhz: (max_framerate.num * 1000)
                .checked_div(max_framerate.denom)
                .unwrap_or(0),
        });
        params::buffers(u32::try_from(probe.planes.len())?)
    }

    fn add_buffer(&mut self, raw: *mut pw_buffer) -> anyhow::Result<()> {
        let raw = NonNull::new(raw).context("PipeWire added a null buffer")?;
        let negotiated = self
            .negotiated
            .context("a buffer arrived before the format")?;
        let allocation =
            self.allocator
                .allocate(negotiated.size, negotiated.fourcc, &[negotiated.modifier])?;
        // Safety: PipeWire hands over a buffer with `n_datas` data blocks, one
        // per plane as the Buffers param asked for, each writable until
        // remove_buffer.
        unsafe {
            let buffer = (*raw.as_ptr()).buffer;
            if (*buffer).n_datas as usize != allocation.planes.len() {
                bail!("PipeWire allocated the wrong number of planes");
            }
            for (index, plane) in allocation.planes.iter().enumerate() {
                let data = &mut *(*buffer).datas.add(index);
                // The dmabuf spec lets consumers ignore both sizes, but
                // GStreamer sizes its memory from them and OBS wants them
                // non-zero, so they carry the real extent.
                data.type_ = DataType::DmaBuf.as_raw();
                data.maxsize = allocation.size;
                data.fd = i64::from(plane.fd.as_raw_fd());
                data.flags = SPA_DATA_FLAG_READWRITE;
                let chunk = &mut *data.chunk;
                chunk.stride = i32::try_from(plane.stride)?;
                chunk.offset = plane.offset;
                chunk.size = allocation.size.saturating_sub(plane.offset);
            }
        }
        self.buffers.push(StreamBuffer { raw, allocation });
        self.generation += 1;
        Ok(())
    }
}

/// # Safety
///
/// `buffer` must point to a live `spa_buffer`.
unsafe fn header(buffer: *mut spa_buffer) -> Option<NonNull<spa_meta_header>> {
    // Safety: guaranteed by the caller; `metas` holds `n_metas` entries.
    unsafe {
        (0..(*buffer).n_metas as usize)
            .map(|index| &*(*buffer).metas.add(index))
            .find(|meta| {
                meta.type_ == SPA_META_Header && meta.size as usize >= size_of::<spa_meta_header>()
            })
            .and_then(|meta| NonNull::new(meta.data.cast()))
    }
}
