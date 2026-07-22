//! Logical runtime and keyboard-focus actions.

use gpui::{KeyBinding, actions};

pub(crate) const RECOVERY_BUTTON_CONTEXT: &str = "RecoveryButton";

actions!(
    pi_gui,
    [
        Connect,
        Retry,
        Stop,
        ActivateRecovery,
        FocusNext,
        FocusPrevious,
        ComposerBackspace,
        ComposerDelete,
        ComposerLeft,
        ComposerRight,
        ComposerUp,
        ComposerDown,
        ComposerSelectLeft,
        ComposerSelectRight,
        ComposerSelectUp,
        ComposerSelectDown,
        ComposerLineStart,
        ComposerLineEnd,
        ComposerSelectLineStart,
        ComposerSelectLineEnd,
        ComposerSelectAll,
        ComposerCopy,
        ComposerCut,
        ComposerPaste,
        ComposerUndo,
        ComposerRedo,
        InsertNewline,
        AcceptInput,
        QueueFollowUp,
        AbortRun,
        TranscriptCopy,
        TranscriptSelectAll
    ]
);

pub(crate) fn transcript_key_bindings() -> Vec<KeyBinding> {
    let context = Some("TranscriptText");
    vec![
        KeyBinding::new("ctrl-c", TranscriptCopy, context),
        KeyBinding::new("ctrl-a", TranscriptSelectAll, context),
    ]
}

pub(crate) fn composer_key_bindings() -> Vec<KeyBinding> {
    let context = Some("Composer");
    vec![
        KeyBinding::new("backspace", ComposerBackspace, context),
        KeyBinding::new("delete", ComposerDelete, context),
        KeyBinding::new("left", ComposerLeft, context),
        KeyBinding::new("right", ComposerRight, context),
        KeyBinding::new("up", ComposerUp, context),
        KeyBinding::new("down", ComposerDown, context),
        KeyBinding::new("shift-left", ComposerSelectLeft, context),
        KeyBinding::new("shift-right", ComposerSelectRight, context),
        KeyBinding::new("shift-up", ComposerSelectUp, context),
        KeyBinding::new("shift-down", ComposerSelectDown, context),
        KeyBinding::new("home", ComposerLineStart, context),
        KeyBinding::new("end", ComposerLineEnd, context),
        KeyBinding::new("shift-home", ComposerSelectLineStart, context),
        KeyBinding::new("shift-end", ComposerSelectLineEnd, context),
        KeyBinding::new("ctrl-a", ComposerSelectAll, context),
        KeyBinding::new("ctrl-c", ComposerCopy, context),
        KeyBinding::new("ctrl-x", ComposerCut, context),
        KeyBinding::new("ctrl-v", ComposerPaste, context),
        KeyBinding::new("ctrl-z", ComposerUndo, context),
        KeyBinding::new("ctrl-y", ComposerRedo, context),
        KeyBinding::new("ctrl-shift-z", ComposerRedo, context),
        KeyBinding::new("shift-enter", InsertNewline, context),
        KeyBinding::new("enter", AcceptInput, context),
        KeyBinding::new("alt-enter", QueueFollowUp, context),
        KeyBinding::new("escape", AbortRun, None),
    ]
}
