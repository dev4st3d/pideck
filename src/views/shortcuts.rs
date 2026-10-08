//! Connects configurable shortcuts to GPUI actions and rebinds them while the app runs.
//!
//! GPUI cannot remove a binding, but a later `NoAction` binding with the same
//! chord and context disables an earlier one, and later bindings outrank
//! earlier ones. A change therefore appends `NoAction` for vacated chords
//! first, then the new chords, in one batch so a swap of two shortcuts works.

use gpui::{App, KeyBinding, Keystroke, NoAction};

use super::file_editor::SaveFile;
use super::terminal_manager::{
    AddProject, CloseTerminal, FocusProjects, NewTerminal, NextProject, NextTerminal, OpenSettings,
    PreviousProject, PreviousTerminal, ToggleChecklist, ToggleProjects,
};
use crate::services::keymap::{Keymap, Shortcut, ShortcutGroup};

const WORKBENCH_CONTEXT: &str = "TerminalManager";
const EDITOR_CONTEXT: &str = "FileEditor";

/// Key context of a shortcut's binding. Terminal shortcuts are read by the
/// terminal's own key handler, so they have no GPUI binding.
fn context(shortcut: Shortcut) -> Option<&'static str> {
    match shortcut.group() {
        ShortcutGroup::Workbench => Some(WORKBENCH_CONTEXT),
        ShortcutGroup::Editor => Some(EDITOR_CONTEXT),
        ShortcutGroup::Terminal => None,
    }
}

fn binding(shortcut: Shortcut, chord: &str) -> Option<KeyBinding> {
    let context = Some(context(shortcut)?);
    Some(match shortcut {
        Shortcut::AddProject => KeyBinding::new(chord, AddProject, context),
        Shortcut::NewTerminal => KeyBinding::new(chord, NewTerminal, context),
        Shortcut::CloseTerminal => KeyBinding::new(chord, CloseTerminal, context),
        Shortcut::NextTerminal => KeyBinding::new(chord, NextTerminal, context),
        Shortcut::PreviousTerminal => KeyBinding::new(chord, PreviousTerminal, context),
        Shortcut::NextProject => KeyBinding::new(chord, NextProject, context),
        Shortcut::PreviousProject => KeyBinding::new(chord, PreviousProject, context),
        Shortcut::ToggleProjects => KeyBinding::new(chord, ToggleProjects, context),
        Shortcut::ToggleChecklist => KeyBinding::new(chord, ToggleChecklist, context),
        Shortcut::FocusProjects => KeyBinding::new(chord, FocusProjects, context),
        Shortcut::OpenSettings => KeyBinding::new(chord, OpenSettings, context),
        Shortcut::SaveFile => KeyBinding::new(chord, SaveFile, context),
        Shortcut::TerminalCopy | Shortcut::TerminalPaste | Shortcut::TerminalSelectAll => {
            return None;
        }
    })
}

fn unbinding(shortcut: Shortcut, chord: &str) -> Option<KeyBinding> {
    Some(KeyBinding::new(chord, NoAction, Some(context(shortcut)?)))
}

fn has_chord(chords: &[Keystroke], chord: &Keystroke) -> bool {
    chords
        .iter()
        .any(|other| other.modifiers == chord.modifiers && other.key == chord.key)
}

/// Bind the active chords of one shortcut. Used once at startup.
pub(crate) fn bind(cx: &mut App, shortcut: Shortcut) {
    let bindings: Vec<_> = Keymap::global(cx)
        .bindings(shortcut)
        .iter()
        .filter_map(|keystroke| binding(shortcut, &keystroke.unparse()))
        .collect();
    cx.bind_keys(bindings);
}

/// Bind every shortcut in a group. Used once at startup.
pub(crate) fn bind_group(cx: &mut App, group: ShortcutGroup) {
    for shortcut in Shortcut::ALL {
        if shortcut.group() == group {
            bind(cx, shortcut);
        }
    }
}

/// Plain Shift+Tab is unused on purpose. GPUI Component binds it to "focus
/// previous" (and its editor to "outdent"), which pulled focus out of the
/// terminal. A context-free `NoAction` outranks every context-bound binding
/// added before it, so call this after those libraries and views have bound
/// their keys. Ctrl+Shift+Tab is a different chord and is unaffected.
pub(crate) fn disable_shift_tab(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("shift-tab", NoAction, None)]);
}

/// Make GPUI follow a keymap change from `previous` to `current`.
pub(crate) fn rebind(cx: &mut App, previous: &Keymap, current: &Keymap) {
    let mut vacated = Vec::new();
    let mut added = Vec::new();
    for shortcut in Shortcut::ALL {
        let before = previous.bindings(shortcut);
        let after = current.bindings(shortcut);
        vacated.extend(
            before
                .iter()
                .filter(|chord| !has_chord(&after, chord))
                .filter_map(|chord| unbinding(shortcut, &chord.unparse())),
        );
        added.extend(
            after
                .iter()
                .filter(|chord| !has_chord(&before, chord))
                .filter_map(|chord| binding(shortcut, &chord.unparse())),
        );
    }
    cx.bind_keys(vacated.into_iter().chain(added));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_chord_builds_a_binding_or_belongs_to_the_terminal() {
        for shortcut in Shortcut::ALL {
            for chord in shortcut.defaults() {
                let built = binding(shortcut, chord);
                assert_eq!(
                    built.is_some(),
                    shortcut.group() != ShortcutGroup::Terminal,
                    "{shortcut:?} {chord}"
                );
            }
        }
    }
}
