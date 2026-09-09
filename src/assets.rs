use std::borrow::Cow;

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

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
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
