//! Shared visual tokens for the harness desk.

use std::sync::atomic::{AtomicU8, Ordering};

use gpui::{Pixels, Rems, Rgba, SharedString, px, rems, rgba};

use crate::fonts::{self, FontRole};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    PiDeckDark,
    CursorDark,
}

impl ThemeId {
    pub const fn label(self) -> &'static str {
        match self {
            Self::PiDeckDark => "Pideck Dark",
            Self::CursorDark => "Cursor Dark",
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::PiDeckDark => Self::CursorDark,
            Self::CursorDark => Self::PiDeckDark,
        }
    }

    const fn index(self) -> u8 {
        match self {
            Self::PiDeckDark => 0,
            Self::CursorDark => 1,
        }
    }

    const fn palette(self) -> &'static Palette {
        match self {
            Self::PiDeckDark => &PIDECK_DARK,
            Self::CursorDark => &CURSOR_DARK,
        }
    }
}

static ACTIVE_THEME: AtomicU8 = AtomicU8::new(ThemeId::PiDeckDark.index());

pub fn active() -> ThemeId {
    match ACTIVE_THEME.load(Ordering::Relaxed) {
        1 => ThemeId::CursorDark,
        _ => ThemeId::PiDeckDark,
    }
}

pub fn set_active(theme: ThemeId) {
    ACTIVE_THEME.store(theme.index(), Ordering::Relaxed);
}

pub fn main() -> SharedString {
    fonts::family(FontRole::Main)
}

pub fn sans() -> SharedString {
    fonts::family(FontRole::Sans)
}

pub fn mono() -> SharedString {
    fonts::family(FontRole::Mono)
}

// Layout
pub const SIDE_W: f32 = 236.0;
pub const HISTORY_W: f32 = 360.0;
pub const INSPECT_W: f32 = 304.0;
pub const TITLE_H: f32 = 50.0;
pub const RADIUS: f32 = 4.0;
pub const RADIUS_SM: f32 = 3.0;
pub const PAD_X: f32 = 18.0;
pub const STREAM_PAD_X: f32 = 32.0;
pub const SCROLLBAR: f32 = 8.0;

// Type scale
const DEFAULT_REM_SIZE: f32 = 16.0;
const MIN_FONT_SCALE_LEVEL: i8 = -2;
const MAX_FONT_SCALE_LEVEL: i8 = 3;
const FONT_SCALE_STEP_PERCENT: i8 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FontScale {
    level: i8,
}

impl FontScale {
    pub fn increase(&mut self) -> bool {
        self.adjust(1)
    }

    pub fn decrease(&mut self) -> bool {
        self.adjust(-1)
    }

    pub fn rem_size(self) -> Pixels {
        px(DEFAULT_REM_SIZE * self.factor())
    }

    pub fn percent(self) -> u16 {
        (100 + i16::from(self.level) * i16::from(FONT_SCALE_STEP_PERCENT)) as u16
    }

    fn factor(self) -> f32 {
        self.percent() as f32 / 100.0
    }

    fn adjust(&mut self, delta: i8) -> bool {
        let next = (self.level + delta).clamp(MIN_FONT_SCALE_LEVEL, MAX_FONT_SCALE_LEVEL);
        if next == self.level {
            return false;
        }
        self.level = next;
        true
    }
}

/// Converts a pixel-based type token into a window-relative text size.
///
/// Keeping typography in rems lets `Window::set_rem_size` scale all text while
/// preserving the existing type hierarchy.
pub fn text_size(base_pixels: f32) -> Rems {
    rems(base_pixels / DEFAULT_REM_SIZE)
}

pub const T_WORDMARK: f32 = 17.0;
pub const T_TITLE: f32 = 15.0;
pub const T_BODY: f32 = 15.5;
pub const T_BODY_SM: f32 = 14.5;
pub const T_UI: f32 = 14.0;
pub const T_UI_SM: f32 = 13.0;
pub const T_LABEL: f32 = 12.0;
pub const T_MONO: f32 = 12.0;
pub const T_MONO_SM: f32 = 11.5;
pub const T_TINY: f32 = 11.0;

struct Palette {
    canvas: u32,
    floor: u32,
    panel: u32,
    panel_lift: u32,
    panel_hover: u32,
    user_message: u32,
    user_message_edge: u32,
    edge: u32,
    edge_hard: u32,
    edge_soft: u32,
    bone: u32,
    bone_dim: u32,
    ash: u32,
    smoke: u32,
    signal: u32,
    signal_deep: u32,
    signal_hot: u32,
    focus: u32,
    error: u32,
    error_wash: u32,
    live: u32,
    live_wash: u32,
    working: u32,
    data: u32,
    data_wash: u32,
}

// The original application palette, now named PiDeck Dark.
const PIDECK_DARK: Palette = Palette {
    canvas: 0x0b0a09ff,
    floor: 0x12100eff,
    panel: 0x1a1714ff,
    panel_lift: 0x221e1aff,
    panel_hover: 0x2a241fff,
    user_message: 0x201a16ff,
    user_message_edge: 0xc75a3838,
    edge: 0xebe4d618,
    edge_hard: 0xebe4d62c,
    edge_soft: 0xebe4d612,
    bone: 0xefe7d8ff,
    bone_dim: 0xbbb2a2ff,
    ash: 0x9a9082ff,
    smoke: 0x847b70ff,
    signal: 0xc75a38ff,
    signal_deep: 0x8a3f28ff,
    signal_hot: 0xd46a48ff,
    focus: 0xffd39aff,
    error: 0xe18263ff,
    error_wash: 0xe1826314,
    live: 0xc5d2a8ff,
    live_wash: 0xc5d2a812,
    working: 0x78a9d1ff,
    data: 0xe0b07aff,
    data_wash: 0xe0b07a16,
};

// Cursor Dark by Nexmoe, adapted onto a deeper PiDeck-like neutral base with
// its blue family subdued to keep the interface dark rather than luminous:
// https://github.com/nexmoe/cursor-themes-for-zed/blob/main/themes/cursor-dark.json
const CURSOR_DARK: Palette = Palette {
    canvas: 0x0b0b0bff,       // deepened background / editor.background
    floor: 0x080808ff,        // deepened surface / panel / title bar
    panel: 0x121212ff,        // opaque panel; prevents content bleed-through
    panel_lift: 0x1b1b1bff,   // opaque active / selected surface
    panel_hover: 0x171717ff,  // opaque hover surface
    user_message: 0x171a1dff, // subtle cool lift from agent work
    user_message_edge: 0x5a718842,
    edge: 0xe4e4e413,        // border
    edge_hard: 0xe4e4e426,   // border.focused
    edge_soft: 0xe4e4e413,   // border.variant
    bone: 0xe4e4e4eb,        // text
    bone_dim: 0xe4e4e48d,    // text.muted
    ash: 0xe4e4e48d,         // text.muted
    smoke: 0xe4e4e45e,       // text.placeholder
    signal: 0x5a7188ff,      // subdued Cursor blue
    signal_deep: 0x465a6dff, // pressed Cursor blue
    signal_hot: 0x657f97ff,  // hover Cursor blue
    focus: 0x6b8399ff,       // visible, muted blue focus
    error: 0xe34671ff,       // error
    error_wash: 0xb8004922,  // error.background
    live: 0x3fa266ff,        // success
    live_wash: 0x3fa26622,   // success.background
    working: 0x557984ff,     // subdued Cursor cyan
    data: 0xd2943eff,        // modified / yellow accent
    data_wash: 0x222222cc,   // deepened modified.background
};

fn color(select: impl FnOnce(&Palette) -> u32) -> Rgba {
    rgba(select(active().palette()))
}

pub fn canvas() -> Rgba {
    color(|palette| palette.canvas)
}

pub fn floor() -> Rgba {
    color(|palette| palette.floor)
}

pub fn panel() -> Rgba {
    color(|palette| palette.panel)
}

pub fn panel_lift() -> Rgba {
    color(|palette| palette.panel_lift)
}

pub fn panel_hover() -> Rgba {
    color(|palette| palette.panel_hover)
}

pub fn user_message() -> Rgba {
    color(|palette| palette.user_message)
}

pub fn user_message_edge() -> Rgba {
    color(|palette| palette.user_message_edge)
}

pub fn edge() -> Rgba {
    color(|palette| palette.edge)
}

pub fn edge_hard() -> Rgba {
    color(|palette| palette.edge_hard)
}

pub fn edge_soft() -> Rgba {
    color(|palette| palette.edge_soft)
}

pub fn bone() -> Rgba {
    color(|palette| palette.bone)
}

pub fn bone_dim() -> Rgba {
    color(|palette| palette.bone_dim)
}

pub fn ash() -> Rgba {
    color(|palette| palette.ash)
}

pub fn smoke() -> Rgba {
    color(|palette| palette.smoke)
}

pub fn signal() -> Rgba {
    color(|palette| palette.signal)
}

pub fn signal_deep() -> Rgba {
    color(|palette| palette.signal_deep)
}

pub fn signal_hot() -> Rgba {
    color(|palette| palette.signal_hot)
}

pub fn focus() -> Rgba {
    color(|palette| palette.focus)
}

pub fn error() -> Rgba {
    color(|palette| palette.error)
}

pub fn error_wash() -> Rgba {
    color(|palette| palette.error_wash)
}

pub fn live() -> Rgba {
    color(|palette| palette.live)
}

pub fn live_wash() -> Rgba {
    color(|palette| palette.live_wash)
}

pub fn working() -> Rgba {
    color(|palette| palette.working)
}

pub fn data() -> Rgba {
    color(|palette| palette.data)
}

pub fn data_wash() -> Rgba {
    color(|palette| palette.data_wash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_scale_is_bounded_and_keeps_default_typography_unchanged() {
        let mut scale = FontScale::default();
        assert_eq!(scale.percent(), 100);
        assert_eq!(scale.rem_size(), px(16.0));
        assert_eq!(text_size(T_BODY), rems(T_BODY / 16.0));

        for _ in 0..10 {
            scale.increase();
        }
        assert_eq!(scale.percent(), 130);
        assert!(!scale.increase());

        for _ in 0..10 {
            scale.decrease();
        }
        assert_eq!(scale.percent(), 80);
        assert!(!scale.decrease());
    }

    #[test]
    fn theme_names_and_cycle_are_stable() {
        assert_eq!(ThemeId::PiDeckDark.label(), "Pideck Dark");
        assert_eq!(ThemeId::PiDeckDark.next(), ThemeId::CursorDark);
        assert_eq!(ThemeId::CursorDark.label(), "Cursor Dark");
        assert_eq!(ThemeId::CursorDark.next(), ThemeId::PiDeckDark);
    }

    #[test]
    fn pideck_dark_preserves_the_original_palette() {
        assert_eq!(PIDECK_DARK.canvas, 0x0b0a09ff);
        assert_eq!(PIDECK_DARK.floor, 0x12100eff);
        assert_eq!(PIDECK_DARK.user_message, 0x201a16ff);
        assert_eq!(PIDECK_DARK.bone, 0xefe7d8ff);
        assert_eq!(PIDECK_DARK.signal, 0xc75a38ff);
    }

    #[test]
    fn cursor_dark_uses_subdued_accents_on_deeper_surfaces() {
        assert_eq!(CURSOR_DARK.canvas, 0x0b0b0bff);
        assert_eq!(CURSOR_DARK.floor, 0x080808ff);
        assert_eq!(CURSOR_DARK.panel, 0x121212ff);
        assert_eq!(CURSOR_DARK.panel_lift, 0x1b1b1bff);
        assert_eq!(CURSOR_DARK.panel_hover, 0x171717ff);
        assert_eq!(CURSOR_DARK.user_message, 0x171a1dff);
        assert_eq!(CURSOR_DARK.edge, 0xe4e4e413);
        assert_eq!(CURSOR_DARK.bone, 0xe4e4e4eb);
        assert_eq!(CURSOR_DARK.signal, 0x5a7188ff);
        assert_eq!(CURSOR_DARK.working, 0x557984ff);
        assert_eq!(CURSOR_DARK.error, 0xe34671ff);
        assert_eq!(CURSOR_DARK.live, 0x3fa266ff);
        assert_eq!(CURSOR_DARK.data, 0xd2943eff);
    }
}
