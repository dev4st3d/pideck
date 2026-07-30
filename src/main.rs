#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    velopack::VelopackApp::build().run();
    pi_gui::app::run();
}
