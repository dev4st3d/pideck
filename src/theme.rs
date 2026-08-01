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

    /// Stable settings key for local persistence.
    pub const fn key(self) -> &'static str {
        match self {
            Self::PiDeckDark => "pideck-dark",
            Self::CursorDark => "cursor-dark",
            Self::MossFoundry => "moss-foundry",
            Self::InkHarbor => "ink-harbor",
            Self::VoltWorkshop => "volt-workshop",
            Self::PlumArchive => "plum-archive",
            Self::SaltFlat => "salt-flat",
            Self::SaffronLoom => "saffron-loom",
            Self::JuniperCoil => "juniper-coil",
            Self::SmokeLibrary => "smoke-library",
            Self::PewterHall => "pewter-hall",
            Self::OliveStudy => "olive-study",
            Self::ParchmentDesk => "parchment-desk",
            Self::MistOrchard => "mist-orchard",
            Self::CoralLedger => "coral-ledger",
            Self::ChalkBlueprint => "chalk-blueprint",
            Self::HoneyComb => "honey-comb",
            Self::PorcelainLab => "porcelain-lab",
            Self::CitrusGrove => "citrus-grove",
            Self::Letterpress => "letterpress",
            Self::LinenGallery => "linen-gallery",
            Self::RicePaper => "rice-paper",
            Self::BoneChina => "bone-china",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|theme| theme.key() == key)
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
pub const HISTORY_W: f32 = 280.0;
pub const INSPECT_W: f32 = 304.0;
pub const TITLE_H: f32 = 50.0;
pub const RADIUS: f32 = 4.0;
pub const RADIUS_SM: f32 = 3.0;
/// Softer step for controls nested inside the prompt card tray.
pub const RADIUS_MD: f32 = 6.0;
/// The prompt card itself: the hero surface of the desk, so it earns the
/// softest corner in the system.
pub const RADIUS_LG: f32 = 8.0;
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
    user_message: 0x2c241eff,
    user_message_edge: 0xc75a3866,
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
    user_message: 0x242c34ff, // cool lift — clear step above agent canvas
    user_message_edge: 0x5a718866,
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

// Green-black foundry floor. Moss-grey paper type, a dull moss signal and
// old-brass data: mineral and workshop-quiet, no mint neon left.
const MOSS_FOUNDRY: Palette = Palette {
    canvas: 0x090b09ff,
    floor: 0x0f120fff,
    panel: 0x171b17ff,
    panel_lift: 0x1e241eff,
    panel_hover: 0x262e26ff,
    user_message: 0x2b3329ff,
    user_message_edge: 0x8ba07a66,
    edge: 0xe2e8d81e,
    edge_hard: 0xe2e8d830,
    edge_soft: 0xe2e8d814,
    bone: 0xecf0e4ff,
    bone_dim: 0xbcc4b2ff,
    ash: 0x97a18fff,
    smoke: 0x707a68ff,
    signal: 0x8ba07aff,
    signal_deep: 0x6a7e58ff,
    signal_hot: 0x9db28cff,
    focus: 0xc6d4b2ff,
    error: 0xd08a6aff,
    error_wash: 0xd08a6a14,
    live: 0x8cb294ff,
    live_wash: 0x8cb29414,
    working: 0x7e9aa8ff,
    data: 0xb59a6cff,
    data_wash: 0xb59a6c16,
};

// Harbor night in real navy ink. Fog-grey type, sea-glass signal, flag-rose
// error: maritime dusk instead of a glowing marina.
const INK_HARBOR: Palette = Palette {
    canvas: 0x07090cff,
    floor: 0x0c1015ff,
    panel: 0x141a21ff,
    panel_lift: 0x1d242dff,
    panel_hover: 0x262e39ff,
    user_message: 0x29323cff,
    user_message_edge: 0x78a49e66,
    edge: 0xdce4ec1e,
    edge_hard: 0xdce4ec30,
    edge_soft: 0xdce4ec14,
    bone: 0xe8eef2ff,
    bone_dim: 0xb6c2ccff,
    ash: 0x92a2aeff,
    smoke: 0x6c7c88ff,
    signal: 0x78a49eff,
    signal_deep: 0x5d857fff,
    signal_hot: 0x8ab5afff,
    focus: 0xb8d2ccff,
    error: 0xc98a90ff,
    error_wash: 0xc98a9014,
    live: 0x86b39bff,
    live_wash: 0x86b39b14,
    working: 0x7a96aeff,
    data: 0xbfa273ff,
    data_wash: 0xbfa27316,
};

// Graphite garage under a sodium lamp. Warm grey type and one dim amber
// signal; industrial night kept tonal instead of electric.
const VOLT_WORKSHOP: Palette = Palette {
    canvas: 0x0a0908ff,
    floor: 0x100f0eff,
    panel: 0x181716ff,
    panel_lift: 0x21201eff,
    panel_hover: 0x2a2927ff,
    user_message: 0x2e2c29ff,
    user_message_edge: 0xc4a06a66,
    edge: 0xe8e6de1e,
    edge_hard: 0xe8e6de30,
    edge_soft: 0xe8e6de14,
    bone: 0xefeee9ff,
    bone_dim: 0xbebdb5ff,
    ash: 0x9a998fff,
    smoke: 0x74736bff,
    signal: 0xc4a06aff,
    signal_deep: 0xa07e50ff,
    signal_hot: 0xd2b180ff,
    focus: 0xe0cba4ff,
    error: 0xd47a6aff,
    error_wash: 0xd47a6a14,
    live: 0x8cae7fff,
    live_wash: 0x8cae7f14,
    working: 0x7c93a4ff,
    data: 0xa89268ff,
    data_wash: 0xa8926816,
};

// Aubergine-black archive. Mauve-grey paper, a dusty mauve signal and
// periwinkle working files: archive-box dusk, no rose-gold glare.
const PLUM_ARCHIVE: Palette = Palette {
    canvas: 0x0b090cff,
    floor: 0x110d13ff,
    panel: 0x1a151dff,
    panel_lift: 0x231e28ff,
    panel_hover: 0x2d2733ff,
    user_message: 0x322a38ff,
    user_message_edge: 0xa98eb066,
    edge: 0xe4dae61e,
    edge_hard: 0xe4dae630,
    edge_soft: 0xe4dae614,
    bone: 0xefeaf0ff,
    bone_dim: 0xc4bac5ff,
    ash: 0x9e93a1ff,
    smoke: 0x776b7dff,
    signal: 0xa98eb0ff,
    signal_deep: 0x896e91ff,
    signal_hot: 0xba9fc0ff,
    focus: 0xd3bed6ff,
    error: 0xcd889aff,
    error_wash: 0xcd889a14,
    live: 0x8aad97ff,
    live_wash: 0x8aad9714,
    working: 0x8d9bb4ff,
    data: 0xb89a75ff,
    data_wash: 0xb89a7516,
};

// Vellum paper and walnut ink with an iron-oxide signal: a drafting desk in
// warm daylight — deeper and orangier than Bone China, paper rather than cream.
const PARCHMENT_DESK: Palette = Palette {
    canvas: 0xf7f0e3ff,
    floor: 0xefe5d3ff,
    panel: 0xe6dcc8ff,
    panel_lift: 0xdcd0b8ff,
    panel_hover: 0xd0c2a6ff,
    user_message: 0xe9decbff,
    user_message_edge: 0x9e4626aa,
    edge: 0x3a2a1e30,
    edge_hard: 0x3a2a1e4c,
    edge_soft: 0x3a2a1e1e,
    bone: 0x211a12ff,
    bone_dim: 0x443a2cff,
    ash: 0x61544aff,
    smoke: 0x7d7163ff,
    signal: 0x9e4626ff,
    signal_deep: 0x7c3519ff,
    signal_hot: 0xb4552fff,
    focus: 0x7f5410ff,
    error: 0xa42c1cff,
    error_wash: 0xa42c1c18,
    live: 0x3d6e2eff,
    live_wash: 0x3d6e2e18,
    working: 0x2f6183ff,
    data: 0x8a5a10ff,
    data_wash: 0x8a5a1018,
};

// Grey-green morning fog with fir ink and a viridian signal: a misted
// glasshouse — the green lives in the paper and the ink, not in a neon leaf.
const MIST_ORCHARD: Palette = Palette {
    canvas: 0xf1f5f0ff,
    floor: 0xe7ede5ff,
    panel: 0xdce5dbff,
    panel_lift: 0xcfd9ccff,
    panel_hover: 0xbfcbbcff,
    user_message: 0xdfe8deff,
    user_message_edge: 0x2d6b47aa,
    edge: 0x18251d30,
    edge_hard: 0x18251d4c,
    edge_soft: 0x18251d1e,
    bone: 0x141d17ff,
    bone_dim: 0x36422fff,
    ash: 0x546351ff,
    smoke: 0x70806aff,
    signal: 0x2d6b47ff,
    signal_deep: 0x1d5233ff,
    signal_hot: 0x3a8359ff,
    focus: 0x20705fff,
    error: 0xa83228ff,
    error_wash: 0xa8322818,
    live: 0x2e7a44ff,
    live_wash: 0x2e7a4418,
    working: 0x2f6a8fff,
    data: 0x8c6410ff,
    data_wash: 0x8c641018,
};

// Shell-blush paper with espresso-rose ink and a persimmon signal: terracotta
// and warm clay — coral with the orange still in it, not a second Letterpress red.
const CORAL_LEDGER: Palette = Palette {
    canvas: 0xfbf2eeff,
    floor: 0xf3e4ddff,
    panel: 0xe9d6cdff,
    panel_lift: 0xddc6baff,
    panel_hover: 0xd0b5a6ff,
    user_message: 0xefded6ff,
    user_message_edge: 0xbc4b34aa,
    edge: 0x33201a30,
    edge_hard: 0x33201a4c,
    edge_soft: 0x33201a1e,
    bone: 0x241512ff,
    bone_dim: 0x4a2f28ff,
    ash: 0x6a4e46ff,
    smoke: 0x85695fff,
    signal: 0xbc4b34ff,
    signal_deep: 0x963922ff,
    signal_hot: 0xd15a40ff,
    focus: 0x8f4a2eff,
    error: 0xa02020ff,
    error_wash: 0xa0202018,
    live: 0x2e7a45ff,
    live_wash: 0x2e7a4518,
    working: 0x38648eff,
    data: 0x93621aff,
    data_wash: 0x93621a18,
};

// Chalk-blue paper with navy ink and a true cobalt signal; petrol for the
// working set — drafting-table blue, not hyperlink blue.
const CHALK_BLUEPRINT: Palette = Palette {
    canvas: 0xf6f8fbff,
    floor: 0xebf0f6ff,
    panel: 0xe1e8f1ff,
    panel_lift: 0xd3dce9ff,
    panel_hover: 0xc3cedeff,
    user_message: 0xe3eaf3ff,
    user_message_edge: 0x2e5da0aa,
    edge: 0x14202e30,
    edge_hard: 0x14202e4c,
    edge_soft: 0x14202e1e,
    bone: 0x14202eff,
    bone_dim: 0x2f3f52ff,
    ash: 0x506379ff,
    smoke: 0x708397ff,
    signal: 0x2e5da0ff,
    signal_deep: 0x1f4783ff,
    signal_hot: 0x3c71bcff,
    focus: 0x146c84ff,
    error: 0xa82434ff,
    error_wash: 0xa8243418,
    live: 0x1f7a4aff,
    live_wash: 0x1f7a4a18,
    working: 0x14708cff,
    data: 0x986612ff,
    data_wash: 0x98661218,
};

// Golden wax paper with dark-honey ink and a bronze-amber signal: the yellow
// seat in the warm row — clearly not Parchment's orange, clearly not ivory.
const HONEY_COMB: Palette = Palette {
    canvas: 0xfaf3dfff,
    floor: 0xf2e7c9ff,
    panel: 0xe9dcbaff,
    panel_lift: 0xdccfa4ff,
    panel_hover: 0xcfc094ff,
    user_message: 0xeee2c2ff,
    user_message_edge: 0x9a6c0eaa,
    edge: 0x33270f30,
    edge_hard: 0x33270f4c,
    edge_soft: 0x33270f1e,
    bone: 0x20190bff,
    bone_dim: 0x43381dff,
    ash: 0x61542fff,
    smoke: 0x7d7048ff,
    signal: 0x9a6c0eff,
    signal_deep: 0x785307ff,
    signal_hot: 0xb28113ff,
    focus: 0x7c5a08ff,
    error: 0xa82c14ff,
    error_wash: 0xa82c1418,
    live: 0x3f7422ff,
    live_wash: 0x3f742218,
    working: 0x356489ff,
    data: 0x8a5c08ff,
    data_wash: 0x8a5c0818,
};

// Dry salt pan at night. Near-monochrome cold graphite, salt-white type and
// one pale glacier-cyan signal: sterile and high-altitude without glowing.
const SALT_FLAT: Palette = Palette {
    canvas: 0x090a0bff,
    floor: 0x0e1011ff,
    panel: 0x16181aff,
    panel_lift: 0x1f2224ff,
    panel_hover: 0x282b2eff,
    user_message: 0x2c3033ff,
    user_message_edge: 0x8fbcc266,
    edge: 0xe4e8ea22,
    edge_hard: 0xe4e8ea34,
    edge_soft: 0xe4e8ea16,
    bone: 0xecf1f3ff,
    bone_dim: 0xbfc8ccff,
    ash: 0x9aa4aaff,
    smoke: 0x6e787eff,
    signal: 0x8fbcc2ff,
    signal_deep: 0x70979dff,
    signal_hot: 0xa2ccd1ff,
    focus: 0xc2dde0ff,
    error: 0xcc8c82ff,
    error_wash: 0xcc8c8214,
    live: 0x92baa2ff,
    live_wash: 0x92baa214,
    working: 0x82a0b6ff,
    data: 0xb9a67aff,
    data_wash: 0xb9a67a16,
};

// Night-market loom: espresso warp, ivory thread, deep saffron shuttles.
// Spice-desk warmth with the market-stall neon pulled all the way down.
const SAFFRON_LOOM: Palette = Palette {
    canvas: 0x0b0906ff,
    floor: 0x12100bff,
    panel: 0x1b1710ff,
    panel_lift: 0x252018ff,
    panel_hover: 0x302a20ff,
    user_message: 0x362e22ff,
    user_message_edge: 0xc08a4a66,
    edge: 0xefe0c81e,
    edge_hard: 0xefe0c830,
    edge_soft: 0xefe0c814,
    bone: 0xf3ecddff,
    bone_dim: 0xcabea6ff,
    ash: 0xa39880ff,
    smoke: 0x796e58ff,
    signal: 0xc08a4aff,
    signal_deep: 0x996c38ff,
    signal_hot: 0xd09c5eff,
    focus: 0xe2c493ff,
    error: 0xce7d60ff,
    error_wash: 0xce7d6014,
    live: 0x9aaa71ff,
    live_wash: 0x9aaa7114,
    working: 0x8398aeff,
    data: 0xc9ac72ff,
    data_wash: 0xc9ac7216,
};

// Pine-black botanical cabinet. Cool silver type, juniper-berry violet kept
// dusty, fern for the living: medicinal and dry, not a lilac SaaS night.
const JUNIPER_COIL: Palette = Palette {
    canvas: 0x080b0cff,
    floor: 0x0d1113ff,
    panel: 0x151a1cff,
    panel_lift: 0x1e2426ff,
    panel_hover: 0x272e30ff,
    user_message: 0x2b3337ff,
    user_message_edge: 0x9d8fb866,
    edge: 0xdde2ec1e,
    edge_hard: 0xdde2ec30,
    edge_soft: 0xdde2ec14,
    bone: 0xe9ecf1ff,
    bone_dim: 0xbcc1ceff,
    ash: 0x979babff,
    smoke: 0x6f7486ff,
    signal: 0x9d8fb8ff,
    signal_deep: 0x7f7299ff,
    signal_hot: 0xb0a2c8ff,
    focus: 0xcfc2dcff,
    error: 0xc97e8eff,
    error_wash: 0xc97e8e14,
    live: 0x84ac92ff,
    live_wash: 0x84ac9214,
    working: 0x7d9bb0ff,
    data: 0xb59e74ff,
    data_wash: 0xb59e7416,
};

// Clinical porcelain morning — hard white, neutral charcoal ink, and one
// deep orchid-madder spark on the bench. Lab-clean, not pastel nursery.
const PORCELAIN_LAB: Palette = Palette {
    canvas: 0xf9fafcff,
    floor: 0xeff1f5ff,
    panel: 0xe5e8eeff,
    panel_lift: 0xd8dce4ff,
    panel_hover: 0xc8cdd8ff,
    user_message: 0xe8ebf1ff,
    user_message_edge: 0x9a2c70aa,
    edge: 0x14171d32,
    edge_hard: 0x14171d50,
    edge_soft: 0x14171d20,
    bone: 0x161a20ff,
    bone_dim: 0x333945ff,
    ash: 0x525b69ff,
    smoke: 0x707886ff,
    signal: 0x9a2c70ff,
    signal_deep: 0x781d56ff,
    signal_hot: 0xb23a84ff,
    focus: 0x20558fff,
    error: 0xaa1f2eff,
    error_wash: 0xaa1f2e18,
    live: 0x1d7747ff,
    live_wash: 0x1d774718,
    working: 0x1f5fa8ff,
    data: 0x97530fff,
    data_wash: 0x97530f18,
};

// Midday citrus grove — lemon paper, olive-black ink, and a shaded grove-leaf
// signal with the lime neon pulled out. Mediterranean orchard, not candy.
const CITRUS_GROVE: Palette = Palette {
    canvas: 0xfbf7e8ff,
    floor: 0xf3eddaff,
    panel: 0xeae2c9ff,
    panel_lift: 0xded5b6ff,
    panel_hover: 0xd2c7a3ff,
    user_message: 0xefe8cfff,
    user_message_edge: 0x4a731caa,
    edge: 0x252a1230,
    edge_hard: 0x252a124c,
    edge_soft: 0x252a121e,
    bone: 0x1c200eff,
    bone_dim: 0x3d4423ff,
    ash: 0x5a6342ff,
    smoke: 0x767e5cff,
    signal: 0x4a731cff,
    signal_deep: 0x355a10ff,
    signal_hot: 0x5c8c27ff,
    focus: 0x166f72ff,
    error: 0xa32a17ff,
    error_wash: 0xa32a1718,
    live: 0x387a28ff,
    live_wash: 0x387a2818,
    working: 0x2f678fff,
    data: 0x9c6f0aff,
    data_wash: 0x9c6f0a18,
};

// Newsprint grey with blue-black printer's ink and an oxblood crimson signal:
// press-room red kept in the ink family, not a fire alarm. Letterpress shop.
const LETTERPRESS: Palette = Palette {
    canvas: 0xf8f8f3ff,
    floor: 0xf0f0eaff,
    panel: 0xe5e5dcff,
    panel_lift: 0xd8d8ccff,
    panel_hover: 0xc9c9baff,
    user_message: 0xe9e9dfff,
    user_message_edge: 0xb02333aa,
    edge: 0x10121634,
    edge_hard: 0x10121652,
    edge_soft: 0x1012161e,
    bone: 0x12151bff,
    bone_dim: 0x2e3238ff,
    ash: 0x4e535aff,
    smoke: 0x6d7278ff,
    signal: 0xb02333ff,
    signal_deep: 0x8a1626ff,
    signal_hot: 0xc72e40ff,
    focus: 0x1c4788ff,
    error: 0x9e1420ff,
    error_wash: 0x9e142018,
    live: 0x1f7442ff,
    live_wash: 0x1f744218,
    working: 0x24529bff,
    data: 0x8f5f10ff,
    data_wash: 0x8f5f1018,
};

// Reading room after hours. Warm smoked charcoal, paper-cream type and a
// rosewood signal low like a banked lamp: low chroma, no fireplace drama.
const SMOKE_LIBRARY: Palette = Palette {
    canvas: 0x0d0b09ff,
    floor: 0x14110dff,
    panel: 0x1c1915ff,
    panel_lift: 0x25221dff,
    panel_hover: 0x2f2b25ff,
    user_message: 0x332e27ff,
    user_message_edge: 0xb8887066,
    edge: 0xe5dccb1e,
    edge_hard: 0xe5dccb30,
    edge_soft: 0xe5dccb14,
    bone: 0xf2ebe2ff,
    bone_dim: 0xc4b8acff,
    ash: 0x968a7eff,
    smoke: 0x736a60ff,
    signal: 0xb88870ff,
    signal_deep: 0x8a6650ff,
    signal_hot: 0xca9e88ff,
    focus: 0xd3b697ff,
    error: 0xcf8173ff,
    error_wash: 0xcf817316,
    live: 0x93b296ff,
    live_wash: 0x93b29616,
    working: 0x8799adff,
    data: 0xc2a483ff,
    data_wash: 0xc2a48316,
};

// Architectural evening hall. Cool pewter greys, soft silver type and a
// restrained slate signal: metal and stone, kept quiet.
const PEWTER_HALL: Palette = Palette {
    canvas: 0x0a0b0dff,
    floor: 0x111316ff,
    panel: 0x191b1eff,
    panel_lift: 0x222528ff,
    panel_hover: 0x2b2e33ff,
    user_message: 0x30343aff,
    user_message_edge: 0x7a8ea066,
    edge: 0xdce2e81e,
    edge_hard: 0xdce2e830,
    edge_soft: 0xdce2e814,
    bone: 0xeef1f4ff,
    bone_dim: 0xb8c0c8ff,
    ash: 0x8a949eff,
    smoke: 0x687078ff,
    signal: 0x7a8ea0ff,
    signal_deep: 0x5d7080ff,
    signal_hot: 0x93a6b8ff,
    focus: 0xa8bccaff,
    error: 0xc98a84ff,
    error_wash: 0xc98a8416,
    live: 0x8ab0a2ff,
    live_wash: 0x8ab0a216,
    working: 0x8299b5ff,
    data: 0xb5a686ff,
    data_wash: 0xb5a68616,
};

// Olive scholar's study. Deep olive-brown black, cream type and an
// antique-brass signal: dry botanical calm, no jungle green.
const OLIVE_STUDY: Palette = Palette {
    canvas: 0x0a0b08ff,
    floor: 0x111310ff,
    panel: 0x191b15ff,
    panel_lift: 0x22241eff,
    panel_hover: 0x2b2e26ff,
    user_message: 0x31332aff,
    user_message_edge: 0xa99c6e66,
    edge: 0xdfe3d01e,
    edge_hard: 0xdfe3d030,
    edge_soft: 0xdfe3d014,
    bone: 0xf0eee4ff,
    bone_dim: 0xc0c0b0ff,
    ash: 0x909484ff,
    smoke: 0x6e7264ff,
    signal: 0xa99c6eff,
    signal_deep: 0x857c55ff,
    signal_hot: 0xbcb088ff,
    focus: 0xccc2a0ff,
    error: 0xcb8777ff,
    error_wash: 0xcb877716,
    live: 0x8fb18aff,
    live_wash: 0x8fb18a16,
    working: 0x8a99abff,
    data: 0xb9a172ff,
    data_wash: 0xb9a17216,
};

// Soft gallery morning — grey-warm linen, charcoal-brown ink, raw-umber
// signal kept deliberately quiet. Museum wall calm; the tonal one of the set.
const LINEN_GALLERY: Palette = Palette {
    canvas: 0xf7f5f1ff,
    floor: 0xefece7ff,
    panel: 0xe6e1daff,
    panel_lift: 0xd9d3c9ff,
    panel_hover: 0xcbc3b6ff,
    user_message: 0xe8e3daff,
    user_message_edge: 0x7a6850aa,
    edge: 0x28242030,
    edge_hard: 0x28242048,
    edge_soft: 0x2824201c,
    bone: 0x1e1b18ff,
    bone_dim: 0x3f3a34ff,
    ash: 0x5d5650ff,
    smoke: 0x79726aff,
    signal: 0x7a6850ff,
    signal_deep: 0x5e4f3bff,
    signal_hot: 0x927e63ff,
    focus: 0x4f6172ff,
    error: 0x9e3a30ff,
    error_wash: 0x9e3a3016,
    live: 0x487456ff,
    live_wash: 0x48745616,
    working: 0x4f6583ff,
    data: 0x866c3cff,
    data_wash: 0x866c3c16,
};

// Calligraphy desk — cool rice paper, soft sumi ink, dyed indigo (ai-iro).
// Grey-blue quiet with one dip of the brush; not anime, not neon.
const RICE_PAPER: Palette = Palette {
    canvas: 0xf7f8f6ff,
    floor: 0xeff0edff,
    panel: 0xe4e6e1ff,
    panel_lift: 0xd8dad3ff,
    panel_hover: 0xcacdc3ff,
    user_message: 0xe7e8e3ff,
    user_message_edge: 0x37486eaa,
    edge: 0x1a1c2230,
    edge_hard: 0x1a1c224c,
    edge_soft: 0x1a1c221e,
    bone: 0x17191fff,
    bone_dim: 0x34373fff,
    ash: 0x535761ff,
    smoke: 0x71757eff,
    signal: 0x37486eff,
    signal_deep: 0x283654ff,
    signal_hot: 0x475b88ff,
    focus: 0x43537aff,
    error: 0x9c3038ff,
    error_wash: 0x9c303816,
    live: 0x356e4cff,
    live_wash: 0x356e4c16,
    working: 0x3c6894ff,
    data: 0x85613aff,
    data_wash: 0x85613a16,
};

// Afternoon tea service — polished bone-china ivory, warm slate ink, gilt
// brass signal. The palest warm white in the set; china cabinet, not carnival.
const BONE_CHINA: Palette = Palette {
    canvas: 0xfaf7f1ff,
    floor: 0xf3eee6ff,
    panel: 0xe9e3d8ff,
    panel_lift: 0xddd5c6ff,
    panel_hover: 0xd1c7b4ff,
    user_message: 0xeae4d7ff,
    user_message_edge: 0x8a6c34aa,
    edge: 0x27221b30,
    edge_hard: 0x27221b4a,
    edge_soft: 0x27221b1e,
    bone: 0x201b15ff,
    bone_dim: 0x403a31ff,
    ash: 0x5f584dff,
    smoke: 0x7b7366ff,
    signal: 0x8a6c34ff,
    signal_deep: 0x6b5224ff,
    signal_hot: 0xa1803fff,
    focus: 0x6e5a34ff,
    error: 0xa03c32ff,
    error_wash: 0xa03c3216,
    live: 0x4d7350ff,
    live_wash: 0x4d735016,
    working: 0x506a8cff,
    data: 0x84683aff,
    data_wash: 0x84683a16,
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
        assert_eq!(PIDECK_DARK.user_message, 0x2c241eff);
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
        assert_eq!(CURSOR_DARK.user_message, 0x242c34ff);
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
    fn user_message_separates_from_floor_and_canvas() {
        // User prompts must read as a distinct band from the turn floor and the
        // recessed agent reply (canvas): lighter on dark themes, darker on light.
        for theme in ThemeId::ALL {
            let palette = theme.palette();
            let user_l = relative_luminance(palette.user_message);
            let floor_l = relative_luminance(palette.floor);
            let canvas_l = relative_luminance(palette.canvas);
            match theme.mode() {
                ThemeMode::Dark => {
                    assert!(
                        user_l > floor_l,
                        "{theme:?} dark user_message is not lighter than floor"
                    );
                    assert!(
                        user_l > canvas_l,
                        "{theme:?} dark user_message is not lighter than canvas"
                    );
                    assert!(
                        (user_l - floor_l) >= 0.008,
                        "{theme:?} dark user/floor contrast {user_l:.4}/{floor_l:.4} is too subtle"
                    );
                }
                ThemeMode::Light => {
                    assert!(
                        user_l < floor_l,
                        "{theme:?} light user_message is not darker than floor"
                    );
                    assert!(
                        user_l < canvas_l,
                        "{theme:?} light user_message is not darker than canvas"
                    );
                    assert!(
                        (floor_l - user_l) >= 0.02,
                        "{theme:?} light user/floor contrast {user_l:.4}/{floor_l:.4} is too subtle"
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
        assert_eq!(MOSS_FOUNDRY.signal, 0x8ba07aff);
        assert_eq!(INK_HARBOR.signal, 0x78a49eff);
        assert_eq!(VOLT_WORKSHOP.signal, 0xc4a06aff);
        assert_eq!(PLUM_ARCHIVE.signal, 0xa98eb0ff);
        assert_eq!(SALT_FLAT.signal, 0x8fbcc2ff);
        assert_eq!(SAFFRON_LOOM.signal, 0xc08a4aff);
        assert_eq!(JUNIPER_COIL.signal, 0x9d8fb8ff);
        assert_eq!(SMOKE_LIBRARY.signal, 0xb88870ff);
        assert_eq!(PEWTER_HALL.signal, 0x7a8ea0ff);
        assert_eq!(OLIVE_STUDY.signal, 0xa99c6eff);
        assert_ne!(MOSS_FOUNDRY.canvas, INK_HARBOR.canvas);
        assert_ne!(VOLT_WORKSHOP.signal, PLUM_ARCHIVE.signal);
        assert_ne!(SALT_FLAT.signal, SAFFRON_LOOM.signal);
        assert_ne!(MOSS_FOUNDRY.signal, PIDECK_DARK.signal);
    }

    #[test]
    fn unique_light_themes_keep_distinct_accent_families() {
        assert_eq!(PARCHMENT_DESK.signal, 0x9e4626ff);
        assert_eq!(MIST_ORCHARD.signal, 0x2d6b47ff);
        assert_eq!(CORAL_LEDGER.signal, 0xbc4b34ff);
        assert_eq!(CHALK_BLUEPRINT.signal, 0x2e5da0ff);
        assert_eq!(HONEY_COMB.signal, 0x9a6c0eff);
        assert_eq!(PORCELAIN_LAB.signal, 0x9a2c70ff);
        assert_eq!(CITRUS_GROVE.signal, 0x4a731cff);
        assert_eq!(LETTERPRESS.signal, 0xb02333ff);
        assert_eq!(LINEN_GALLERY.signal, 0x7a6850ff);
        assert_eq!(RICE_PAPER.signal, 0x37486eff);
        assert_eq!(BONE_CHINA.signal, 0x8a6c34ff);
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
