//! Embedded typography from the supplied Pideck design references.

use std::borrow::Cow;
use std::env;
use std::path::PathBuf;

use gpui::{App, SharedString};

const DEFAULT_MONO: &str = "JetBrains Mono";
pub(crate) struct FontCatalog {
    pub(crate) settings_path: PathBuf,
}

pub(crate) fn initialize(cx: &App) -> FontCatalog {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../design/fonts/Geist-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../design/fonts/Geist-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../design/fonts/Newsreader16pt-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../design/fonts/Newsreader16pt-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../design/fonts/JetBrainsMono-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../design/fonts/JetBrainsMono-Medium.ttf")),
    ];
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        eprintln!("Pideck design fonts could not be registered: {error}");
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
