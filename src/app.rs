use gpui::{
    App, Application, Bounds, KeyBinding, WindowBounds, WindowOptions, prelude::*, px, size,
};

use crate::actions::{
    ActivateRecovery, Connect, DecreaseFontSize, FocusNext, FocusPrevious, IncreaseFontSize,
    OpenCommandPalette, RECOVERY_BUTTON_CONTEXT, Retry, ShowHotkeys, Stop, composer_key_bindings,
    history_key_bindings, image_preview_key_bindings, orchestration_key_bindings,
    transcript_key_bindings,
};
use crate::assets::Assets;
use crate::controller::RuntimeController;
use crate::services::projects::ProjectRegistry;
use crate::services::session_catalog::{SessionCatalogConfig, without_windows_verbatim_prefix};
use crate::{fonts, views::RootView};

const WINDOW_WIDTH: f32 = 1440.0;
const WINDOW_HEIGHT: f32 = 860.0;

pub fn run() {
    let working_directory = std::env::current_dir()
        .map(|path| without_windows_verbatim_prefix(&path))
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let catalog_config = SessionCatalogConfig::from_environment(working_directory.clone());
    let project_load = ProjectRegistry::load(
        catalog_config.agent_dir.join("pideck-projects.json"),
        working_directory,
    );
    let workspace = project_load.registry.active_path().to_path_buf();
    let preferred_session = project_load
        .registry
        .active_project()
        .last_session
        .clone()
        .filter(|path| path.is_file());

    let application = Application::new().with_assets(Assets);
    application.run(move |cx: &mut App| {
        let font_catalog = fonts::initialize(cx);
        cx.bind_keys([
            KeyBinding::new("ctrl-alt-c", Connect, None),
            KeyBinding::new("ctrl-alt-r", Retry, None),
            KeyBinding::new("ctrl-alt-s", Stop, None),
            KeyBinding::new("ctrl-shift-p", OpenCommandPalette, None),
            KeyBinding::new("ctrl-/", ShowHotkeys, None),
            KeyBinding::new("ctrl-+", IncreaseFontSize, None),
            KeyBinding::new("ctrl-=", IncreaseFontSize, None),
            KeyBinding::new("ctrl--", DecreaseFontSize, None),
            KeyBinding::new("enter", ActivateRecovery, Some(RECOVERY_BUTTON_CONTEXT)),
            KeyBinding::new("space", ActivateRecovery, Some(RECOVERY_BUTTON_CONTEXT)),
            KeyBinding::new("tab", FocusNext, None),
            KeyBinding::new("shift-tab", FocusPrevious, None),
        ]);
        cx.bind_keys(composer_key_bindings());
        cx.bind_keys(transcript_key_bindings());
        cx.bind_keys(history_key_bindings());
        cx.bind_keys(orchestration_key_bindings());
        cx.bind_keys(image_preview_key_bindings());

        let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
        let workspace = workspace.clone();
        let preferred_session = preferred_session.clone();
        let projects = project_load.registry.clone();
        let projects_warning = project_load.warning.clone();
        let projects_need_save = project_load.needs_save;
        let font_catalog = font_catalog.clone();

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(1080.0), px(640.0))),
                ..Default::default()
            },
            move |window, cx| {
                let controller =
                    cx.new(|cx| RuntimeController::for_workspace(workspace.clone(), cx));
                let weak_controller = controller.downgrade();
                cx.on_window_closed(move |cx| {
                    weak_controller
                        .update(cx, |controller, _| controller.shutdown())
                        .ok();
                    if cx.windows().is_empty() {
                        cx.quit();
                    }
                })
                .detach();

                let root = cx.new(|cx| {
                    RootView::new(
                        window,
                        controller.clone(),
                        projects.clone(),
                        projects_warning.clone(),
                        projects_need_save,
                        font_catalog.clone(),
                        cx,
                    )
                });
                controller.update(cx, |controller, cx| {
                    controller.connect_to_session(preferred_session.clone(), cx)
                });
                root
            },
        )
        .expect("failed to open the main window");

        cx.activate(true);
    });
}
