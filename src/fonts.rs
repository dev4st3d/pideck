//! System-font discovery and persisted typography preferences.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use gpui::{App, SharedString};
use serde::{Deserialize, Serialize};

const SETTINGS_FILE: &str = "settings.json";
const DEFAULT_MAIN: &str = "Segoe UI";
const DEFAULT_SANS: &str = "Segoe UI";
const DEFAULT_MONO: &str = "Cascadia Mono";

static ACTIVE: OnceLock<RwLock<FontPreferences>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontRole {
    Main,
    Sans,
    Mono,
}

impl FontRole {
    pub const ALL: [Self; 3] = [Self::Main, Self::Sans, Self::Mono];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Main => "Main",
            Self::Sans => "Sans",
            Self::Mono => "Mono",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Main => "Wordmark, controls, and display text",
            Self::Sans => "Conversation and interface text",
            Self::Mono => "Code, commands, and technical data",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FontPreferences {
    pub main: String,
    pub sans: String,
    pub mono: String,
}

impl Default for FontPreferences {
    fn default() -> Self {
        Self {
            main: DEFAULT_MAIN.to_owned(),
            sans: DEFAULT_SANS.to_owned(),
            mono: DEFAULT_MONO.to_owned(),
        }
    }
}

impl FontPreferences {
    pub fn family(&self, role: FontRole) -> &str {
        match role {
            FontRole::Main => &self.main,
            FontRole::Sans => &self.sans,
            FontRole::Mono => &self.mono,
        }
    }

    pub fn set(&mut self, role: FontRole, family: impl Into<String>) {
        let family = family.into();
        match role {
            FontRole::Main => self.main = family,
            FontRole::Sans => self.sans = family,
            FontRole::Mono => self.mono = family,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FontCatalog {
    pub families: Vec<String>,
    pub preferences: FontPreferences,
    pub settings_path: PathBuf,
    pub load_warning: Option<String>,
}

pub fn initialize(cx: &App) -> FontCatalog {
    let mut families = cx.text_system().all_font_names();
    sort_and_deduplicate(&mut families);

    let settings_path = settings_path();
    let (mut preferences, load_warning) = match load(&settings_path) {
        Ok(preferences) => (preferences, None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => (FontPreferences::default(), None),
        Err(error) => (
            FontPreferences::default(),
            Some(format!("Font settings could not be loaded: {error}")),
        ),
    };
    apply_available_defaults(&mut preferences, &families);
    install(preferences.clone());

    FontCatalog {
        families,
        preferences,
        settings_path,
        load_warning,
    }
}

pub fn install(preferences: FontPreferences) {
    let lock = ACTIVE.get_or_init(|| RwLock::new(FontPreferences::default()));
    if let Ok(mut active) = lock.write() {
        *active = preferences;
    }
}

pub fn family(role: FontRole) -> SharedString {
    ACTIVE
        .get_or_init(|| RwLock::new(FontPreferences::default()))
        .read()
        .map(|preferences| SharedString::from(preferences.family(role).to_owned()))
        .unwrap_or_else(|_| SharedString::from(FontPreferences::default().family(role).to_owned()))
}

pub fn save(path: &Path, preferences: &FontPreferences) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(preferences)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes)?;
    if let Err(error) = fs::rename(&temporary, path) {
        if path.exists() {
            fs::remove_file(path)?;
            fs::rename(&temporary, path)
        } else {
            Err(error)
        }
    } else {
        Ok(())
    }
}

fn load(path: &Path) -> io::Result<FontPreferences> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn settings_path() -> PathBuf {
    if let Some(path) = env::var_os("PI_GUI_SETTINGS_PATH") {
        return PathBuf::from(path);
    }
    if let Some(root) = env::var_os("APPDATA") {
        return PathBuf::from(root).join("Pideck").join(SETTINGS_FILE);
    }
    if let Some(root) = env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(root).join("pideck").join(SETTINGS_FILE);
    }
    if let Some(root) = env::var_os("HOME") {
        return PathBuf::from(root)
            .join(".config")
            .join("pideck")
            .join(SETTINGS_FILE);
    }
    PathBuf::from(SETTINGS_FILE)
}

fn sort_and_deduplicate(families: &mut Vec<String>) {
    families.sort_by_key(|family| family.to_lowercase());
    let mut seen = HashSet::new();
    families.retain(|family| seen.insert(family.to_lowercase()));
}

fn apply_available_defaults(preferences: &mut FontPreferences, families: &[String]) {
    let first = families
        .first()
        .map(String::as_str)
        .unwrap_or(".SystemUIFont");
    for role in FontRole::ALL {
        let available = families
            .iter()
            .any(|family| family.eq_ignore_ascii_case(preferences.family(role)));
        if !available {
            preferences.set(role, default_for(role, families).unwrap_or(first));
        }
    }
}

fn default_for(role: FontRole, families: &[String]) -> Option<&str> {
    let preferred = match role {
        FontRole::Main | FontRole::Sans => ["Segoe UI", "SF Pro Text", "Noto Sans", "Arial"],
        FontRole::Mono => ["Cascadia Mono", "SF Mono", "Noto Sans Mono", "Consolas"],
    };
    preferred.into_iter().find_map(|candidate| {
        families
            .iter()
            .find(|family| family.eq_ignore_ascii_case(candidate))
            .map(String::as_str)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_round_trip() {
        let root = env::temp_dir().join(format!("pideck-fonts-{}", std::process::id()));
        let path = root.join("settings.json");
        let preferences = FontPreferences {
            main: "Georgia".to_owned(),
            sans: "Segoe UI".to_owned(),
            mono: "Consolas".to_owned(),
        };

        save(&path, &preferences).unwrap();
        assert_eq!(load(&path).unwrap(), preferences);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn catalog_names_are_case_insensitively_unique() {
        let mut names = vec![
            "Segoe UI".to_owned(),
            "consolas".to_owned(),
            "Consolas".to_owned(),
        ];
        sort_and_deduplicate(&mut names);
        assert_eq!(names, ["consolas", "Segoe UI"]);
    }

    #[test]
    fn blank_values_receive_available_defaults() {
        let mut preferences = FontPreferences {
            main: String::new(),
            sans: String::new(),
            mono: String::new(),
        };
        apply_available_defaults(
            &mut preferences,
            &["Consolas".to_owned(), "Segoe UI".to_owned()],
        );
        assert_eq!(preferences.main, "Segoe UI");
        assert_eq!(preferences.sans, "Segoe UI");
        assert_eq!(preferences.mono, "Consolas");
    }
}
