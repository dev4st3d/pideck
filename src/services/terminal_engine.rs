//! Alacritty terminal state and protocol handling, independent of the UI and PTY.

use std::borrow::Cow;
use std::sync::mpsc;
use std::time::Instant;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Osc52, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Processor, Rgb};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalColor {
    DefaultForeground,
    DefaultBackground,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalCell {
    pub(crate) text: Cow<'static, str>,
    pub(crate) fg: TerminalColor,
    pub(crate) bg: TerminalColor,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) underline: bool,
    pub(crate) strikethrough: bool,
    pub(crate) dim: bool,
    pub(crate) inverse: bool,
    pub(crate) selected: bool,
    pub(crate) wide: bool,
    pub(crate) wide_spacer: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalCursorShape {
    Block,
    Beam,
    Underline,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TerminalModes {
    pub(crate) app_cursor: bool,
    pub(crate) app_keypad: bool,
    pub(crate) bracketed_paste: bool,
    pub(crate) focus_reporting: bool,
    pub(crate) mouse_tracking: bool,
    pub(crate) alternate_screen: bool,
    pub(crate) alternate_scroll: bool,
}

pub(crate) struct TerminalSnapshot {
    pub(crate) rows: Vec<Vec<TerminalCell>>,
    pub(crate) cursor: Option<(usize, usize)>,
    pub(crate) cursor_shape: TerminalCursorShape,
    pub(crate) cursor_blinking: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelectionKind {
    Simple,
    Semantic,
    Lines,
    Block,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MouseButton {
    Left,
    Middle,
    Right,
    None,
    WheelUp,
    WheelDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MouseAction {
    Press,
    Release,
    Motion,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MouseModifiers {
    pub(crate) shift: bool,
    pub(crate) alt: bool,
    pub(crate) control: bool,
}

struct EventSender(mpsc::Sender<Event>);

impl EventListener for EventSender {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(_)
            | Event::Title(_)
            | Event::ResetTitle
            | Event::ColorRequest(_, _)
            | Event::TextAreaSizeRequest(_) => {
                let _ = self.0.send(event);
            }
            // Clipboard requests never cross this boundary. Cursor and repaint
            // events are represented by the next snapshot, without queueing.
            Event::ClipboardLoad(_, _)
            | Event::ClipboardStore(_, _)
            | Event::MouseCursorDirty
            | Event::CursorBlinkingChange
            | Event::Wakeup
            | Event::Bell
            | Event::Exit
            | Event::ChildExit(_) => {}
        }
    }
}

#[derive(Clone, Copy)]
struct GridSize {
    rows: u16,
    cols: u16,
}

impl GridSize {
    fn new(rows: u16, cols: u16) -> Self {
        Self {
            rows: rows.max(1),
            cols: cols.max(2),
        }
    }
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        usize::from(self.rows)
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.rows)
    }

    fn columns(&self) -> usize {
        usize::from(self.cols)
    }
}

pub(crate) struct TerminalEngine {
    term: Term<EventSender>,
    parser: Processor,
    events: mpsc::Receiver<Event>,
    title: String,
    cell_width: u16,
    cell_height: u16,
    default_foreground: Rgb,
    default_background: Rgb,
}

impl TerminalEngine {
    pub(crate) fn new(rows: u16, cols: u16) -> Self {
        let (sender, events) = mpsc::channel();
        let config = Config {
            scrolling_history: 10_000,
            // Do not advertise input protocols the UI does not encode.
            kitty_keyboard: false,
            osc52: Osc52::Disabled,
            ..Config::default()
        };
        Self {
            term: Term::new(config, &GridSize::new(rows, cols), EventSender(sender)),
            parser: Processor::new(),
            events,
            title: String::new(),
            cell_width: 1,
            cell_height: 1,
            default_foreground: Rgb {
                r: 0xc5,
                g: 0xc8,
                b: 0xc6,
            },
            default_background: Rgb {
                r: 0x1d,
                g: 0x1f,
                b: 0x21,
            },
        }
    }

    /// Responses must be written back to this terminal's PTY without user-input transforms.
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut replies = self.flush_sync_timeout(Instant::now());
        self.parser.advance(&mut self.term, bytes);
        replies.extend(self.drain_events());
        replies
    }

    /// The UI schedules a wakeup here so an interrupted synchronized redraw cannot freeze output.
    pub(crate) fn sync_deadline(&self) -> Option<Instant> {
        self.parser.sync_timeout().sync_timeout()
    }

    pub(crate) fn flush_sync_timeout(&mut self, now: Instant) -> Vec<u8> {
        if self.sync_deadline().is_some_and(|deadline| deadline <= now) {
            self.parser.stop_sync(&mut self.term);
        }
        self.drain_events()
    }

    fn drain_events(&mut self) -> Vec<u8> {
        let mut replies = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::PtyWrite(text) => replies.extend_from_slice(text.as_bytes()),
                Event::Title(title) => {
                    self.title = title
                        .chars()
                        .filter(|character| !character.is_control())
                        .take(256)
                        .collect();
                }
                Event::ResetTitle => self.title.clear(),
                Event::ColorRequest(index, format) => {
                    replies.extend_from_slice(format(self.color_rgb(index)).as_bytes());
                }
                Event::TextAreaSizeRequest(format) => {
                    let (rows, cols) = self.size();
                    replies.extend_from_slice(
                        format(WindowSize {
                            num_lines: rows,
                            num_cols: cols,
                            cell_width: self.cell_width,
                            cell_height: self.cell_height,
                        })
                        .as_bytes(),
                    );
                }
                Event::ClipboardLoad(_, _)
                | Event::ClipboardStore(_, _)
                | Event::MouseCursorDirty
                | Event::CursorBlinkingChange
                | Event::Wakeup
                | Event::Bell
                | Event::Exit
                | Event::ChildExit(_) => {}
            }
        }
        replies
    }

    pub(crate) fn resize(&mut self, rows: u16, cols: u16) {
        self.term.resize(GridSize::new(rows, cols));
    }

    pub(crate) fn size(&self) -> (u16, u16) {
        (self.term.screen_lines() as u16, self.term.columns() as u16)
    }

    pub(crate) fn set_cell_size(&mut self, width: u16, height: u16) {
        self.cell_width = width.max(1);
        self.cell_height = height.max(1);
    }

    pub(crate) fn set_default_colors(
        &mut self,
        foreground: (u8, u8, u8),
        background: (u8, u8, u8),
    ) {
        self.default_foreground = Rgb {
            r: foreground.0,
            g: foreground.1,
            b: foreground.2,
        };
        self.default_background = Rgb {
            r: background.0,
            g: background.1,
            b: background.2,
        };
    }

    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    pub(crate) fn modes(&self) -> TerminalModes {
        let mode = self.term.mode();
        TerminalModes {
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            app_keypad: mode.contains(TermMode::APP_KEYPAD),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
            focus_reporting: mode.contains(TermMode::FOCUS_IN_OUT),
            mouse_tracking: mode.intersects(TermMode::MOUSE_MODE),
            alternate_screen: mode.contains(TermMode::ALT_SCREEN),
            alternate_scroll: mode.contains(TermMode::ALTERNATE_SCROLL),
        }
    }

    pub(crate) fn scroll(&mut self, delta: i32) {
        self.term.scroll_display(Scroll::Delta(delta));
    }

    pub(crate) fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    pub(crate) fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    fn viewport_point(&self, row: usize, col: usize) -> Point {
        Point::new(
            Line(row.min(self.term.screen_lines() - 1) as i32 - self.display_offset() as i32),
            Column(col.min(self.term.columns() - 1)),
        )
    }

    pub(crate) fn select_start(
        &mut self,
        kind: SelectionKind,
        row: usize,
        col: usize,
        right_half: bool,
    ) {
        let kind = match kind {
            SelectionKind::Simple => SelectionType::Simple,
            SelectionKind::Semantic => SelectionType::Semantic,
            SelectionKind::Lines => SelectionType::Lines,
            SelectionKind::Block => SelectionType::Block,
        };
        self.term.selection = Some(Selection::new(
            kind,
            self.viewport_point(row, col),
            side(right_half),
        ));
    }

    pub(crate) fn select_update(&mut self, row: usize, col: usize, right_half: bool) {
        let point = self.viewport_point(row, col);
        if let Some(selection) = self.term.selection.as_mut() {
            selection.update(point, side(right_half));
        }
    }

    pub(crate) fn select_all(&mut self) {
        let mut selection = Selection::new(
            SelectionType::Simple,
            Point::new(self.term.topmost_line(), Column(0)),
            Side::Left,
        );
        selection.update(
            Point::new(self.term.bottommost_line(), self.term.last_column()),
            Side::Right,
        );
        self.term.selection = Some(selection);
    }

    pub(crate) fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    pub(crate) fn selected_text(&self) -> Option<String> {
        self.term
            .selection_to_string()
            .filter(|text| !text.is_empty())
    }

    pub(crate) fn visible_text(&self) -> String {
        self.term.bounds_to_string(
            self.viewport_point(0, 0),
            self.viewport_point(self.term.screen_lines() - 1, self.term.columns() - 1),
        )
    }

    pub(crate) fn snapshot(&self) -> TerminalSnapshot {
        let content = self.term.renderable_content();
        let mut rows: Vec<Vec<TerminalCell>> = (0..self.term.screen_lines())
            .map(|_| Vec::with_capacity(self.term.columns()))
            .collect();
        for indexed in content.display_iter {
            let row = (indexed.point.line.0 + content.display_offset as i32) as usize;
            let cell = indexed.cell;
            let flags = cell.flags;
            let wide_spacer =
                flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
            let text = if flags.contains(Flags::HIDDEN) || wide_spacer {
                Cow::Borrowed("")
            } else {
                cell_text(cell.c, cell.zerowidth().unwrap_or_default())
            };
            let selected = content.selection.is_some_and(|selection| {
                selection.contains(indexed.point)
                    || (flags.contains(Flags::WIDE_CHAR)
                        && selection
                            .contains(Point::new(indexed.point.line, indexed.point.column + 1)))
            });
            rows[row].push(TerminalCell {
                text,
                fg: self.render_color(cell.fg),
                bg: self.render_color(cell.bg),
                bold: flags.contains(Flags::BOLD),
                italic: flags.contains(Flags::ITALIC),
                underline: flags.intersects(Flags::ALL_UNDERLINES),
                strikethrough: flags.contains(Flags::STRIKEOUT),
                dim: flags.contains(Flags::DIM),
                inverse: flags.contains(Flags::INVERSE),
                selected,
                wide: flags.contains(Flags::WIDE_CHAR),
                wide_spacer,
            });
        }
        let cursor_row = content.cursor.point.line.0 + content.display_offset as i32;
        let cursor = (content.cursor.shape != CursorShape::Hidden
            && cursor_row >= 0
            && cursor_row < self.term.screen_lines() as i32)
            .then_some((cursor_row as usize, content.cursor.point.column.0));
        TerminalSnapshot {
            rows,
            cursor,
            cursor_shape: match content.cursor.shape {
                CursorShape::Beam => TerminalCursorShape::Beam,
                CursorShape::Underline => TerminalCursorShape::Underline,
                CursorShape::Block | CursorShape::HollowBlock | CursorShape::Hidden => {
                    TerminalCursorShape::Block
                }
            },
            cursor_blinking: self.term.cursor_style().blinking,
        }
    }

    /// Encode xterm mouse protocols. Shift always reserves the gesture for local selection.
    pub(crate) fn mouse_report(
        &self,
        button: MouseButton,
        action: MouseAction,
        row: usize,
        col: usize,
        modifiers: MouseModifiers,
    ) -> Option<Vec<u8>> {
        let mode = self.term.mode();
        if modifiers.shift || !mode.intersects(TermMode::MOUSE_MODE) {
            return None;
        }
        let wheel = matches!(button, MouseButton::WheelUp | MouseButton::WheelDown);
        if wheel && action != MouseAction::Press {
            return None;
        }
        if action == MouseAction::Motion
            && !mode.contains(TermMode::MOUSE_MOTION)
            && !(mode.contains(TermMode::MOUSE_DRAG) && button != MouseButton::None)
        {
            return None;
        }
        if button == MouseButton::None && action != MouseAction::Motion {
            return None;
        }
        let mut code = match button {
            MouseButton::Left => 0_u8,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            MouseButton::None => 3,
            MouseButton::WheelUp => 64,
            MouseButton::WheelDown => 65,
        };
        let sgr = mode.contains(TermMode::SGR_MOUSE);
        if action == MouseAction::Release && !sgr {
            code = 3;
        } else if action == MouseAction::Motion {
            code += 32;
        }
        code += u8::from(modifiers.alt) * 8 + u8::from(modifiers.control) * 16;
        let row = row.min(self.term.screen_lines() - 1);
        let col = col.min(self.term.columns() - 1);
        if sgr {
            let suffix = if action == MouseAction::Release {
                'm'
            } else {
                'M'
            };
            return Some(format!("\x1b[<{code};{};{}{suffix}", col + 1, row + 1).into_bytes());
        }
        let utf8 = mode.contains(TermMode::UTF8_MOUSE);
        let max_coordinate = if utf8 { 2015 } else { 223 };
        if row >= max_coordinate || col >= max_coordinate {
            return None;
        }
        let mut bytes = vec![0x1b, b'[', b'M', code + 32];
        for coordinate in [col, row] {
            let encoded = coordinate as u32 + 33;
            if utf8 {
                let character = char::from_u32(encoded)?;
                let mut buffer = [0; 4];
                bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
            } else {
                bytes.push(encoded as u8);
            }
        }
        Some(bytes)
    }

    fn render_color(&self, color: Color) -> TerminalColor {
        let index = match color {
            Color::Spec(color) => return TerminalColor::Rgb(color.r, color.g, color.b),
            Color::Indexed(index) => usize::from(index),
            Color::Named(name) => name as usize,
        };
        if let Some(color) = self.term.colors()[index] {
            return TerminalColor::Rgb(color.r, color.g, color.b);
        }
        match color {
            Color::Named(NamedColor::Background) => TerminalColor::DefaultBackground,
            Color::Named(
                NamedColor::Foreground
                | NamedColor::BrightForeground
                | NamedColor::DimForeground
                | NamedColor::Cursor,
            ) => TerminalColor::DefaultForeground,
            Color::Indexed(index) => TerminalColor::Indexed(index),
            Color::Named(name) if (name as usize) < 16 => TerminalColor::Indexed(name as u8),
            Color::Named(_) => {
                let color = self.color_rgb(index);
                TerminalColor::Rgb(color.r, color.g, color.b)
            }
            Color::Spec(color) => TerminalColor::Rgb(color.r, color.g, color.b),
        }
    }

    fn color_rgb(&self, index: usize) -> Rgb {
        if index < alacritty_terminal::term::color::COUNT
            && let Some(color) = self.term.colors()[index]
        {
            return color;
        }
        if index < 256 {
            let (r, g, b) = xterm_color(index as u8);
            return Rgb { r, g, b };
        }
        if index == NamedColor::Background as usize {
            return self.default_background;
        }
        if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize).contains(&index) {
            let (r, g, b) = xterm_color((index - NamedColor::DimBlack as usize) as u8);
            return Rgb {
                r: (f32::from(r) * 0.66) as u8,
                g: (f32::from(g) * 0.66) as u8,
                b: (f32::from(b) * 0.66) as u8,
            };
        }
        self.default_foreground
    }
}

fn cell_text(character: char, zerowidth: &[char]) -> Cow<'static, str> {
    const PRINTABLE_ASCII: &str = r##" !"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\]^_`abcdefghijklmnopqrstuvwxyz{|}~"##;
    let character = if character == '\t' { ' ' } else { character };
    if zerowidth.is_empty() && (' '..='~').contains(&character) {
        let index = character as usize - usize::from(b' ');
        return Cow::Borrowed(&PRINTABLE_ASCII[index..index + 1]);
    }
    let capacity = character.len_utf8()
        + zerowidth
            .iter()
            .map(|character| character.len_utf8())
            .sum::<usize>();
    let mut text = String::with_capacity(capacity);
    text.push(character);
    text.extend(zerowidth);
    Cow::Owned(text)
}

fn side(right_half: bool) -> Side {
    if right_half { Side::Right } else { Side::Left }
}

pub(crate) fn xterm_color(index: u8) -> (u8, u8, u8) {
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
        let component = |value: u8| if value == 0 { 0 } else { 55 + value * 40 };
        return (
            component(cube / 36),
            component((cube % 36) / 6),
            component(cube % 6),
        );
    }
    let gray = 8 + (index - 232) * 10;
    (gray, gray, gray)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(
        engine: &TerminalEngine,
        button: MouseButton,
        action: MouseAction,
    ) -> Option<Vec<u8>> {
        engine.mouse_report(button, action, 2, 4, MouseModifiers::default())
    }

    #[test]
    fn cursor_queries_reflect_real_grid_and_incremental_parser_state() {
        let mut engine = TerminalEngine::new(24, 80);
        assert!(engine.feed(b"hello\x1b[").is_empty());
        assert_eq!(engine.feed(b"6n"), b"\x1b[1;6R");
        assert_eq!(engine.feed(b"\x1b[7;19H\x1b[6n"), b"\x1b[7;19R");
        assert_eq!(engine.feed(b"\x1b[5n"), b"\x1b[0n");
        assert_eq!(engine.snapshot().cursor, Some((6, 18)));
    }

    #[test]
    fn terminal_modes_follow_set_and_reset_sequences() {
        let mut engine = TerminalEngine::new(24, 80);
        engine.feed(b"\x1b[?1h\x1b=\x1b[?2004h\x1b[?1004h\x1b[?1002h");
        let modes = engine.modes();
        assert!(modes.app_cursor && modes.app_keypad && modes.bracketed_paste);
        assert!(modes.focus_reporting && modes.mouse_tracking);
        engine.feed(b"\x1b[?1l\x1b>\x1b[?2004l\x1b[?1004l\x1b[?1002l");
        let modes = engine.modes();
        assert!(!modes.app_cursor && !modes.app_keypad && !modes.bracketed_paste);
        assert!(!modes.focus_reporting && !modes.mouse_tracking);
    }

    #[test]
    fn alternate_screen_restores_primary_content_and_cursor() {
        let mut engine = TerminalEngine::new(4, 20);
        engine.feed(b"primary\x1b[?1049h\x1b[Halternate");
        assert!(engine.modes().alternate_screen);
        assert!(engine.visible_text().starts_with("alternate"));
        engine.feed(b"\x1b[?1049l");
        assert!(!engine.modes().alternate_screen);
        assert!(engine.visible_text().starts_with("primary"));
        assert_eq!(engine.snapshot().cursor, Some((0, 7)));
    }

    #[test]
    fn unicode_and_styles_preserve_combining_marks_and_wide_cells() {
        let mut engine = TerminalEngine::new(4, 20);
        let bytes = "e\u{301}界".as_bytes();
        engine.feed(&bytes[..2]);
        engine.feed(&bytes[2..]);
        engine.feed(b"\x1b[1;2;3;4;9;38;2;10;20;30;48;5;123mX");
        let snapshot = engine.snapshot();
        assert_eq!(snapshot.rows[0][0].text, "e\u{301}");
        assert_eq!(snapshot.rows[0][1].text, "界");
        assert!(snapshot.rows[0][1].wide);
        assert!(snapshot.rows[0][2].wide_spacer);
        let styled = &snapshot.rows[0][3];
        assert_eq!(styled.text, "X");
        assert_eq!(styled.fg, TerminalColor::Rgb(10, 20, 30));
        assert_eq!(styled.bg, TerminalColor::Indexed(123));
        assert!(
            styled.bold && styled.dim && styled.italic && styled.underline && styled.strikethrough
        );
    }

    #[test]
    fn printable_ascii_and_blank_snapshots_borrow_text_while_unicode_stays_exact() {
        for byte in b' '..=b'~' {
            let text = cell_text(char::from(byte), &[]);
            assert!(matches!(text, Cow::Borrowed(_)));
            assert_eq!(text.as_bytes(), &[byte]);
        }
        assert_eq!(cell_text('\t', &[]), Cow::Borrowed(" "));
        let mut engine = TerminalEngine::new(4, 20);
        engine.feed(b"ASCII !~");
        let snapshot = engine.snapshot();
        assert!(
            snapshot
                .rows
                .iter()
                .flatten()
                .all(|cell| matches!(cell.text, Cow::Borrowed(_)))
        );
        let blank = snapshot.rows[3][19].clone();
        assert!(matches!(blank.text, Cow::Borrowed(" ")));
        engine.feed("e\u{301}界".as_bytes());
        let snapshot = engine.snapshot();
        assert!(matches!(&snapshot.rows[0][8].text, Cow::Owned(text) if text == "e\u{301}"));
        assert!(matches!(&snapshot.rows[0][9].text, Cow::Owned(text) if text == "界"));
        assert!(matches!(snapshot.rows[0][10].text, Cow::Borrowed("")));
    }

    #[test]
    fn selection_respects_cell_sides_words_lines_and_wide_text() {
        let mut engine = TerminalEngine::new(4, 20);
        engine.feed("hello world\r\n界 e\u{301}".as_bytes());
        engine.select_start(SelectionKind::Simple, 0, 0, false);
        assert_eq!(engine.selected_text(), None);
        engine.select_update(0, 4, true);
        assert_eq!(engine.selected_text().as_deref(), Some("hello"));
        assert!(engine.snapshot().rows[0][4].selected);
        assert!(!engine.snapshot().rows[0][5].selected);
        engine.select_start(SelectionKind::Semantic, 0, 7, false);
        assert_eq!(engine.selected_text().as_deref(), Some("world"));
        engine.select_start(SelectionKind::Lines, 0, 7, false);
        assert_eq!(engine.selected_text().as_deref(), Some("hello world\n"));
        engine.select_start(SelectionKind::Simple, 1, 1, false);
        engine.select_update(1, 3, true);
        assert_eq!(engine.selected_text().as_deref(), Some("界 e\u{301}"));
        engine.clear_selection();
        assert_eq!(engine.selected_text(), None);
    }

    #[test]
    fn scrollback_selection_uses_viewport_coordinates_and_survives_output() {
        let mut engine = TerminalEngine::new(2, 20);
        engine.feed(b"first\r\nsecond\r\nthird");
        engine.scroll(1);
        assert_eq!(engine.display_offset(), 1);
        engine.select_start(SelectionKind::Semantic, 0, 0, false);
        assert_eq!(engine.selected_text().as_deref(), Some("first"));
        engine.feed(b"\r\nfourth");
        assert_eq!(engine.selected_text().as_deref(), Some("first"));
        engine.select_all();
        let selected = engine.selected_text().unwrap();
        assert!(selected.contains("first") && selected.contains("fourth"));
        engine.scroll_to_bottom();
        assert_eq!(engine.display_offset(), 0);
        engine.resize(4, 30);
        assert_eq!(engine.size(), (4, 30));
        assert_eq!(engine.snapshot().rows.len(), 4);
        assert!(engine.snapshot().rows.iter().all(|row| row.len() == 30));
    }

    #[test]
    fn sgr_mouse_encodes_buttons_modifiers_drag_release_and_wheel() {
        let mut engine = TerminalEngine::new(24, 80);
        engine.feed(b"\x1b[?1002h\x1b[?1006h");
        assert_eq!(
            report(&engine, MouseButton::Left, MouseAction::Press).unwrap(),
            b"\x1b[<0;5;3M"
        );
        assert_eq!(
            report(&engine, MouseButton::Left, MouseAction::Motion).unwrap(),
            b"\x1b[<32;5;3M"
        );
        assert_eq!(
            report(&engine, MouseButton::Left, MouseAction::Release).unwrap(),
            b"\x1b[<0;5;3m"
        );
        assert_eq!(
            report(&engine, MouseButton::WheelUp, MouseAction::Press).unwrap(),
            b"\x1b[<64;5;3M"
        );
        assert_eq!(
            report(&engine, MouseButton::WheelDown, MouseAction::Release),
            None
        );
        assert_eq!(
            report(&engine, MouseButton::None, MouseAction::Motion),
            None
        );
        let modifiers = MouseModifiers {
            control: true,
            alt: true,
            shift: false,
        };
        assert_eq!(
            engine
                .mouse_report(MouseButton::Right, MouseAction::Press, 2, 4, modifiers)
                .unwrap(),
            b"\x1b[<26;5;3M"
        );
        assert_eq!(
            engine.mouse_report(
                MouseButton::Left,
                MouseAction::Press,
                2,
                4,
                MouseModifiers {
                    shift: true,
                    ..MouseModifiers::default()
                }
            ),
            None
        );
    }

    #[test]
    fn mouse_motion_reporting_requires_the_requested_tracking_mode() {
        let mut engine = TerminalEngine::new(24, 80);
        assert_eq!(report(&engine, MouseButton::Left, MouseAction::Press), None);
        engine.feed(b"\x1b[?1000h\x1b[?1006h");
        assert_eq!(
            report(&engine, MouseButton::Left, MouseAction::Motion),
            None
        );
        engine.feed(b"\x1b[?1003h");
        assert_eq!(
            report(&engine, MouseButton::None, MouseAction::Motion).unwrap(),
            b"\x1b[<35;5;3M"
        );
    }

    #[test]
    fn legacy_and_utf8_mouse_use_correct_coordinate_encoding() {
        let mut engine = TerminalEngine::new(300, 300);
        engine.feed(b"\x1b[?1000h");
        assert_eq!(
            report(&engine, MouseButton::Middle, MouseAction::Press).unwrap(),
            b"\x1b[M!%#"
        );
        assert_eq!(
            report(&engine, MouseButton::Middle, MouseAction::Release).unwrap(),
            b"\x1b[M#%#"
        );
        assert_eq!(
            engine.mouse_report(
                MouseButton::Left,
                MouseAction::Press,
                0,
                223,
                MouseModifiers::default()
            ),
            None
        );
        engine.feed(b"\x1b[?1005h");
        let bytes = engine
            .mouse_report(
                MouseButton::Left,
                MouseAction::Press,
                0,
                223,
                MouseModifiers::default(),
            )
            .unwrap();
        assert_eq!(bytes, "\x1b[M \u{100}!".as_bytes());
    }

    #[test]
    fn title_color_and_size_queries_follow_terminal_state_without_clipboard_access() {
        let mut engine = TerminalEngine::new(24, 80);
        engine.set_cell_size(9, 18);
        engine.set_default_colors((0x12, 0x34, 0x56), (0xab, 0xcd, 0xef));
        engine.feed(b"\x1b]2;Claude Code\x07");
        assert_eq!(engine.title(), "Claude Code");
        assert_eq!(engine.feed(b"\x1b[18t"), b"\x1b[8;24;80t");
        assert_eq!(
            engine.feed(b"\x1b]10;?\x07"),
            b"\x1b]10;rgb:1212/3434/5656\x07"
        );
        assert_eq!(
            engine.feed(b"\x1b]11;?\x07"),
            b"\x1b]11;rgb:abab/cdcd/efef\x07"
        );
        assert!(engine.feed(b"\x1b]52;c;?\x07").is_empty());
        engine.feed(b"\x1b]4;1;rgb:11/22/33\x07\x1b[31mR");
        assert_eq!(
            engine.snapshot().rows[0][0].fg,
            TerminalColor::Rgb(0x11, 0x22, 0x33)
        );
    }

    #[test]
    fn synchronized_output_is_atomic_and_timeout_releases_an_interrupted_frame() {
        let mut engine = TerminalEngine::new(4, 20);
        engine.feed(b"old");
        // Drive the parser directly so scheduler delays cannot affect the
        // explicit deadline assertions in this test.
        engine
            .parser
            .advance(&mut engine.term, b"\x1b[?2026h\rnew\x1b[6n");
        let deadline = engine.sync_deadline().unwrap();
        assert!(engine.visible_text().starts_with("old"));
        assert!(
            engine
                .flush_sync_timeout(deadline - std::time::Duration::from_nanos(1))
                .is_empty()
        );
        assert!(engine.visible_text().starts_with("old"));
        assert_eq!(engine.flush_sync_timeout(deadline), b"\x1b[1;4R");
        assert!(engine.visible_text().starts_with("new"));
        assert_eq!(engine.sync_deadline(), None);
        engine
            .parser
            .advance(&mut engine.term, b"\x1b[?2026h\rnext");
        assert!(engine.visible_text().starts_with("new"));
        engine.parser.advance(&mut engine.term, b"\x1b[?2026l");
        assert!(engine.visible_text().starts_with("next"));
        assert_eq!(engine.sync_deadline(), None);
    }

    #[test]
    fn snapshot_preserves_full_background_rows_and_inverse_default_semantics() {
        let mut engine = TerminalEngine::new(3, 10);
        engine.feed(b"\x1b[7mX\x1b[0m\x1b[41m\x1b[2K");
        let snapshot = engine.snapshot();
        assert_eq!(snapshot.rows.len(), 3);
        assert!(snapshot.rows.iter().all(|row| row.len() == 10));
        assert!(
            snapshot.rows[0]
                .iter()
                .all(|cell| cell.bg == TerminalColor::Indexed(1))
        );
        engine.feed(b"\x1b[0m\r\x1b[7mX");
        let cell = &engine.snapshot().rows[0][0];
        assert_eq!(cell.fg, TerminalColor::DefaultForeground);
        assert_eq!(cell.bg, TerminalColor::DefaultBackground);
        assert!(cell.inverse);
    }

    #[test]
    fn wide_cursor_anchors_to_glyph_and_hidden_cursor_is_absent() {
        let mut engine = TerminalEngine::new(3, 10);
        engine.feed("界\x1b[1;2H".as_bytes());
        assert_eq!(engine.snapshot().cursor, Some((0, 0)));
        engine.feed(b"\x1b[?25l");
        assert_eq!(engine.snapshot().cursor, None);
        engine.feed(b"\x1b[?25h\x1b[5 q");
        let snapshot = engine.snapshot();
        assert_eq!(snapshot.cursor_shape, TerminalCursorShape::Beam);
        assert!(snapshot.cursor_blinking);
    }

    #[test]
    #[ignore = "manual timing baseline; no performance threshold"]
    fn benchmark_terminal_engine_feed_and_snapshot() {
        const ITERATIONS: u32 = 64;
        for (rows, cols) in [(40, 120), (100, 240)] {
            let line = "x".repeat(usize::from(cols));
            let dense = format!(
                "\x1b[H\x1b[38;5;117m{}",
                (0..rows)
                    .map(|_| line.as_str())
                    .collect::<Vec<_>>()
                    .join("\r\n")
            );
            for (label, bytes) in [
                ("blank", b"\x1b[2J\x1b[H".as_slice()),
                ("dense", dense.as_bytes()),
            ] {
                let mut engine = TerminalEngine::new(rows, cols);
                let started = Instant::now();
                for _ in 0..ITERATIONS {
                    std::hint::black_box(engine.feed(std::hint::black_box(bytes)));
                }
                let feed_elapsed = started.elapsed();
                let snapshot = engine.snapshot();
                assert_eq!(snapshot.rows.len(), usize::from(rows));
                assert!(
                    snapshot
                        .rows
                        .iter()
                        .all(|row| row.len() == usize::from(cols))
                );
                let text_allocations = snapshot
                    .rows
                    .iter()
                    .flatten()
                    .filter(|cell| matches!(cell.text, Cow::Owned(_)))
                    .count();
                let started = Instant::now();
                for _ in 0..ITERATIONS {
                    std::hint::black_box(engine.snapshot());
                }
                let snapshot_elapsed = started.elapsed();
                println!(
                    "terminal engine {cols}x{rows} {label}: feed {:.3} ms/iteration; snapshot {:.3} ms/iteration; {text_allocations} text allocations/snapshot ({ITERATIONS} iterations; excludes PTY and GPUI)",
                    feed_elapsed.as_secs_f64() * 1000.0 / f64::from(ITERATIONS),
                    snapshot_elapsed.as_secs_f64() * 1000.0 / f64::from(ITERATIONS),
                );
            }
        }
    }
}
