//! Embedded typography from the supplied Pideck design references.

use std::borrow::Cow;
use std::env;
use std::path::PathBuf;

use gpui::{App, SharedString};

const DEFAULT_MONO: &str = "IBM Plex Mono";
pub(crate) struct FontCatalog {
    pub(crate) settings_path: PathBuf,
}

pub(crate) fn initialize(cx: &App) -> FontCatalog {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../design/fonts/DMSans-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../design/fonts/DMSans-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../design/fonts/DMSans-SemiBold.ttf")),
        Cow::Borrowed(include_bytes!(
            "../design/fonts/InstrumentSerif-Regular.ttf"
        )),
        Cow::Borrowed(include_bytes!("../design/fonts/IBMPlexMono-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../design/fonts/IBMPlexMono-Medium.ttf")),
    ];
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        eprintln!("Pideck design fonts could not be registered: {error}");
    }
    // The headless test platform has no font collection. On the native platform,
    // Check the registered collection, not resolve_font's requested descriptor:
    // DirectWrite can silently substitute Segoe UI while retaining that descriptor.
    #[cfg(not(test))]
    {
        let registered = cx.text_system().all_font_names();
        for family in [
            crate::theme::terminal_manager::CHROME_FONT,
            crate::theme::terminal_manager::HEADING_FONT,
            DEFAULT_MONO,
        ] {
            if !registered.iter().any(|name| name == family) {
                eprintln!("Pideck could not register its bundled font family: {family}");
            }
        }
    }
    let settings_path = settings_path();
    FontCatalog { settings_path }
}

pub(crate) fn mono() -> SharedString {
    // Legacy font overrides must not replace the supplied design faces.
    DEFAULT_MONO.into()
}

fn settings_path() -> PathBuf {
    if let Some(path) = env::var_os("PI_GUI_SETTINGS_PATH") {
        return PathBuf::from(path);
    }
    if let Some(root) = env::var_os("APPDATA") {
        return PathBuf::from(root).join("Pideck").join("settings.json");
    }
    if let Some(root) = env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(root).join("pideck").join("settings.json");
    }
    if let Some(root) = env::var_os("HOME") {
        return PathBuf::from(root)
            .join(".config")
            .join("pideck")
            .join("settings.json");
    }
    PathBuf::from("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    // GPUI's headless platform uses NoopTextSystem. Check the actual SFNT
    // family records here; the native startup check below covers DirectWrite.
    fn family_names(bytes: &[u8]) -> Vec<String> {
        let u16_at = |offset| u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
        let u32_at = |offset| {
            u32::from_be_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ]) as usize
        };
        let table = (0..u16_at(4))
            .map(|index| 12 + index * 16)
            .find(|offset| &bytes[*offset..*offset + 4] == b"name")
            .map(|offset| u32_at(offset + 8))
            .unwrap();
        let storage = table + u16_at(table + 4);
        (0..u16_at(table + 2))
            .filter_map(|index| {
                let record = table + 6 + index * 12;
                if u16_at(record) != 3 || ![1, 16].contains(&u16_at(record + 6)) {
                    return None;
                }
                let start = storage + u16_at(record + 10);
                let end = start + u16_at(record + 8);
                let utf16: Vec<_> = bytes[start..end]
                    .chunks_exact(2)
                    .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                    .collect();
                Some(String::from_utf16(&utf16).unwrap())
            })
            .collect()
    }

    #[test]
    fn embedded_faces_have_the_requested_family_names() {
        for (bytes, family) in [
            (
                include_bytes!("../design/fonts/DMSans-Regular.ttf").as_slice(),
                "DM Sans 9pt",
            ),
            (
                include_bytes!("../design/fonts/DMSans-Medium.ttf").as_slice(),
                "DM Sans 9pt",
            ),
            (
                include_bytes!("../design/fonts/DMSans-SemiBold.ttf").as_slice(),
                "DM Sans 9pt",
            ),
            (
                include_bytes!("../design/fonts/InstrumentSerif-Regular.ttf").as_slice(),
                crate::theme::terminal_manager::HEADING_FONT,
            ),
            (
                include_bytes!("../design/fonts/IBMPlexMono-Regular.ttf").as_slice(),
                DEFAULT_MONO,
            ),
            (
                include_bytes!("../design/fonts/IBMPlexMono-Medium.ttf").as_slice(),
                DEFAULT_MONO,
            ),
        ] {
            assert!(
                family_names(bytes).iter().any(|name| name == family),
                "font family mismatch: {family}"
            );
        }
    }
}
