use gpui::{
    App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size,
};

use crate::assets::Assets;
use crate::fonts;
use crate::views::terminal_manager::TerminalManager;

pub fn run() {
    let working_directory = std::env::current_dir().unwrap_or_else(|_| ".".into());
    Application::new()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            crate::services::accessibility::refresh_motion_preference();
            let font_catalog = fonts::initialize(cx);
            crate::views::file_editor::FileEditor::initialize(cx);
            TerminalManager::bind_keys(cx);
            let storage_path = font_catalog
                .settings_path
                .with_file_name("terminal-workspace.json");
            // Leave room for the taskbar and resize borders on smaller displays.
            let initial_size =
                cx.primary_display()
                    .map_or(size(px(1440.0), px(900.0)), |display| {
                        let available = display.bounds().size;
                        size(
                            px(1440.0_f32.min((f32::from(available.width) - 48.0).max(640.0))),
                            px(900.0_f32.min((f32::from(available.height) - 80.0).max(480.0))),
                        )
                    });
            let bounds = Bounds::centered(None, initial_size, cx);
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(800.0), px(540.0))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Zidesk — Terminals".into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    app_id: Some("pideck".into()),
                    ..Default::default()
                },
                move |window, cx| {
                    let root = cx.new(|cx| {
                        TerminalManager::new(
                            working_directory.clone(),
                            storage_path.clone(),
                            window,
                            cx,
                        )
                    });
                    let weak = root.downgrade();
                    window.on_window_should_close(cx, move |window, cx| {
                        weak.update(cx, |root, cx| root.request_close(window, cx))
                            .unwrap_or(true)
                    });
                    cx.new(|cx| gpui_component::Root::new(root, window, cx))
                },
            );
            if let Err(error) = opened {
                eprintln!("The terminal window could not be opened: {error}");
                cx.quit();
                return;
            }
            cx.activate(true);
        });
}
