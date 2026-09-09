//! The Windows animation preference is process-local, never telemetry.
use std::sync::atomic::{AtomicBool, Ordering};

static MOTION_ENABLED: AtomicBool = AtomicBool::new(false);

pub(crate) fn motion_enabled() -> bool {
    MOTION_ENABLED.load(Ordering::Relaxed)
}

/// Return whether the preference changed. Failure defaults to reduced motion.
pub(crate) fn refresh_motion_preference() -> bool {
    let enabled = platform_motion_enabled();
    MOTION_ENABLED.swap(enabled, Ordering::Relaxed) != enabled
}

#[cfg(windows)]
fn platform_motion_enabled() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
    };
    let mut enabled: i32 = 0;
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL to the valid local
    // pointer. The pointer is not retained and no system setting is modified.
    unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut enabled as *mut i32).cast(),
            0,
        ) != 0
            && enabled != 0
    }
}

#[cfg(not(windows))]
fn platform_motion_enabled() -> bool {
    false
}
