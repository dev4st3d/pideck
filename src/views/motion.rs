//! Interruptible programmatic scrolling for view transitions.

use std::time::Duration;

use gpui::{Context, Pixels, ScrollHandle, Task, Timer, ease_out_quint, point, px};

const SCROLL_MS: u64 = 280;

pub fn smooth_scroll_y<V: 'static>(
    cx: &mut Context<V>,
    handle: ScrollHandle,
    target_y: Pixels,
    generation: u64,
    current_generation: impl Fn(&V) -> u64 + 'static,
) -> Task<()> {
    let from_y = handle.offset().y;
    let duration = Duration::from_millis(SCROLL_MS);

    cx.spawn(async move |this, cx| {
        let start = std::time::Instant::now();
        loop {
            let still_current = this
                .update(cx, |view, _| current_generation(view) == generation)
                .unwrap_or(false);
            if !still_current {
                return;
            }

            let progress = (start.elapsed().as_secs_f32() / duration.as_secs_f32()).min(1.0);
            let eased = ease_out_quint()(progress);
            let y = from_y + (target_y - from_y) * eased;
            handle.set_offset(point(px(0.0), y));

            let _ = this.update(cx, |_view, cx| cx.notify());
            if progress >= 1.0 {
                return;
            }

            Timer::after(Duration::from_millis(16)).await;
        }
    })
}

pub fn scroll_bottom_y(handle: &ScrollHandle) -> Pixels {
    -handle.max_offset().height
}
