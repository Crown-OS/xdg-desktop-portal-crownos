//! The name an app goes by, from its desktop entry.

use std::path::PathBuf;

const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";

/// `Name=` of `<app_id>.desktop` in the first data directory that has one.
pub fn display_name(app_id: &str) -> Option<String> {
    if app_id.is_empty() || app_id.contains('/') {
        return None;
    }
    let file = format!("applications/{app_id}.desktop");
    data_dirs()
        .into_iter()
        .find_map(|dir| std::fs::read_to_string(dir.join(&file)).ok())
        .and_then(|entry| name_of(&entry).map(str::to_owned))
}

fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| DEFAULT_DATA_DIRS.to_owned());
    home.into_iter()
        .chain(system.split(':').map(PathBuf::from))
        .collect()
}

fn name_of(entry: &str) -> Option<&str> {
    entry
        .lines()
        .skip_while(|line| line.trim() != "[Desktop Entry]")
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .find_map(|line| line.strip_prefix("Name="))
        .map(str::trim)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_comes_from_the_desktop_entry_group() {
        let entry =
            "[Desktop Entry]\nName[de]=Feuerfuchs\nName=Firefox\n[Desktop Action new]\nName=New";
        assert_eq!(name_of(entry), Some("Firefox"));
    }

    #[test]
    fn other_groups_are_not_read() {
        assert_eq!(name_of("[Desktop Action new]\nName=New"), None);
    }
}
