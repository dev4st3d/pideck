//! Neutral palettes from design/pideck-reimagined and its editable Pen master.

use std::cell::Cell;

use gpui::{Rems, Rgba, SharedString, rems, rgba};

pub(crate) mod terminal_manager;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Appearance {
    #[default]
    Black,
    Light,
    Graphite,
    Charcoal,
}

impl Appearance {
    pub(crate) const ALL: [Self; 4] = [Self::Black, Self::Light, Self::Graphite, Self::Charcoal];

    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::Black => "black",
            Self::Light => "light",
            Self::Graphite => "graphite",
            Self::Charcoal => "charcoal",
        }
    }
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Black => "Black",
            Self::Light => "Light",
            Self::Graphite => "Graphite",
            Self::Charcoal => "Charcoal",
        }
    }
    pub(crate) fn from_id(id: &str) -> Option<Self> {
        // Read existing preferences without resetting the user's light/dark choice.
        match id {
            "black" | "midnight" => Some(Self::Black),
            "light" | "paper" | "linen" => Some(Self::Light),
            "graphite" | "evergreen" | "dusk" => Some(Self::Graphite),
            "charcoal" | "ember" => Some(Self::Charcoal),
            _ => None,
        }
    }
    pub(crate) const fn is_dark(self) -> bool {
        !matches!(self, Self::Light)
    }
    fn palette(self) -> &'static Palette {
        match self {
            Self::Black => &BLACK,
            Self::Light => &LIGHT,
            Self::Graphite => &GRAPHITE,
            Self::Charcoal => &CHARCOAL,
        }
    }
}

thread_local! {
    // Rendering and theme changes stay on GPUI's UI thread. Workers receive
    // resolved colors or an explicit Appearance, never ambient theme state.
    static APPEARANCE: Cell<Appearance> = const { Cell::new(Appearance::Black) };
}

pub(crate) fn appearance() -> Appearance {
    APPEARANCE.get()
}

pub(crate) fn set_appearance(appearance: Appearance) {
    APPEARANCE.set(appearance);
}

struct Palette {
    canvas: u32,
    chrome: u32,
    success: u32,
    floor: u32,
    panel_hover: u32,
    edge: u32,
    edge_hard: u32,
    bone: u32,
    ash: u32,
    focus: u32,
    accent_hover: u32,
    accent_pressed: u32,
    on_accent: u32,
    error: u32,
    error_wash: u32,
    selection: u32,
    working: u32,
    diff_added: u32,
    diff_removed: u32,
    diff_empty: u32,
}

const BLACK: Palette = Palette {
    canvas: 0x080808ff,
    chrome: 0x141414ff,
    success: 0x99bda3ff,
    floor: 0x101010ff,
    panel_hover: 0x1d1d1dff,
    edge: 0x303030ff,
    edge_hard: 0xabababff,
    bone: 0xeeeeeeff,
    ash: 0xabababff,
    focus: 0xdededeff,
    accent_hover: 0xeeeeeeff,
    accent_pressed: 0xbbbbbbff,
    on_accent: 0x121212ff,
    error: 0xd4a0a0ff,
    error_wash: 0x2b1e1eff,
    selection: 0x282828ff,
    working: 0xabababff,
    diff_added: 0x17251cff,
    diff_removed: 0x2b1e1eff,
    diff_empty: 0x141414ff,
};

const LIGHT: Palette = Palette {
    canvas: 0xfcfcfcff,
    chrome: 0xf7f7f7ff,
    success: 0x326047ff,
    floor: 0xf3f3f3ff,
    panel_hover: 0xeaeaeaff,
    edge: 0xd5d5d5ff,
    edge_hard: 0x616161ff,
    bone: 0x242424ff,
    ash: 0x616161ff,
    focus: 0x404040ff,
    accent_hover: 0x303030ff,
    accent_pressed: 0x242424ff,
    on_accent: 0xffffffff,
    error: 0x954545ff,
    error_wash: 0xf6eaeaff,
    selection: 0xe4e4e4ff,
    working: 0x616161ff,
    diff_added: 0xe8f1eaff,
    diff_removed: 0xf6eaeaff,
    diff_empty: 0xf7f7f7ff,
};

const GRAPHITE: Palette = Palette {
    canvas: 0x171717ff,
    chrome: 0x232323ff,
    success: 0xa5bdaaff,
    floor: 0x1e1e1eff,
    panel_hover: 0x2c2c2cff,
    edge: 0x424242ff,
    edge_hard: 0xb3b3b3ff,
    bone: 0xeaeaeaff,
    ash: 0xb3b3b3ff,
    focus: 0xc9c9c9ff,
    accent_hover: 0xeeeeeeff,
    accent_pressed: 0xbbbbbbff,
    on_accent: 0x202020ff,
    error: 0xcfa2a2ff,
    error_wash: 0x332626ff,
    selection: 0x383838ff,
    working: 0xb3b3b3ff,
    diff_added: 0x223027ff,
    diff_removed: 0x332626ff,
    diff_empty: 0x232323ff,
};

const CHARCOAL: Palette = Palette {
    canvas: 0x262626ff,
    chrome: 0x313131ff,
    success: 0xacc8b3ff,
    floor: 0x2c2c2cff,
    panel_hover: 0x3b3b3bff,
    edge: 0x535353ff,
    edge_hard: 0xbdbdbdff,
    bone: 0xf2f2f2ff,
    ash: 0xbdbdbdff,
    focus: 0xdadadaff,
    accent_hover: 0xeeeeeeff,
    accent_pressed: 0xbbbbbbff,
    on_accent: 0x282828ff,
    error: 0xd9b0b0ff,
    error_wash: 0x453232ff,
    selection: 0x494949ff,
    working: 0xbdbdbdff,
    diff_added: 0x2a3c30ff,
    diff_removed: 0x453232ff,
    diff_empty: 0x313131ff,
};
fn palette() -> &'static Palette {
    appearance().palette()
}

pub(crate) fn mono() -> SharedString {
    crate::fonts::mono()
}

pub(crate) fn text_size(pixels: f32) -> Rems {
    rems(pixels / 16.0)
}

pub(crate) const T_MONO: f32 = 13.0;

pub(crate) fn canvas() -> Rgba {
    rgba(palette().canvas)
}
pub(crate) fn chrome() -> Rgba {
    rgba(palette().chrome)
}
pub(crate) fn success() -> Rgba {
    rgba(palette().success)
}
pub(crate) fn floor() -> Rgba {
    rgba(palette().floor)
}
pub(crate) fn panel() -> Rgba {
    floor()
}
pub(crate) fn panel_lift() -> Rgba {
    canvas()
}
pub(crate) fn panel_hover() -> Rgba {
    rgba(palette().panel_hover)
}
pub(crate) fn edge() -> Rgba {
    rgba(palette().edge)
}
pub(crate) fn edge_hard() -> Rgba {
    rgba(palette().edge_hard)
}
pub(crate) fn edge_soft() -> Rgba {
    edge()
}
pub(crate) fn bone() -> Rgba {
    rgba(palette().bone)
}
pub(crate) fn bone_dim() -> Rgba {
    bone()
}
pub(crate) fn ash() -> Rgba {
    rgba(palette().ash)
}
pub(crate) fn smoke() -> Rgba {
    ash()
}
pub(crate) fn focus() -> Rgba {
    rgba(palette().focus)
}
pub(crate) fn accent_hover() -> Rgba {
    rgba(palette().accent_hover)
}
pub(crate) fn accent_pressed() -> Rgba {
    rgba(palette().accent_pressed)
}
pub(crate) fn error() -> Rgba {
    rgba(palette().error)
}
pub(crate) fn error_wash() -> Rgba {
    rgba(palette().error_wash)
}
pub(crate) fn selection() -> Rgba {
    rgba(palette().selection)
}
pub(crate) fn on_accent() -> Rgba {
    rgba(palette().on_accent)
}
pub(crate) fn working() -> Rgba {
    rgba(palette().working)
}
pub(crate) fn diff_added() -> Rgba {
    rgba(palette().diff_added)
}
pub(crate) fn diff_removed() -> Rgba {
    rgba(palette().diff_removed)
}
pub(crate) fn diff_empty() -> Rgba {
    rgba(palette().diff_empty)
}

const ANSI_LIGHT: [u32; 16] = [
    0x242424ff, 0x9e3238ff, 0x2f6843ff, 0x765b00ff, 0x315ea3ff, 0x854080ff, 0x226d70ff, 0x616161ff,
    0x616161ff, 0xb13c40ff, 0x316c45ff, 0x7d6000ff, 0x395f9bff, 0x90518aff, 0x267076ff, 0x494949ff,
];

const ANSI_DARK: [u32; 16] = [
    0xaaaaaaff, 0xf09085ff, 0x9dc88fff, 0xd9bf83ff, 0x91b6e8ff, 0xc5a0dcff, 0x89c8caff, 0xddddddff,
    0xbbbbbbff, 0xffaca0ff, 0xb7de9fff, 0xead59bff, 0xb0caffff, 0xdcbce8ff, 0xa1dee0ff, 0xeeeeeeff,
];

pub(crate) fn terminal_ansi(index: u8) -> Option<Rgba> {
    let colors = if appearance().is_dark() {
        &ANSI_DARK
    } else {
        &ANSI_LIGHT
    };
    colors.get(usize::from(index)).copied().map(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: u32) -> f32 {
        let color = rgba(color);
        let linear = |component: f32| {
            if component <= 0.04045 {
                component / 12.92
            } else {
                ((component + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
    }

    #[test]
    fn workbench_text_and_focus_remain_readable_in_every_appearance() {
        let contrast = |a, b| {
            let a = luminance(a);
            let b = luminance(b);
            (a.max(b) + 0.05) / (a.min(b) + 0.05)
        };
        for appearance in Appearance::ALL {
            let p = appearance.palette();
            for background in [p.canvas, p.floor] {
                assert!(contrast(p.bone, background) >= 4.5, "{appearance:?}");
                assert!(contrast(p.ash, background) >= 4.5, "{appearance:?}");
                assert!(contrast(p.focus, background) >= 3.0, "{appearance:?}");
            }
            let muted_surface_minimum = 4.5;
            for background in [p.panel_hover, p.selection] {
                assert!(contrast(p.bone, background) >= 4.5, "{appearance:?}");
                assert!(
                    contrast(p.ash, background) >= muted_surface_minimum,
                    "{appearance:?}"
                );
                assert!(contrast(p.focus, background) >= 3.0, "{appearance:?}");
            }
            for accent in [p.focus, p.accent_hover, p.accent_pressed] {
                assert!(contrast(p.on_accent, accent) >= 4.5, "{appearance:?}");
            }
            let muted_diff_minimum = if appearance.is_dark() { 4.5 } else { 3.0 };
            for background in [p.diff_added, p.diff_removed, p.diff_empty] {
                assert!(contrast(p.bone, background) >= 4.5, "{appearance:?}");
                assert!(
                    contrast(p.ash, background) >= muted_diff_minimum,
                    "{appearance:?}"
                );
            }
            assert!(contrast(p.error, p.error_wash) >= 4.5, "{appearance:?}");
            assert!(contrast(p.error, p.diff_removed) >= 4.5, "{appearance:?}");
            assert!(contrast(p.success, p.diff_added) >= 4.5, "{appearance:?}");
            let ansi = if appearance.is_dark() {
                ANSI_DARK
            } else {
                ANSI_LIGHT
            };
            for color in ansi {
                assert!(
                    contrast(color, p.canvas) >= 4.5,
                    "{appearance:?}: {color:08x}"
                );
            }
        }
    }

    #[test]
    fn appearance_ids_are_stable_and_unique() {
        let ids = Appearance::ALL.map(Appearance::id);
        assert_eq!(ids, ["black", "light", "graphite", "charcoal"]);
        for (index, id) in ids.iter().enumerate() {
            assert!(!ids[..index].contains(id), "duplicate appearance ID: {id}");
        }
    }

    #[test]
    fn appearance_names_roundtrip_and_state_is_thread_local() {
        assert_eq!(Appearance::from_id("paper"), Some(Appearance::Light));
        assert_eq!(Appearance::from_id("midnight"), Some(Appearance::Black));
        assert_eq!(Appearance::from_id("ember"), Some(Appearance::Charcoal));
        for appearance in Appearance::ALL {
            assert_eq!(Appearance::from_id(appearance.id()), Some(appearance));
        }
        assert_eq!(Appearance::from_id("unknown"), None);
        let original = appearance();
        set_appearance(Appearance::Black);
        assert_eq!(appearance(), Appearance::Black);
        assert_eq!(
            std::thread::spawn(appearance).join().unwrap(),
            Appearance::Black
        );
        set_appearance(original);
    }
}
