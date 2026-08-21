//! Adjustable workspace terminal panel backed by a real operating-system PTY.

use std::path::PathBuf;

use gpui::{
    ClipboardItem, Context, CursorStyle, Entity, EventEmitter, FocusHandle, FontStyle, FontWeight,
    HighlightStyle, IntoElement, KeyDownEvent, Keystroke, MouseButton, Render, ScrollWheelEvent,
    SharedString, StyledText, Task, TextRun, TextStyle, UnderlineStyle, Window, div, prelude::*,
    px, svg,
};

use crate::services::terminal::{TerminalEvent, TerminalSize, TerminalWorker};
use crate::theme;

// vt100 stores full-width cells; keep history useful without allowing an
// unbounded terminal session to grow the process indefinitely.
const SCROLLBACK_ROWS: usize = 1_000;
const TERMINAL_LINE_HEIGHT: f32 = 18.0;
const MAX_TERMINAL_TABS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalPanelEvent {
    CloseRequested,
}

struct TerminalLine {
    text: String,
    runs: Vec<TextRun>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TerminalCellStyle {
    foreground: vt100::Color,
    background: vt100::Color,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TerminalStatus {
    Dormant,
    Starting,
    Running,
    Exited(u32),
    Failed(String),
}

struct TerminalSession {
    workspace: PathBuf,
    worker: Option<TerminalWorker>,
    parser: vt100::Parser,
    size: TerminalSize,
    status: TerminalStatus,
    generation: u64,
    focus_handle: FocusHandle,
    _event_task: Option<Task<()>>,
}

impl TerminalSession {
    fn new(workspace: PathBuf, size: TerminalSize, cx: &mut Context<Self>) -> Self {
        Self {
            workspace,
            worker: None,
            parser: vt100::Parser::new(size.rows, size.cols, SCROLLBACK_ROWS),
            size,
            status: TerminalStatus::Dormant,
            generation: 1,
            focus_handle: cx.focus_handle(),
            _event_task: None,
        }
    }

    fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    fn activate(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.status,
            TerminalStatus::Dormant | TerminalStatus::Exited(_) | TerminalStatus::Failed(_)
        ) {
            self.restart(cx);
        }
    }

    fn set_workspace(&mut self, workspace: PathBuf, cx: &mut Context<Self>) {
        if self.workspace == workspace {
            return;
        }
        self.workspace = workspace;
        if self.worker.is_some() {
            self.restart(cx);
        }
    }

    fn resize(&mut self, size: TerminalSize, cx: &mut Context<Self>) {
        if self.size == size {
            return;
        }
        self.size = size;
        self.parser.screen_mut().set_size(size.rows, size.cols);
        if let Some(worker) = &self.worker {
            let _ = worker.resize(size);
        }
        cx.notify();
    }

    fn send_input(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        if bytes.is_empty() || !matches!(self.status, TerminalStatus::Running) {
            return;
        }
        if !self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.write_bytes(bytes))
        {
            self.status = TerminalStatus::Failed(
                "The terminal process is no longer accepting input. Close and reopen it to restart."
                    .to_owned(),
            );
            cx.notify();
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control && modifiers.shift && event.keystroke.key == "c" {
            self.copy_screen(cx);
            cx.stop_propagation();
            return;
        }
        if modifiers.control && modifiers.shift && event.keystroke.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                let mut bytes = Vec::with_capacity(text.len() + 12);
                if self.parser.screen().bracketed_paste() {
                    bytes.extend_from_slice(b"\x1b[200~");
                }
                bytes.extend_from_slice(text.as_bytes());
                if self.parser.screen().bracketed_paste() {
                    bytes.extend_from_slice(b"\x1b[201~");
                }
                self.send_input(bytes, cx);
            }
            cx.stop_propagation();
            return;
        }
        let Some(bytes) =
            terminal_key_bytes(&event.keystroke, self.parser.screen().application_cursor())
        else {
            return;
        };
        self.send_input(bytes, cx);
        cx.stop_propagation();
    }

    fn pump_events(
        events: async_channel::Receiver<TerminalEvent>,
        generation: u64,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        cx.spawn(async move |view, cx| {
            while let Ok(first) = events.recv().await {
                let mut batch = Vec::with_capacity(8);
                batch.push(first);
                while batch.len() < 64 {
                    let Ok(event) = events.try_recv() else {
                        break;
                    };
                    batch.push(event);
                }
                let keep_pumping = view
                    .update(cx, |view, cx| {
                        if view.generation != generation {
                            return false;
                        }
                        view.apply_events(batch, cx);
                        true
                    })
                    .unwrap_or(false);
                if !keep_pumping {
                    break;
                }
            }
        })
    }

    fn apply_events(&mut self, events: Vec<TerminalEvent>, cx: &mut Context<Self>) {
        for event in events {
            match event {
                TerminalEvent::Started { .. } => self.status = TerminalStatus::Running,
                TerminalEvent::Output(bytes) => self.parser.process(&bytes),
                TerminalEvent::Exited { code } => self.status = TerminalStatus::Exited(code),
                TerminalEvent::Error { summary } => self.status = TerminalStatus::Failed(summary),
            }
        }
        cx.notify();
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.worker = Some(TerminalWorker::spawn(self.workspace.clone(), self.size));
        self.parser = vt100::Parser::new(self.size.rows, self.size.cols, SCROLLBACK_ROWS);
        self.status = TerminalStatus::Starting;
        if let Some(worker) = &self.worker {
            self._event_task = Some(Self::pump_events(worker.events(), self.generation, cx));
        }
        cx.notify();
    }

    fn copy_screen(&self, cx: &mut Context<Self>) {
        let contents = self.parser.screen().contents();
        if !contents.trim().is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(contents));
        }
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(window.line_height()).y;
        let current = self.parser.screen().scrollback();
        let next = if delta > px(0.0) {
            current.saturating_add(3)
        } else if delta < px(0.0) {
            current.saturating_sub(3)
        } else {
            current
        };
        self.parser.screen_mut().set_scrollback(next);
        if self.parser.screen().scrollback() != current {
            cx.notify();
        }
    }

    fn output_lines(&self, default_style: &TextStyle, show_cursor: bool) -> Vec<TerminalLine> {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let cursor = show_cursor.then(|| screen.cursor_position());
        (0..rows)
            .map(|row| {
                terminal_line(
                    screen,
                    row,
                    cols,
                    cursor.and_then(|(cursor_row, col)| (cursor_row == row).then_some(col)),
                    default_style,
                )
            })
            .collect()
    }
}

struct TerminalTab {
    id: u64,
    session: Entity<TerminalSession>,
}

pub(crate) struct TerminalView {
    workspace: PathBuf,
    size: TerminalSize,
    tabs: Vec<TerminalTab>,
    active: usize,
    next_id: u64,
    fallback_focus: FocusHandle,
}

impl TerminalView {
    pub(crate) fn new(workspace: PathBuf, cx: &mut Context<Self>) -> Self {
        let size = TerminalSize::default();
        let session = cx.new(|cx| TerminalSession::new(workspace.clone(), size, cx));
        Self {
            workspace,
            size,
            tabs: vec![TerminalTab { id: 1, session }],
            active: 0,
            next_id: 2,
            fallback_focus: cx.focus_handle(),
        }
    }

    pub(crate) fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.tabs
            .get(self.active)
            .map(|tab| tab.session.read(cx).focus_handle())
            .unwrap_or_else(|| self.fallback_focus.clone())
    }

    pub(crate) fn activate(&mut self, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            self.push_tab(cx);
        }
        if let Some(tab) = self.tabs.get(self.active) {
            tab.session.update(cx, |session, cx| session.activate(cx));
        }
    }

    pub(crate) fn set_workspace(&mut self, workspace: PathBuf, cx: &mut Context<Self>) {
        if self.workspace == workspace {
            return;
        }
        self.workspace = workspace.clone();
        for tab in &self.tabs {
            tab.session.update(cx, |session, cx| {
                session.set_workspace(workspace.clone(), cx)
            });
        }
    }

    pub(crate) fn resize(&mut self, size: TerminalSize, cx: &mut Context<Self>) {
        if self.size == size {
            return;
        }
        self.size = size;
        for tab in &self.tabs {
            tab.session
                .update(cx, |session, cx| session.resize(size, cx));
        }
    }

    fn push_tab(&mut self, cx: &mut Context<Self>) -> bool {
        if self.tabs.len() >= MAX_TERMINAL_TABS {
            return false;
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let workspace = self.workspace.clone();
        let size = self.size;
        let session = cx.new(|cx| TerminalSession::new(workspace, size, cx));
        self.tabs.push(TerminalTab { id, session });
        self.active = self.tabs.len() - 1;
        true
    }

    fn add_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.push_tab(cx) {
            return;
        }
        let tab = &self.tabs[self.active];
        tab.session.update(cx, |session, cx| session.activate(cx));
        window.focus(&tab.session.read(cx).focus_handle());
        cx.notify();
    }

    fn select_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        self.active = index;
        let tab = &self.tabs[index];
        tab.session.update(cx, |session, cx| session.activate(cx));
        window.focus(&tab.session.read(cx).focus_handle());
        cx.notify();
    }

    fn close_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.active = 0;
            cx.emit(TerminalPanelEvent::CloseRequested);
            return;
        }
        self.active = active_index_after_close(self.active, index, self.tabs.len());
        let tab = &self.tabs[self.active];
        window.focus(&tab.session.read(cx).focus_handle());
        cx.notify();
    }
}

fn active_index_after_close(active: usize, removed: usize, remaining: usize) -> usize {
    if removed < active {
        active - 1
    } else {
        active.min(remaining.saturating_sub(1))
    }
}

fn terminal_key_bytes(keystroke: &Keystroke, application_cursor: bool) -> Option<Vec<u8>> {
    let modifiers = keystroke.modifiers;
    // Let the root-level terminal toggle keep ownership of Ctrl+backtick.
    if modifiers.control && keystroke.key == "`" {
        return None;
    }

    let named = match keystroke.key.as_str() {
        "space" => Some(b" ".as_slice()),
        "enter" => Some(b"\r".as_slice()),
        "backspace" => Some(b"\x7f".as_slice()),
        "tab" if modifiers.shift => Some(b"\x1b[Z".as_slice()),
        "tab" => Some(b"\t".as_slice()),
        "escape" => Some(b"\x1b".as_slice()),
        "up" if application_cursor => Some(b"\x1bOA".as_slice()),
        "down" if application_cursor => Some(b"\x1bOB".as_slice()),
        "right" if application_cursor => Some(b"\x1bOC".as_slice()),
        "left" if application_cursor => Some(b"\x1bOD".as_slice()),
        "up" => Some(b"\x1b[A".as_slice()),
        "down" => Some(b"\x1b[B".as_slice()),
        "right" => Some(b"\x1b[C".as_slice()),
        "left" => Some(b"\x1b[D".as_slice()),
        "home" => Some(b"\x1b[H".as_slice()),
        "end" => Some(b"\x1b[F".as_slice()),
        "delete" => Some(b"\x1b[3~".as_slice()),
        "insert" => Some(b"\x1b[2~".as_slice()),
        "pageup" => Some(b"\x1b[5~".as_slice()),
        "pagedown" => Some(b"\x1b[6~".as_slice()),
        "f1" => Some(b"\x1bOP".as_slice()),
        "f2" => Some(b"\x1bOQ".as_slice()),
        "f3" => Some(b"\x1bOR".as_slice()),
        "f4" => Some(b"\x1bOS".as_slice()),
        "f5" => Some(b"\x1b[15~".as_slice()),
        "f6" => Some(b"\x1b[17~".as_slice()),
        "f7" => Some(b"\x1b[18~".as_slice()),
        "f8" => Some(b"\x1b[19~".as_slice()),
        "f9" => Some(b"\x1b[20~".as_slice()),
        "f10" => Some(b"\x1b[21~".as_slice()),
        "f11" => Some(b"\x1b[23~".as_slice()),
        "f12" => Some(b"\x1b[24~".as_slice()),
        _ => None,
    };
    if let Some(named) = named {
        return Some(named.to_vec());
    }

    if modifiers.control && !modifiers.alt && !modifiers.platform {
        let bytes = keystroke.key.as_bytes();
        if bytes.len() == 1 {
            let byte = bytes[0].to_ascii_lowercase();
            if byte.is_ascii_lowercase() {
                return Some(vec![byte - b'a' + 1]);
            }
            return match byte {
                b'[' => Some(vec![0x1b]),
                b'\\' => Some(vec![0x1c]),
                b']' => Some(vec![0x1d]),
                b'^' => Some(vec![0x1e]),
                b'_' => Some(vec![0x1f]),
                _ => None,
            };
        }
        return None;
    }
    if modifiers.platform {
        return None;
    }

    let text = keystroke.key_char.as_ref()?;
    let mut bytes = Vec::with_capacity(text.len() + usize::from(modifiers.alt));
    if modifiers.alt {
        bytes.push(0x1b);
    }
    bytes.extend_from_slice(text.as_bytes());
    Some(bytes)
}

fn terminal_line(
    screen: &vt100::Screen,
    row: u16,
    cols: u16,
    cursor_col: Option<u16>,
    default_style: &TextStyle,
) -> TerminalLine {
    let last_content = (0..cols).rev().find(|col| {
        screen
            .cell(row, *col)
            .is_some_and(vt100::Cell::has_contents)
    });
    let last_content = match (last_content, cursor_col) {
        (Some(content), Some(cursor)) => Some(content.max(cursor)),
        (content, cursor) => content.or(cursor),
    };
    let Some(last_content) = last_content else {
        return TerminalLine {
            text: " ".to_owned(),
            runs: vec![default_style.to_run(1)],
        };
    };

    let mut text = String::with_capacity(usize::from(last_content) + 1);
    let mut runs = Vec::new();
    let mut active_style = None;
    let mut active_len = 0;

    for col in 0..=last_content {
        let Some(cell) = screen.cell(row, col) else {
            continue;
        };
        if cell.is_wide_continuation() {
            continue;
        }
        let contents = if cell.has_contents() {
            cell.contents()
        } else {
            " "
        };
        let style = TerminalCellStyle {
            foreground: cell.fgcolor(),
            background: cell.bgcolor(),
            bold: cell.bold(),
            dim: cell.dim(),
            italic: cell.italic(),
            underline: cell.underline(),
            inverse: cell.inverse() ^ (cursor_col == Some(col)),
        };

        if active_style.is_some_and(|active| active != style) {
            if let Some(active) = active_style {
                runs.push(terminal_text_run(active, active_len, default_style));
            }
            active_len = 0;
        }
        active_style = Some(style);
        active_len += contents.len();
        text.push_str(contents);
    }

    if let Some(active) = active_style {
        runs.push(terminal_text_run(active, active_len, default_style));
    }
    if runs.is_empty() {
        runs.push(default_style.to_run(text.len()));
    }
    TerminalLine { text, runs }
}

fn terminal_text_run(
    style: TerminalCellStyle,
    length: usize,
    default_style: &TextStyle,
) -> TextRun {
    let (foreground, background) = if style.inverse {
        (style.background, style.foreground)
    } else {
        (style.foreground, style.background)
    };
    let foreground = terminal_color(foreground, true, style.dim);
    let background = match background {
        vt100::Color::Default if !style.inverse => None,
        color => Some(terminal_color(color, false, false).into()),
    };
    let highlighted = default_style.clone().highlight(HighlightStyle {
        color: Some(foreground.into()),
        font_weight: style.bold.then_some(FontWeight::BOLD),
        font_style: style.italic.then_some(FontStyle::Italic),
        background_color: background,
        underline: style.underline.then_some(UnderlineStyle {
            thickness: px(1.0),
            color: Some(foreground.into()),
            ..Default::default()
        }),
        strikethrough: None,
        fade_out: None,
    });
    highlighted.to_run(length)
}

fn terminal_color(color: vt100::Color, foreground: bool, dim: bool) -> gpui::Rgba {
    let default = if foreground {
        theme::bone_dim()
    } else {
        theme::canvas()
    };
    let (red, green, blue) = match color {
        vt100::Color::Default => {
            return if dim {
                with_alpha(default, 0x99)
            } else {
                default
            };
        }
        vt100::Color::Rgb(red, green, blue) => (red, green, blue),
        vt100::Color::Idx(index) => xterm_color(index),
    };
    let alpha = if dim { 0x99 } else { 0xff };
    gpui::rgba((u32::from(red) << 24) | (u32::from(green) << 16) | (u32::from(blue) << 8) | alpha)
}

fn with_alpha(color: gpui::Rgba, alpha: u8) -> gpui::Rgba {
    gpui::rgba((u32::from(color) & 0xffff_ff00) | u32::from(alpha))
}

fn xterm_color(index: u8) -> (u8, u8, u8) {
    const ANSI: [(u8, u8, u8); 16] = [
        (0x1d, 0x1f, 0x21),
        (0xcc, 0x66, 0x66),
        (0xb5, 0xbd, 0x68),
        (0xf0, 0xc6, 0x74),
        (0x81, 0xa2, 0xbe),
        (0xb2, 0x94, 0xbb),
        (0x8a, 0xbe, 0xb7),
        (0xc5, 0xc8, 0xc6),
        (0x66, 0x66, 0x66),
        (0xd5, 0x4e, 0x53),
        (0xb9, 0xca, 0x4a),
        (0xe7, 0xc5, 0x47),
        (0x7a, 0xa6, 0xda),
        (0xc3, 0x97, 0xd8),
        (0x70, 0xc0, 0xb1),
        (0xea, 0xea, 0xea),
    ];
    if index < 16 {
        return ANSI[usize::from(index)];
    }
    if index < 232 {
        let cube = index - 16;
        let red = cube / 36;
        let green = (cube % 36) / 6;
        let blue = cube % 6;
        let component = |value: u8| if value == 0 { 0 } else { 55 + value * 40 };
        return (component(red), component(green), component(blue));
    }
    let gray = 8 + (index - 232) * 10;
    (gray, gray, gray)
}

impl Render for TerminalSession {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut output_style = window.text_style();
        output_style.font_family = theme::mono();
        output_style.font_size = theme::text_size(theme::T_MONO).into();
        output_style.color = theme::bone_dim().into();
        let focused = self.focus_handle.is_focused(window);
        let output_lines = self.output_lines(&output_style, focused);
        let error = match &self.status {
            TerminalStatus::Failed(summary) => Some(summary.clone()),
            _ => None,
        };

        div()
            .id("terminal-session")
            .track_focus(&self.focus_handle)
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(theme::canvas())
            .child(
                div()
                    .id("terminal-output")
                    .track_focus(&self.focus_handle)
                    .tab_index(0)
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .px(px(12.0))
                    .py(px(8.0))
                    .bg(theme::canvas())
                    // Soft left edge marks focus without a hard top border under the tab strip.
                    .border_l_2()
                    .border_color(if focused {
                        theme::focus()
                    } else {
                        gpui::rgba(0x0000_0000)
                    })
                    .cursor(CursorStyle::IBeam)
                    .font_family(theme::mono())
                    .text_size(theme::text_size(theme::T_MONO))
                    .line_height(px(TERMINAL_LINE_HEIGHT))
                    .text_color(theme::bone_dim())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _, window, _| window.focus(&view.focus_handle)),
                    )
                    .on_key_down(cx.listener(Self::on_key_down))
                    .on_scroll_wheel(cx.listener(Self::on_scroll))
                    .children(output_lines.into_iter().map(|line| {
                        div()
                            .h(px(TERMINAL_LINE_HEIGHT))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(StyledText::new(line.text).with_runs(line.runs))
                    })),
            )
            .when_some(error, |panel, error| {
                panel.child(
                    div()
                        .px(px(12.0))
                        .py(px(6.0))
                        .flex_shrink_0()
                        .bg(theme::error_wash())
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_TINY))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::error())
                        .child(error),
                )
            })
    }
}

impl EventEmitter<TerminalPanelEvent> for TerminalView {}

impl Render for TerminalView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_session = self.tabs.get(self.active).map(|tab| tab.session.clone());
        let can_add = self.tabs.len() < MAX_TERMINAL_TABS;
        let tabs = self
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                terminal_tab(tab.id, index + 1, index == self.active, cx).into_any_element()
            })
            .collect::<Vec<_>>();

        div()
            .id("workspace-terminal")
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(theme::floor())
            .child(
                div()
                    .h(px(36.0))
                    .px(px(10.0))
                    .flex_shrink_0()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .bg(theme::floor())
                    .border_b_1()
                    .border_color(theme::edge_soft())
                    .child(
                        div()
                            .id("terminal-tabs")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_x_scroll()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(2.0))
                            .children(tabs),
                    )
                    .child(add_tab_button(can_add, cx)),
            )
            .when_some(active_session, |panel, session| {
                panel.child(div().flex_1().min_h_0().child(session))
            })
    }
}

fn terminal_tab(
    id: u64,
    number: usize,
    selected: bool,
    cx: &mut Context<TerminalView>,
) -> impl IntoElement {
    let select_id = id;
    let close_id = id;
    div()
        .id(SharedString::from(format!("terminal-tab-{id}")))
        .h(px(24.0))
        .pl(px(8.0))
        .pr(px(4.0))
        .flex_shrink_0()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(5.0))
        .rounded(px(theme::RADIUS_MD))
        .bg(if selected {
            theme::panel_lift()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .border_1()
        .border_color(if selected {
            theme::edge()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .text_color(if selected {
            theme::bone()
        } else {
            theme::ash()
        })
        .tab_index(0)
        .cursor_pointer()
        .hover(|tab| {
            if selected {
                tab
            } else {
                tab.bg(theme::panel()).text_color(theme::bone())
            }
        })
        .focus(|tab| tab.border_color(theme::focus()).text_color(theme::focus()))
        .on_click(cx.listener(move |view, _, window, cx| view.select_tab(select_id, window, cx)))
        .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                view.select_tab(id, window, cx);
            }
        }))
        .child(
            svg()
                .path("icons/terminal.svg")
                .size(px(12.0))
                .flex_shrink_0()
                .text_color(if selected {
                    theme::data()
                } else {
                    theme::smoke()
                }),
        )
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .font_family(theme::main())
                .text_size(theme::text_size(theme::T_TINY))
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .child(format!("Terminal {number}")),
        )
        .child(
            div()
                .id(SharedString::from(format!("close-terminal-tab-{id}")))
                .size(px(16.0))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme::RADIUS_SM))
                .font_family(theme::sans())
                .text_size(theme::text_size(theme::T_TINY))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if selected {
                    theme::ash()
                } else {
                    theme::smoke()
                })
                .tab_index(0)
                .cursor_pointer()
                .hover(|button| button.bg(theme::panel_hover()).text_color(theme::bone()))
                .focus(|button| button.bg(theme::panel_hover()).text_color(theme::focus()))
                .on_click(cx.listener(move |view, _, window, cx| {
                    cx.stop_propagation();
                    view.close_tab(close_id, window, cx)
                }))
                .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        view.close_tab(id, window, cx);
                    }
                }))
                .child("×"),
        )
}

fn add_tab_button(enabled: bool, cx: &mut Context<TerminalView>) -> impl IntoElement {
    div()
        .id("add-terminal-tab")
        .size(px(24.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_SM))
        .font_family(theme::sans())
        .text_size(theme::text_size(theme::T_UI))
        .font_weight(FontWeight::MEDIUM)
        .text_color(if enabled {
            theme::ash()
        } else {
            theme::smoke()
        })
        .when(enabled, |button| {
            button
                .tab_index(0)
                .cursor_pointer()
                .hover(|button| button.bg(theme::panel()).text_color(theme::bone()))
                .active(|button| button.bg(theme::panel_lift()))
                .focus(|button| {
                    button
                        .border_1()
                        .border_color(theme::focus())
                        .text_color(theme::focus())
                })
                .on_click(cx.listener(|view, _, window, cx| view.add_tab(window, cx)))
                .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        view.add_tab(window, cx);
                    }
                }))
        })
        .child("+")
}

#[cfg(test)]
mod tests {
    use gpui::Modifiers;

    use super::*;

    fn key(key: &str, key_char: Option<&str>, modifiers: Modifiers) -> Keystroke {
        Keystroke {
            modifiers,
            key: key.to_owned(),
            key_char: key_char.map(ToOwned::to_owned),
        }
    }

    #[test]
    fn active_tab_stays_stable_when_other_tabs_close() {
        assert_eq!(active_index_after_close(2, 0, 2), 1);
        assert_eq!(active_index_after_close(1, 1, 2), 1);
        assert_eq!(active_index_after_close(2, 2, 2), 1);
        assert_eq!(MAX_TERMINAL_TABS, 8);
    }

    #[test]
    fn terminal_keymap_supports_text_navigation_and_control_input() {
        assert_eq!(
            terminal_key_bytes(&key("a", Some("a"), Modifiers::default()), false),
            Some(b"a".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(&key("space", None, Modifiers::default()), false),
            Some(b" ".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(&key("enter", None, Modifiers::default()), false),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(&key("up", None, Modifiers::default()), false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(&key("up", None, Modifiers::default()), true),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(
                &key(
                    "c",
                    None,
                    Modifiers {
                        control: true,
                        ..Default::default()
                    },
                ),
                false,
            ),
            Some(vec![0x03])
        );
        assert_eq!(
            terminal_key_bytes(
                &key(
                    "x",
                    Some("x"),
                    Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                ),
                false,
            ),
            Some(b"\x1bx".to_vec())
        );
    }

    #[test]
    fn root_toggle_keeps_ctrl_backtick() {
        assert_eq!(
            terminal_key_bytes(
                &key(
                    "`",
                    Some("`"),
                    Modifiers {
                        control: true,
                        ..Default::default()
                    },
                ),
                false,
            ),
            None
        );
    }

    #[test]
    fn vt_parser_applies_ansi_cursor_and_clear_sequences() {
        let mut parser = vt100::Parser::new(4, 20, 20);
        parser.process(b"first\r\nsecond\x1b[1A\rupdated");
        let rows = parser.screen().rows(0, 20).collect::<Vec<_>>();
        assert_eq!(rows[0], "updated");
        assert_eq!(rows[1], "second");

        parser.process(b"\x1b[2J\x1b[Hready");
        assert_eq!(parser.screen().rows(0, 20).next().unwrap(), "ready");
    }

    #[test]
    fn vt_parser_strips_control_sequences_from_visible_text() {
        let mut parser = vt100::Parser::new(2, 20, 0);
        parser.process(b"\x1b[31mred\x1b[0m normal");
        assert_eq!(parser.screen().rows(0, 20).next().unwrap(), "red normal");
    }

    #[test]
    fn xterm_palette_covers_ansi_cube_and_grayscale() {
        assert_eq!(xterm_color(0), (0x1d, 0x1f, 0x21));
        assert_eq!(xterm_color(16), (0, 0, 0));
        assert_eq!(xterm_color(231), (255, 255, 255));
        assert_eq!(xterm_color(232), (8, 8, 8));
        assert_eq!(xterm_color(255), (238, 238, 238));
    }
}
