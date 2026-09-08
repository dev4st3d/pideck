//! Resolve workspace columns from available logical pixels, not physical DPI.
//! User intent is retained while navigation yields space on a narrow window.

pub(crate) const RAIL_WIDTH: f32 = 64.0;
pub(crate) const NAVIGATION_WIDTH: f32 = 240.0;
pub(crate) const HISTORY_WIDTH: f32 = 272.0;
pub(crate) const INSPECTOR_WIDTH: f32 = 320.0;
const MIN_CENTER_WIDTH: f32 = 480.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WorkspaceLayout {
    pub navigation: bool,
    pub history: bool,
    pub inspector: bool,
    pub center_width: f32,
}

impl WorkspaceLayout {
    pub fn resolve(width: f32, navigation: bool, history: bool, inspector: bool) -> Self {
        let width = if width.is_finite() { width.max(0.0) } else { 800.0 };
        let width = (width - RAIL_WIDTH).max(0.0);
        let history = history && width - HISTORY_WIDTH >= MIN_CENTER_WIDTH;
        let inspector = inspector && !history && width - INSPECTOR_WIDTH >= MIN_CENTER_WIDTH;
        let companion = if history { HISTORY_WIDTH } else if inspector { INSPECTOR_WIDTH } else { 0.0 };
        let navigation = navigation && width - companion - NAVIGATION_WIDTH >= MIN_CENTER_WIDTH;
        let center_width = (width - companion - if navigation { NAVIGATION_WIDTH } else { 0.0 }).max(0.0);
        Self { navigation, history, inspector, center_width }
    }
}

/// Geometry, not the index of the final visible turn: a single turn can span
/// several screens. Non-finite layout information must not steal scroll intent.
pub(crate) fn at_transcript_tail(offset: f32, maximum: f32) -> bool {
    offset.is_finite() && maximum.is_finite() && offset >= maximum.max(0.0) - 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partially_visible_large_final_turn_does_not_enable_following() {
        assert!(!at_transcript_tail(100.0, 2400.0));
        assert!(at_transcript_tail(2398.0, 2400.0));
        assert!(at_transcript_tail(0.0, 0.0));
        assert!(!at_transcript_tail(f32::NAN, 2400.0));
    }

    #[test]
    fn wide_workspaces_keep_projects_conversation_and_inspector_visible() {
        let layout = WorkspaceLayout::resolve(1440.0, true, false, true);
        assert!(layout.navigation && layout.inspector);
        assert_eq!(layout.center_width, 816.0);
    }

    #[test]
    fn laptop_scaling_never_squeezes_conversation_between_two_rails() {
        for width in [800.0, 910.0, 1024.0] {
            let layout = WorkspaceLayout::resolve(width, true, false, true);
            assert!(!layout.inspector || !layout.navigation);
            assert!(layout.center_width >= 480.0);
            assert!(WorkspaceLayout::resolve(width, true, false, false).navigation);
        }
    }

    #[test]
    fn history_and_inspector_cannot_compete_for_the_same_width() {
        let layout = WorkspaceLayout::resolve(850.0, true, true, true);
        assert!(layout.history);
        assert!(!layout.inspector);
        assert!(!layout.navigation);
        assert_eq!(layout.center_width, 514.0);
    }
}
