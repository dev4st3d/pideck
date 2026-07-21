use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};

use crate::views::RootView;

const WINDOW_WIDTH: f32 = 960.0;
const WINDOW_HEIGHT: f32 = 640.0;

pub fn run() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(640.0), px(420.0))),
                ..Default::default()
            },
            |_, cx| cx.new(RootView::new),
        )
        .expect("failed to open the main window");

        cx.activate(true);
    });
}
