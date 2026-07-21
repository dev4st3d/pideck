use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};

use crate::{fonts, views::RootView};

const WINDOW_WIDTH: f32 = 1440.0;
const WINDOW_HEIGHT: f32 = 900.0;

pub fn run() {
    Application::new().run(|cx: &mut App| {
        fonts::register(cx);

        let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(1100.0), px(700.0))),
                ..Default::default()
            },
            |_, cx| cx.new(RootView::new),
        )
        .expect("failed to open the main window");

        cx.activate(true);
    });
}
