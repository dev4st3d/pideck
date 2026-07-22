//! Shared visual tokens for the harness desk.

use gpui::{Rgba, rgb, rgba};

pub const SANS: &str = "Switzer";
pub const DISPLAY: &str = "Tanker";
pub const MONO: &str = "Cascadia Mono";

// Layout
pub const SIDE_W: f32 = 236.0;
pub const INSPECT_W: f32 = 304.0;
pub const TITLE_H: f32 = 50.0;
pub const RADIUS: f32 = 4.0;
pub const RADIUS_SM: f32 = 3.0;
pub const PAD_X: f32 = 18.0;
pub const STREAM_PAD_X: f32 = 32.0;
pub const SCROLLBAR: f32 = 8.0;

// Type scale
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

// Warm charcoal surfaces
pub fn canvas() -> Rgba {
    rgb(0x0b0a09)
}

pub fn floor() -> Rgba {
    rgb(0x12100e)
}

pub fn panel() -> Rgba {
    rgb(0x1a1714)
}

pub fn panel_lift() -> Rgba {
    rgb(0x221e1a)
}

pub fn panel_hover() -> Rgba {
    rgb(0x2a241f)
}

pub fn edge() -> Rgba {
    rgba(0xebe4_d618)
}

pub fn edge_hard() -> Rgba {
    rgba(0xebe4_d62c)
}

pub fn edge_soft() -> Rgba {
    rgba(0xebe4_d612)
}

// Text and semantic accents
pub fn bone() -> Rgba {
    rgb(0xefe7d8)
}

pub fn bone_dim() -> Rgba {
    rgb(0xbbb2a2)
}

pub fn ash() -> Rgba {
    rgb(0x9a9082)
}

pub fn smoke() -> Rgba {
    rgb(0x847b70)
}

pub fn signal() -> Rgba {
    rgb(0xc75a38)
}

pub fn signal_deep() -> Rgba {
    rgb(0x8a3f28)
}

pub fn signal_hot() -> Rgba {
    rgb(0xd46a48)
}

pub fn focus() -> Rgba {
    rgb(0xffd39a)
}

pub fn error() -> Rgba {
    rgb(0xe18263)
}

pub fn live() -> Rgba {
    rgb(0xc5d2a8)
}

pub fn data() -> Rgba {
    rgb(0xe0b07a)
}

pub fn data_wash() -> Rgba {
    rgba(0xe0b0_7a16)
}
