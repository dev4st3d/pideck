//! Shared workbench palettes; Paper preserves the supplied design reference.

use std::cell::Cell;

use gpui::{Rems, Rgba, SharedString, rems, rgba};

pub(crate) mod terminal_manager;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Appearance {
    #[default]
    Paper,
    Linen,
    Graphite,
    Midnight,
}

impl Appearance {
    pub(crate) const ALL: [Self; 4] = [Self::Paper, Self::Linen, Self::Graphite, Self::Midnight];

    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Linen => "linen",
            Self::Graphite => "graphite",
            Self::Midnight => "midnight",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Paper => "Paper",
            Self::Linen => "Linen",
            Self::Graphite => "Graphite",
            Self::Midnight => "Midnight",
        }
    }

    pub(crate) fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|appearance| appearance.id() == id)
    }

    pub(crate) const fn is_dark(self) -> bool {
        matches!(self, Self::Graphite | Self::Midnight)
    }

    fn palette(self) -> &'static Palette {
        match self {
            Self::Paper => &PAPER,
            Self::Linen => &LINEN,
            Self::Graphite => &GRAPHITE,
            Self::Midnight => &MIDNIGHT,
        }
    }
}

thread_local! {
    // Rendering and theme changes stay on GPUI's UI thread. Workers receive
    // resolved colors or an explicit Appearance, never ambient theme state.
    static APPEARANCE: Cell<Appearance> = const { Cell::new(Appearance::Paper) };
}

pub(crate) fn appearance() -> Appearance {
    APPEARANCE.get()
}

pub(crate) fn set_appearance(appearance: Appearance) {
    APPEARANCE.set(appearance);
}

struct Palette {
    canvas: u32,
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

const PAPER: Palette = Palette {
    canvas: 0xf6f5f0ff,
    floor: 0xecebe4ff,
    panel_hover: 0xe2e6dcff,
    edge: 0xd5d8cfff,
    edge_hard: 0xb4bbaeff,
    bone: 0x242824ff,
    ash: 0x646a62ff,
    focus: 0x315c49ff,
    accent_hover: 0x284e3dff,
    accent_pressed: 0x234334ff,
    on_accent: 0xffffffff,
    error: 0x875346ff,
    error_wash: 0xf0e3dcff,
    selection: 0xdbe3d8ff,
    working: 0x756025ff,
    diff_added: 0xe1ebdcff,
    diff_removed: 0xf0e3dcff,
    diff_empty: 0xeeefe8ff,
};

const LINEN: Palette = Palette {
    canvas: 0xf7f3ebff,
    floor: 0xede6daff,
    panel_hover: 0xe6ddcdff,
    edge: 0xd8cfbfff,
    edge_hard: 0xb6a58fff,
    bone: 0x302b25ff,
    ash: 0x6c6255ff,
    focus: 0x6b4b34ff,
    accent_hover: 0x593d2aff,
    accent_pressed: 0x493122ff,
    on_accent: 0xffffffff,
    error: 0x8a453aff,
    error_wash: 0xf0ddd2ff,
    selection: 0xe5d8c5ff,
    working: 0x765923ff,
    diff_added: 0xe0e8d7ff,
    diff_removed: 0xf0ddd2ff,
    diff_empty: 0xeee9deff,
};

const GRAPHITE: Palette = Palette {
    canvas: 0x20211fff,
    floor: 0x191c19ff,
    panel_hover: 0x30382cff,
    edge: 0x3c4338ff,
    edge_hard: 0x737e69ff,
    bone: 0xeeefe8ff,
    ash: 0xabb3a4ff,
    focus: 0xc3d6a3ff,
    accent_hover: 0xd1e2b6ff,
    accent_pressed: 0xb1c78fff,
    on_accent: 0x20251bff,
    error: 0xf0b1a2ff,
    error_wash: 0x422c28ff,
    selection: 0x39482fff,
    working: 0xdec48cff,
    diff_added: 0x293d28ff,
    diff_removed: 0x422c28ff,
    diff_empty: 0x242922ff,
};

const MIDNIGHT: Palette = Palette {
    canvas: 0x141d2bff,
    floor: 0x101925ff,
    panel_hover: 0x23364fff,
    edge: 0x30445eff,
    edge_hard: 0x647f9eff,
    bone: 0xeaf0faff,
    ash: 0xa6b7ccff,
    focus: 0xadc6ffff,
    accent_hover: 0xc4d6ffff,
    accent_pressed: 0x96b5f5ff,
    on_accent: 0x142037ff,
    error: 0xf0adbaff,
    error_wash: 0x422d40ff,
    selection: 0x2a4264ff,
    working: 0xe7c69bff,
    diff_added: 0x1d3b37ff,
    diff_removed: 0x422d40ff,
    diff_empty: 0x1c293aff,
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
    0x242824ff, 0x9e3238ff, 0x2f6843ff, 0x765b00ff, 0x315ea3ff, 0x854080ff, 0x226d70ff, 0x646a62ff,
    0x646a62ff, 0xb13c40ff, 0x316c45ff, 0x7d6000ff, 0x395f9bff, 0x90518aff, 0x267076ff, 0x495048ff,
];

const ANSI_DARK: [u32; 16] = [
    0x8d9785ff, 0xf09085ff, 0x9dc88fff, 0xd9bf83ff, 0x91b6e8ff, 0xc5a0dcff, 0x89c8caff, 0xe0e7d8ff,
    0x9ba88eff, 0xffaca0ff, 0xb7de9fff, 0xead59bff, 0xb0caffff, 0xdcbce8ff, 0xa1dee0ff, 0xf6f7efff,
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
                for foreground in [p.bone, p.ash] {
                    assert!(contrast(foreground, background) >= 4.5, "{appearance:?}");
                }
                assert!(contrast(p.focus, background) >= 3.0, "{appearance:?}");
            }
            assert!(contrast(p.bone, p.selection) >= 4.5, "{appearance:?}");
            // Selected labels use ink; muted file icons need non-text contrast.
            assert!(contrast(p.ash, p.selection) >= 3.0, "{appearance:?}");
            assert!(contrast(p.focus, p.selection) >= 3.0, "{appearance:?}");
            for accent in [p.focus, p.accent_hover, p.accent_pressed] {
                assert!(contrast(p.on_accent, accent) >= 4.5, "{appearance:?}");
            }
            assert!(contrast(p.error, p.diff_removed) >= 4.5, "{appearance:?}");
            assert!(contrast(p.focus, p.diff_added) >= 4.5, "{appearance:?}");
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
    fn appearance_names_roundtrip_and_state_is_thread_local() {
        for appearance in Appearance::ALL {
            assert_eq!(Appearance::from_id(appearance.id()), Some(appearance));
        }
        assert_eq!(Appearance::from_id("unknown"), None);
        let original = appearance();
        set_appearance(Appearance::Midnight);
        assert_eq!(appearance(), Appearance::Midnight);
        assert_eq!(
            std::thread::spawn(appearance).join().unwrap(),
            Appearance::Paper
        );
        set_appearance(original);
    }
}
