//! Bottom terminal dock and resize affordance.

use super::*;

pub(super) fn terminal_splitter(
    root: Entity<RootView>,
    dragging: bool,
    viewport_height: Pixels,
) -> impl IntoElement {
    let mouse_down_root = root.clone();
    let mouse_move_root = root.clone();
    let mouse_up_root = root;

    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |bounds, hitbox, window, _| {
            if dragging {
                window.set_window_cursor_style(CursorStyle::ResizeRow);
            } else {
                window.set_cursor_style(CursorStyle::ResizeRow, &hitbox);
            }

            let track = Bounds::new(
                point(bounds.left(), bounds.top() + px(3.0)),
                size(bounds.size.width, px(if dragging { 2.0 } else { 1.0 })),
            );
            window.paint_quad(fill(
                track,
                if dragging {
                    theme::focus()
                } else {
                    theme::edge_hard()
                },
            ));

            let mouse_down_bounds = bounds;
            window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture
                    || event.button != MouseButton::Left
                    || !mouse_down_bounds.contains(&event.position)
                {
                    return;
                }
                mouse_down_root.update(cx, |view, cx| {
                    view.begin_terminal_resize(event.position.y, cx)
                });
                cx.stop_propagation();
            });

            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let handled = mouse_move_root.update(cx, |view, cx| {
                    view.update_terminal_resize(event.position.y, viewport_height, cx)
                });
                if handled {
                    cx.stop_propagation();
                }
            });

            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
                    return;
                }
                let handled = mouse_up_root.update(cx, |view, cx| view.end_terminal_resize(cx));
                if handled {
                    cx.stop_propagation();
                }
            });
        },
    )
    .h(px(7.0))
    .w_full()
    .flex_shrink_0()
}

impl RootView {
    pub(super) fn set_terminal_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal_open == open {
            return;
        }
        self.terminal_open = open;
        self.terminal_drag_origin = None;
        if open {
            self.terminal
                .update(cx, |terminal, cx| terminal.activate(cx));
            window.focus(&self.terminal.read(cx).focus_handle(cx));
        } else {
            window.focus(&self.composer.read(cx).focus_handle(cx));
        }
        cx.notify();
    }

    pub(super) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_terminal_open(!self.terminal_open, window, cx);
    }

    pub(super) fn on_toggle_terminal(
        &mut self,
        _: &ToggleTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_terminal(window, cx);
    }

    pub(super) fn on_terminal_panel_event(
        &mut self,
        event: &TerminalPanelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TerminalPanelEvent::CloseRequested => self.set_terminal_open(false, window, cx),
        }
    }

    pub(super) fn begin_terminal_resize(&mut self, pointer_y: Pixels, cx: &mut Context<Self>) {
        self.terminal_drag_origin = Some((pointer_y, self.terminal_height));
        cx.notify();
    }

    pub(super) fn update_terminal_resize(
        &mut self,
        pointer_y: Pixels,
        viewport_height: Pixels,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((start_y, start_height)) = self.terminal_drag_origin else {
            return false;
        };
        let delta = f32::from(start_y - pointer_y);
        let max_height = (f32::from(viewport_height) - theme::TITLE_H - 210.0).max(180.0);
        let next = (start_height + delta).clamp(180.0, max_height);
        if (next - self.terminal_height).abs() >= 0.5 {
            self.terminal_height = next;
            cx.notify();
        }
        true
    }

    pub(super) fn end_terminal_resize(&mut self, cx: &mut Context<Self>) -> bool {
        if self.terminal_drag_origin.take().is_none() {
            return false;
        }
        cx.notify();
        true
    }

    pub(super) fn terminal_size(&self, window: &Window) -> TerminalSize {
        let mut width = f32::from(window.viewport_size().width);
        if self.sidebar_open {
            width -= theme::SIDE_W;
        }
        if self.history_open {
            width -= theme::HISTORY_W;
        }
        let rows = ((self.terminal_height - 50.0) / 18.0).floor().max(4.0) as u16;
        let cols = ((width - 24.0) / 7.4).floor().max(24.0) as u16;
        TerminalSize::new(rows, cols)
    }

}
