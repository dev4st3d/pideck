use super::*;

pub(super) fn popup_sheet() -> gpui::Div {
    // Fully opaque fill so conversation chrome cannot show through the overlay.
    div()
        .w_full()
        .flex()
        .flex_col()
        .rounded(px(theme::RADIUS_SM))
        .border_1()
        .border_color(theme::panel_hover())
        .bg(theme::panel())
        .overflow_hidden()
}

pub(super) fn popup_sheet_header(
    title: &'static str,
    close_id: &'static str,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    div()
        .h(px(28.0))
        .px(px(8.0))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .bg(theme::panel())
        .border_b_1()
        .border_color(theme::panel_hover())
        .child(
            div()
                .font_family(theme::sans())
                .text_size(theme::text_size(theme::T_TINY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::ash())
                .child(title),
        )
        .child(controls::chrome_action(
            close_id,
            "Close",
            true,
            Box::new(cx.listener(|view, _, window, cx| view.close_model_panel(window, cx))),
        ))
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
