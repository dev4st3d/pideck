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
            "icons/chevron-down.svg" => &include_bytes!("../assets/icons/chevron-down.svg")[..],
            "icons/chevron-left.svg" => &include_bytes!("../assets/icons/chevron-left.svg")[..],
            "icons/chevron-right.svg" => &include_bytes!("../assets/icons/chevron-right.svg")[..],
            "icons/chevron-up.svg" => &include_bytes!("../assets/icons/chevron-up.svg")[..],
            "icons/cog.svg" => &include_bytes!("../assets/icons/cog.svg")[..],
            "icons/diff.svg" => &include_bytes!("../assets/icons/diff.svg")[..],
            "icons/expand.svg" => &include_bytes!("../assets/icons/expand.svg")[..],
            "icons/folder.svg" => &include_bytes!("../assets/icons/folder.svg")[..],
            "icons/info.svg" => &include_bytes!("../assets/icons/info.svg")[..],
            "icons/inspector.svg" => &include_bytes!("../assets/icons/inspector.svg")[..],
            "icons/moon.svg" => &include_bytes!("../assets/icons/moon.svg")[..],
            "icons/pencil.svg" => &include_bytes!("../assets/icons/pencil.svg")[..],
            "icons/sidebar.svg" => &include_bytes!("../assets/icons/sidebar.svg")[..],
            "icons/sun.svg" => &include_bytes!("../assets/icons/sun.svg")[..],
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
                "chevron-down.svg".into(),
                "chevron-left.svg".into(),
                "chevron-right.svg".into(),
                "chevron-up.svg".into(),
                "cog.svg".into(),
                "diff.svg".into(),
                "expand.svg".into(),
                "folder.svg".into(),
                "info.svg".into(),
                "inspector.svg".into(),
                "moon.svg".into(),
                "pencil.svg".into(),
                "sidebar.svg".into(),
                "sun.svg".into(),
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
