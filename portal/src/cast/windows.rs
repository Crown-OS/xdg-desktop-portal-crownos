//! The compositor's windows, from `ext-foreign-toplevel-list`.

use wayland_client::Proxy;
use wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::{
    self, ExtForeignToplevelHandleV1,
};

use super::WindowInfo;

struct Tracked {
    proxy: ExtForeignToplevelHandleV1,
    info: WindowInfo,
    pending: WindowInfo,
}

#[derive(Default)]
pub(super) struct Windows {
    tracked: Vec<Tracked>,
}

impl Windows {
    pub(super) fn add(&mut self, proxy: ExtForeignToplevelHandleV1) {
        self.tracked.push(Tracked {
            proxy,
            info: WindowInfo::default(),
            pending: WindowInfo::default(),
        });
    }

    /// Windows that have finished describing themselves at least once.
    pub(super) fn infos(&self) -> Vec<WindowInfo> {
        self.tracked
            .iter()
            .filter(|window| !window.info.identifier.is_empty())
            .map(|window| window.info.clone())
            .collect()
    }

    pub(super) fn apply(
        &mut self,
        proxy: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
    ) {
        use ext_foreign_toplevel_handle_v1::Event;

        let Some(index) = self
            .tracked
            .iter()
            .position(|window| window.proxy == *proxy)
        else {
            return;
        };
        let window = &mut self.tracked[index];
        match event {
            Event::Identifier { identifier } => window.pending.identifier = identifier,
            Event::Title { title } => window.pending.title = title,
            Event::AppId { app_id } => window.pending.app_id = app_id,
            Event::Done => window.info = window.pending.clone(),
            Event::Closed => {
                if proxy.is_alive() {
                    proxy.destroy();
                }
                self.tracked.swap_remove(index);
            }
            _ => {}
        }
    }
}
