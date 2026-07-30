pub mod git_diff;
pub mod path_actions;
pub mod pi_process;
pub mod projects;
pub mod rpc;
pub mod runtime_worker;
pub mod sdk_bridge;
pub mod session_catalog;
pub mod terminal;

pub(crate) fn suppress_console_window(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }

    #[cfg(not(windows))]
    let _ = command;
}
