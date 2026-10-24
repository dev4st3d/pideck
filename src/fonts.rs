//! Embedded typography.

use std::borrow::Cow;
use std::env;
use std::path::{Path, PathBuf};

use gpui::{App, SharedString, px};

const DEFAULT_MONO: &str = crate::services::terminal_font::DEFAULT_FAMILY;
pub(crate) struct FontCatalog {
    pub(crate) settings_path: PathBuf,
}

pub(crate) fn initialize(cx: &App) -> FontCatalog {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!(
            "../assets/embedded-fonts/DMSans-Regular.ttf"
        )),
        Cow::Borrowed(include_bytes!("../assets/embedded-fonts/DMSans-Medium.ttf")),
        Cow::Borrowed(include_bytes!(
            "../assets/embedded-fonts/DMSans-SemiBold.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../assets/embedded-fonts/InstrumentSerif-Regular.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../assets/embedded-fonts/IBMPlexMono-Regular.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../assets/embedded-fonts/IBMPlexMono-Medium.ttf"
        )),
    ];
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        eprintln!("Zeod design fonts could not be registered: {error}");
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
                eprintln!("Zeod could not register its bundled font family: {family}");
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

/// Name fragments that make a family worth probing; the width check decides.
const FIXED_PITCH_HINTS: [&str; 11] = [
    "mono",
    "code",
    "consol",
    "courier",
    "terminal",
    "typewriter",
    "fixed",
    "menlo",
    "monaco",
    "hack",
    "iosevka",
];

/// The bundled face first, then installed families that keep a fixed cell width.
pub(crate) fn terminal_families(cx: &App) -> Vec<SharedString> {
    let system = cx.text_system();
    let mut families: Vec<SharedString> = vec![DEFAULT_MONO.into()];
    for name in system.all_font_names() {
        let lower = name.to_lowercase();
        if name.starts_with('.')
            || name == DEFAULT_MONO
            || !FIXED_PITCH_HINTS.iter().any(|hint| lower.contains(hint))
        {
            continue;
        }
        // A grid needs equal advances; compare the narrowest and widest common glyphs.
        let id = system.resolve_font(&gpui::font(name.clone()));
        let probe = px(100.0);
        let fixed = match (
            system.advance(id, probe, 'i'),
            system.advance(id, probe, 'M'),
        ) {
            (Ok(narrow), Ok(wide)) => f32::from(narrow.width - wide.width).abs() < 0.5,
            _ => false,
        };
        if fixed {
            families.push(name.into());
        }
    }
    families
}

/// Whether the platform still offers this family, so a missing font never reaches the grid.
pub(crate) fn is_installed(cx: &App, family: &str) -> bool {
    family == DEFAULT_MONO
        || cx
            .text_system()
            .all_font_names()
            .iter()
            .any(|name| name == family)
}

/// New installs use `Zeod`. A `ZiDeck` or `Pideck` folder from before a rename
/// stays in use until a `Zeod` one exists, so settings, layout, and shortcuts carry over.
fn app_data_dir(root: &Path) -> PathBuf {
    let current = root.join("Zeod");
    if current.exists() {
        return current;
    }
    ["ZiDeck", "Pideck"]
        .into_iter()
        .map(|name| root.join(name))
        .find(|legacy| legacy.is_dir())
        .unwrap_or(current)
}

fn settings_path() -> PathBuf {
    if let Some(path) = env::var_os("ZEOD_SETTINGS_PATH") {
        return PathBuf::from(path);
    }
    if let Some(root) = env::var_os("APPDATA") {
        return app_data_dir(Path::new(&root)).join("settings.json");
    }
    if let Some(root) = env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(root).join("zeod").join("settings.json");
    }
    if let Some(root) = env::var_os("HOME") {
        return PathBuf::from(root)
            .join(".config")
            .join("zeod")
            .join("settings.json");
    }
    PathBuf::from("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_data_keeps_a_pre_rename_folder_until_a_new_one_exists() {
        let root = env::temp_dir().join(format!("zeod-app-data-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        assert_eq!(app_data_dir(&root), root.join("Zeod"));
        std::fs::create_dir(root.join("Pideck")).unwrap();
        assert_eq!(app_data_dir(&root), root.join("Pideck"));
        std::fs::create_dir(root.join("ZiDeck")).unwrap();
        assert_eq!(app_data_dir(&root), root.join("ZiDeck"));
        std::fs::create_dir(root.join("Zeod")).unwrap();
        assert_eq!(app_data_dir(&root), root.join("Zeod"));

        std::fs::remove_dir_all(&root).unwrap();
    }

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
                include_bytes!("../assets/embedded-fonts/DMSans-Regular.ttf").as_slice(),
                "DM Sans 9pt",
            ),
            (
                include_bytes!("../assets/embedded-fonts/DMSans-Medium.ttf").as_slice(),
                "DM Sans 9pt",
            ),
            (
                include_bytes!("../assets/embedded-fonts/DMSans-SemiBold.ttf").as_slice(),
                "DM Sans 9pt",
            ),
            (
                include_bytes!("../assets/embedded-fonts/InstrumentSerif-Regular.ttf").as_slice(),
                crate::theme::terminal_manager::HEADING_FONT,
            ),
            (
                include_bytes!("../assets/embedded-fonts/IBMPlexMono-Regular.ttf").as_slice(),
                DEFAULT_MONO,
            ),
            (
                include_bytes!("../assets/embedded-fonts/IBMPlexMono-Medium.ttf").as_slice(),
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
