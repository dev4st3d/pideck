//! Shared visual tokens for the harness desk.

use std::sync::atomic::{AtomicU8, Ordering};

use gpui::{Pixels, Rems, Rgba, SharedString, px, rems, rgba};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    // Dark
    PiDeckDark,
    CursorDark,
    MossFoundry,
    InkHarbor,
    VoltWorkshop,
    PlumArchive,
    SaltFlat,
    SaffronLoom,
    JuniperCoil,
    SmokeLibrary,
    PewterHall,
    OliveStudy,
    // Light
    ParchmentDesk,
    MistOrchard,
    CoralLedger,
    ChalkBlueprint,
    HoneyComb,
    PorcelainLab,
    CitrusGrove,
    Letterpress,
    LinenGallery,
    RicePaper,
    BoneChina,
}

impl ThemeId {
    pub const ALL: [Self; 23] = [
        Self::PiDeckDark,
        Self::CursorDark,
        Self::MossFoundry,
        Self::InkHarbor,
        Self::VoltWorkshop,
        Self::PlumArchive,
        Self::SaltFlat,
        Self::SaffronLoom,
        Self::JuniperCoil,
        Self::SmokeLibrary,
        Self::PewterHall,
        Self::OliveStudy,
        Self::ParchmentDesk,
        Self::MistOrchard,
        Self::CoralLedger,
        Self::ChalkBlueprint,
        Self::HoneyComb,
        Self::PorcelainLab,
        Self::CitrusGrove,
        Self::Letterpress,
        Self::LinenGallery,
        Self::RicePaper,
        Self::BoneChina,
    ];

    pub const DARK: [Self; 12] = [
        Self::PiDeckDark,
        Self::CursorDark,
        Self::MossFoundry,
        Self::InkHarbor,
        Self::VoltWorkshop,
        Self::PlumArchive,
        Self::SaltFlat,
        Self::SaffronLoom,
        Self::JuniperCoil,
        Self::SmokeLibrary,
        Self::PewterHall,
        Self::OliveStudy,
    ];

    pub const LIGHT: [Self; 11] = [
        Self::ParchmentDesk,
        Self::MistOrchard,
        Self::CoralLedger,
        Self::ChalkBlueprint,
        Self::HoneyComb,
        Self::PorcelainLab,
        Self::CitrusGrove,
        Self::Letterpress,
        Self::LinenGallery,
        Self::RicePaper,
        Self::BoneChina,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::PiDeckDark => "Pideck Dark",
            Self::CursorDark => "Cursor Dark",
            Self::MossFoundry => "Moss Foundry",
            Self::InkHarbor => "Ink Harbor",
            Self::VoltWorkshop => "Volt Workshop",
            Self::PlumArchive => "Plum Archive",
            Self::SaltFlat => "Salt Flat",
            Self::SaffronLoom => "Saffron Loom",
            Self::JuniperCoil => "Juniper Coil",
            Self::SmokeLibrary => "Smoke Library",
            Self::PewterHall => "Pewter Hall",
            Self::OliveStudy => "Olive Study",
            Self::ParchmentDesk => "Parchment Desk",
            Self::MistOrchard => "Mist Orchard",
            Self::CoralLedger => "Coral Ledger",
            Self::ChalkBlueprint => "Chalk Blueprint",
            Self::HoneyComb => "Honey Comb",
            Self::PorcelainLab => "Porcelain Lab",
            Self::CitrusGrove => "Citrus Grove",
            Self::Letterpress => "Letterpress",
            Self::LinenGallery => "Linen Gallery",
            Self::RicePaper => "Rice Paper",
            Self::BoneChina => "Bone China",
        }
    }

    pub const fn mode(self) -> ThemeMode {
        match self {
            Self::PiDeckDark
            | Self::CursorDark
            | Self::MossFoundry
            | Self::InkHarbor
            | Self::VoltWorkshop
            | Self::PlumArchive
            | Self::SaltFlat
            | Self::SaffronLoom
            | Self::JuniperCoil
            | Self::SmokeLibrary
            | Self::PewterHall
            | Self::OliveStudy => ThemeMode::Dark,
            Self::ParchmentDesk
            | Self::MistOrchard
            | Self::CoralLedger
            | Self::ChalkBlueprint
            | Self::HoneyComb
            | Self::PorcelainLab
            | Self::CitrusGrove
            | Self::Letterpress
            | Self::LinenGallery
            | Self::RicePaper
            | Self::BoneChina => ThemeMode::Light,
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
            Self::PiDeckDark => Self::CursorDark,
            Self::CursorDark => Self::MossFoundry,
            Self::MossFoundry => Self::InkHarbor,
            Self::InkHarbor => Self::VoltWorkshop,
            Self::VoltWorkshop => Self::PlumArchive,
            Self::PlumArchive => Self::SaltFlat,
            Self::SaltFlat => Self::SaffronLoom,
            Self::SaffronLoom => Self::JuniperCoil,
            Self::JuniperCoil => Self::SmokeLibrary,
            Self::SmokeLibrary => Self::PewterHall,
            Self::PewterHall => Self::OliveStudy,
            Self::OliveStudy => Self::ParchmentDesk,
            Self::ParchmentDesk => Self::MistOrchard,
            Self::MistOrchard => Self::CoralLedger,
            Self::CoralLedger => Self::ChalkBlueprint,
            Self::ChalkBlueprint => Self::HoneyComb,
            Self::HoneyComb => Self::PorcelainLab,
            Self::PorcelainLab => Self::CitrusGrove,
            Self::CitrusGrove => Self::Letterpress,
            Self::Letterpress => Self::LinenGallery,
            Self::LinenGallery => Self::RicePaper,
            Self::RicePaper => Self::BoneChina,
            Self::BoneChina => Self::PiDeckDark,
        }
    }

    const fn index(self) -> u8 {
        match self {
            Self::PiDeckDark => 0,
            Self::CursorDark => 1,
            Self::MossFoundry => 2,
            Self::InkHarbor => 3,
            Self::VoltWorkshop => 4,
            Self::PlumArchive => 5,
            Self::SaltFlat => 6,
            Self::SaffronLoom => 7,
            Self::JuniperCoil => 8,
            Self::SmokeLibrary => 9,
            Self::PewterHall => 10,
            Self::OliveStudy => 11,
            Self::ParchmentDesk => 12,
            Self::MistOrchard => 13,
            Self::CoralLedger => 14,
            Self::ChalkBlueprint => 15,
            Self::HoneyComb => 16,
            Self::PorcelainLab => 17,
            Self::CitrusGrove => 18,
            Self::Letterpress => 19,
            Self::LinenGallery => 20,
            Self::RicePaper => 21,
            Self::BoneChina => 22,
        }
    }

    const fn from_index(index: u8) -> Self {
        match index {
            1 => Self::CursorDark,
            2 => Self::MossFoundry,
            3 => Self::InkHarbor,
            4 => Self::VoltWorkshop,
            5 => Self::PlumArchive,
            6 => Self::SaltFlat,
            7 => Self::SaffronLoom,
            8 => Self::JuniperCoil,
            9 => Self::SmokeLibrary,
            10 => Self::PewterHall,
            11 => Self::OliveStudy,
            12 => Self::ParchmentDesk,
            13 => Self::MistOrchard,
            14 => Self::CoralLedger,
            15 => Self::ChalkBlueprint,
            16 => Self::HoneyComb,
            17 => Self::PorcelainLab,
            18 => Self::CitrusGrove,
            19 => Self::Letterpress,
            20 => Self::LinenGallery,
            21 => Self::RicePaper,
            22 => Self::BoneChina,
            _ => Self::PiDeckDark,
        }
    }

    const fn palette(self) -> &'static Palette {
        match self {
            Self::PiDeckDark => &PIDECK_DARK,
            Self::CursorDark => &CURSOR_DARK,
            Self::MossFoundry => &MOSS_FOUNDRY,
            Self::InkHarbor => &INK_HARBOR,
            Self::VoltWorkshop => &VOLT_WORKSHOP,
            Self::PlumArchive => &PLUM_ARCHIVE,
            Self::SaltFlat => &SALT_FLAT,
            Self::SaffronLoom => &SAFFRON_LOOM,
            Self::JuniperCoil => &JUNIPER_COIL,
            Self::SmokeLibrary => &SMOKE_LIBRARY,
            Self::PewterHall => &PEWTER_HALL,
            Self::OliveStudy => &OLIVE_STUDY,
            Self::ParchmentDesk => &PARCHMENT_DESK,
            Self::MistOrchard => &MIST_ORCHARD,
            Self::CoralLedger => &CORAL_LEDGER,
            Self::ChalkBlueprint => &CHALK_BLUEPRINT,
            Self::HoneyComb => &HONEY_COMB,
            Self::PorcelainLab => &PORCELAIN_LAB,
            Self::CitrusGrove => &CITRUS_GROVE,
            Self::Letterpress => &LETTERPRESS,
            Self::LinenGallery => &LINEN_GALLERY,
            Self::RicePaper => &RICE_PAPER,
            Self::BoneChina => &BONE_CHINA,
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
pub const SIDE_W: f32 = 244.0;
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

// Deep green-black workshop with bright mint text and copper signal — stronger
// surface steps and text contrast than the first pass.
const MOSS_FOUNDRY: Palette = Palette {
    canvas: 0x060908ff,
    floor: 0x0c1210ff,
    panel: 0x141c18ff,
    panel_lift: 0x1e2a24ff,
    panel_hover: 0x28362eff,
    user_message: 0x18241eff,
    user_message_edge: 0xd49a4a55,
    edge: 0xd8ecda30,
    edge_hard: 0xd8ecda4a,
    edge_soft: 0xd8ecda1c,
    bone: 0xf2faf3ff,
    bone_dim: 0xc4d4c8ff,
    ash: 0x96ab9cff,
    smoke: 0x718478ff,
    signal: 0xd49a4aff,
    signal_deep: 0xa67232ff,
    signal_hot: 0xe6ae62ff,
    focus: 0xf0d090ff,
    error: 0xf08a72ff,
    error_wash: 0xf08a7218,
    live: 0x8ed49eff,
    live_wash: 0x8ed49e18,
    working: 0x72b8ccff,
    data: 0xe0b86aff,
    data_wash: 0xe0b86a18,
};

// Navy night desk with ice text and bright seafoam — deeper base, clearer type.
const INK_HARBOR: Palette = Palette {
    canvas: 0x05070cff,
    floor: 0x0a0f16ff,
    panel: 0x121a24ff,
    panel_lift: 0x1c2734ff,
    panel_hover: 0x263242ff,
    user_message: 0x14202cff,
    user_message_edge: 0x3ecfb655,
    edge: 0xd0e4f430,
    edge_hard: 0xd0e4f44a,
    edge_soft: 0xd0e4f41c,
    bone: 0xf0f6fcff,
    bone_dim: 0xb8c8d8ff,
    ash: 0x8a9cb0ff,
    smoke: 0x66788cff,
    signal: 0x3ecfb6ff,
    signal_deep: 0x2a9e8aff,
    signal_hot: 0x5ae0c8ff,
    focus: 0x7edcf0ff,
    error: 0xff8a8aff,
    error_wash: 0xff8a8a18,
    live: 0x6ed4aeff,
    live_wash: 0x6ed4ae18,
    working: 0x6aa8e0ff,
    data: 0xf0b86aff,
    data_wash: 0xf0b86a18,
};

// Graphite garage with electric amber — high-energy industrial night desk.
const VOLT_WORKSHOP: Palette = Palette {
    canvas: 0x070708ff,
    floor: 0x0e0e10ff,
    panel: 0x17171aff,
    panel_lift: 0x222228ff,
    panel_hover: 0x2c2c34ff,
    user_message: 0x1a1a20ff,
    user_message_edge: 0xffc04055,
    edge: 0xe8e8f030,
    edge_hard: 0xe8e8f04a,
    edge_soft: 0xe8e8f01c,
    bone: 0xf7f7faff,
    bone_dim: 0xc4c4ceff,
    ash: 0x9494a2ff,
    smoke: 0x6e6e7cff,
    signal: 0xffc040ff,
    signal_deep: 0xd49a20ff,
    signal_hot: 0xffd060ff,
    focus: 0xffe080ff,
    error: 0xff6a6aff,
    error_wash: 0xff6a6a18,
    live: 0x5ad48aff,
    live_wash: 0x5ad48a18,
    working: 0x5a9ae0ff,
    data: 0xffb050ff,
    data_wash: 0xffb05018,
};

// Deep aubergine archive with cream text and rose-gold signal.
const PLUM_ARCHIVE: Palette = Palette {
    canvas: 0x0a070cff,
    floor: 0x120e16ff,
    panel: 0x1c1622ff,
    panel_lift: 0x28202eff,
    panel_hover: 0x342a3cff,
    user_message: 0x221a28ff,
    user_message_edge: 0xe8a87855,
    edge: 0xf0e0f030,
    edge_hard: 0xf0e0f04a,
    edge_soft: 0xf0e0f01c,
    bone: 0xfaf4f6ff,
    bone_dim: 0xd4c4ceff,
    ash: 0xa894a4ff,
    smoke: 0x7e6c7cff,
    signal: 0xe8a878ff,
    signal_deep: 0xc07e52ff,
    signal_hot: 0xf0bc90ff,
    focus: 0xf0c8a0ff,
    error: 0xf07890ff,
    error_wash: 0xf0789018,
    live: 0x8ad4a0ff,
    live_wash: 0x8ad4a018,
    working: 0x9a88e0ff,
    data: 0xf0b070ff,
    data_wash: 0xf0b07018,
};

// Warm ivory paper with deep sepia ink — clearer type and panel edges.
const PARCHMENT_DESK: Palette = Palette {
    canvas: 0xfbf7f0ff,
    floor: 0xf2ebe0ff,
    panel: 0xe8dfd0ff,
    panel_lift: 0xddd2c0ff,
    panel_hover: 0xd0c4b0ff,
    user_message: 0xefe6d8ff,
    user_message_edge: 0xb84a28aa,
    edge: 0x2a201830,
    edge_hard: 0x2a20184a,
    edge_soft: 0x2a20181c,
    bone: 0x1a1410ff,
    bone_dim: 0x3a3228ff,
    ash: 0x5a5044ff,
    smoke: 0x7a6e60ff,
    signal: 0xb84a28ff,
    signal_deep: 0x8e3418ff,
    signal_hot: 0xd05e38ff,
    focus: 0x8a5a08ff,
    error: 0xb02018ff,
    error_wash: 0xb0201818,
    live: 0x2e6e28ff,
    live_wash: 0x2e6e2818,
    working: 0x28608cff,
    data: 0x8a5a10ff,
    data_wash: 0x8a5a1018,
};

// Clean sage-white glasshouse with dark forest ink and leaf green signal.
const MIST_ORCHARD: Palette = Palette {
    canvas: 0xf4faf5ff,
    floor: 0xe8f0eaff,
    panel: 0xdce8e0ff,
    panel_lift: 0xced8cfff,
    panel_hover: 0xbed0c0ff,
    user_message: 0xe4eee6ff,
    user_message_edge: 0x1e7a3eaa,
    edge: 0x14201830,
    edge_hard: 0x1420184a,
    edge_soft: 0x1420181c,
    bone: 0x101a14ff,
    bone_dim: 0x2a3a30ff,
    ash: 0x4a5e52ff,
    smoke: 0x6a7e70ff,
    signal: 0x1e7a3eff,
    signal_deep: 0x145c2cff,
    signal_hot: 0x2e9450ff,
    focus: 0x186878ff,
    error: 0xb02828ff,
    error_wash: 0xb0282818,
    live: 0x1e8a48ff,
    live_wash: 0x1e8a4818,
    working: 0x2868a0ff,
    data: 0x9a6810ff,
    data_wash: 0x9a681018,
};

// Soft blush paper with espresso ink and vivid coral signal.
const CORAL_LEDGER: Palette = Palette {
    canvas: 0xfff8f6ff,
    floor: 0xf6ebe8ff,
    panel: 0xecdfdaff,
    panel_lift: 0xe0d0caff,
    panel_hover: 0xd2c0b8ff,
    user_message: 0xf4e8e4ff,
    user_message_edge: 0xc83a3aaa,
    edge: 0x28181030,
    edge_hard: 0x2818104a,
    edge_soft: 0x2818101c,
    bone: 0x18100eff,
    bone_dim: 0x3a2a26ff,
    ash: 0x5e4a44ff,
    smoke: 0x7e6860ff,
    signal: 0xc83a3aff,
    signal_deep: 0x9e2828ff,
    signal_hot: 0xe04e4eff,
    focus: 0x8a3a28ff,
    error: 0xb01010ff,
    error_wash: 0xb0101018,
    live: 0x287a48ff,
    live_wash: 0x287a4818,
    working: 0x3a5e9cff,
    data: 0xa06018ff,
    data_wash: 0xa0601818,
};

// Near-white chalk board with deep navy ink and blueprint cyan accents.
const CHALK_BLUEPRINT: Palette = Palette {
    canvas: 0xf7f9fcff,
    floor: 0xecf0f6ff,
    panel: 0xe0e6f0ff,
    panel_lift: 0xd0d8e6ff,
    panel_hover: 0xc0cadcff,
    user_message: 0xe8edf6ff,
    user_message_edge: 0x1860c0aa,
    edge: 0x10182830,
    edge_hard: 0x1018284a,
    edge_soft: 0x1018281c,
    bone: 0x0c1420ff,
    bone_dim: 0x283848ff,
    ash: 0x4a5e74ff,
    smoke: 0x6a7e94ff,
    signal: 0x1860c0ff,
    signal_deep: 0x104890ff,
    signal_hot: 0x2878e0ff,
    focus: 0x0e7088ff,
    error: 0xb01828ff,
    error_wash: 0xb0182818,
    live: 0x187848ff,
    live_wash: 0x18784818,
    working: 0x0e88a8ff,
    data: 0xa06010ff,
    data_wash: 0xa0601018,
};

// Bright warm paper with espresso ink and honey-amber signal.
const HONEY_COMB: Palette = Palette {
    canvas: 0xfffbf4ff,
    floor: 0xf6f0e4ff,
    panel: 0xeee4d4ff,
    panel_lift: 0xe2d6c2ff,
    panel_hover: 0xd4c6aeff,
    user_message: 0xf4eadcff,
    user_message_edge: 0xc07a10aa,
    edge: 0x20180830,
    edge_hard: 0x2018084a,
    edge_soft: 0x2018081c,
    bone: 0x16120aff,
    bone_dim: 0x3a3224ff,
    ash: 0x5e5444ff,
    smoke: 0x7e7260ff,
    signal: 0xc07a10ff,
    signal_deep: 0x945c08ff,
    signal_hot: 0xdc9220ff,
    focus: 0x8a5808ff,
    error: 0xb02018ff,
    error_wash: 0xb0201818,
    live: 0x2e6e20ff,
    live_wash: 0x2e6e2018,
    working: 0x28688cff,
    data: 0xa06008ff,
    data_wash: 0xa0600818,
};

// Mineral night on a dry salt pan — nearly monochrome graphite, salt-white type,
// one glacial cyan spark. Crisp, sterile, high-altitude.
const SALT_FLAT: Palette = Palette {
    canvas: 0x060708ff,
    floor: 0x0c0e10ff,
    panel: 0x14171aff,
    panel_lift: 0x1e2226ff,
    panel_hover: 0x282e34ff,
    user_message: 0x181c20ff,
    user_message_edge: 0x3ad4e055,
    edge: 0xf0f4f838,
    edge_hard: 0xf0f4f858,
    edge_soft: 0xf0f4f820,
    bone: 0xfafcfeff,
    bone_dim: 0xc8d0d8ff,
    ash: 0x96a0aaff,
    smoke: 0x6e7882ff,
    signal: 0x3ad4e0ff,
    signal_deep: 0x22a8b4ff,
    signal_hot: 0x5ae8f0ff,
    focus: 0x9af0f8ff,
    error: 0xff6e6eff,
    error_wash: 0xff6e6e18,
    live: 0x48d498ff,
    live_wash: 0x48d49818,
    working: 0x5aa8e8ff,
    data: 0xf0c050ff,
    data_wash: 0xf0c05018,
};

// Night market loom — espresso black, ivory thread, pure saffron. Warm spice
// without muddiness; textile, not tropical cliché.
const SAFFRON_LOOM: Palette = Palette {
    canvas: 0x0a0806ff,
    floor: 0x12100cff,
    panel: 0x1c1812ff,
    panel_lift: 0x282218ff,
    panel_hover: 0x342e20ff,
    user_message: 0x221c14ff,
    user_message_edge: 0xf0b02055,
    edge: 0xf8ecd038,
    edge_hard: 0xf8ecd058,
    edge_soft: 0xf8ecd020,
    bone: 0xfff6e8ff,
    bone_dim: 0xd8c8aeff,
    ash: 0xa89478ff,
    smoke: 0x7e6e56ff,
    signal: 0xf0b020ff,
    signal_deep: 0xc08810ff,
    signal_hot: 0xffc840ff,
    focus: 0xffd878ff,
    error: 0xf06048ff,
    error_wash: 0xf0604818,
    live: 0x70c860ff,
    live_wash: 0x70c86018,
    working: 0x68a0d0ff,
    data: 0xe8a828ff,
    data_wash: 0xe8a82818,
};

// Blue-black botanical pharmacy — cool silver type, juniper-berry violet signal.
// Dry, aromatic, slightly medicinal.
const JUNIPER_COIL: Palette = Palette {
    canvas: 0x07080cff,
    floor: 0x0e1016ff,
    panel: 0x161a22ff,
    panel_lift: 0x202630ff,
    panel_hover: 0x2a3240ff,
    user_message: 0x1a1e28ff,
    user_message_edge: 0xb878e055,
    edge: 0xe0e4f038,
    edge_hard: 0xe0e4f058,
    edge_soft: 0xe0e4f020,
    bone: 0xf2f4faff,
    bone_dim: 0xc0c6d4ff,
    ash: 0x8e96a8ff,
    smoke: 0x687088ff,
    signal: 0xb878e0ff,
    signal_deep: 0x8e52b8ff,
    signal_hot: 0xcc94f0ff,
    focus: 0xd8aef8ff,
    error: 0xf07088ff,
    error_wash: 0xf0708818,
    live: 0x60d0a0ff,
    live_wash: 0x60d0a018,
    working: 0x7090e8ff,
    data: 0xe8a860ff,
    data_wash: 0xe8a86018,
};

// Clinical porcelain morning — pure cool white, hard charcoal ink, orchid
// magenta spark. Lab-clean, not pastel nursery.
const PORCELAIN_LAB: Palette = Palette {
    canvas: 0xfafbfdff,
    floor: 0xf0f2f6ff,
    panel: 0xe4e8eeff,
    panel_lift: 0xd6dce6ff,
    panel_hover: 0xc6cedcff,
    user_message: 0xeceff4ff,
    user_message_edge: 0xc02080aa,
    edge: 0x10141c34,
    edge_hard: 0x10141c52,
    edge_soft: 0x10141c1e,
    bone: 0x0e1218ff,
    bone_dim: 0x2c3440ff,
    ash: 0x4e5868ff,
    smoke: 0x6e7888ff,
    signal: 0xc02080ff,
    signal_deep: 0x941860ff,
    signal_hot: 0xd83898ff,
    focus: 0x1860a8ff,
    error: 0xb01020ff,
    error_wash: 0xb0102018,
    live: 0x107848ff,
    live_wash: 0x10784818,
    working: 0x2060c0ff,
    data: 0xa05010ff,
    data_wash: 0xa0501018,
};

// Midday citrus grove — lemon-white paper, deep olive ink, sharp lime. Bright
// without neon; Mediterranean orchard, not candy.
const CITRUS_GROVE: Palette = Palette {
    canvas: 0xfffdf4ff,
    floor: 0xf6f2e4ff,
    panel: 0xece6d4ff,
    panel_lift: 0xe0d8c2ff,
    panel_hover: 0xd2c8aeff,
    user_message: 0xf4eedeff,
    user_message_edge: 0x3a8c18aa,
    edge: 0x1c201034,
    edge_hard: 0x1c201052,
    edge_soft: 0x1c20101e,
    bone: 0x14180cff,
    bone_dim: 0x343c24ff,
    ash: 0x566040ff,
    smoke: 0x768058ff,
    signal: 0x3a8c18ff,
    signal_deep: 0x286c10ff,
    signal_hot: 0x4eac28ff,
    focus: 0x187080ff,
    error: 0xb02018ff,
    error_wash: 0xb0201818,
    live: 0x2e7c20ff,
    live_wash: 0x2e7c2018,
    working: 0x2868a0ff,
    data: 0xb07008ff,
    data_wash: 0xb0700818,
};

// Newspaper desk — pure newsprint, blue-black printer's ink, press red. Typographic
// crispness; letterpress shop, not comic book.
const LETTERPRESS: Palette = Palette {
    canvas: 0xfafaf8ff,
    floor: 0xf0f0ecff,
    panel: 0xe4e4deff,
    panel_lift: 0xd6d6ceff,
    panel_hover: 0xc6c6bcff,
    user_message: 0xeeeee8ff,
    user_message_edge: 0xc01828aa,
    edge: 0x10101034,
    edge_hard: 0x10101052,
    edge_soft: 0x1010101e,
    bone: 0x0c0e12ff,
    bone_dim: 0x2a2e36ff,
    ash: 0x4c525cff,
    smoke: 0x6c7280ff,
    signal: 0xc01828ff,
    signal_deep: 0x941018ff,
    signal_hot: 0xdc2838ff,
    focus: 0x1840a0ff,
    error: 0xa01018ff,
    error_wash: 0xa0101818,
    live: 0x187040ff,
    live_wash: 0x18704018,
    working: 0x2050b0ff,
    data: 0x986010ff,
    data_wash: 0x98601018,
};

// Quiet reading room after hours — warm smoked charcoal, soft paper type, muted
// rosewood. Low chroma, high comfort; library lamp, not fireplace drama.
const SMOKE_LIBRARY: Palette = Palette {
    canvas: 0x0c0b0aff,
    floor: 0x141210ff,
    panel: 0x1c1a17ff,
    panel_lift: 0x26231fff,
    panel_hover: 0x302c27ff,
    user_message: 0x201c18ff,
    user_message_edge: 0xb8887044,
    edge: 0xe8e0d428,
    edge_hard: 0xe8e0d442,
    edge_soft: 0xe8e0d418,
    bone: 0xf2ebe2ff,
    bone_dim: 0xc4b8acff,
    ash: 0x968a7eff,
    smoke: 0x72685eff,
    signal: 0xb88870ff,
    signal_deep: 0x8e6854ff,
    signal_hot: 0xc89c84ff,
    focus: 0xd4b898ff,
    error: 0xd08070ff,
    error_wash: 0xd0807016,
    live: 0x90b898ff,
    live_wash: 0x90b89816,
    working: 0x8898b0ff,
    data: 0xc4a888ff,
    data_wash: 0xc4a88816,
};

// Architectural evening hall — cool pewter greys, soft silver type, restrained
// slate signal. Quiet metal and stone; gallery corridor, not spaceship.
const PEWTER_HALL: Palette = Palette {
    canvas: 0x0a0b0cff,
    floor: 0x121416ff,
    panel: 0x1a1c1fff,
    panel_lift: 0x24272bff,
    panel_hover: 0x2e3237ff,
    user_message: 0x1e2125ff,
    user_message_edge: 0x7a8ea044,
    edge: 0xdce2e828,
    edge_hard: 0xdce2e842,
    edge_soft: 0xdce2e818,
    bone: 0xeef1f4ff,
    bone_dim: 0xb8c0c8ff,
    ash: 0x8a949eff,
    smoke: 0x687078ff,
    signal: 0x7a8ea0ff,
    signal_deep: 0x5c6e80ff,
    signal_hot: 0x92a6b8ff,
    focus: 0xa8bcc8ff,
    error: 0xc88888ff,
    error_wash: 0xc8888816,
    live: 0x88b0a0ff,
    live_wash: 0x88b0a016,
    working: 0x8098b8ff,
    data: 0xb8a888ff,
    data_wash: 0xb8a88816,
};

// Scholarly olive study — deep olive-brown black, soft cream type, sage-bronze
// signal. Dry botanical calm; antique desk, not jungle.
const OLIVE_STUDY: Palette = Palette {
    canvas: 0x0a0b09ff,
    floor: 0x121410ff,
    panel: 0x1a1c16ff,
    panel_lift: 0x242720ff,
    panel_hover: 0x2e3228ff,
    user_message: 0x1e221aff,
    user_message_edge: 0xa8986844,
    edge: 0xe0e4d428,
    edge_hard: 0xe0e4d442,
    edge_soft: 0xe0e4d418,
    bone: 0xf0eee4ff,
    bone_dim: 0xc0c0b0ff,
    ash: 0x909484ff,
    smoke: 0x6e7264ff,
    signal: 0xa89868ff,
    signal_deep: 0x807850ff,
    signal_hot: 0xbcac80ff,
    focus: 0xc8c098ff,
    error: 0xc88878ff,
    error_wash: 0xc8887816,
    live: 0x90b088ff,
    live_wash: 0x90b08816,
    working: 0x8898a8ff,
    data: 0xb8a070ff,
    data_wash: 0xb8a07016,
};

// Soft gallery morning — warm linen white, soft charcoal ink, stone-taupe
// accent. Museum wall calm; nothing shouts.
const LINEN_GALLERY: Palette = Palette {
    canvas: 0xfaf8f5ff,
    floor: 0xf2efeaff,
    panel: 0xe8e4ddff,
    panel_lift: 0xddd8cfff,
    panel_hover: 0xd0cac0ff,
    user_message: 0xf0ece6ff,
    user_message_edge: 0x8a7a6844,
    edge: 0x2a262230,
    edge_hard: 0x2a262248,
    edge_soft: 0x2a26221c,
    bone: 0x1a1816ff,
    bone_dim: 0x3c3834ff,
    ash: 0x5e5852ff,
    smoke: 0x7e7870ff,
    signal: 0x8a7a68ff,
    signal_deep: 0x6a5c4cff,
    signal_hot: 0xa29280ff,
    focus: 0x5a6878ff,
    error: 0xa84038ff,
    error_wash: 0xa8403816,
    live: 0x487858ff,
    live_wash: 0x48785816,
    working: 0x506888ff,
    data: 0x8a7040ff,
    data_wash: 0x8a704016,
};

// Calligraphy desk — cool rice-paper white, soft sumi ink, restrained indigo.
// Quiet brushwork; not anime, not neon.
const RICE_PAPER: Palette = Palette {
    canvas: 0xf8f7f4ff,
    floor: 0xf0efeaff,
    panel: 0xe6e4deff,
    panel_lift: 0xdad8d0ff,
    panel_hover: 0xcccac0ff,
    user_message: 0xeeece8ff,
    user_message_edge: 0x3a4a7044,
    edge: 0x1c1c2030,
    edge_hard: 0x1c1c2048,
    edge_soft: 0x1c1c201c,
    bone: 0x16161aff,
    bone_dim: 0x36363cff,
    ash: 0x56565eff,
    smoke: 0x76767eff,
    signal: 0x3a4a70ff,
    signal_deep: 0x2a3654ff,
    signal_hot: 0x4e6090ff,
    focus: 0x4a6080ff,
    error: 0xa03838ff,
    error_wash: 0xa0383816,
    live: 0x3a7050ff,
    live_wash: 0x3a705016,
    working: 0x4870a0ff,
    data: 0x8a6840ff,
    data_wash: 0x8a684016,
};

// Afternoon tea service — soft bone-china ivory, warm slate ink, dusty brass.
// Polite and warm; china cabinet, not carnival.
const BONE_CHINA: Palette = Palette {
    canvas: 0xfbf9f6ff,
    floor: 0xf4f0ebff,
    panel: 0xebe6e0ff,
    panel_lift: 0xe0d9d1ff,
    panel_hover: 0xd4ccc2ff,
    user_message: 0xf2ede8ff,
    user_message_edge: 0xa0806044,
    edge: 0x28241e30,
    edge_hard: 0x28241e48,
    edge_soft: 0x28241e1c,
    bone: 0x1c1814ff,
    bone_dim: 0x3e3832ff,
    ash: 0x605850ff,
    smoke: 0x807870ff,
    signal: 0xa08060ff,
    signal_deep: 0x7a6048ff,
    signal_hot: 0xb89878ff,
    focus: 0x786858ff,
    error: 0xa84840ff,
    error_wash: 0xa8484016,
    live: 0x507850ff,
    live_wash: 0x50785016,
    working: 0x587090ff,
    data: 0x907040ff,
    data_wash: 0x90704016,
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
    fn theme_catalog_covers_light_and_dark() {
        assert_eq!(
            ThemeId::ALL.len(),
            ThemeId::DARK.len() + ThemeId::LIGHT.len()
        );
        for theme in ThemeId::DARK {
            assert_eq!(theme.mode(), ThemeMode::Dark);
        }
        for theme in ThemeId::LIGHT {
            assert_eq!(theme.mode(), ThemeMode::Light);
        }
        assert_eq!(ThemeId::PiDeckDark.label(), "Pideck Dark");
        assert_eq!(ThemeId::MossFoundry.label(), "Moss Foundry");
        assert_eq!(ThemeId::InkHarbor.label(), "Ink Harbor");
        assert_eq!(ThemeId::VoltWorkshop.label(), "Volt Workshop");
        assert_eq!(ThemeId::PlumArchive.label(), "Plum Archive");
        assert_eq!(ThemeId::SaltFlat.label(), "Salt Flat");
        assert_eq!(ThemeId::SaffronLoom.label(), "Saffron Loom");
        assert_eq!(ThemeId::JuniperCoil.label(), "Juniper Coil");
        assert_eq!(ThemeId::SmokeLibrary.label(), "Smoke Library");
        assert_eq!(ThemeId::PewterHall.label(), "Pewter Hall");
        assert_eq!(ThemeId::OliveStudy.label(), "Olive Study");
        assert_eq!(ThemeId::ParchmentDesk.label(), "Parchment Desk");
        assert_eq!(ThemeId::MistOrchard.label(), "Mist Orchard");
        assert_eq!(ThemeId::CoralLedger.label(), "Coral Ledger");
        assert_eq!(ThemeId::ChalkBlueprint.label(), "Chalk Blueprint");
        assert_eq!(ThemeId::HoneyComb.label(), "Honey Comb");
        assert_eq!(ThemeId::PorcelainLab.label(), "Porcelain Lab");
        assert_eq!(ThemeId::CitrusGrove.label(), "Citrus Grove");
        assert_eq!(ThemeId::Letterpress.label(), "Letterpress");
        assert_eq!(ThemeId::LinenGallery.label(), "Linen Gallery");
        assert_eq!(ThemeId::RicePaper.label(), "Rice Paper");
        assert_eq!(ThemeId::BoneChina.label(), "Bone China");
    }

    #[test]
    fn theme_cycle_walks_the_full_catalog() {
        let mut theme = ThemeId::PiDeckDark;
        let mut seen = Vec::new();
        for _ in 0..ThemeId::ALL.len() {
            seen.push(theme);
            theme = theme.next();
        }
        assert_eq!(theme, ThemeId::PiDeckDark);
        assert_eq!(seen.len(), ThemeId::ALL.len());
        for expected in ThemeId::ALL {
            assert!(seen.contains(&expected), "missing {expected:?}");
        }
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

    #[test]
    fn primary_text_meets_aa_contrast_against_canvas() {
        // WCAG AA for normal text is 4.5:1. UI body tokens should clear this.
        for theme in ThemeId::ALL {
            let palette = theme.palette();
            let ratio = contrast_ratio(palette.bone, palette.canvas);
            assert!(
                ratio >= 4.5,
                "{theme:?} bone/canvas contrast {ratio:.2} is below 4.5"
            );
            let secondary = contrast_ratio(palette.bone_dim, palette.canvas);
            assert!(
                secondary >= 3.0,
                "{theme:?} bone_dim/canvas contrast {secondary:.2} is below 3.0"
            );
        }
    }

    #[test]
    fn surface_stack_separates_panels_from_canvas() {
        for theme in ThemeId::ALL {
            let palette = theme.palette();
            let canvas_l = relative_luminance(palette.canvas);
            let panel_l = relative_luminance(palette.panel);
            let lift_l = relative_luminance(palette.panel_lift);
            match theme.mode() {
                ThemeMode::Dark => {
                    // Elevated panels should sit above the canvas.
                    assert!(
                        panel_l > canvas_l,
                        "{theme:?} dark panel is not lighter than canvas"
                    );
                    assert!(
                        lift_l >= panel_l,
                        "{theme:?} dark panel_lift is not at least as light as panel"
                    );
                }
                ThemeMode::Light => {
                    // Recessed panels should sit below the paper canvas.
                    assert!(
                        panel_l < canvas_l,
                        "{theme:?} light panel is not darker than canvas"
                    );
                    assert!(
                        lift_l <= panel_l,
                        "{theme:?} light panel_lift is not at least as dark as panel"
                    );
                }
            }
        }
    }

    #[test]
    fn light_themes_use_light_surfaces_and_dark_ink() {
        for theme in ThemeId::LIGHT {
            let palette = theme.palette();
            assert!(
                relative_luminance(palette.canvas) > 0.75,
                "{theme:?} canvas is not light enough"
            );
            assert!(
                relative_luminance(palette.bone) < 0.12,
                "{theme:?} bone ink is not dark enough"
            );
        }
    }

    #[test]
    fn unique_dark_themes_keep_distinct_accent_families() {
        assert_eq!(MOSS_FOUNDRY.signal, 0xd49a4aff);
        assert_eq!(INK_HARBOR.signal, 0x3ecfb6ff);
        assert_eq!(VOLT_WORKSHOP.signal, 0xffc040ff);
        assert_eq!(PLUM_ARCHIVE.signal, 0xe8a878ff);
        assert_eq!(SALT_FLAT.signal, 0x3ad4e0ff);
        assert_eq!(SAFFRON_LOOM.signal, 0xf0b020ff);
        assert_eq!(JUNIPER_COIL.signal, 0xb878e0ff);
        assert_eq!(SMOKE_LIBRARY.signal, 0xb88870ff);
        assert_eq!(PEWTER_HALL.signal, 0x7a8ea0ff);
        assert_eq!(OLIVE_STUDY.signal, 0xa89868ff);
        assert_ne!(MOSS_FOUNDRY.canvas, INK_HARBOR.canvas);
        assert_ne!(VOLT_WORKSHOP.signal, PLUM_ARCHIVE.signal);
        assert_ne!(SALT_FLAT.signal, SAFFRON_LOOM.signal);
        assert_ne!(MOSS_FOUNDRY.signal, PIDECK_DARK.signal);
    }

    #[test]
    fn unique_light_themes_keep_distinct_accent_families() {
        assert_eq!(PARCHMENT_DESK.signal, 0xb84a28ff);
        assert_eq!(MIST_ORCHARD.signal, 0x1e7a3eff);
        assert_eq!(CORAL_LEDGER.signal, 0xc83a3aff);
        assert_eq!(CHALK_BLUEPRINT.signal, 0x1860c0ff);
        assert_eq!(HONEY_COMB.signal, 0xc07a10ff);
        assert_eq!(PORCELAIN_LAB.signal, 0xc02080ff);
        assert_eq!(CITRUS_GROVE.signal, 0x3a8c18ff);
        assert_eq!(LETTERPRESS.signal, 0xc01828ff);
        assert_eq!(LINEN_GALLERY.signal, 0x8a7a68ff);
        assert_eq!(RICE_PAPER.signal, 0x3a4a70ff);
        assert_eq!(BONE_CHINA.signal, 0xa08060ff);
    }

    #[test]
    fn signal_accents_are_unique_across_the_catalog() {
        let mut signals = Vec::new();
        for theme in ThemeId::ALL {
            let signal = theme.palette().signal;
            assert!(
                !signals.contains(&signal),
                "duplicate signal color for {theme:?}"
            );
            signals.push(signal);
        }
    }
}
