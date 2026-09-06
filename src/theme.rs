//! Workbench design system: Graphite and Paper, readable at laptop scale.

use std::sync::atomic::{AtomicU8, Ordering};

use gpui::{BoxShadow, Pixels, Rems, Rgba, SharedString, point, px, rems, rgba};

use crate::fonts::{self, FontRole};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

impl ThemeMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }
}

/// Two fully supported appearances. Legacy keys are mapped without touching
/// user settings on disk until the user explicitly changes a preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    PiDeckDark,
    ParchmentDesk,
}

impl ThemeId {
    pub const ALL: [Self; 2] = [Self::PiDeckDark, Self::ParchmentDesk];
    pub const DARK: [Self; 1] = [Self::PiDeckDark];
    pub const LIGHT: [Self; 1] = [Self::ParchmentDesk];

    pub const fn label(self) -> &'static str {
        match self {
            Self::PiDeckDark => "Graphite",
            Self::ParchmentDesk => "Paper",
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::PiDeckDark => "pideck-dark",
            Self::ParchmentDesk => "pideck-light",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "pideck-dark" | "cursor-dark" | "moss-foundry" | "ink-harbor"
            | "volt-workshop" | "plum-archive" | "salt-flat" | "saffron-loom"
            | "juniper-coil" | "smoke-library" | "pewter-hall" | "olive-study" => {
                Some(Self::PiDeckDark)
            }
            "pideck-light" | "parchment-desk" | "mist-orchard" | "coral-ledger"
            | "chalk-blueprint" | "honey-comb" | "porcelain-lab" | "citrus-grove"
            | "letterpress" | "linen-gallery" | "rice-paper" | "bone-china" => {
                Some(Self::ParchmentDesk)
            }
            _ => None,
        }
    }

    pub const fn mode(self) -> ThemeMode {
        match self {
            Self::PiDeckDark => ThemeMode::Dark,
            Self::ParchmentDesk => ThemeMode::Light,
        }
    }

    pub fn for_mode(mode: ThemeMode) -> &'static [Self] {
        match mode {
            ThemeMode::Dark => &Self::DARK,
            ThemeMode::Light => &Self::LIGHT,
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::PiDeckDark => Self::ParchmentDesk,
            Self::ParchmentDesk => Self::PiDeckDark,
        }
    }

    const fn index(self) -> u8 {
        match self {
            Self::PiDeckDark => 0,
            Self::ParchmentDesk => 1,
        }
    }

    const fn from_index(index: u8) -> Self {
        match index {
            1 => Self::ParchmentDesk,
            _ => Self::PiDeckDark,
        }
    }

    const fn palette(self) -> &'static Palette {
        match self {
            Self::PiDeckDark => &GRAPHITE,
            Self::ParchmentDesk => &PAPER,
        }
    }
}

static ACTIVE_THEME: AtomicU8 = AtomicU8::new(ThemeId::PiDeckDark.index());

pub fn active() -> ThemeId {
    ThemeId::from_index(ACTIVE_THEME.load(Ordering::Relaxed))
}

pub fn set_active(theme: ThemeId) {
    ACTIVE_THEME.store(theme.index(), Ordering::Relaxed);
}

pub(crate) use crate::services::accessibility::motion_enabled;

pub fn main() -> SharedString {
    fonts::family(FontRole::Main)
}

pub fn sans() -> SharedString {
    fonts::family(FontRole::Sans)
}

pub fn mono() -> SharedString {
    fonts::family(FontRole::Mono)
}

// Layout. 4px rhythm. Chrome recedes; the transcript and prompt dock
// keep the widest measure and the softest corners.
pub const SIDE_W: f32 = crate::state::workspace_layout::NAVIGATION_WIDTH;
pub const HISTORY_W: f32 = crate::state::workspace_layout::HISTORY_WIDTH;
pub const INSPECT_W: f32 = crate::state::workspace_layout::INSPECTOR_WIDTH;
pub const TITLE_H: f32 = 48.0;
/// Default hit target for titlebar and rail icon buttons (Fitts).
pub const CHROME: f32 = 34.0;
pub const RADIUS: f32 = 6.0;
pub const RADIUS_SM: f32 = 4.0;
/// Nested controls inside a dock or sheet.
pub const RADIUS_MD: f32 = 8.0;
/// Floating sheets and the inspector companion.
pub const RADIUS_LG: f32 = 8.0;
/// Prompt dock — the largest surface, so it owns the softest corner.
pub const RADIUS_XL: f32 = 10.0;
pub const PAD_X: f32 = 16.0;
pub const STREAM_PAD_X: f32 = 24.0;
pub const READING_W: f32 = 960.0;
pub const SCROLLBAR: f32 = 6.0;

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
pub const T_BODY: f32 = 16.0;
pub const T_BODY_SM: f32 = 15.0;
pub const T_UI: f32 = 14.0;
pub const T_UI_SM: f32 = 13.0;
pub const T_LABEL: f32 = 13.0;
pub const T_MONO: f32 = 13.0;
pub const T_MONO_SM: f32 = 12.5;
pub const T_TINY: f32 = 12.0;

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

// Opaque neutral surfaces prevent transcript bleed-through. Accent is reserved
// for interaction; status has a separate semantic color in both appearances.
const GRAPHITE: Palette = Palette {
    canvas: 0x181b1fff,
    floor: 0x13161aff,
    panel: 0x20242aff,
    panel_lift: 0x2a3038ff,
    panel_hover: 0x252b32ff,
    user_message: 0x222a34ff,
    user_message_edge: 0x96b6df42,
    edge: 0xe7e9ec20,
    edge_hard: 0xe7e9ec48,
    edge_soft: 0xe7e9ec12,
    bone: 0xe7e9ecff,
    bone_dim: 0xc0c7d0ff,
    ash: 0xadb6c2ff,
    smoke: 0xa7b0bdff,
    signal: 0xa7c5edff,
    signal_deep: 0x8eafd9ff,
    signal_hot: 0xc0d7f6ff,
    focus: 0xb5d1f5ff,
    error: 0xf0a69dff,
    error_wash: 0xf0a69d16,
    live: 0xa5c7b1ff,
    live_wash: 0xa5c7b116,
    working: 0xd9c08dff,
    data: 0xbbbee4ff,
    data_wash: 0xbbbee416,
};

const PAPER: Palette = Palette {
    canvas: 0xf9faf8ff,
    floor: 0xeff1edff,
    panel: 0xf4f5f2ff,
    panel_lift: 0xe8ece6ff,
    panel_hover: 0xe9ede8ff,
    user_message: 0xedf1f5ff,
    user_message_edge: 0x355c8842,
    edge: 0x25303a22,
    edge_hard: 0x25303a48,
    edge_soft: 0x25303a12,
    bone: 0x20272fff,
    bone_dim: 0x424e59ff,
    ash: 0x525e69ff,
    smoke: 0x596570ff,
    signal: 0x305780ff,
    signal_deep: 0x26486cff,
    signal_hot: 0x244467ff,
    focus: 0x2b557cff,
    error: 0x9c3d35ff,
    error_wash: 0x9c3d3512,
    live: 0x356349ff,
    live_wash: 0x35634912,
    working: 0x75571fff,
    data: 0x5d5488ff,
    data_wash: 0x5d548812,
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

/// Recognition swatch for the theme picker: paper + signal accent.
pub fn preview_swatch(theme: ThemeId) -> (Rgba, Rgba) {
    let palette = theme.palette();
    (rgba(palette.canvas), rgba(palette.signal))
}

/// Tight downward lift for the prompt dock and its popovers.
pub fn dock_shadow() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: rgba(0x0000_0020).into(),
        offset: point(px(0.0), px(4.0)),
        blur_radius: px(12.0),
        spread_radius: px(-8.0),
    }]
}

/// Heavier lift for summoned sheets that overlay the workspace.
pub fn sheet_shadow() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: rgba(0x0000_0055).into(),
        offset: point(px(0.0), px(14.0)),
        blur_radius: px(36.0),
        spread_radius: px(-4.0),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(component: f32) -> f32 {
        if component <= 0.03928 {
            component / 12.92
        } else {
            ((component + 0.055) / 1.055).powf(2.4)
        }
    }

    fn relative_luminance(color: u32) -> f32 {
        let r = channel(((color >> 24) & 0xff) as f32 / 255.0);
        let g = channel(((color >> 16) & 0xff) as f32 / 255.0);
        let b = channel(((color >> 8) & 0xff) as f32 / 255.0);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    fn contrast_ratio(foreground: u32, background: u32) -> f32 {
        let light = relative_luminance(foreground);
        let dark = relative_luminance(background);
        let (hi, lo) = if light > dark {
            (light, dark)
        } else {
            (dark, light)
        };
        (hi + 0.05) / (lo + 0.05)
    }

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
    fn theme_keys_round_trip_and_are_unique() {
        let mut keys = std::collections::HashSet::new();
        for theme in ThemeId::ALL {
            assert!(
                keys.insert(theme.key()),
                "duplicate theme key {}",
                theme.key()
            );
            assert_eq!(ThemeId::from_key(theme.key()), Some(theme));
        }
        assert_eq!(ThemeId::from_key("not-a-theme"), None);
        assert_eq!(ThemeId::from_key("Pideck Dark"), None);
    }

    #[test]
    fn legacy_appearances_migrate_with_their_original_brightness() {
        for key in [
            "cursor-dark", "moss-foundry", "ink-harbor", "volt-workshop",
            "plum-archive", "salt-flat", "saffron-loom", "juniper-coil",
            "smoke-library", "pewter-hall", "olive-study",
        ] {
            assert_eq!(ThemeId::from_key(key), Some(ThemeId::PiDeckDark));
        }
        for key in [
            "parchment-desk", "mist-orchard", "coral-ledger", "chalk-blueprint",
            "honey-comb", "porcelain-lab", "citrus-grove", "letterpress",
            "linen-gallery", "rice-paper", "bone-china",
        ] {
            assert_eq!(ThemeId::from_key(key), Some(ThemeId::ParchmentDesk));
        }
        assert_eq!(ThemeId::PiDeckDark.next().next(), ThemeId::PiDeckDark);
    }

    #[test]
    fn readable_text_and_status_on_every_opaque_surface() {
        for theme in ThemeId::ALL {
            let p = theme.palette();
            for background in [p.canvas, p.floor, p.panel, p.panel_lift, p.panel_hover, p.user_message] {
                for foreground in [p.bone, p.bone_dim, p.ash, p.smoke, p.signal, p.error, p.live, p.working, p.data] {
                    assert!(
                        contrast_ratio(foreground, background) >= 4.5,
                        "{}: {foreground:08x} on {background:08x}", theme.label(),
                    );
                }
                assert!(contrast_ratio(p.focus, background) >= 3.0);
            }
        }
    }
}
