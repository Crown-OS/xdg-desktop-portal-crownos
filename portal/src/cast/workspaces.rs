//! The compositor's workspaces, from `ext-workspace`, and the output each
//! one's group is on.

use wayland_client::protocol::wl_output::WlOutput;
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1},
    ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1, State},
};

use super::WorkspaceInfo;

struct Group {
    proxy: ExtWorkspaceGroupHandleV1,
    output: Option<WlOutput>,
    workspaces: Vec<ExtWorkspaceHandleV1>,
}

struct Tracked {
    proxy: ExtWorkspaceHandleV1,
    id: String,
    name: String,
    active: bool,
}

#[derive(Default)]
pub(super) struct Workspaces {
    groups: Vec<Group>,
    tracked: Vec<Tracked>,
}

impl Workspaces {
    pub(super) fn add_group(&mut self, proxy: ExtWorkspaceGroupHandleV1) {
        self.groups.push(Group {
            proxy,
            output: None,
            workspaces: Vec::new(),
        });
    }

    pub(super) fn add(&mut self, proxy: ExtWorkspaceHandleV1) {
        self.tracked.push(Tracked {
            proxy,
            id: String::new(),
            name: String::new(),
            active: false,
        });
    }

    /// `output_name` names a `wl_output` proxy, so groups can be reported by
    /// the output the chooser shows.
    pub(super) fn infos(
        &self,
        output_name: impl Fn(&WlOutput) -> Option<String>,
    ) -> Vec<WorkspaceInfo> {
        self.tracked
            .iter()
            .filter(|workspace| !workspace.id.is_empty())
            .map(|workspace| WorkspaceInfo {
                id: workspace.id.clone(),
                name: workspace.name.clone(),
                output: self
                    .groups
                    .iter()
                    .find(|group| group.workspaces.contains(&workspace.proxy))
                    .and_then(|group| group.output.as_ref())
                    .and_then(&output_name),
                active: workspace.active,
            })
            .collect()
    }

    pub(super) fn apply_group(
        &mut self,
        proxy: &ExtWorkspaceGroupHandleV1,
        event: ext_workspace_group_handle_v1::Event,
    ) {
        use ext_workspace_group_handle_v1::Event;

        let Some(index) = self.groups.iter().position(|group| group.proxy == *proxy) else {
            return;
        };
        let group = &mut self.groups[index];
        match event {
            Event::OutputEnter { output } => group.output = Some(output),
            Event::OutputLeave { output } if group.output.as_ref() == Some(&output) => {
                group.output = None;
            }
            Event::WorkspaceEnter { workspace } => group.workspaces.push(workspace),
            Event::WorkspaceLeave { workspace } => {
                group.workspaces.retain(|known| *known != workspace);
            }
            Event::Removed => {
                proxy.destroy();
                self.groups.swap_remove(index);
            }
            _ => {}
        }
    }

    pub(super) fn apply(
        &mut self,
        proxy: &ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
    ) {
        use ext_workspace_handle_v1::Event;

        let Some(index) = self
            .tracked
            .iter()
            .position(|workspace| workspace.proxy == *proxy)
        else {
            return;
        };
        let workspace = &mut self.tracked[index];
        match event {
            Event::Id { id } => workspace.id = id,
            Event::Name { name } => workspace.name = name,
            Event::State { state } => {
                workspace.active = state
                    .into_result()
                    .is_ok_and(|state| state.contains(State::Active));
            }
            Event::Removed => {
                proxy.destroy();
                self.tracked.swap_remove(index);
            }
            _ => {}
        }
    }
}
