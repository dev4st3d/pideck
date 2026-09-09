use std::borrow::Cow;
use std::{collections::HashMap, path::Path, sync::LazyLock};

use gpui::{AssetSource, Result, SharedString};

const ICONS: [(&str, &[u8]); 18] = [
    ("branch.svg", include_bytes!("../assets/icons/branch.svg")),
    ("diff.svg", include_bytes!("../assets/icons/diff.svg")),
    (
        "chevron-left.svg",
        include_bytes!("../assets/icons/chevron-left.svg"),
    ),
    (
        "case-sensitive.svg",
        include_bytes!("../assets/icons/case-sensitive.svg"),
    ),
    (
        "circle-x.svg",
        include_bytes!("../assets/icons/circle-x.svg"),
    ),
    ("replace.svg", include_bytes!("../assets/icons/replace.svg")),
    (
        "chevron-down.svg",
        include_bytes!("../assets/icons/chevron-down.svg"),
    ),
    ("close.svg", include_bytes!("../assets/icons/close.svg")),
    ("file.svg", include_bytes!("../assets/icons/file.svg")),
    ("refresh.svg", include_bytes!("../assets/icons/refresh.svg")),
    (
        "window-min.svg",
        include_bytes!("../assets/icons/window-min.svg"),
    ),
    (
        "window-max.svg",
        include_bytes!("../assets/icons/window-max.svg"),
    ),
    (
        "window-close.svg",
        include_bytes!("../assets/icons/window-close.svg"),
    ),
    (
        "chevron-right.svg",
        include_bytes!("../assets/icons/chevron-right.svg"),
    ),
    ("folder.svg", include_bytes!("../assets/icons/folder.svg")),
    ("plus.svg", include_bytes!("../assets/icons/plus.svg")),
    ("sidebar.svg", include_bytes!("../assets/icons/sidebar.svg")),
    (
        "terminal.svg",
        include_bytes!("../assets/icons/terminal.svg"),
    ),
];

pub(crate) struct Assets;

#[derive(serde::Deserialize)]
struct IconBundle {
    theme: serde_json::Value,
    icons: HashMap<String, String>,
}

static CATPPUCCIN: LazyLock<IconBundle> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../assets/catppuccin/latte.json"))
        .expect("bundled Catppuccin icon data must be valid")
});

pub(crate) fn project_icon(path: &Path, directory: bool, expanded: bool) -> SharedString {
    let theme = &CATPPUCCIN.theme;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let icon = if directory {
        let state = if expanded { "expanded" } else { "collapsed" };
        theme["named_directory_icons"][name][state]
            .as_str()
            .or_else(|| theme["directory_icons"][state].as_str())
    } else {
        let stem = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or(name);
        let mut key = theme["file_stems"][name]
            .as_str()
            .or_else(|| theme["file_stems"][stem].as_str());
        let mut suffix = name;
        while key.is_none() {
            key = theme["file_suffixes"][suffix].as_str();
            let Some((_, rest)) = suffix.split_once('.') else {
                break;
            };
            suffix = rest;
        }
        key.and_then(|key| theme["file_icons"][key]["path"].as_str())
    };
    icon.map(|icon| format!("catppuccin/{icon}").into())
        .unwrap_or_else(|| "icons/file.svg".into())
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(path) = path.strip_prefix("catppuccin/") {
            return Ok(CATPPUCCIN
                .icons
                .get(path)
                .map(|svg| Cow::Borrowed(svg.as_bytes())));
        }
        Ok(path.strip_prefix("icons/").and_then(|name| {
            ICONS
                .iter()
                .find(|(icon, _)| *icon == name)
                .map(|(_, bytes)| Cow::Borrowed(*bytes))
        }))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(if path == "icons" {
            ICONS.iter().map(|(name, _)| (*name).into()).collect()
        } else {
            Vec::new()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_theme_resolves_files_and_both_folder_states() {
        for (name, directory) in [
            ("src", true),
            ("arbitrary-folder", true),
            ("main.rs", false),
            ("README.md", false),
            ("file.unrecognized", false),
        ] {
            for expanded in [false, true] {
                let path = project_icon(Path::new(name), directory, expanded);
                assert!(Assets.load(&path).unwrap().is_some(), "{path}");
            }
        }
        assert_ne!(
            project_icon(Path::new("src"), true, true),
            project_icon(Path::new("src"), true, false)
        );
    }
}
