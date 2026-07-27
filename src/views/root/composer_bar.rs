use super::model_panels::{model_switcher_sheet, thinking_select_sheet};
use super::overlays::{command_suggestion_sheet, extension_status_bar, extension_widgets};
use super::*;

pub(super) struct ComposerBarParams<'a> {
    pub(super) composer: &'a Entity<Composer>,
    pub(super) models: &'a ModelRuntimeProjection,
    pub(super) projection: &'a ShellProjection,
    pub(super) panel: Option<ModelPanel>,
    pub(super) provider_filter: Option<&'a str>,
    pub(super) search: &'a Entity<Composer>,
    pub(super) slash_commands: &'a [CommandEntry],
    pub(super) command_selection: usize,
    pub(super) command_scroll: &'a ScrollHandle,
    pub(super) model_scroll: &'a ScrollHandle,
    pub(super) provider_scroll: &'a ScrollHandle,
    pub(super) thinking_scroll: &'a ScrollHandle,
    pub(super) slash_dismissed: bool,
    pub(super) extension_ui: &'a ExtensionUiProjection,
}

pub(super) fn composer_bar(
    params: ComposerBarParams<'_>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let ComposerBarParams {
        composer,
        models,
        projection,
        panel,
        provider_filter,
        search,
        slash_commands,
        command_selection,
        command_scroll,
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
    let slash_completion =
        (!slash_dismissed && !slash_commands.is_empty()).then_some(slash_commands);

    div()
        .flex_shrink_0()
        .px(px(theme::STREAM_PAD_X))
        .pt(px(8.0))
        .pb(px(10.0))
        // Overlay host: popups are absolute and must not grow this bar's layout height.
        .relative()
        .child(
            div()
                .w_full()
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
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .rounded(px(theme::RADIUS_SM))
                        .border_1()
                        .border_color(theme::edge_hard())
                        .bg(theme::panel())
                        .overflow_hidden()
                        .child(
                            div()
                                .px(px(8.0))
                                .pt(px(4.0))
                                .pb(px(4.0))
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(4.0))
                                .border_b_1()
                                .border_color(theme::edge_soft())
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(1.0))
                                        .flex_shrink_0()
                                        .child(controls::compact_select(
                                            "prompt-model-picker",
                                            model_label,
                                            model_open,
                                            can_pick_model,
                                            148.0,
                                            Box::new(cx.listener(|view, _, window, cx| {
                                                view.toggle_model_panel(
                                                    ModelPanel::Switcher,
                                                    window,
                                                    cx,
                                                )
                                            })),
                                        ))
                                        .child(
                                            div()
                                                .w(px(1.0))
                                                .h(px(14.0))
                                                .rounded(px(1.0))
                                                .bg(theme::edge()),
                                        )
                                        .child(controls::compact_select(
                                            "prompt-thinking-select",
                                            thinking_label,
                                            thinking_open,
                                            can_pick_thinking,
                                            108.0,
                                            Box::new(cx.listener(|view, _, window, cx| {
                                                view.toggle_model_panel(
                                                    ModelPanel::Thinking,
                                                    window,
                                                    cx,
                                                )
                                            })),
                                        )),
                                )
                                .child(div().flex_1().min_w_0())
                                .when_some(models.clamp_notice.clone(), |row, notice| {
                                    row.child(
                                        div()
                                            .min_w_0()
                                            .max_w(px(220.0))
                                            .font_family(theme::sans())
                                            .text_size(theme::text_size(theme::T_TINY))
                                            .text_color(theme::data())
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .child(notice),
                                    )
                                })
                                .when_some(
                                    models
                                        .feedback
                                        .clone()
                                        .filter(|_| !model_open && !thinking_open),
                                    |row, fb| {
                                        row.child(
                                            div()
                                                .min_w_0()
                                                .max_w(px(180.0))
                                                .font_family(theme::mono())
                                                .text_size(theme::text_size(theme::T_TINY))
                                                .text_color(theme::smoke())
                                                .overflow_hidden()
                                                .text_ellipsis()
                                                .whitespace_nowrap()
                                                .child(fb),
                                        )
                                    },
                                )
                                .child(controls::chrome_icon_action(
                                    "prompt-model-settings",
                                    "icons/cog.svg",
                                    true,
                                    Box::new(cx.listener(|view, _, window, cx| {
                                        view.show_model_panel(
                                            ModelPanel::Settings(ModelSettingsTab::Providers),
                                            window,
                                            cx,
                                        )
                                    })),
                                )),
                        )
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
}

fn short_model_label(projection: &ShellProjection, models: &ModelRuntimeProjection) -> String {
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
    compact_label(&raw, 22)
}

fn short_thinking_label(projection: &ShellProjection, models: &ModelRuntimeProjection) -> String {
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
    format!("Think: {value}")
}

fn thinking_short(level: ThinkingLevel) -> String {
    match level {
        ThinkingLevel::Off => "Off".to_owned(),
        ThinkingLevel::Minimal => "Min".to_owned(),
        ThinkingLevel::Low => "Low".to_owned(),
        ThinkingLevel::Medium => "Med".to_owned(),
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
