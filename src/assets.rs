use std::borrow::Cow;
use std::path::Path;

use gpui::{AssetSource, Result, SharedString};

const ICONS: [(&str, &[u8]); 48] = [
    (
        "folder-open.svg",
        include_bytes!("../assets/icons/folder-open.svg"),
    ),
    ("files.svg", include_bytes!("../assets/icons/files.svg")),
    (
        "file-code.svg",
        include_bytes!("../assets/icons/file-code.svg"),
    ),
    (
        "collapse-all.svg",
        include_bytes!("../assets/icons/collapse-all.svg"),
    ),
    (
        "agent-diamond.svg",
        include_bytes!("../assets/icons/agent-diamond.svg"),
    ),
    (
        "agent-nodes.svg",
        include_bytes!("../assets/icons/agent-nodes.svg"),
    ),
    (
        "agent-ring.svg",
        include_bytes!("../assets/icons/agent-ring.svg"),
    ),
    (
        "agent-tiles.svg",
        include_bytes!("../assets/icons/agent-tiles.svg"),
    ),
    (
        "arrow-up.svg",
        include_bytes!("../assets/icons/arrow-up.svg"),
    ),
    ("branch.svg", include_bytes!("../assets/icons/branch.svg")),
    (
        "case-sensitive.svg",
        include_bytes!("../assets/icons/case-sensitive.svg"),
    ),
    ("check.svg", include_bytes!("../assets/icons/check.svg")),
    (
        "chevron-down.svg",
        include_bytes!("../assets/icons/chevron-down.svg"),
    ),
    (
        "chevron-left.svg",
        include_bytes!("../assets/icons/chevron-left.svg"),
    ),
    (
        "chevron-right.svg",
        include_bytes!("../assets/icons/chevron-right.svg"),
    ),
    (
        "chevron-up.svg",
        include_bytes!("../assets/icons/chevron-up.svg"),
    ),
    (
        "circle-x.svg",
        include_bytes!("../assets/icons/circle-x.svg"),
    ),
    ("close.svg", include_bytes!("../assets/icons/close.svg")),
    ("cog.svg", include_bytes!("../assets/icons/cog.svg")),
    ("diff.svg", include_bytes!("../assets/icons/diff.svg")),
    ("expand.svg", include_bytes!("../assets/icons/expand.svg")),
    (
        "external.svg",
        include_bytes!("../assets/icons/external.svg"),
    ),
    ("file.svg", include_bytes!("../assets/icons/file.svg")),
    ("folder.svg", include_bytes!("../assets/icons/folder.svg")),
    ("image.svg", include_bytes!("../assets/icons/image.svg")),
    ("info.svg", include_bytes!("../assets/icons/info.svg")),
    (
        "inspector.svg",
        include_bytes!("../assets/icons/inspector.svg"),
    ),
    ("link.svg", include_bytes!("../assets/icons/link.svg")),
    ("moon.svg", include_bytes!("../assets/icons/moon.svg")),
    (
        "overflow.svg",
        include_bytes!("../assets/icons/overflow.svg"),
    ),
    (
        "paperclip.svg",
        include_bytes!("../assets/icons/paperclip.svg"),
    ),
    ("pencil.svg", include_bytes!("../assets/icons/pencil.svg")),
    ("plus.svg", include_bytes!("../assets/icons/plus.svg")),
    (
        "projects.svg",
        include_bytes!("../assets/icons/projects.svg"),
    ),
    (
        "queue-return.svg",
        include_bytes!("../assets/icons/queue-return.svg"),
    ),
    ("refresh.svg", include_bytes!("../assets/icons/refresh.svg")),
    ("replace.svg", include_bytes!("../assets/icons/replace.svg")),
    ("search.svg", include_bytes!("../assets/icons/search.svg")),
    (
        "sessions.svg",
        include_bytes!("../assets/icons/sessions.svg"),
    ),
    ("sidebar.svg", include_bytes!("../assets/icons/sidebar.svg")),
    (
        "stop-square.svg",
        include_bytes!("../assets/icons/stop-square.svg"),
    ),
    ("sun.svg", include_bytes!("../assets/icons/sun.svg")),
    (
        "terminal.svg",
        include_bytes!("../assets/icons/terminal.svg"),
    ),
    ("trash.svg", include_bytes!("../assets/icons/trash.svg")),
    ("undo.svg", include_bytes!("../assets/icons/undo.svg")),
    (
        "window-close.svg",
        include_bytes!("../assets/icons/window-close.svg"),
    ),
    (
        "window-max.svg",
        include_bytes!("../assets/icons/window-max.svg"),
    ),
    (
        "window-min.svg",
        include_bytes!("../assets/icons/window-min.svg"),
    ),
];

pub(crate) struct Assets;

pub(crate) fn project_icon(path: &Path, directory: bool, expanded: bool) -> &'static str {
    if directory {
        if expanded {
            "icons/folder-open.svg"
        } else {
            "icons/folder.svg"
        }
    } else if path.extension().is_some_and(|ext| {
        matches!(
            ext.to_str(),
            Some("rs" | "tsx" | "ts" | "jsx" | "js" | "json" | "toml")
        )
    }) {
        "icons/file-code.svg"
    } else {
        "icons/file.svg"
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_ui_icon_is_bundled() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons");
        let sources: BTreeSet<_> = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "svg"))
            .map(|path| path.file_name().unwrap().to_str().unwrap().to_owned())
            .collect();
        let bundled: BTreeSet<_> = ICONS.iter().map(|(name, _)| (*name).to_owned()).collect();
        assert_eq!(bundled.len(), ICONS.len(), "duplicate icon names");
        assert_eq!(sources, bundled, "the asset bundle must include every SVG");
        for name in bundled {
            assert!(Assets.load(&format!("icons/{name}")).unwrap().is_some());
        }
    }

    #[gpui::test]
    fn ui_icons_rasterize_with_clear_edges_at_supported_sizes(cx: &mut gpui::TestAppContext) {
        let renderer = cx.update(|cx| cx.svg_renderer());
        for (name, bytes) in ICONS {
            let source = std::str::from_utf8(bytes).unwrap();
            for logical_size in [12, 16, 20, 24, 32] {
                // Match GPUI's 2x SVG alpha-mask rasterization, including picker sizes.
                let size = logical_size * 2;
                let scaled = source
                    .replacen("width=\"16\"", &format!("width=\"{size}\""), 1)
                    .replacen("height=\"16\"", &format!("height=\"{size}\""), 1);
                let image = gpui::Image::from_bytes(gpui::ImageFormat::Svg, scaled.into_bytes())
                    .to_image_data(renderer.clone())
                    .unwrap_or_else(|error| panic!("{name} at {logical_size}px: {error}"));
                assert_eq!(image.size(0).width.0, size);
                assert_eq!(image.size(0).height.0, size);
                let pixels = image.as_bytes(0).unwrap();
                assert!(
                    pixels.chunks_exact(4).any(|pixel| pixel[3] > 0),
                    "{name} at {logical_size}px is blank"
                );
                for (index, pixel) in pixels.chunks_exact(4).enumerate() {
                    let x = index % size as usize;
                    let y = index / size as usize;
                    if x == 0 || y == 0 || x == size as usize - 1 || y == size as usize - 1 {
                        assert_eq!(pixel[3], 0, "{name} at {logical_size}px touches an edge");
                    }
                }
            }
        }
    }

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
