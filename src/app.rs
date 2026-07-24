use std::sync::Arc;

use gpui::{
    App, Application, Bounds, KeyBinding, WindowBounds, WindowOptions, prelude::*, px, size,
};

use crate::actions::{
    ActivateRecovery, Connect, FocusNext, FocusPrevious, OpenCommandPalette,
    RECOVERY_BUTTON_CONTEXT, Retry, ShowHotkeys, Stop, composer_key_bindings, history_key_bindings,
    image_preview_key_bindings, orchestration_key_bindings, transcript_key_bindings,
};
use crate::controller::RuntimeController;
use crate::services::runtime_worker::{RpcRuntimeService, RuntimeService};
use crate::services::session_catalog::{SessionCatalogConfig, without_windows_verbatim_prefix};
use crate::{fonts, views::RootView};

const WINDOW_WIDTH: f32 = 1440.0;
const WINDOW_HEIGHT: f32 = 860.0;

pub fn run() {
    let working_directory = std::env::current_dir()
        .map(|path| without_windows_verbatim_prefix(&path))
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let workspace = working_directory.to_string_lossy().into_owned();
    let catalog_config = SessionCatalogConfig::from_environment(working_directory.clone());
    let session_root = catalog_config.resolve_root().path;
    let service: Arc<dyn RuntimeService> = Arc::new(RpcRuntimeService::persisted_profile(
        working_directory,
        session_root,
    ));

    Application::new().run(move |cx: &mut App| {
        fonts::register(cx);
        cx.bind_keys([
            KeyBinding::new("ctrl-alt-c", Connect, None),
            KeyBinding::new("ctrl-alt-r", Retry, None),
            KeyBinding::new("ctrl-alt-s", Stop, None),
            KeyBinding::new("ctrl-shift-p", OpenCommandPalette, None),
            KeyBinding::new("ctrl-/", ShowHotkeys, None),
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
        let service = Arc::clone(&service);

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(1080.0), px(640.0))),
                ..Default::default()
            },
            move |window, cx| {
                let controller = cx.new(|cx| {
                    RuntimeController::new(
                        workspace.clone(),
                        Arc::clone(&service),
                        catalog_config.clone(),
                        cx,
                    )
                });
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

                let root = cx.new(|cx| RootView::new(window, controller.clone(), cx));
                controller.update(cx, |controller, cx| controller.connect(cx));
                root
            },
        )
        .expect("failed to open the main window");

        cx.activate(true);
    });
}
