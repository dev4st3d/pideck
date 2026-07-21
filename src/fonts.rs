//! Embedded display and body fonts used by the desk theme.

use std::borrow::Cow;

use gpui::App;

const SWITZER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Switzer-Regular.ttf");
const SWITZER_MEDIUM: &[u8] = include_bytes!("../assets/fonts/Switzer-Medium.ttf");
const SWITZER_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Switzer-Semibold.ttf");
const SWITZER_BOLD: &[u8] = include_bytes!("../assets/fonts/Switzer-Bold.ttf");
const TANKER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Tanker-Regular.ttf");

pub fn register(cx: &App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(SWITZER_REGULAR),
        Cow::Borrowed(SWITZER_MEDIUM),
        Cow::Borrowed(SWITZER_SEMIBOLD),
        Cow::Borrowed(SWITZER_BOLD),
        Cow::Borrowed(TANKER_REGULAR),
    ];

    if let Err(error) = cx.text_system().add_fonts(fonts) {
        eprintln!("failed to register embedded fonts: {error}");
    }
}
