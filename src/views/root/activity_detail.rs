//! Activity-detail overlay ownership and modal keyboard routing.

use super::*;

impl RootView {
    pub(in crate::views) fn open_activity_detail(
        &mut self,
        detail: Arc<ActivityDetail>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activity_detail_restore_focus = window.focused(cx);
        self.activity_detail = Some(detail);
        self.activity_detail_scroll
            .set_offset(point(px(0.0), px(0.0)));
        window.focus(&self.activity_detail_focus);
        cx.notify();
    }

    pub(super) fn close_activity_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.activity_detail.take().is_none() {
            return;
        }
        if let Some(focus) = self.activity_detail_restore_focus.take() {
            window.focus(&focus);
        } else {
            window.focus(&self.composer.read(cx).focus_handle(cx));
        }
        cx.notify();
    }

    pub(in crate::views) fn on_activity_detail_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.keystroke.key.as_str() {
            "escape" => {
                cx.stop_propagation();
                self.close_activity_detail(window, cx);
            }
            "tab" => {
                cx.stop_propagation();
                window.focus(&self.activity_detail_focus);
            }
            _ => {}
        }
    }

}
