//! Stable prompt surface with explicit delivery, recovery and model controls.

use super::model_panels::{model_switcher_sheet, thinking_select_sheet};
use super::overlays::{
    command_suggestion_sheet, extension_status_bar, extension_widgets, file_suggestion_sheet,
};
use super::*;
use crate::file_completion::FileMatch;
use crate::views::composer::ComposerFeedback;
use gpui::{SharedString, rgba};

/// One vertical rhythm for every control in the prompt tray.
const TRAY_CONTROL_H: f32 = 36.0;

fn clear() -> gpui::Rgba {
    rgba(0x0000_0000)
}

pub(super) struct ComposerBarParams<'a> {
    pub(super) composer: &'a Entity<Composer>,
    pub(super) queue: &'a QueueContents,
    pub(super) queue_clear_pending: bool,
    pub(super) saved_input_count: usize,
    pub(super) draft_feedback: Option<&'a str>,
    pub(super) attachment_picker_pending: bool,
    pub(super) models: &'a ModelRuntimeProjection,
    pub(super) projection: &'a ShellProjection,
    pub(super) panel: Option<ModelPanel>,
    pub(super) provider_filter: Option<&'a str>,
    pub(super) search: &'a Entity<Composer>,
    pub(super) slash_commands: &'a [CommandEntry],
    pub(super) command_selection: usize,
    pub(super) command_scroll: &'a ScrollHandle,
    pub(super) file_matches: &'a [FileMatch],
    pub(super) file_selection: usize,
    pub(super) file_scroll: &'a ScrollHandle,
    pub(super) model_scroll: &'a ScrollHandle,
    pub(super) provider_scroll: &'a ScrollHandle,
    pub(super) thinking_scroll: &'a ScrollHandle,
    pub(super) slash_dismissed: bool,
    pub(super) extension_ui: &'a ExtensionUiProjection,
}

pub(super) fn composer_bar(
    params: ComposerBarParams<'_>,
    _window: &Window,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let ComposerBarParams {
        composer,
        queue,
        queue_clear_pending,
        saved_input_count,
        draft_feedback,
        attachment_picker_pending,
        models,
        projection,
        panel,
        provider_filter,
        search,
        slash_commands,
        command_selection,
        command_scroll,
        file_matches,
        file_selection,
        file_scroll,
        model_scroll,
        provider_scroll,
        thinking_scroll,
        slash_dismissed,
        extension_ui,
    } = params;
    let model_open = matches!(panel, Some(ModelPanel::Switcher));
    let thinking_open = matches!(panel, Some(ModelPanel::Thinking));
    let model_label = short_model_label(projection, models);
    let thinking_label = short_thinking_label(projection, models);
    let catalog_ready = models.catalog.is_some();
    let can_pick_model = catalog_ready || !models.stock_models.is_empty();
    let can_pick_thinking = catalog_ready || models.active_thinking.is_some();
    let can_attach = !attachment_picker_pending && composer.read(cx).can_add_attachments();
    let slash_completion =
        (!slash_dismissed && !slash_commands.is_empty()).then_some(slash_commands);
    // Slash menu wins when both could appear; file menu only when slash is idle.
    let file_completion =
        (slash_completion.is_none() && !file_matches.is_empty()).then_some(file_matches);

    let availability = composer.read(cx).availability();
    let running = availability == ComposerAvailability::Running;
    let bash_running = availability == ComposerAvailability::BashRunning;
    let can_submit = composer.read(cx).can_submit() && !queue_clear_pending;

    // The tray carries one line of meaning at a time: a clamp notice outranks
    // a binding notice, which outranks the composer's own status. While the
    // desk is simply idle the line offers keyboard hints instead of noise.
    let quiet_feedback = matches!(composer.read(cx).feedback(),
        ComposerFeedback::Ready | ComposerFeedback::Accepted(_) | ComposerFeedback::BashCompleted);
    let feedback_color = match composer.read(cx).feedback() {
        ComposerFeedback::Rejected(_) | ComposerFeedback::Uncertain => theme::error(),
        ComposerFeedback::Pending(_)
        | ComposerFeedback::BashRunning { .. }
        | ComposerFeedback::LoadingAttachments => theme::data(),
        ComposerFeedback::Accepted(_) | ComposerFeedback::BashCompleted => theme::live(),
        ComposerFeedback::Ready => theme::ash(),
    };
    // (text, color, mono): clamp notices read as amber prose, binding feedback
    // and keyboard hints as quiet data, composer status as prose.
    let tray_line: Option<(String, gpui::Rgba, bool)> =
        if let Some(notice) = models.clamp_notice.clone() {
            Some((notice, theme::data(), false))
        } else if let Some(binding) = models
            .feedback
            .clone()
            .filter(|_| !model_open && !thinking_open)
        {
            Some((binding, theme::smoke(), true))
        } else if quiet_feedback || (running && !matches!(composer.read(cx).feedback(),
            ComposerFeedback::Rejected(_) | ComposerFeedback::Uncertain | ComposerFeedback::LoadingAttachments)) {
            None
        } else {
            let status = composer.read(cx).status_text();
            (!status.is_empty()).then_some((status, feedback_color, false))
        };

    let submit_composer = composer.clone();
    let abort_composer = composer.clone();
    let follow_composer = composer.clone();

    div()
        .flex()
        .flex_col()
        .items_center()
        .flex_shrink_0()
        .px(px(theme::STREAM_PAD_X))
        .pt(px(0.0))
        .pb(px(13.0))
        // Overlay host: popups are absolute and must not grow this bar's layout height.
        .relative()
        .child(
            div()
                .w_full()
                .max_w(px(theme::READING_W))
                .relative()
                .when_some(slash_completion, |host, completion| {
                    host.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_full()
                            .pb(px(10.0))
                            .occlude()
                            .flex()
                            .justify_center()
                            .child(command_suggestion_sheet(
                                completion,
                                command_selection,
                                command_scroll,
                                cx,
                            )),
                    )
                })
                .when_some(file_completion, |host, matches| {
                    host.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_full()
                            .pb(px(10.0))
                            .occlude()
                            .flex()
                            .justify_center()
                            .child(file_suggestion_sheet(
                                matches,
                                file_selection,
                                file_scroll,
                                cx,
                            )),
                    )
                })
                .when(model_open || thinking_open, |host| {
                    // Clear gap so popup bottom border never stacks on the prompt top border.
                    host.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_full()
                            .pb(px(10.0))
                            .occlude()
                            .flex()
                            .justify_center()
                            .child(if model_open {
                                model_switcher_sheet(
                                    models,
                                    provider_filter,
                                    search,
                                    model_scroll,
                                    provider_scroll,
                                    cx,
                                )
                                .into_any_element()
                            } else {
                                thinking_select_sheet(models, thinking_scroll, cx)
                                    .into_any_element()
                            }),
                    )
                })
                .child(queue_preview(queue, queue_clear_pending, cx))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .rounded(px(theme::RADIUS_XL))
                        .border_1()
                        .border_color(theme::edge())
                        .bg(theme::panel())
                        .overflow_hidden()
                        .can_drop(move |value, _, _| can_attach && value.is::<ExternalPaths>())
                        .drag_over::<ExternalPaths>(|style, _, _, _| {
                            style.border_color(theme::focus()).bg(theme::panel_lift())
                        })
                        .on_drop(cx.listener(|view, paths: &ExternalPaths, _, cx| {
                            view.attach_dropped_paths(paths.paths(), cx);
                        }))
                        .when(saved_input_count > 0, |panel| {
                            panel.child(
                                div().flex().items_center().justify_between().px(px(12.0)).py(px(6.0))
                                    .font_family(theme::sans()).text_size(theme::text_size(theme::T_UI_SM))
                                    .text_color(theme::smoke())
                                    .child(format!("{saved_input_count} saved input{}", if saved_input_count == 1 { "" } else { "s" }))
                                    .child(tray_quiet_action(
                                        "prompt-restore-input", "Restore next", true,
                                        Box::new(cx.listener(|view, _, window, cx| view.restore_saved_input(window, cx))),
                                    )),
                            )
                        })
                        .when(
                            extension_ui.widgets.iter().any(|(_, widget)| {
                                widget.placement == WidgetPlacement::AboveEditor
                            }),
                            |panel| {
                                panel.child(extension_widgets(
                                    extension_ui,
                                    WidgetPlacement::AboveEditor,
                                ))
                            },
                        )
                        .child(composer.clone())
                        .child(div().min_h(px(47.0)).px(px(19.0)).pb(px(11.0)).flex().items_center()
                            .justify_between().gap(px(12.0)).flex_wrap()
                            .child(div().flex().items_center().gap(px(20.0)).flex_shrink_0()
                                .child(div().w(px(74.0)).child(attach_button(can_attach,
                                    Box::new(cx.listener(|view, _, _, cx| view.choose_attachments(cx))))))
                                .child(div().w(px(108.0)).child(tray_select(
                                    "prompt-model-picker", model_label, model_open, can_pick_model, 96.0, "Switch model",
                                    Box::new(cx.listener(|view, _, window, cx| view.toggle_model_panel(ModelPanel::Switcher, window, cx))),
                                )))
                                .child(tray_select(
                                    "prompt-thinking-select", thinking_label, thinking_open, can_pick_thinking, 132.0, "Thinking effort",
                                    Box::new(cx.listener(|view, _, window, cx| view.toggle_model_panel(ModelPanel::Thinking, window, cx))),
                                )))
                            .child(div().flex().items_center().gap(px(8.0)).flex_shrink_0()
                                .when(running || bash_running, |tray| tray.child(delivery_button(
                                    "prompt-abort", if bash_running { "Stop Bash" } else { "Stop" }, "icons/stop-square.svg",
                                    if bash_running { 104.0 } else { 84.0 }, true, false,
                                    Box::new(move |_, _, cx| { abort_composer.update(cx, |composer, cx| composer.request_abort(cx)); }),
                                )))
                                .when(running, |tray| tray.child(delivery_button(
                                    "prompt-follow-up", "Queue", "icons/queue-return.svg", 94.0, can_submit, false,
                                    Box::new(move |_, _, cx| { follow_composer.update(cx, |composer, cx| composer.emit_accept(true, cx)); }),
                                )))
                                .child(submit_button("prompt-submit", running, can_submit,
                                    Box::new(move |_, _, cx| { submit_composer.update(cx, |composer, cx| composer.emit_accept(false, cx)); }),
                                ))))
                        .when_some(tray_line, |panel, (text, color, mono)| {
                            panel.child(
                                div().px(px(14.0)).pb(px(10.0))
                                    .font_family(if mono { theme::mono() } else { theme::sans() })
                                    .text_size(theme::text_size(theme::T_UI_SM))
                                    .text_color(color).child(text),
                            )
                        })
                        .when_some(draft_feedback, |panel, feedback| {
                            panel.child(draft_storage_notice(feedback, cx))
                        })
                        .when(
                            extension_ui.widgets.iter().any(|(_, widget)| {
                                widget.placement == WidgetPlacement::BelowEditor
                            }),
                            |panel| {
                                panel.child(extension_widgets(
                                    extension_ui,
                                    WidgetPlacement::BelowEditor,
                                ))
                            },
                        )
                        .when(!extension_ui.statuses.is_empty(), |panel| {
                            panel.child(extension_status_bar(extension_ui))
                        }),
                ),
        )
        .child(div().w_full().max_w(px(theme::READING_W)).mt(px(12.0)).h(px(18.0))
            .flex().items_center().justify_between().gap(px(16.0))
            .font_family(theme::sans()).text_size(theme::text_size(11.0)).line_height(px(18.0))
            .text_color(theme::ash())
            .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(composer.read(cx).hint_text()))
            .child(div().flex_shrink_0().child(usage_label(projection))))
}

/// Shared by the prompt dock and full-page settings, so a failed close never
/// leaves its recovery action on a hidden screen.
pub(super) fn draft_storage_notice(feedback: &str, cx: &mut Context<RootView>) -> impl IntoElement {
    div()
        .px(px(14.0))
        .py(px(8.0))
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .border_t_1()
        .border_color(theme::edge_soft())
        .font_family(theme::sans())
        .text_size(theme::text_size(theme::T_UI_SM))
        .text_color(theme::data())
        .child(div().min_w_0().flex_1().child(feedback.to_owned()))
        .child(tray_quiet_action(
            "draft-status-retry", "Retry storage", true,
            Box::new(cx.listener(|view, _, window, cx| view.retry_draft_storage(window, cx))),
        ))
        .child(tray_quiet_action(
            "draft-status-dismiss", "Dismiss", true,
            Box::new(cx.listener(|view, _, _, cx| {
                view.draft_feedback = None;
                cx.notify();
            })),
        ))
}

/// Tray select trigger: a quiet, borderless-looking chip until hovered or
/// open, so the tray reads as one surface instead of a row of boxes.
fn tray_select(
    id: impl Into<SharedString>, label: SharedString, open: bool, enabled: bool,
    width: f32, tooltip_label: &'static str, on_click: controls::ClickHandler,
) -> impl IntoElement {
    div().id(id.into()).h(px(TRAY_CONTROL_H)).w(px(width)).rounded(px(5.0))
        .flex().items_center().gap(px(10.0)).flex_shrink_0()
        .bg(if open { theme::selection() } else { clear() })
        .text_color(if tooltip_label == "Thinking effort" || !enabled { theme::ash() } else { theme::bone() })
        .when(enabled, |button| button.tab_index(0).cursor_pointer()
            .hover(|style| style.bg(theme::panel_hover())).focus(|style| style.bg(theme::selection()))
            .on_click(move |event, window, cx| on_click(event, window, cx)))
        .tooltip(controls::text_tooltip(tooltip_label, None::<&str>))
        .child(div().min_w_0().flex_1().font_family(theme::sans()).text_size(theme::text_size(12.0))
            .line_height(px(18.0)).font_weight(FontWeight::NORMAL).overflow_hidden().text_ellipsis().whitespace_nowrap().child(label))
        .child(svg().path(if open { "icons/chevron-up.svg" } else { "icons/chevron-down.svg" })
            .size(px(16.0)).text_color(theme::ash()).flex_shrink_0())
}

fn attach_button(enabled: bool, on_click: controls::ClickHandler) -> impl IntoElement {
    div().id("prompt-attach-files").h(px(36.0)).flex().items_center().gap(px(7.0))
        .text_color(theme::ash()).font_family(theme::sans()).text_size(theme::text_size(12.0))
        .when(enabled, |button| button.tab_index(0).cursor_pointer().focus(|style| style.bg(theme::selection()))
            .hover(|style| style.text_color(theme::bone()))
            .on_click(move |event, window, cx| on_click(event, window, cx)))
        .tooltip(controls::text_tooltip("Attach files", Some("Ctrl+O")))
        .child(svg().path("icons/paperclip.svg").size(px(16.0)).text_color(theme::ash())).child("Attach")
}

fn delivery_button(
    id: impl Into<SharedString>, label: &'static str, icon: &'static str,
    width: f32, enabled: bool, primary: bool, on_click: controls::ClickHandler,
) -> impl IntoElement {
    div().id(id.into()).h(px(36.0)).w(px(width)).px(px(12.0)).rounded(px(6.0))
        .flex().items_center().justify_between().flex_shrink_0()
        .font_family(theme::sans()).text_size(theme::text_size(13.0)).font_weight(FontWeight::MEDIUM)
        .bg(if primary { theme::signal() } else { theme::panel_lift() })
        .text_color(if primary { theme::on_accent() } else { theme::bone() })
        .when(enabled, |button| button.tab_index(0).cursor_pointer()
            .hover(move |style| style.bg(if primary { theme::signal_hot() } else { theme::panel_hover() }))
            .focus(|style| style.border_1().border_color(theme::focus()))
            .on_click(move |event, window, cx| on_click(event, window, cx)))
        .tooltip(controls::text_tooltip(if enabled { label } else { "Write a prompt or attach a file first" }, None::<&str>))
        .child(label)
        .child(svg().path(icon).size(px(16.0)).text_color(if primary { theme::on_accent() } else { theme::ash() }))
}

fn queue_preview(queue: &QueueContents, clearing: bool, cx: &mut Context<RootView>) -> impl IntoElement {
    let (count, message) = match queue {
        QueueContents::Unknown { pending_count } => (*pending_count as usize, format!("{pending_count} pending messages")),
        QueueContents::Known { steering, follow_up } => {
            let count = steering.len() + follow_up.len();
            let first = steering.first().or_else(|| follow_up.first()).cloned().unwrap_or_default();
            (count, if count > 1 { format!("{first}  (+{} more)", count - 1) } else { first })
        }
    };
    div().when(count > 0, |host| host.child(div().id("queued-input-preview").h(px(47.0)).w_full()
        .border_t_1().border_color(theme::edge_soft()).pl(px(12.0)).pr(px(20.0))
        .flex().items_center().gap(px(0.0)).font_family(theme::sans())
        .child(div().w(px(68.0)).flex_shrink_0().text_size(theme::text_size(11.0)).font_weight(FontWeight::MEDIUM).text_color(theme::signal()).child("Queued"))
        .child(div().min_w_0().flex_1().pr(px(16.0)).text_size(theme::text_size(12.0)).text_color(theme::ash())
            .overflow_hidden().text_ellipsis().whitespace_nowrap().child(message))
        .child(div().id("remove-queued-inputs").h(px(32.0)).flex().items_center().gap(px(12.0))
            .flex_shrink_0().text_size(theme::text_size(11.0)).text_color(theme::ash())
            .when(!clearing, |button| button.tab_index(0).cursor_pointer().focus(|style| style.bg(theme::selection()))
                .tooltip(controls::text_tooltip("Remove queued inputs without stopping Pi. Removed inputs remain in saved inputs.", None::<&str>))
                .on_click(cx.listener(|view, _, _, cx| { view.controller.update(cx, |controller, cx| { controller.clear_queued_inputs(cx); }); })))
            .child(if clearing { "Removing…" } else if count > 1 { "Clear all" } else { "Remove" })
            .child(svg().path("icons/close.svg").size(px(16.0)).text_color(theme::ash())))))
}

fn usage_label(projection: &ShellProjection) -> String {
    use crate::state::DisplayValue;
    let context = projection.context_percent.as_ref().map(|percent| format!("{percent} context"));
    let cost = match &projection.cost {
        DisplayValue::Known(value) => Some(value.clone()),
        DisplayValue::Stale(value) => Some(format!("{value} · stale")),
        _ => None,
    };
    [context, cost].into_iter().flatten().collect::<Vec<_>>().join(" · ")
}



/// Quiet text action for run-state affordances (Abort, Follow up).
fn tray_quiet_action(
    id: impl Into<SharedString>,
    label: &'static str,
    enabled: bool,
    on_click: controls::ClickHandler,
) -> impl IntoElement {
    div()
        .id(id.into())
        .h(px(TRAY_CONTROL_H))
        .px(px(9.0))
        .rounded(px(theme::RADIUS_MD))
        .flex()
        .items_center()
        .flex_shrink_0()
        .font_family(theme::main())
        .text_size(theme::text_size(theme::T_UI_SM))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(if enabled {
            theme::bone_dim()
        } else {
            theme::smoke()
        })
        .when(enabled, |button| {
            button
                .tab_index(0)
                .cursor_pointer()
                .hover(|button| button.bg(theme::panel_lift()).text_color(theme::bone()))
                .focus(|button| button.text_color(theme::focus()))
                .active(|button| button.bg(theme::panel_hover()))
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .child(label)
}

/// A labeled primary action. Delivery mode is legible without a tooltip.
fn submit_button(id: impl Into<SharedString>, running: bool, can_submit: bool, on_click: controls::ClickHandler) -> impl IntoElement {
    delivery_button(id, if running { "Steer" } else { "Send" }, "icons/arrow-up.svg",
        if running { 96.0 } else { 92.0 }, can_submit, true, on_click)
}


fn short_model_label(
    projection: &ShellProjection,
    models: &ModelRuntimeProjection,
) -> SharedString {
    let raw = if let Some(identity) = models.active_model.as_ref() {
        if let Some(entry) = models
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.model(identity))
        {
            entry.name.clone()
        } else {
            identity.id.clone()
        }
    } else {
        let label = projection.model.label();
        if label == "Unknown" || label == "Loading" || label == "Awaiting" {
            "Model".to_owned()
        } else {
            label
        }
    };
    compact_label(&raw, 22).into()
}

fn short_thinking_label(
    projection: &ShellProjection,
    models: &ModelRuntimeProjection,
) -> SharedString {
    let level = models
        .effective_thinking
        .or(models.active_thinking)
        .or(models.requested_thinking);
    let value = if let Some(level) = level {
        thinking_short(level)
    } else {
        let label = projection.thinking.label();
        if label == "Unknown" || label == "Loading" || label == "Awaiting" {
            "Off".to_owned()
        } else {
            compact_label(&label, 10)
        }
    };
    format!("Thinking: {value}").into()
}

fn thinking_short(level: ThinkingLevel) -> String {
    match level {
        ThinkingLevel::Off => "Off".to_owned(),
        ThinkingLevel::Minimal => "Minimal".to_owned(),
        ThinkingLevel::Low => "Low".to_owned(),
        ThinkingLevel::Medium => "Medium".to_owned(),
        ThinkingLevel::High => "High".to_owned(),
        ThinkingLevel::Xhigh => "XHigh".to_owned(),
        ThinkingLevel::Max => "Max".to_owned(),
    }
}

fn compact_label(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_owned();
    }
    let mut out = trimmed
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
}
