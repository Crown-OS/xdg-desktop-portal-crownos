//! The `a{sv}` dictionaries the ScreenCast methods take and return.

use zbus::zvariant::{DeserializeDict, OwnedValue, SerializeDict, Type};

use crate::cast::{Catalog, Source};

/// Identifies restore data this portal wrote; anything else is ignored.
pub const RESTORE_VENDOR: &str = "crownos";
/// Version 1 held an output name; version 2 any kind of source.
pub const RESTORE_VERSION: u32 = 2;
const RESTORE_VERSION_OUTPUT_ONLY: u32 = 1;

pub const SOURCE_MONITOR: u32 = 1;
pub const SOURCE_WINDOW: u32 = 2;
pub const CURSOR_HIDDEN: u32 = 1;
pub const CURSOR_EMBEDDED: u32 = 2;

pub type RestoreData = (String, u32, OwnedValue);

#[derive(Debug, Default, DeserializeDict, Type)]
#[zvariant(signature = "a{sv}")]
pub struct SelectSourcesOptions {
    pub types: Option<u32>,
    pub cursor_mode: Option<u32>,
    pub restore_data: Option<RestoreData>,
    pub persist_mode: Option<u32>,
}

#[derive(Debug, Default, SerializeDict, Type)]
#[zvariant(signature = "a{sv}")]
pub struct StartResults {
    pub streams: Option<Vec<(u32, StreamProperties)>>,
    pub persist_mode: Option<u32>,
    pub restore_data: Option<RestoreData>,
}

/// Position and size only describe monitors; a window's size is whatever
/// the stream negotiates.
#[derive(Debug, SerializeDict, Type)]
#[zvariant(signature = "a{sv}")]
pub struct StreamProperties {
    pub position: Option<(i32, i32)>,
    pub size: Option<(i32, i32)>,
    pub source_type: u32,
}

impl StreamProperties {
    pub fn of(source: &Source, catalog: &Catalog) -> Self {
        let monitor = |name: Option<&str>| {
            let output = name.and_then(|name| catalog.output(name));
            Self {
                position: output.map(|output| output.position),
                size: output.map(|output| output.logical_size()),
                source_type: SOURCE_MONITOR,
            }
        };
        match source {
            Source::Output(name) => monitor(Some(name)),
            Source::Workspace(id) => monitor(
                catalog
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.id == *id)
                    .and_then(|workspace| workspace.output.as_deref()),
            ),
            Source::Window(_) => Self {
                position: None,
                size: None,
                source_type: SOURCE_WINDOW,
            },
        }
    }
}

/// A choice an app asked to have remembered. Window and workspace
/// identifiers do not outlive the session, so they are remembered by what
/// the user would recognise them by instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remembered {
    Output(String),
    Window { app_id: String, title: String },
    Workspace { output: String, name: String },
}

/// Separates the fields of a remembered choice; it cannot occur in any of them.
const SEPARATOR: char = '\u{1f}';

impl Remembered {
    pub fn of(source: &Source, catalog: &Catalog) -> Option<Self> {
        match source {
            Source::Output(name) => Some(Self::Output(name.clone())),
            Source::Window(identifier) => catalog
                .windows
                .iter()
                .find(|window| window.identifier == *identifier)
                .map(|window| Self::Window {
                    app_id: window.app_id.clone(),
                    title: window.title.clone(),
                }),
            Source::Workspace(id) => catalog
                .workspaces
                .iter()
                .find(|workspace| workspace.id == *id)
                .map(|workspace| Self::Workspace {
                    output: workspace.output.clone().unwrap_or_default(),
                    name: workspace.name.clone(),
                }),
        }
    }

    /// The source this stands for now: an app's window with the same title
    /// if it still has one, else its first.
    pub fn resolve(&self, catalog: &Catalog) -> Option<Source> {
        match self {
            Self::Output(name) => catalog.output(name).map(|_| Source::Output(name.clone())),
            Self::Window { app_id, title } => {
                let windows = || {
                    catalog
                        .windows
                        .iter()
                        .filter(|window| window.app_id == *app_id)
                };
                windows()
                    .find(|window| window.title == *title)
                    .or_else(|| windows().next())
                    .map(|window| Source::Window(window.identifier.clone()))
            }
            Self::Workspace { output, name } => catalog
                .workspaces
                .iter()
                .find(|workspace| {
                    workspace.name == *name && workspace.output.as_deref().unwrap_or("") == output
                })
                .map(|workspace| Source::Workspace(workspace.id.clone())),
        }
    }

    pub fn encode(&self) -> Option<RestoreData> {
        let text = match self {
            Self::Output(name) => format!("output{SEPARATOR}{name}"),
            Self::Window { app_id, title } => {
                format!("window{SEPARATOR}{app_id}{SEPARATOR}{title}")
            }
            Self::Workspace { output, name } => {
                format!("workspace{SEPARATOR}{output}{SEPARATOR}{name}")
            }
        };
        let value = OwnedValue::try_from(zbus::zvariant::Value::from(text)).ok()?;
        Some((RESTORE_VENDOR.to_owned(), RESTORE_VERSION, value))
    }

    /// `None` for data another portal, or a garbled one, wrote.
    pub fn decode(data: &RestoreData) -> Option<Self> {
        let (vendor, version, value) = data;
        if vendor != RESTORE_VENDOR {
            return None;
        }
        let text = String::try_from(value.clone()).ok()?;
        if *version == RESTORE_VERSION_OUTPUT_ONLY {
            return Some(Self::Output(text));
        }
        if *version != RESTORE_VERSION {
            return None;
        }
        let mut fields = text.split(SEPARATOR);
        let (kind, first, second) = (fields.next()?, fields.next()?, fields.next());
        match (kind, second) {
            ("output", None) => Some(Self::Output(first.to_owned())),
            ("window", Some(title)) => Some(Self::Window {
                app_id: first.to_owned(),
                title: title.to_owned(),
            }),
            ("workspace", Some(name)) => Some(Self::Workspace {
                output: first.to_owned(),
                name: name.to_owned(),
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cast::{OutputInfo, WindowInfo, WorkspaceInfo};

    fn catalog() -> Catalog {
        Catalog {
            outputs: vec![OutputInfo {
                name: "DP-1".into(),
                mode: (3840, 2160),
                scale: 2,
                position: (0, 0),
                ..OutputInfo::default()
            }],
            windows: vec![
                WindowInfo {
                    identifier: "a".into(),
                    title: "Inbox".into(),
                    app_id: "mail".into(),
                },
                WindowInfo {
                    identifier: "b".into(),
                    title: "Draft".into(),
                    app_id: "mail".into(),
                },
            ],
            workspaces: vec![WorkspaceInfo {
                id: "ws-7".into(),
                name: "2".into(),
                output: Some("DP-1".into()),
                active: false,
            }],
        }
    }

    #[test]
    fn every_kind_of_choice_round_trips() {
        let catalog = catalog();
        for source in [
            Source::Output("DP-1".into()),
            Source::Window("b".into()),
            Source::Workspace("ws-7".into()),
        ] {
            let remembered = Remembered::of(&source, &catalog).expect("the source exists");
            let decoded = Remembered::decode(&remembered.encode().expect("encodable"))
                .expect("our own data decodes");
            assert_eq!(decoded.resolve(&catalog), Some(source));
        }
    }

    #[test]
    fn a_window_falls_back_to_another_of_its_app() {
        let remembered = Remembered::Window {
            app_id: "mail".into(),
            title: "Gone".into(),
        };
        assert_eq!(
            remembered.resolve(&catalog()),
            Some(Source::Window("a".into()))
        );
    }

    #[test]
    fn version_one_data_is_an_output() {
        let value = OwnedValue::try_from(zbus::zvariant::Value::from("DP-1")).unwrap();
        let data = (RESTORE_VENDOR.to_owned(), 1, value);
        assert_eq!(
            Remembered::decode(&data),
            Some(Remembered::Output("DP-1".into()))
        );
    }

    #[test]
    fn foreign_restore_data_is_ignored() {
        let data = Remembered::Output("DP-1".into()).encode().unwrap();
        assert_eq!(
            Remembered::decode(&("wlr".to_owned(), data.1, data.2)),
            None
        );
    }

    #[test]
    fn windows_have_no_monitor_geometry() {
        let properties = StreamProperties::of(&Source::Window("a".into()), &catalog());
        assert_eq!(properties.source_type, SOURCE_WINDOW);
        assert!(properties.size.is_none());
        let workspace = StreamProperties::of(&Source::Workspace("ws-7".into()), &catalog());
        assert_eq!(workspace.size, Some((1920, 1080)));
    }
}
