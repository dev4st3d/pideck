//! Shared motion tokens for the desktop shell.
//!
//! Motion stays deliberately short and state-driven: layout-affecting
//! transitions finish quickly, drawers use the same easing curve, and
//! reduced work happens by changing one animation key rather than scheduling
//! per-frame timers in view code.

use std::time::Duration;

use gpui::{Animation, ease_out_quint};

pub const SIDEBAR_MS: u64 = 220;
pub const DRAWER_MS: u64 = 200;
pub const COMPOSER_MS: u64 = 170;

pub fn settle(milliseconds: u64) -> Animation {
    Animation::new(Duration::from_millis(milliseconds)).with_easing(ease_out_quint())
}
