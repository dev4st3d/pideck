//! Shared logical-pixel geometry for the native terminal workbench chrome.

// DirectWrite appends the STAT optical-size name to these Google Fonts faces.
// The SFNT name table alone says "DM Sans 9pt"; using it on Windows silently
// shapes Segoe UI. This name is verified against the native font collection.
#[cfg(windows)]
pub(crate) const CHROME_FONT: &str = "DM Sans 9pt 14pt";
#[cfg(not(windows))]
pub(crate) const CHROME_FONT: &str = "DM Sans 9pt";
pub(crate) const TREE_ROW_HEIGHT: f32 = 28.0;
pub(crate) const TREE_INDENT: f32 = 16.0;
pub(crate) const TREE_INSET: f32 = 6.0;
pub(crate) const CHROME_TEXT_SIZE: f32 = 13.0;
pub(crate) const HEADING_FONT: &str = "Instrument Serif";
pub(crate) const TITLEBAR_HEIGHT: f32 = 40.0;
pub(crate) const TITLEBAR_INSET: f32 = 16.0;
pub(crate) const TITLEBAR_TEXT_SIZE: f32 = 12.0;
pub(crate) const WORDMARK_SIZE: f32 = 24.0;
pub(crate) const COLLAPSED_BRAND_WIDTH: f32 = 128.0;
// Matches the native Windows caption button width; controls run flush to the edge.
pub(crate) const WINDOW_CONTROL_WIDTH: f32 = 46.0;
pub(crate) const DETAIL_TEXT_SIZE: f32 = 11.0;
pub(crate) const DETAIL_LINE_HEIGHT: f32 = 16.0;
pub(crate) const CONTROL_TEXT_SIZE: f32 = 13.0;
pub(crate) const CONTROL_LINE_HEIGHT: f32 = 20.0;
// All chrome shares the user-resizable sidebar boundary.
pub(crate) const SIDEBAR_WIDTH: f32 = 288.0;
pub(crate) const HEADER_HEIGHT: f32 = 42.0;
pub(crate) const SIDEBAR_NAV_HEIGHT: f32 = 42.0;
pub(crate) const TOOLBAR_HEIGHT: f32 = 52.0;
pub(crate) const TOOLBAR_INSET: f32 = 20.0;
pub(crate) const SIDEBAR_INSET: f32 = 16.0;
pub(crate) const CONTENT_INSET: f32 = 28.0;
pub(crate) const FOOTER_HEIGHT: f32 = 28.0;
pub(crate) const CONTROL_HEIGHT: f32 = 32.0;
pub(crate) const MAIN_CONTROL_HEIGHT: f32 = 32.0;
pub(crate) const ROW_HEIGHT: f32 = 64.0;
pub(crate) const ICON_SIZE: f32 = 16.0;
pub(crate) const INSET: f32 = 12.0;
pub(crate) const GAP: f32 = 12.0;
pub(crate) const SMALL_GAP: f32 = 6.0;
pub(crate) const CONTROL_INSET: f32 = 12.0;
pub(crate) const CONTROL_RADIUS: f32 = 4.0;
pub(crate) const COMPACT_GAP: f32 = 8.0;
pub(crate) const APPEARANCE_CONTROL_WIDTH: f32 = 124.0;
pub(crate) const MENU_WIDTH: f32 = 220.0;
pub(crate) const MENU_ROW_HEIGHT: f32 = 34.0;
pub(crate) const MENU_MAX_HEIGHT: f32 = 360.0;
pub(crate) const MENU_GAP: f32 = 6.0;
pub(crate) const TAB_INSET: f32 = 16.0;
pub(crate) const TAB_LIST_BUTTON: f32 = 28.0;
pub(crate) const TAB_LIST_MENU_WIDTH: f32 = 280.0;
// Terminal pane grid: pane title bars, draggable dividers, and layout glyphs.
pub(crate) const PANE_HEADER_HEIGHT: f32 = 30.0;
pub(crate) const PANE_CLOSE_SIZE: f32 = 22.0;
pub(crate) const DIVIDER_HIT: f32 = 6.0;
pub(crate) const MIN_PANE_WIDTH: f32 = 200.0;
pub(crate) const MIN_PANE_HEIGHT: f32 = 120.0;
pub(crate) const LAYOUT_GLYPH_WIDTH: f32 = 18.0;
pub(crate) const LAYOUT_GLYPH_HEIGHT: f32 = 14.0;
// Floating select menus: inner padding and option rows.
pub(crate) const MENU_INSET: f32 = 4.0;
pub(crate) const OPTION_ROW_HEIGHT: f32 = 30.0;
pub(crate) const ROW_DETAIL_GAP: f32 = 2.0;
pub(crate) const CHROME_LINE_HEIGHT: f32 = 20.0;
pub(crate) const TECH_LINE_HEIGHT: f32 = 23.0;
pub(crate) const WIDE_TOOLBAR_MIN_WIDTH: f32 = 1100.0;
pub(crate) const TOOLTIP_MAX_WIDTH: f32 = 480.0;
// Added to GPUI's fixed 500 ms delay for a total of two seconds.
pub(crate) const TOOLTIP_EXTRA_DELAY: std::time::Duration = std::time::Duration::from_millis(1500);

// Modal settings dialog.
pub(crate) const DIALOG_WIDTH: f32 = 720.0;
pub(crate) const DIALOG_HEIGHT: f32 = 560.0;
pub(crate) const DIALOG_MARGIN: f32 = 48.0;
pub(crate) const FONT_LIST_WIDTH: f32 = 252.0;
pub(crate) const FONT_ROW_HEIGHT: f32 = 32.0;
pub(crate) const SHORTCUT_ROW_HEIGHT: f32 = 36.0;
pub(crate) const STEPPER_BUTTON: f32 = 28.0;
pub(crate) const SCROLLBAR_GUTTER: f32 = 10.0;

pub(crate) const SIDEBAR_MIN: f32 = 256.0;
pub(crate) const SIDEBAR_MAX: f32 = 420.0;

// Headerless reminder inspector from Pencil variation 05.
pub(crate) const CHECKLIST_WIDTH: f32 = 352.0;
pub(crate) const CHECKLIST_ROW_HEIGHT: f32 = 32.0;
pub(crate) const CHECKLIST_HEADING_HEIGHT: f32 = 40.0;
pub(crate) const CHECKLIST_HEADING_SIZE: f32 = 23.0;
pub(crate) const CHECKLIST_INDENT: f32 = 18.0;
pub(crate) const CHECKLIST_DISCLOSURE_WIDTH: f32 = 20.0;
pub(crate) const CHECKLIST_CONTROL_SIZE: f32 = 24.0;
pub(crate) const CHECKLIST_GAP: f32 = 6.0;
pub(crate) const CHECKLIST_TEXT_INSET: f32 =
    10.0 + CHECKLIST_DISCLOSURE_WIDTH + CHECKLIST_GAP + 14.0 + CHECKLIST_GAP;
