//! The compositor's outputs, as the chooser lists them and a cast finds them.

use wayland_client::protocol::wl_output::{self, Mode, WlOutput};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{Dispatch, QueueHandle, WEnum};

use super::OutputInfo;

/// Name, description and the other v4 events.
const OUTPUT_VERSION: u32 = 4;
const FALLBACK_REFRESH_MHZ: i32 = 60_000;

pub(super) struct TrackedOutput {
    pub(super) global: u32,
    pub(super) proxy: WlOutput,
    pub(super) info: OutputInfo,
    pending: OutputInfo,
}

#[derive(Default)]
pub(super) struct Outputs {
    tracked: Vec<TrackedOutput>,
}

impl Outputs {
    pub(super) fn bind<State>(
        &mut self,
        registry: &WlRegistry,
        global: u32,
        version: u32,
        queue: &QueueHandle<State>,
    ) where
        State: Dispatch<WlOutput, u32> + 'static,
    {
        let proxy = registry.bind(global, version.min(OUTPUT_VERSION), queue, global);
        self.tracked.push(TrackedOutput {
            global,
            proxy,
            info: OutputInfo::default(),
            pending: OutputInfo::default(),
        });
    }

    pub(super) fn remove(&mut self, global: u32) {
        self.tracked.retain(|output| {
            let keep = output.global != global;
            if !keep {
                output.proxy.release();
            }
            keep
        });
    }

    pub(super) fn find(&self, name: &str) -> Option<&TrackedOutput> {
        self.tracked.iter().find(|output| output.info.name == name)
    }

    pub(super) fn name_of(&self, proxy: &WlOutput) -> Option<&str> {
        self.tracked
            .iter()
            .find(|output| output.proxy == *proxy)
            .map(|output| output.info.name.as_str())
            .filter(|name| !name.is_empty())
    }

    pub(super) fn infos(&self) -> Vec<OutputInfo> {
        self.tracked
            .iter()
            .filter(|output| !output.info.name.is_empty())
            .map(|output| output.info.clone())
            .collect()
    }

    pub(super) fn apply(&mut self, global: u32, event: wl_output::Event) {
        let Some(output) = self
            .tracked
            .iter_mut()
            .find(|output| output.global == global)
        else {
            return;
        };
        let pending = &mut output.pending;
        match event {
            wl_output::Event::Geometry { x, y, model, .. } => {
                pending.position = (x, y);
                pending.model = model;
            }
            wl_output::Event::Mode {
                flags: WEnum::Value(flags),
                width,
                height,
                refresh,
            } if flags.contains(Mode::Current) => {
                pending.mode = (width, height);
                pending.refresh_mhz = if refresh > 0 {
                    refresh
                } else {
                    FALLBACK_REFRESH_MHZ
                };
            }
            wl_output::Event::Scale { factor } => pending.scale = factor.max(1),
            wl_output::Event::Name { name } => pending.name = name,
            wl_output::Event::Description { description } => pending.description = description,
            wl_output::Event::Done => output.info = output.pending.clone(),
            _ => {}
        }
    }
}
