use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let bytes = match path {
            "icons/agent-diamond.svg" => &include_bytes!("../assets/icons/agent-diamond.svg")[..],
            "icons/agent-nodes.svg" => &include_bytes!("../assets/icons/agent-nodes.svg")[..],
            "icons/agent-ring.svg" => &include_bytes!("../assets/icons/agent-ring.svg")[..],
            "icons/agent-tiles.svg" => &include_bytes!("../assets/icons/agent-tiles.svg")[..],
            "icons/arrow-up.svg" => &include_bytes!("../assets/icons/arrow-up.svg")[..],
            "icons/chevron-down.svg" => &include_bytes!("../assets/icons/chevron-down.svg")[..],
            "icons/chevron-left.svg" => &include_bytes!("../assets/icons/chevron-left.svg")[..],
            "icons/chevron-right.svg" => &include_bytes!("../assets/icons/chevron-right.svg")[..],
            "icons/chevron-up.svg" => &include_bytes!("../assets/icons/chevron-up.svg")[..],
            "icons/check.svg" => &include_bytes!("../assets/icons/check.svg")[..],
            "icons/close.svg" => &include_bytes!("../assets/icons/close.svg")[..],
            "icons/cog.svg" => &include_bytes!("../assets/icons/cog.svg")[..],
            "icons/diff.svg" => &include_bytes!("../assets/icons/diff.svg")[..],
            "icons/expand.svg" => &include_bytes!("../assets/icons/expand.svg")[..],
            "icons/file.svg" => &include_bytes!("../assets/icons/file.svg")[..],
            "icons/folder.svg" => &include_bytes!("../assets/icons/folder.svg")[..],
            "icons/image.svg" => &include_bytes!("../assets/icons/image.svg")[..],
            "icons/info.svg" => &include_bytes!("../assets/icons/info.svg")[..],
            "icons/inspector.svg" => &include_bytes!("../assets/icons/inspector.svg")[..],
            "icons/link.svg" => &include_bytes!("../assets/icons/link.svg")[..],
            "icons/moon.svg" => &include_bytes!("../assets/icons/moon.svg")[..],
            "icons/paperclip.svg" => &include_bytes!("../assets/icons/paperclip.svg")[..],
            "icons/pencil.svg" => &include_bytes!("../assets/icons/pencil.svg")[..],
            "icons/plus.svg" => &include_bytes!("../assets/icons/plus.svg")[..],
            "icons/refresh.svg" => &include_bytes!("../assets/icons/refresh.svg")[..],
            "icons/sidebar.svg" => &include_bytes!("../assets/icons/sidebar.svg")[..],
            "icons/sun.svg" => &include_bytes!("../assets/icons/sun.svg")[..],
            "icons/terminal.svg" => &include_bytes!("../assets/icons/terminal.svg")[..],
            "icons/trash.svg" => &include_bytes!("../assets/icons/trash.svg")[..],
            "icons/undo.svg" => &include_bytes!("../assets/icons/undo.svg")[..],
            _ => return Ok(None),
        };

        Ok(Some(Cow::Borrowed(bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(match path {
            "icons" => vec![
                "agent-diamond.svg".into(),
                "agent-nodes.svg".into(),
                "agent-ring.svg".into(),
                "agent-tiles.svg".into(),
                "arrow-up.svg".into(),
                "chevron-down.svg".into(),
                "chevron-left.svg".into(),
                "chevron-right.svg".into(),
                "chevron-up.svg".into(),
                "check.svg".into(),
                "close.svg".into(),
                "cog.svg".into(),
                "diff.svg".into(),
                "expand.svg".into(),
                "file.svg".into(),
                "folder.svg".into(),
                "image.svg".into(),
                "info.svg".into(),
                "inspector.svg".into(),
                "link.svg".into(),
                "moon.svg".into(),
                "paperclip.svg".into(),
                "pencil.svg".into(),
                "plus.svg".into(),
                "refresh.svg".into(),
                "sidebar.svg".into(),
                "sun.svg".into(),
                "terminal.svg".into(),
                "trash.svg".into(),
                "undo.svg".into(),
            ],
            _ => Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_icon_is_embedded() {
        let assets = Assets;
        for icon in assets.list("icons").unwrap() {
            let path = format!("icons/{icon}");
            assert!(assets.load(&path).unwrap().is_some(), "missing {path}");
        }
    }
}
