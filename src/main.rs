#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    velopack::VelopackApp::build()
        // Restart is an explicit, guarded action after workspace persistence.
        .set_auto_apply_on_startup(false)
        .run();
    pi_gui::app::run();
}
