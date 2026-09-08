//! Semantic colors and measured geometry from the six supplied PiDeck reference images.

use std::sync::atomic::{AtomicU8, Ordering};

use gpui::{BoxShadow, Pixels, Rems, Rgba, SharedString, px, rems, rgba};

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

/// Existing keys retain their meaning. Each appearance shares identical geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    PiDeckDark,
    ParchmentDesk,
    Linen,
    Midnight,
}

impl ThemeId {
    pub const ALL: [Self; 4] = [Self::ParchmentDesk, Self::Linen, Self::PiDeckDark, Self::Midnight];
    pub const DARK: [Self; 2] = [Self::PiDeckDark, Self::Midnight];
    pub const LIGHT: [Self; 2] = [Self::ParchmentDesk, Self::Linen];

    pub const fn label(self) -> &'static str {
        match self {
            Self::PiDeckDark => "Graphite",
            Self::ParchmentDesk => "Original",
            Self::Linen => "Linen",
            Self::Midnight => "Midnight",
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::PiDeckDark => "pideck-dark",
            Self::ParchmentDesk => "pideck-light",
            Self::Linen => "linen",
            Self::Midnight => "midnight",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "linen" => Some(Self::Linen),
            "midnight" => Some(Self::Midnight),
            "original" => Some(Self::ParchmentDesk),
            "graphite" => Some(Self::PiDeckDark),
            "pideck-dark" | "cursor-dark" | "moss-foundry" | "ink-harbor"
            | "volt-workshop" | "plum-archive" | "salt-flat" | "saffron-loom"
            | "juniper-coil" | "smoke-library" | "pewter-hall" | "olive-study" => Some(Self::PiDeckDark),
            "pideck-light" | "parchment-desk" | "mist-orchard" | "coral-ledger"
            | "chalk-blueprint" | "honey-comb" | "porcelain-lab" | "citrus-grove"
            | "letterpress" | "linen-gallery" | "rice-paper" | "bone-china" => Some(Self::ParchmentDesk),
            _ => None,
        }
    }

    pub const fn mode(self) -> ThemeMode {
        match self {
            Self::PiDeckDark | Self::Midnight => ThemeMode::Dark,
            Self::ParchmentDesk | Self::Linen => ThemeMode::Light,
        }
    }

    pub fn for_mode(mode: ThemeMode) -> &'static [Self] {
        match mode { ThemeMode::Dark => &Self::DARK, ThemeMode::Light => &Self::LIGHT }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::ParchmentDesk => Self::Linen,
            Self::Linen => Self::PiDeckDark,
            Self::PiDeckDark => Self::Midnight,
            Self::Midnight => Self::ParchmentDesk,
        }
    }

    const fn index(self) -> u8 {
        match self { Self::PiDeckDark => 0, Self::ParchmentDesk => 1, Self::Linen => 2, Self::Midnight => 3 }
    }
    const fn from_index(index: u8) -> Self {
        match index { 0 => Self::PiDeckDark, 2 => Self::Linen, 3 => Self::Midnight, _ => Self::ParchmentDesk }
    }
    const fn palette(self) -> &'static Palette {
        match self { Self::PiDeckDark => &GRAPHITE, Self::ParchmentDesk => &ORIGINAL, Self::Linen => &LINEN, Self::Midnight => &MIDNIGHT }
    }
}

static ACTIVE_THEME: AtomicU8 = AtomicU8::new(ThemeId::ParchmentDesk.index());

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

pub fn serif() -> SharedString {
    "Instrument Serif".into()
}

pub fn mono() -> SharedString {
    fonts::family(FontRole::Mono)
}

// Layout. 4px rhythm. Chrome recedes; the transcript and prompt dock
// keep the widest measure and the softest corners.
pub const SIDE_W: f32 = crate::state::workspace_layout::NAVIGATION_WIDTH;
pub const HISTORY_W: f32 = crate::state::workspace_layout::HISTORY_WIDTH;
pub const INSPECT_W: f32 = crate::state::workspace_layout::INSPECTOR_WIDTH;
pub const TITLE_H: f32 = 40.0;
pub const TOOLBAR_H: f32 = 60.0;
pub const RAIL_W: f32 = crate::state::workspace_layout::RAIL_WIDTH;
pub const SIDEBAR_PAD: f32 = 24.0;
pub const COMPOSER_H: f32 = 112.0;
pub const COMPOSER_BOTTOM: f32 = 43.0;
/// Default hit target for titlebar and rail icon buttons (Fitts).
pub const CHROME: f32 = 36.0;
pub const RADIUS: f32 = 6.0;
pub const RADIUS_SM: f32 = 4.0;
/// Nested controls inside a dock or sheet.
pub const RADIUS_MD: f32 = 8.0;
/// Floating sheets and the inspector companion.
pub const RADIUS_LG: f32 = 8.0;
/// Prompt dock — the largest surface, so it owns the softest corner.
pub const RADIUS_XL: f32 = 11.0;
pub const PAD_X: f32 = 16.0;
pub const STREAM_PAD_X: f32 = 24.0;
pub const READING_W: f32 = 840.0;
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

pub const T_WORDMARK: f32 = 25.0;
pub const T_TITLE: f32 = 24.0;
pub const T_BODY: f32 = 16.0;
pub const T_BODY_SM: f32 = 15.0;
pub const T_UI: f32 = 13.0;
pub const T_UI_SM: f32 = 12.0;
pub const T_LABEL: f32 = 12.0;
pub const T_MONO: f32 = 12.0;
pub const T_MONO_SM: f32 = 12.0;
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
    rail: u32,
    rail_muted: u32,
    selection: u32,
    on_accent: u32,
    tool_branch: u32,
    search_surface: u32,
}

// Flat colors sampled from the PNGs. Working subtitles are amber in the
// themed exports, despite the conflicting historical prose in design.md.
const ORIGINAL: Palette = Palette {
    canvas: 0xfaf9f6ff,
    floor: 0xf0efebff,
    panel: 0xffffffff,
    panel_lift: 0xf0efebff,
    panel_hover: 0xe8edfcff,
    user_message: 0xf0efebff,
    user_message_edge: 0xf0efebff,
    edge: 0xddded9ff,
    edge_hard: 0xddded9ff,
    edge_soft: 0xddded980,
    bone: 0x24272cff,
    bone_dim: 0x24272cff,
    ash: 0x6a6d73ff,
    smoke: 0x6a6d73ff,
    signal: 0x304cdcff,
    signal_deep: 0x304cdcff,
    signal_hot: 0x304cdcff,
    focus: 0x304cdcff,
    error: 0x9a3f37ff,
    error_wash: 0x9a3f3716,
    live: 0x326c52ff,
    live_wash: 0x326c5216,
    working: 0x304cdcff,
    data: 0x62518eff,
    data_wash: 0x62518e16,
    rail: 0x24272cff,
    rail_muted: 0xd4d7ddff,
    selection: 0xe8edfcff,
    on_accent: 0xffffffff,
    tool_branch: 0xb7bab6ff,
    search_surface: 0xfaf9f6ff,
};

const LINEN: Palette = Palette {
    canvas: 0xf7f4edff,
    floor: 0xefebe2ff,
    panel: 0xfffefaff,
    panel_lift: 0xf0f0e8ff,
    panel_hover: 0xdde7dcff,
    user_message: 0xf0f0e8ff,
    user_message_edge: 0xf0f0e8ff,
    edge: 0xd8d8ceff,
    edge_hard: 0xd8d8ceff,
    edge_soft: 0xd8d8ce80,
    bone: 0x242823ff,
    bone_dim: 0x242823ff,
    ash: 0x62675eff,
    smoke: 0x62675eff,
    signal: 0x355e4bff,
    signal_deep: 0x355e4bff,
    signal_hot: 0x355e4bff,
    focus: 0x355e4bff,
    error: 0x9a3f37ff,
    error_wash: 0x9a3f3716,
    live: 0x2f664aff,
    live_wash: 0x2f664a16,
    working: 0x795817ff,
    data: 0x5f557fff,
    data_wash: 0x5f557f16,
    rail: 0x202521ff,
    rail_muted: 0xb2baaeff,
    selection: 0xdde7dcff,
    on_accent: 0xffffffff,
    tool_branch: 0x62675eff,
    search_surface: 0xfffefaff,
};

const GRAPHITE: Palette = Palette {
    canvas: 0x20211fff,
    floor: 0x1c1e1bff,
    panel: 0x282a27ff,
    panel_lift: 0x30332cff,
    panel_hover: 0x36402dff,
    user_message: 0x30332cff,
    user_message_edge: 0x30332cff,
    edge: 0x41453dff,
    edge_hard: 0x41453dff,
    edge_soft: 0x41453d80,
    bone: 0xeeefe8ff,
    bone_dim: 0xeeefe8ff,
    ash: 0xabb0a4ff,
    smoke: 0xabb0a4ff,
    signal: 0xc3d6a3ff,
    signal_deep: 0xc3d6a3ff,
    signal_hot: 0xc3d6a3ff,
    focus: 0xc3d6a3ff,
    error: 0xedaaa1ff,
    error_wash: 0xedaaa116,
    live: 0xabd4adff,
    live_wash: 0xabd4ad16,
    working: 0xddc28dff,
    data: 0xc3bde1ff,
    data_wash: 0xc3bde116,
    rail: 0x141613ff,
    rail_muted: 0xabb5a4ff,
    selection: 0x36402dff,
    on_accent: 0x202521ff,
    tool_branch: 0xabb0a4ff,
    search_surface: 0x282a27ff,
};

const MIDNIGHT: Palette = Palette {
    canvas: 0x141d2bff,
    floor: 0x111a28ff,
    panel: 0x1a2638ff,
    panel_lift: 0x23334aff,
    panel_hover: 0x263f64ff,
    user_message: 0x23334aff,
    user_message_edge: 0x23334aff,
    edge: 0x34465fff,
    edge_hard: 0x34465fff,
    edge_soft: 0x34465f80,
    bone: 0xeaf0faff,
    bone_dim: 0xeaf0faff,
    ash: 0xa6b5ccff,
    smoke: 0xa6b5ccff,
    signal: 0xadc6ffff,
    signal_deep: 0xadc6ffff,
    signal_hot: 0xadc6ffff,
    focus: 0xadc6ffff,
    error: 0xf0a8b1ff,
    error_wash: 0xf0a8b116,
    live: 0x9cd7c2ff,
    live_wash: 0x9cd7c216,
    working: 0xe7c69bff,
    data: 0xc6b8e8ff,
    data_wash: 0xc6b8e816,
    rail: 0x0b121dff,
    rail_muted: 0xa6b5ccff,
    selection: 0x263f64ff,
    on_accent: 0x152033ff,
    tool_branch: 0xa6b5ccff,
    search_surface: 0x1a2638ff,
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

/// Reference surfaces are flat. Popovers use an opaque fill and outline.
pub fn dock_shadow() -> Vec<BoxShadow> { Vec::new() }
pub fn sheet_shadow() -> Vec<BoxShadow> { Vec::new() }

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
        assert_eq!(ThemeId::PiDeckDark.next().next().next().next(), ThemeId::PiDeckDark);
    }

    #[test]
    fn readable_primary_text_and_unselected_metadata() {
        for theme in ThemeId::ALL {
            let p = theme.palette();
            for background in [p.canvas, p.floor, p.panel, p.panel_lift, p.user_message] {
                for foreground in [p.bone, p.bone_dim, p.ash, p.smoke] {
                    assert!(
                        contrast_ratio(foreground, background) >= 4.5,
                        "{}: {foreground:08x} on {background:08x}", theme.label(),
                    );
                }
                assert!(contrast_ratio(p.focus, background) >= 3.0);
            }
            assert!(contrast_ratio(p.bone, p.selection) >= 4.5);
            assert!(contrast_ratio(p.on_accent, p.signal) >= 4.5);
            assert!(contrast_ratio(p.rail_muted, p.rail) >= 4.5);
            // Original's exact reference muted ink on selection is 4.43:1,
            // so do not assert that every possible palette combination is AA.
        }
    }
}

pub fn rail() -> Rgba { color(|palette| palette.rail) }

pub fn rail_muted() -> Rgba { color(|palette| palette.rail_muted) }

pub fn selection() -> Rgba { color(|palette| palette.selection) }

pub fn on_accent() -> Rgba { color(|palette| palette.on_accent) }

pub fn tool_branch() -> Rgba { color(|palette| palette.tool_branch) }

/// The Original search surface is canvas-colored; the other references use panel.
pub fn search_surface() -> Rgba { color(|palette| palette.search_surface) }
