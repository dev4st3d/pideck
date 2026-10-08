//! Rebindable shortcuts: stable identifiers, defaults, validation, and saved overrides.
//!
//! Bindings are GPUI keystroke strings. Connecting them to actions lives in
//! `views::shortcuts`, so this module stays free of view and action types.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::LazyLock;

use gpui::{App, Global, Keystroke};
use serde::{Deserialize, Serialize};

static DEFAULT_KEYMAP: LazyLock<Keymap> = LazyLock::new(Keymap::default);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Shortcut {
    AddProject,
    NewTerminal,
    CloseTerminal,
    NextTerminal,
    PreviousTerminal,
    NextProject,
    PreviousProject,
    ToggleProjects,
    ToggleChecklist,
    FocusProjects,
    OpenSettings,
    SaveFile,
    TerminalCopy,
    TerminalPaste,
    TerminalSelectAll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShortcutGroup {
    Workbench,
    Editor,
    Terminal,
}

impl ShortcutGroup {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Workbench => "Workbench",
            Self::Editor => "Editor",
            Self::Terminal => "Terminal",
        }
    }
}

impl Shortcut {
    pub(crate) const ALL: [Self; 15] = [
        Self::AddProject,
        Self::NewTerminal,
        Self::CloseTerminal,
        Self::NextTerminal,
        Self::PreviousTerminal,
        Self::NextProject,
        Self::PreviousProject,
        Self::ToggleProjects,
        Self::ToggleChecklist,
        Self::FocusProjects,
        Self::OpenSettings,
        Self::SaveFile,
        Self::TerminalCopy,
        Self::TerminalPaste,
        Self::TerminalSelectAll,
    ];

    /// Persisted name; never change an existing value.
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::AddProject => "add-project",
            Self::NewTerminal => "new-terminal",
            Self::CloseTerminal => "close-terminal",
            Self::NextTerminal => "next-terminal",
            Self::PreviousTerminal => "previous-terminal",
            Self::NextProject => "next-project",
            Self::PreviousProject => "previous-project",
            Self::ToggleProjects => "toggle-projects",
            Self::ToggleChecklist => "toggle-checklist",
            Self::FocusProjects => "focus-projects",
            Self::OpenSettings => "open-settings",
            Self::SaveFile => "save-file",
            Self::TerminalCopy => "terminal-copy",
            Self::TerminalPaste => "terminal-paste",
            Self::TerminalSelectAll => "terminal-select-all",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|shortcut| shortcut.id() == id)
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::AddProject => "Open project folder",
            Self::NewTerminal => "New terminal",
            Self::CloseTerminal => "Close terminal tab",
            Self::NextTerminal => "Next terminal tab",
            Self::PreviousTerminal => "Previous terminal tab",
            Self::NextProject => "Next project",
            Self::PreviousProject => "Previous project",
            Self::ToggleProjects => "Show or hide sidebar",
            Self::ToggleChecklist => "Show or hide reminders",
            Self::FocusProjects => "Focus sidebar",
            Self::OpenSettings => "Open settings",
            Self::SaveFile => "Save file",
            Self::TerminalCopy => "Copy selection",
            Self::TerminalPaste => "Paste",
            Self::TerminalSelectAll => "Select all output",
        }
    }

    pub(crate) const fn group(self) -> ShortcutGroup {
        match self {
            Self::SaveFile => ShortcutGroup::Editor,
            Self::TerminalCopy | Self::TerminalPaste | Self::TerminalSelectAll => {
                ShortcutGroup::Terminal
            }
            _ => ShortcutGroup::Workbench,
        }
    }

    pub(crate) const fn defaults(self) -> &'static [&'static str] {
        match self {
            Self::AddProject => &["ctrl-shift-o"],
            Self::NewTerminal => &["ctrl-shift-t", "ctrl-`"],
            Self::CloseTerminal => &["ctrl-shift-w"],
            Self::NextTerminal => &["ctrl-tab"],
            Self::PreviousTerminal => &["ctrl-shift-tab"],
            Self::NextProject => &["ctrl-alt-down"],
            Self::PreviousProject => &["ctrl-alt-up"],
            Self::ToggleProjects => &["ctrl-shift-b"],
            Self::ToggleChecklist => &["ctrl-shift-l"],
            Self::FocusProjects => &["f6"],
            Self::OpenSettings => &["ctrl-,"],
            Self::SaveFile => &["ctrl-s"],
            Self::TerminalCopy => &["ctrl-shift-c"],
            Self::TerminalPaste => &["ctrl-shift-v"],
            Self::TerminalSelectAll => &["ctrl-shift-a"],
        }
    }
}

/// Why a keystroke cannot be assigned. Messages are user-facing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rejection {
    Reserved(&'static str),
    NeedsModifier,
    Taken(Shortcut),
}

impl Rejection {
    pub(crate) fn message(self) -> String {
        match self {
            Self::Reserved(reason) => reason.to_owned(),
            Self::NeedsModifier => "Include Ctrl, Alt, or Win, or use a function key.".to_owned(),
            Self::Taken(other) => format!(
                "Already used by “{}”. Change that shortcut first.",
                other.label()
            ),
        }
    }
}

/// Overrides on top of the built-in defaults. An absent entry means "default".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Keymap {
    overrides: BTreeMap<Shortcut, Keystroke>,
}

impl Global for Keymap {}

impl Keymap {
    /// The active keymap, or the defaults before preferences have loaded.
    pub(crate) fn global(cx: &App) -> &Self {
        cx.try_global::<Self>().unwrap_or(&DEFAULT_KEYMAP)
    }

    /// Current bindings for a shortcut: its override, otherwise every default.
    pub(crate) fn bindings(&self, shortcut: Shortcut) -> Vec<Keystroke> {
        match self.overrides.get(&shortcut) {
            Some(keystroke) => vec![keystroke.clone()],
            None => shortcut
                .defaults()
                .iter()
                .filter_map(|source| Keystroke::parse(source).ok())
                .collect(),
        }
    }

    pub(crate) fn is_customized(&self, shortcut: Shortcut) -> bool {
        self.overrides.contains_key(&shortcut)
    }

    pub(crate) fn has_customizations(&self) -> bool {
        !self.overrides.is_empty()
    }

    pub(crate) fn matches(&self, shortcut: Shortcut, keystroke: &Keystroke) -> bool {
        self.bindings(shortcut)
            .iter()
            .any(|binding| same_chord(binding, keystroke))
    }

    /// Which terminal-handled shortcut, if any, this keystroke triggers.
    pub(crate) fn terminal_shortcut(&self, keystroke: &Keystroke) -> Option<Shortcut> {
        [
            Shortcut::TerminalSelectAll,
            Shortcut::TerminalCopy,
            Shortcut::TerminalPaste,
        ]
        .into_iter()
        .find(|shortcut| self.matches(*shortcut, keystroke))
    }

    /// Text for tooltips, for example "Ctrl+Shift+T or Ctrl+`".
    pub(crate) fn hint(&self, shortcut: Shortcut) -> String {
        self.bindings(shortcut)
            .iter()
            .map(display)
            .collect::<Vec<_>>()
            .join(" or ")
    }

    pub(crate) fn check(&self, shortcut: Shortcut, keystroke: &Keystroke) -> Result<(), Rejection> {
        standalone(shortcut, keystroke)?;
        match self.conflict(shortcut, keystroke) {
            Some(other) => Err(Rejection::Taken(other)),
            None => Ok(()),
        }
    }

    pub(crate) fn assign(
        &mut self,
        shortcut: Shortcut,
        keystroke: &Keystroke,
    ) -> Result<(), Rejection> {
        self.check(shortcut, keystroke)?;
        let defaults = shortcut.defaults();
        let is_only_default = defaults.len() == 1
            && Keystroke::parse(defaults[0]).is_ok_and(|default| same_chord(&default, keystroke));
        if is_only_default {
            self.overrides.remove(&shortcut);
        } else {
            self.overrides.insert(shortcut, normalized(keystroke));
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self, shortcut: Shortcut) {
        self.overrides.remove(&shortcut);
    }

    pub(crate) fn reset_all(&mut self) {
        self.overrides.clear();
    }

    /// Another shortcut that already answers to this keystroke.
    fn conflict(&self, shortcut: Shortcut, keystroke: &Keystroke) -> Option<Shortcut> {
        Shortcut::ALL
            .into_iter()
            .filter(|other| *other != shortcut)
            .find(|other| {
                self.bindings(*other)
                    .iter()
                    .any(|binding| same_chord(binding, keystroke))
            })
    }

    /// Validate a whole file at once: a swap of two shortcuts is valid even
    /// though each assignment alone would conflict with the other's old chord.
    fn from_overrides(overrides: BTreeMap<Shortcut, Keystroke>) -> Result<Self, Rejection> {
        for (shortcut, keystroke) in &overrides {
            standalone(*shortcut, keystroke)?;
        }
        let keymap = Self { overrides };
        for (shortcut, keystroke) in &keymap.overrides {
            if let Some(other) = keymap.conflict(*shortcut, keystroke) {
                return Err(Rejection::Taken(other));
            }
        }
        Ok(keymap)
    }
}

fn same_chord(left: &Keystroke, right: &Keystroke) -> bool {
    left.modifiers == right.modifiers && left.key == right.key
}

/// Drop `key_char`: it describes typed text, not the chord, and is not saved.
fn normalized(keystroke: &Keystroke) -> Keystroke {
    Keystroke {
        modifiers: keystroke.modifiers,
        key: keystroke.key.clone(),
        key_char: None,
    }
}

pub(crate) fn is_modifier_key(key: &str) -> bool {
    matches!(key, "shift" | "control" | "alt" | "platform" | "function")
}

fn is_function_key(key: &str) -> bool {
    key.strip_prefix('f')
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=24).contains(&number))
}

/// Rules that depend only on the chord, not on other shortcuts.
fn standalone(shortcut: Shortcut, keystroke: &Keystroke) -> Result<(), Rejection> {
    let modifiers = keystroke.modifiers;
    let key = keystroke.key.as_str();
    let no_command_modifier = !modifiers.control && !modifiers.alt && !modifiers.platform;
    // `Keystroke::parse` accepts a trailing dash such as "ctrl-" as an empty key.
    if key.is_empty() {
        return Err(Rejection::Reserved(
            "Choose a key to go with the modifiers.",
        ));
    }
    if key == "escape" {
        return Err(Rejection::Reserved("Escape cancels and closes dialogs."));
    }
    if key == "tab" && no_command_modifier {
        return Err(Rejection::Reserved(
            "Tab and Shift+Tab can't be used for shortcuts.",
        ));
    }
    if key == "f6" && modifiers.shift && no_command_modifier {
        return Err(Rejection::Reserved("Shift+F6 moves focus backward."));
    }
    if key == "f4" && modifiers.alt && !modifiers.control && !modifiers.shift {
        return Err(Rejection::Reserved("Alt+F4 closes the window."));
    }
    let control_only =
        modifiers.control && !modifiers.alt && !modifiers.shift && !modifiers.platform;
    if control_only && key.len() == 1 && key.as_bytes()[0].is_ascii_lowercase() {
        // Saving never competes with a running program, so only text editing is off limits there.
        let editing = matches!(key, "a" | "c" | "v" | "x" | "y" | "z");
        if shortcut != Shortcut::SaveFile || editing {
            return Err(Rejection::Reserved(
                "Ctrl+letter is reserved for terminal programs and text editing. Add Shift or Alt.",
            ));
        }
    }
    if is_modifier_key(key) || (no_command_modifier && !is_function_key(key)) {
        return Err(Rejection::NeedsModifier);
    }
    Ok(())
}

fn display_parts(keystroke: &Keystroke) -> Vec<String> {
    let mut parts = Vec::with_capacity(5);
    if keystroke.modifiers.control {
        parts.push("Ctrl".to_owned());
    }
    if keystroke.modifiers.alt {
        parts.push("Alt".to_owned());
    }
    if keystroke.modifiers.shift {
        parts.push("Shift".to_owned());
    }
    if keystroke.modifiers.platform {
        parts.push("Win".to_owned());
    }
    parts.push(match keystroke.key.as_str() {
        "pageup" => "Page Up".to_owned(),
        "pagedown" => "Page Down".to_owned(),
        "escape" => "Esc".to_owned(),
        key => {
            let mut characters = key.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(characters).collect()
            })
        }
    });
    parts
}

/// Readable chord, for example "Ctrl+Shift+T".
pub(crate) fn display(keystroke: &Keystroke) -> String {
    display_parts(keystroke).join("+")
}

/// Short chord for tight controls, for example "Ctrl ⇧ T".
pub(crate) fn compact(keystroke: &Keystroke) -> String {
    display_parts(keystroke)
        .into_iter()
        .map(|part| {
            if part == "Shift" {
                "⇧".to_owned()
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Deserialize, Serialize)]
struct StoredKeymap {
    version: u32,
    shortcuts: BTreeMap<String, String>,
}

fn invalid(message: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(crate) fn load(path: &Path) -> io::Result<Keymap> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Keymap::default()),
        Err(error) => return Err(error),
    };
    let stored: StoredKeymap = serde_json::from_slice(&bytes).map_err(invalid)?;
    if stored.version != 1 {
        return Err(invalid("Unsupported keybindings version"));
    }
    let mut overrides = BTreeMap::new();
    for (id, source) in stored.shortcuts {
        let shortcut = Shortcut::from_id(&id).ok_or_else(|| invalid("Unknown shortcut"))?;
        let keystroke = Keystroke::parse(&source).map_err(invalid)?;
        overrides.insert(shortcut, normalized(&keystroke));
    }
    Keymap::from_overrides(overrides).map_err(|rejection| invalid(rejection.message()))
}

pub(crate) fn save(path: &Path, keymap: &Keymap) -> io::Result<()> {
    let stored = StoredKeymap {
        version: 1,
        shortcuts: keymap
            .overrides
            .iter()
            .map(|(shortcut, keystroke)| (shortcut.id().to_owned(), keystroke.unparse()))
            .collect(),
    };
    let mut bytes = serde_json::to_vec_pretty(&stored).map_err(invalid)?;
    bytes.push(b'\n');
    super::atomic_file::write(path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST: AtomicU64 = AtomicU64::new(1);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "pideck-keymap-{}-{}",
                std::process::id(),
                NEXT_TEST.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn key(source: &str) -> Keystroke {
        Keystroke::parse(source).unwrap()
    }

    #[test]
    fn defaults_parse_are_valid_and_never_collide() {
        let keymap = Keymap::default();
        let mut seen = Vec::new();
        for shortcut in Shortcut::ALL {
            assert_eq!(Shortcut::from_id(shortcut.id()), Some(shortcut));
            assert!(!shortcut.defaults().is_empty(), "{shortcut:?}");
            for source in shortcut.defaults() {
                let keystroke = Keystroke::parse(source)
                    .unwrap_or_else(|_| panic!("{shortcut:?} default {source} must parse"));
                assert_eq!(keymap.conflict(shortcut, &keystroke), None, "{source}");
                assert!(
                    !seen.iter().any(|known| same_chord(known, &keystroke)),
                    "{source} is bound twice"
                );
                seen.push(keystroke);
            }
        }
        let ids: Vec<_> = Shortcut::ALL.iter().map(|shortcut| shortcut.id()).collect();
        for (index, id) in ids.iter().enumerate() {
            assert!(!ids[..index].contains(id), "duplicate shortcut ID: {id}");
        }
    }

    #[test]
    fn display_uses_readable_names() {
        assert_eq!(display(&key("ctrl-shift-t")), "Ctrl+Shift+T");
        assert_eq!(display(&key("ctrl-`")), "Ctrl+`");
        assert_eq!(display(&key("ctrl-alt-down")), "Ctrl+Alt+Down");
        assert_eq!(display(&key("f6")), "F6");
        assert_eq!(display(&key("ctrl-pagedown")), "Ctrl+Page Down");
        assert_eq!(compact(&key("ctrl-shift-t")), "Ctrl ⇧ T");
        assert_eq!(
            Keymap::default().hint(Shortcut::NewTerminal),
            "Ctrl+Shift+T or Ctrl+`"
        );
    }

    #[test]
    fn assignment_rejects_reserved_bare_and_taken_keystrokes() {
        let mut keymap = Keymap::default();
        let new_terminal = Shortcut::NewTerminal;
        for (source, expected) in [
            ("escape", Rejection::Reserved("")),
            ("shift-f6", Rejection::Reserved("")),
            ("alt-f4", Rejection::Reserved("")),
            ("ctrl-c", Rejection::Reserved("")),
            ("ctrl-k", Rejection::Reserved("")),
            ("shift-t", Rejection::NeedsModifier),
            ("t", Rejection::NeedsModifier),
            ("shift", Rejection::NeedsModifier),
            ("ctrl-shift-w", Rejection::Taken(Shortcut::CloseTerminal)),
            ("ctrl-shift-c", Rejection::Taken(Shortcut::TerminalCopy)),
            ("f6", Rejection::Taken(Shortcut::FocusProjects)),
        ] {
            let result = keymap.assign(new_terminal, &key(source));
            match (result, expected) {
                (Err(Rejection::Reserved(_)), Rejection::Reserved(_)) => {}
                (result, expected) => assert_eq!(result, Err(expected), "{source}"),
            }
        }
        assert!(!keymap.has_customizations());
        keymap.assign(new_terminal, &key("ctrl-alt-n")).unwrap();
        keymap.assign(new_terminal, &key("ctrl-alt-n")).unwrap();
        keymap.assign(new_terminal, &key("f9")).unwrap();
        keymap.assign(new_terminal, &key("alt-shift-f4")).unwrap();
    }

    #[test]
    fn save_allows_control_letters_but_not_editing_chords() {
        let mut keymap = Keymap::default();
        keymap.assign(Shortcut::SaveFile, &key("ctrl-k")).unwrap();
        assert_eq!(
            keymap.check(Shortcut::SaveFile, &key("ctrl-v")),
            Err(Rejection::Reserved(
                "Ctrl+letter is reserved for terminal programs and text editing. Add Shift or Alt."
            ))
        );
    }

    #[test]
    fn assignment_frees_old_chords_and_reset_restores_every_default() {
        let mut keymap = Keymap::default();
        keymap
            .assign(Shortcut::NewTerminal, &key("ctrl-alt-n"))
            .unwrap();
        assert!(keymap.is_customized(Shortcut::NewTerminal));
        assert_eq!(keymap.hint(Shortcut::NewTerminal), "Ctrl+Alt+N");
        // The default chord is free for another shortcut after the move.
        keymap
            .assign(Shortcut::CloseTerminal, &key("ctrl-shift-t"))
            .unwrap();
        assert!(!keymap.matches(Shortcut::NewTerminal, &key("ctrl-`")));
        keymap.reset(Shortcut::NewTerminal);
        assert!(keymap.matches(Shortcut::NewTerminal, &key("ctrl-`")));
        // Ctrl+Shift+T is still held by Close, so the default is no longer unique.
        assert_eq!(
            keymap.check(Shortcut::NewTerminal, &key("ctrl-shift-t")),
            Err(Rejection::Taken(Shortcut::CloseTerminal))
        );
        keymap.reset_all();
        assert!(!keymap.has_customizations());
    }

    #[test]
    fn assigning_the_sole_default_drops_the_override() {
        let mut keymap = Keymap::default();
        keymap
            .assign(Shortcut::ToggleProjects, &key("ctrl-alt-b"))
            .unwrap();
        keymap
            .assign(Shortcut::ToggleProjects, &key("ctrl-shift-b"))
            .unwrap();
        assert!(!keymap.is_customized(Shortcut::ToggleProjects));
    }

    #[test]
    fn terminal_shortcuts_match_by_modifiers_and_key_only() {
        let mut keymap = Keymap::default();
        let typed = Keystroke {
            key_char: Some("C".into()),
            ..key("ctrl-shift-c")
        };
        assert_eq!(
            keymap.terminal_shortcut(&typed),
            Some(Shortcut::TerminalCopy)
        );
        assert_eq!(keymap.terminal_shortcut(&key("ctrl-c")), None);
        keymap
            .assign(Shortcut::TerminalCopy, &key("ctrl-alt-c"))
            .unwrap();
        assert_eq!(keymap.terminal_shortcut(&typed), None);
        assert_eq!(
            keymap.terminal_shortcut(&key("ctrl-alt-c")),
            Some(Shortcut::TerminalCopy)
        );
    }

    #[test]
    fn preferences_roundtrip_including_a_swap_of_two_defaults() {
        let root = TestDirectory::new();
        let path = root.0.join("keybindings.json");
        assert_eq!(load(&path).unwrap(), Keymap::default());
        let mut keymap = Keymap::default();
        keymap
            .assign(Shortcut::CloseTerminal, &key("ctrl-alt-w"))
            .unwrap();
        keymap
            .assign(Shortcut::AddProject, &key("ctrl-shift-w"))
            .unwrap();
        save(&path, &keymap).unwrap();
        assert_eq!(load(&path).unwrap(), keymap);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#""add-project": "ctrl-shift-w""#), "{text}");
        assert!(text.ends_with("}\n"));
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
    }

    #[test]
    fn corrupt_unknown_conflicting_and_future_preferences_are_preserved() {
        let root = TestDirectory::new();
        let path = root.0.join("keybindings.json");
        for bytes in [
            b"invalid json".as_slice(),
            br#"{"version":2,"shortcuts":{}}"#.as_slice(),
            br#"{"version":1,"shortcuts":{"teleport":"ctrl-alt-t"}}"#.as_slice(),
            br#"{"version":1,"shortcuts":{"new-terminal":"ctrl-"}}"#.as_slice(),
            br#"{"version":1,"shortcuts":{"new-terminal":"escape"}}"#.as_slice(),
            br#"{"version":1,"shortcuts":{"new-terminal":"ctrl-shift-w"}}"#.as_slice(),
        ] {
            fs::write(&path, bytes).unwrap();
            assert_eq!(load(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}
