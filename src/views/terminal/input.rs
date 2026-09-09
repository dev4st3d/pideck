use std::ops::Range;

use gpui::{
    Bounds, Context, EntityInputHandler, KeyDownEvent, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollWheelEvent, UTF16Selection, Window, px,
};

use super::{TERMINAL_LINE_HEIGHT, TerminalSession, TerminalStatus, terminal_key_bytes};
use crate::services::terminal_engine::{
    MouseAction, MouseButton as TerminalMouseButton, MouseModifiers, SelectionKind,
};

impl TerminalSession {
    pub(super) fn on_key_down(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control && modifiers.shift && event.keystroke.key == "a" {
            self.engine.select_all();
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if modifiers.control && modifiers.shift && event.keystroke.key == "c" {
            self.copy_selection(cx);
            cx.stop_propagation();
            return;
        }
        if modifiers.control && modifiers.shift && event.keystroke.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                self.send_input(paste_bytes(&text, self.engine.modes().bracketed_paste), cx);
            }
            cx.stop_propagation();
            return;
        }
        if modifiers.shift
            && !modifiers.control
            && matches!(event.keystroke.key.as_str(), "pageup" | "pagedown")
        {
            let rows = i32::from(self.size.rows);
            self.engine.scroll(if event.keystroke.key == "pageup" {
                rows
            } else {
                -rows
            });
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // Native text input owns printable text and IME composition. Handling
        // key_char here as well would duplicate committed platform input.
        if !modifiers.platform
            && !modifiers.alt
            && !modifiers.control
            && (event.keystroke.key_char.is_some() || event.keystroke.key == "space")
        {
            return;
        }
        // Windows reports AltGr text with Control and Alt held.
        if modifiers.alt && modifiers.control && event.keystroke.key_char.is_some() {
            return;
        }
        let Some(bytes) = terminal_key_bytes(&event.keystroke, self.engine.modes().app_cursor)
        else {
            return;
        };
        self.send_input(bytes, cx);
        cx.stop_propagation();
    }

    pub(super) fn ensure_focus_tracking(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self._focus_subscriptions.is_some() {
            return;
        }
        let handle = self.focus_handle.clone();
        let focused = cx.on_focus(&handle, window, |session, _, cx| {
            if session.engine.modes().focus_reporting {
                session.write_pty(b"\x1b[I".to_vec(), cx);
            }
            session.start_cursor_blink(cx);
            cx.notify();
        });
        let blurred = cx.on_blur(&handle, window, |session, _, cx| {
            if session.engine.modes().focus_reporting {
                session.write_pty(b"\x1b[O".to_vec(), cx);
            }
            session.selecting = false;
            session.pressed_button = None;
            session._cursor_task.take();
            session.cursor_visible = true;
            cx.notify();
        });
        self._focus_subscriptions = Some((focused, blurred));
        if handle.is_focused(window) {
            self.start_cursor_blink(cx);
        }
    }

    fn start_cursor_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_visible = true;
        self._cursor_task.take();
        if !crate::services::accessibility::motion_enabled() {
            return;
        }
        self._cursor_task = Some(cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(600))
                    .await;
                let alive = view
                    .update(cx, |view, cx| {
                        if !crate::services::accessibility::motion_enabled() {
                            view.cursor_visible = true;
                            cx.notify();
                            return false;
                        }
                        if view.cursor_blinking {
                            view.cursor_visible = !view.cursor_visible;
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        }));
    }

    fn mouse_cell(&self, point: Point<Pixels>) -> Option<(usize, usize, bool)> {
        self.geometry
            .as_ref()
            .map(|geometry| geometry.cell_at(point, self.size))
    }

    pub(super) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        let Some((row, col, right_half)) = self.mouse_cell(event.position) else {
            return;
        };
        let button = terminal_mouse_button(event.button);
        if self.engine.modes().mouse_tracking && !event.modifiers.shift {
            self.pressed_button = Some(button);
            self.last_mouse_cell = Some((row, col));
            if let Some(bytes) = self.engine.mouse_report(
                button,
                MouseAction::Press,
                row,
                col,
                mouse_modifiers(event.modifiers),
            ) {
                self.write_pty(bytes, cx);
            }
        } else if event.button == MouseButton::Left {
            self.selecting = true;
            let kind = match event.click_count {
                2 => SelectionKind::Semantic,
                3.. => SelectionKind::Lines,
                _ if event.modifiers.alt => SelectionKind::Block,
                _ => SelectionKind::Simple,
            };
            self.engine.select_start(kind, row, col, right_half);
            cx.notify();
        }
        cx.stop_propagation();
    }

    pub(super) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((row, col, right_half)) = self.mouse_cell(event.position) else {
            return;
        };
        if self.selecting {
            if let Some(geometry) = &self.geometry {
                if event.position.y < geometry.bounds.top() {
                    self.engine.scroll(1);
                } else if event.position.y > geometry.bounds.bottom() {
                    self.engine.scroll(-1);
                }
            }
            self.engine.select_update(row, col, right_half);
            cx.notify();
        } else if !event.modifiers.shift && self.last_mouse_cell != Some((row, col)) {
            let button = self.pressed_button.unwrap_or(TerminalMouseButton::None);
            if let Some(bytes) = self.engine.mouse_report(
                button,
                MouseAction::Motion,
                row,
                col,
                mouse_modifiers(event.modifiers),
            ) {
                self.write_pty(bytes, cx);
            }
        }
        self.last_mouse_cell = Some((row, col));
    }

    pub(super) fn on_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(button) = self.pressed_button.take()
            && let Some((row, col, _)) = self.mouse_cell(event.position)
            && let Some(bytes) = self.engine.mouse_report(
                button,
                MouseAction::Release,
                row,
                col,
                mouse_modifiers(event.modifiers),
            )
        {
            self.write_pty(bytes, cx);
        }
        self.selecting = false;
    }

    pub(super) fn on_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(px(TERMINAL_LINE_HEIGHT)).y;
        if delta == px(0.0) {
            return;
        }
        let lines = (f32::from(delta) / TERMINAL_LINE_HEIGHT)
            .abs()
            .ceil()
            .clamp(1.0, 20.0) as i32;
        let upward = delta > px(0.0);
        let modes = self.engine.modes();
        if !event.modifiers.shift && modes.mouse_tracking {
            let Some((row, col, _)) = self.mouse_cell(event.position) else {
                return;
            };
            let button = if upward {
                TerminalMouseButton::WheelUp
            } else {
                TerminalMouseButton::WheelDown
            };
            for _ in 0..lines {
                if let Some(bytes) = self.engine.mouse_report(
                    button,
                    MouseAction::Press,
                    row,
                    col,
                    mouse_modifiers(event.modifiers),
                ) {
                    self.write_pty(bytes, cx);
                }
            }
        } else if !event.modifiers.shift && modes.alternate_screen && modes.alternate_scroll {
            let key = if upward { b"\x1bOA" } else { b"\x1bOB" };
            self.write_pty(key.repeat(lines as usize), cx);
        } else {
            self.engine.scroll(if upward { lines } else { -lines });
        }
        cx.stop_propagation();
        cx.notify();
    }
}

fn terminal_mouse_button(button: MouseButton) -> TerminalMouseButton {
    match button {
        MouseButton::Left => TerminalMouseButton::Left,
        MouseButton::Middle => TerminalMouseButton::Middle,
        MouseButton::Right => TerminalMouseButton::Right,
        MouseButton::Navigate(_) => TerminalMouseButton::None,
    }
}

fn mouse_modifiers(modifiers: Modifiers) -> MouseModifiers {
    MouseModifiers {
        shift: modifiers.shift,
        alt: modifiers.alt,
        control: modifiers.control,
    }
}

fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    // Escape cannot be nested safely inside bracketed paste delimiters.
    let text = text.replace('\x1b', "");
    if bracketed {
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.replace("\r\n", "\n").replace('\n', "\r").into_bytes()
    }
}

// The terminal program owns its input buffer. Only uncommitted platform IME
// text lives here; committing writes UTF-8 once and clears this composition.
impl EntityInputHandler for TerminalSession {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let bytes = utf16_byte_range(&self.composition, range);
        *actual_range = Some(
            self.composition[..bytes.start].encode_utf16().count()
                ..self.composition[..bytes.end].encode_utf16().count(),
        );
        Some(self.composition[bytes].to_owned())
    }

    fn selected_text_range(
        &mut self,
        ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if !ignore_disabled_input && !matches!(self.status, TerminalStatus::Running) {
            return None;
        }
        Some(UTF16Selection {
            range: self.composition_selection.clone(),
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        (!self.composition.is_empty()).then(|| 0..self.composition.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.composition.clear();
        self.composition_selection = 0..0;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composition.clear();
        self.composition_selection = 0..0;
        self.send_input(text.as_bytes().to_vec(), cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.status, TerminalStatus::Running) {
            return;
        }
        let bytes = range
            .map(|range| utf16_byte_range(&self.composition, range))
            .unwrap_or(0..self.composition.len());
        self.composition.replace_range(bytes, text);
        let len = self.composition.encode_utf16().count();
        self.composition_selection = selected
            .map(|range| range.start.min(len)..range.end.min(len))
            .unwrap_or(len..len);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.geometry
            .as_ref()
            .map(|geometry| geometry.cursor_bounds)
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.composition_selection.end)
    }
}

fn utf16_byte_range(text: &str, range: Range<usize>) -> Range<usize> {
    let mut utf16 = 0;
    let mut start = text.len();
    let mut end = text.len();
    for (byte, character) in text.char_indices() {
        let next = utf16 + character.len_utf16();
        if utf16 <= range.start && range.start < next {
            start = byte;
        }
        if utf16 < range.end && range.end <= next {
            end = byte + character.len_utf8();
        }
        utf16 = next;
    }
    if range.is_empty() {
        end = start;
    }
    start..end.max(start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, size};

    #[gpui::test]
    fn native_selection_bypasses_mouse_capture_and_copies_only_selected_text(
        cx: &mut gpui::TestAppContext,
    ) {
        let (session, cx) = cx.add_window_view(|_, cx| {
            TerminalSession::new(
                std::path::PathBuf::from("synthetic-project"),
                super::super::TerminalSize::new(8, 80),
                cx,
            )
        });
        cx.update(|window, app| {
            session.update(app, |session, cx| {
                session.engine.feed(b"hello world\x1b[?1003h\x1b[?1006h");
                let bounds = Bounds::new(point(px(12.0), px(12.0)), size(px(640.0), px(144.0)));
                session.geometry = Some(super::super::TerminalGeometry {
                    bounds,
                    cell_width: px(8.0),
                    cell_height: px(18.0),
                    cursor_bounds: Bounds::new(bounds.origin, size(px(8.0), px(18.0))),
                });
                session.on_mouse_down(
                    &MouseDownEvent {
                        button: MouseButton::Left,
                        position: point(px(13.0), px(13.0)),
                        modifiers: Modifiers {
                            shift: true,
                            ..Default::default()
                        },
                        click_count: 2,
                        ..Default::default()
                    },
                    window,
                    cx,
                );
                assert!(session.selecting);
                assert_eq!(session.engine.selected_text().as_deref(), Some("hello"));
                session.copy_selection(cx);
                assert_eq!(
                    cx.read_from_clipboard()
                        .and_then(|item| item.text())
                        .as_deref(),
                    Some("hello")
                );
                session.on_mouse_up(&MouseUpEvent::default(), window, cx);
                assert!(!session.selecting);
            });
        });
    }

    #[gpui::test]
    fn native_ime_composition_is_local_until_committed(cx: &mut gpui::TestAppContext) {
        let (session, cx) = cx.add_window_view(|_, cx| {
            TerminalSession::new(
                std::path::PathBuf::from("synthetic-project"),
                super::super::TerminalSize::new(8, 80),
                cx,
            )
        });
        cx.update(|window, app| {
            session.update(app, |session, cx| {
                session.status = TerminalStatus::Running;
                session.replace_and_mark_text_in_range(None, "A😀B", Some(1..3), window, cx);
                assert_eq!(session.composition, "A😀B");
                assert_eq!(session.composition_selection, 1..3);
                assert_eq!(session.marked_text_range(window, cx), Some(0..4));
                assert_eq!(session.status, TerminalStatus::Running);
                session.unmark_text(window, cx);
                assert!(session.composition.is_empty());
            });
        });
    }

    #[test]
    fn paste_preserves_bracketed_multiline_input_and_blocks_nested_delimiters() {
        assert_eq!(
            paste_bytes("first\nsecond", true),
            b"\x1b[200~first\nsecond\x1b[201~"
        );
        assert_eq!(paste_bytes("first\r\nsecond", false), b"first\rsecond");
        assert_eq!(paste_bytes("\x1b[201~", true), b"\x1b[200~[201~\x1b[201~");
    }

    #[test]
    fn ime_ranges_never_split_unicode_scalars() {
        assert_eq!(utf16_byte_range("A😀B", 1..3), 1..5);
        assert_eq!(utf16_byte_range("A😀B", 2..3), 1..5);
        assert_eq!(utf16_byte_range("A😀B", 4..4), 6..6);
        assert_eq!(utf16_byte_range("", 0..0), 0..0);
    }
}
