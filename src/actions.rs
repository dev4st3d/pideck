//! Logical runtime and keyboard-focus actions.

use gpui::actions;

actions!(
    pi_gui,
    [
        Connect,
        Retry,
        Stop,
        ActivateRecovery,
        FocusNext,
        FocusPrevious
    ]
);
