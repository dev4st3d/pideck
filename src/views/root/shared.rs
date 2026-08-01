use super::*;

pub(super) fn popup_sheet() -> gpui::Div {
    // Fully opaque fill so conversation chrome cannot show through the overlay.
    // Soft corner and a tight directional shadow keep every prompt popover in
    // family with the card it floats above.
    div()
        .w_full()
        .flex()
        .flex_col()
        .rounded(px(theme::RADIUS_LG))
        .border_1()
        .border_color(theme::edge())
        .bg(theme::panel())
        .shadow(vec![gpui::BoxShadow {
            color: gpui::rgba(0x0000_0047).into(),
            offset: point(px(0.0), px(10.0)),
            blur_radius: px(24.0),
            spread_radius: px(-10.0),
        }])
        .overflow_hidden()
}

pub(super) fn short_path(path: &str) -> String {
    let mut parts = path.rsplit(['\\', '/']).filter(|part| !part.is_empty());
    let Some(name) = parts.next() else {
        return path.to_owned();
    };
    let Some(parent) = parts.next() else {
        return path.to_owned();
    };
    if parts.next().is_none() {
        path.to_owned()
    } else {
        format!("…\\{parent}\\{name}")
    }
}

pub(super) fn plural(count: u64) -> &'static str {
    if count == 1 { "" } else { "s" }
}

pub(super) fn action_id(action: RecoveryAction) -> &'static str {
    match action {
        RecoveryAction::Connect => "runtime-connect",
        RecoveryAction::Retry => "runtime-retry",
        RecoveryAction::Stop => "runtime-stop",
    }
}

pub(super) fn runtime_operation_label(operation: &RuntimeOperation) -> &'static str {
    match operation {
        RuntimeOperation::SetModel { .. } => "Switching model",
        RuntimeOperation::SetThinkingLevel(_) => "Changing thinking",
        RuntimeOperation::SetSteeringMode(_) => "Changing steering mode",
        RuntimeOperation::SetFollowUpMode(_) => "Changing follow-up mode",
        RuntimeOperation::Compact => "Compacting",
        RuntimeOperation::SetAutoCompaction(_) => "Changing auto compaction",
        RuntimeOperation::SetAutoRetry(_) => "Changing auto retry",
        RuntimeOperation::SetSessionName(_) => "Renaming session",
        RuntimeOperation::ExportHtml => "Exporting session",
    }
}
