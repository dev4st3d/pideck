use std::sync::Arc;

use gpui::{
    App, Application, Bounds, KeyBinding, WindowBounds, WindowOptions, prelude::*, px, size,
};

use crate::actions::{
    ActivateRecovery, Connect, FocusNext, FocusPrevious, Retry, Stop, composer_key_bindings,
};
use crate::controller::RuntimeController;
use crate::services::runtime_worker::{RpcRuntimeService, RuntimeService};
use crate::{fonts, views::RootView};

const WINDOW_WIDTH: f32 = 1080.0;
const WINDOW_HEIGHT: f32 = 680.0;

pub fn run() {
    let working_directory = std::env::current_dir()
        .ok()
        .and_then(|path| std::fs::canonicalize(&path).ok().or(Some(path)))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let workspace = working_directory.to_string_lossy().into_owned();
    let service: Arc<dyn RuntimeService> =
        Arc::new(RpcRuntimeService::default_profile(working_directory));

    Application::new().run(move |cx: &mut App| {
        fonts::register(cx);
        cx.bind_keys([
            KeyBinding::new("ctrl-alt-c", Connect, None),
            KeyBinding::new("ctrl-alt-r", Retry, None),
            KeyBinding::new("ctrl-alt-s", Stop, None),
            KeyBinding::new("enter", ActivateRecovery, None),
            KeyBinding::new("space", ActivateRecovery, None),
            KeyBinding::new("tab", FocusNext, None),
            KeyBinding::new("shift-tab", FocusPrevious, None),
        ]);
        cx.bind_keys(composer_key_bindings());

        let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
        let workspace = workspace.clone();
        let service = Arc::clone(&service);

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(820.0), px(520.0))),
                ..Default::default()
            },
            move |window, cx| {
                let controller = cx
                    .new(|cx| RuntimeController::new(workspace.clone(), Arc::clone(&service), cx));
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
