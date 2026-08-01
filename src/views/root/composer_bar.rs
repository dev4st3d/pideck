//! The prompt card: a softly rounded, elevated surface that carries the
//! composer input on top and one quiet control tray underneath (model and
//! thinking selects left, status in the middle, tools and the submit orb at
//! the right). It replaces the old header-plus-footer panel chrome.

use super::model_panels::{model_switcher_sheet, thinking_select_sheet};
use super::overlays::{
    command_suggestion_sheet, extension_status_bar, extension_widgets, file_suggestion_sheet,
};
use super::*;
use crate::file_completion::FileMatch;
use crate::views::composer::ComposerFeedback;
use gpui::{BoxShadow, SharedString, rgba};

/// One vertical rhythm for every control in the prompt tray.
const TRAY_CONTROL_H: f32 = 26.0;

fn clear() -> gpui::Rgba {
    rgba(0x0000_0000)
}

pub(super) struct ComposerBarParams<'a> {
    pub(super) composer: &'a Entity<Composer>,
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
    window: &Window,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let ComposerBarParams {
        composer,
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

    let input_focused = composer.read(cx).focus_handle(cx).is_focused(window);
    // Keep the card lit while one of its floating sheets owns focus.
    let card_active = input_focused || model_open || thinking_open;
    let availability = composer.read(cx).availability();
    let running = availability == ComposerAvailability::Running;
    let bash_running = availability == ComposerAvailability::BashRunning;
    let can_submit = composer.read(cx).can_submit();
    let input_enlarged = composer.read(cx).input_enlarged();

    // The tray carries one line of meaning at a time: a clamp notice outranks
    // a binding notice, which outranks the composer's own status. While the
    // desk is simply idle the line offers keyboard hints instead of noise.
    let idle_ready = availability == ComposerAvailability::Idle
        && matches!(composer.read(cx).feedback(), ComposerFeedback::Ready);
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
        } else if idle_ready {
            Some((
                composer.read(cx).hint_text().to_owned(),
                theme::smoke(),
                true,
            ))
        } else {
            let status = composer.read(cx).status_text();
            (!status.is_empty()).then_some((status, feedback_color, false))
        };

    let submit_composer = composer.clone();
    let abort_composer = composer.clone();
    let follow_composer = composer.clone();

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
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .rounded(px(theme::RADIUS_LG))
                        .border_1()
                        .border_color(if card_active {
                            theme::edge_hard()
                        } else {
                            theme::edge()
                        })
                        .bg(theme::panel())
                        // One tight, low-offset shadow cast downward, in family
                        // with the inspector sheet. Deliberate lift, no bloom.
                        .shadow(vec![BoxShadow {
                            color: rgba(0x0000_0047).into(),
                            offset: point(px(0.0), px(10.0)),
                            blur_radius: px(24.0),
                            spread_radius: px(-10.0),
                        }])
                        .overflow_hidden()
                        .can_drop(move |value, _, _| can_attach && value.is::<ExternalPaths>())
                        .drag_over::<ExternalPaths>(|style, _, _, _| {
                            style.border_color(theme::focus()).bg(theme::panel_lift())
                        })
                        .on_drop(cx.listener(|view, paths: &ExternalPaths, _, cx| {
                            view.attach_dropped_paths(paths.paths(), cx);
                        }))
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
                        // Bottom tray: context selects left, one status line in
                        // the middle, tools and the submit orb at the right.
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.0))
                                .px(px(8.0))
                                .pb(px(8.0))
                                .pt(px(1.0))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(4.0))
                                        .flex_shrink_0()
                                        .child(tray_select(
                                            "prompt-model-picker",
                                            model_label,
                                            model_open,
                                            can_pick_model,
                                            148.0,
                                            "Switch model",
                                            Box::new(cx.listener(|view, _, window, cx| {
                                                view.toggle_model_panel(
                                                    ModelPanel::Switcher,
                                                    window,
                                                    cx,
                                                )
                                            })),
                                        ))
                                        .child(tray_select(
                                            "prompt-thinking-select",
                                            thinking_label,
                                            thinking_open,
                                            can_pick_thinking,
                                            108.0,
                                            "Thinking effort",
                                            Box::new(cx.listener(|view, _, window, cx| {
                                                view.toggle_model_panel(
                                                    ModelPanel::Thinking,
                                                    window,
                                                    cx,
                                                )
                                            })),
                                        )),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .overflow_hidden()
                                        .when_some(tray_line, |row, (text, color, mono)| {
                                            if mono {
                                                row.child(
                                                    div()
                                                        .min_w_0()
                                                        .font_family(theme::mono())
                                                        .text_size(theme::text_size(theme::T_TINY))
                                                        .text_color(color)
                                                        .overflow_hidden()
                                                        .text_ellipsis()
                                                        .whitespace_nowrap()
                                                        .child(text),
                                                )
                                            } else {
                                                row.child(
                                                    div()
                                                        .min_w_0()
                                                        .font_family(theme::sans())
                                                        .text_size(theme::text_size(theme::T_UI_SM))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(color)
                                                        .overflow_hidden()
                                                        .text_ellipsis()
                                                        .whitespace_nowrap()
                                                        .child(text),
                                                )
                                            }
                                        }),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(2.0))
                                        .flex_shrink_0()
                                        .child(tray_icon(
                                            "prompt-attach-files",
                                            "icons/paperclip.svg",
                                            false,
                                            can_attach,
                                            "Attach files",
                                            Some("Ctrl+O"),
                                            Box::new(cx.listener(|view, _, _, cx| {
                                                view.choose_attachments(cx);
                                            })),
                                        ))
                                        .child(tray_icon(
                                            "prompt-enlarge-input",
                                            "icons/expand.svg",
                                            input_enlarged,
                                            true,
                                            if input_enlarged {
                                                "Shrink input"
                                            } else {
                                                "Enlarge input"
                                            },
                                            None,
                                            Box::new(cx.listener(|view, _, window, cx| {
                                                view.toggle_composer_enlarged(window, cx);
                                            })),
                                        ))
                                        .child(tray_icon(
                                            "prompt-model-settings",
                                            "icons/cog.svg",
                                            false,
                                            true,
                                            "Model and provider settings",
                                            None,
                                            Box::new(cx.listener(|view, _, window, cx| {
                                                view.show_model_panel(
                                                    ModelPanel::Settings(
                                                        ModelSettingsTab::Providers,
                                                    ),
                                                    window,
                                                    cx,
                                                )
                                            })),
                                        ))
                                        .when(running || bash_running, |tray| {
                                            tray.child(div().w(px(6.0))).child(tray_quiet_action(
                                                "prompt-abort",
                                                if bash_running { "Abort Bash" } else { "Abort" },
                                                true,
                                                Box::new(move |_, _, cx| {
                                                    abort_composer.update(cx, |composer, cx| {
                                                        composer.request_abort(cx);
                                                    });
                                                }),
                                            ))
                                        })
                                        .when(running, |tray| {
                                            tray.child(tray_quiet_action(
                                                "prompt-follow-up",
                                                "Follow up",
                                                can_submit,
                                                Box::new(move |_, _, cx| {
                                                    follow_composer.update(cx, |composer, cx| {
                                                        composer.emit_accept(true, cx);
                                                    });
                                                }),
                                            ))
                                        })
                                        .child(div().w(px(4.0)))
                                        .child(submit_orb(
                                            "prompt-submit",
                                            running,
                                            can_submit,
                                            Box::new(move |_, _, cx| {
                                                submit_composer.update(cx, |composer, cx| {
                                                    composer.emit_accept(false, cx);
                                                });
                                            }),
                                        )),
                                ),
                        )
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

/// Tray select trigger: a quiet, borderless-looking chip until hovered or
/// open, so the tray reads as one surface instead of a row of boxes.
fn tray_select(
    id: impl Into<SharedString>,
    label: SharedString,
    open: bool,
    enabled: bool,
    max_width: f32,
    tooltip_label: &'static str,
    on_click: controls::ClickHandler,
) -> impl IntoElement {
    div()
        .id(id.into())
        .h(px(TRAY_CONTROL_H))
        .max_w(px(max_width))
        .px(px(9.0))
        .rounded(px(theme::RADIUS_MD))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(5.0))
        .flex_shrink_0()
        .bg(if open { theme::panel_lift() } else { clear() })
        .border_1()
        .border_color(if open { theme::edge() } else { clear() })
        .text_color(if !enabled {
            theme::smoke()
        } else if open {
            theme::bone()
        } else {
            theme::bone_dim()
        })
        .when(enabled, |button| {
            button
                .tab_index(0)
                .cursor_pointer()
                .hover(|button| button.bg(theme::panel_lift()).text_color(theme::bone()))
                .focus(|button| button.border_color(theme::focus()))
                .active(|button| button.bg(theme::panel_hover()))
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .tooltip(controls::text_tooltip(tooltip_label, None::<&str>))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .font_family(theme::main())
                .text_size(theme::text_size(theme::T_LABEL))
                .font_weight(FontWeight::SEMIBOLD)
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(label),
        )
        .child(
            svg()
                .path(if open {
                    "icons/chevron-up.svg"
                } else {
                    "icons/chevron-down.svg"
                })
                .size(px(11.0))
                .text_color(if open { theme::data() } else { theme::smoke() })
                .flex_shrink_0(),
        )
}

/// Tray icon action: slightly larger hit target than the old chrome icons,
/// corners in family with the card.
fn tray_icon(
    id: impl Into<SharedString>,
    icon_path: &'static str,
    selected: bool,
    enabled: bool,
    tooltip_label: &'static str,
    tooltip_hint: Option<&'static str>,
    on_click: controls::ClickHandler,
) -> impl IntoElement {
    div()
        .id(id.into())
        .size(px(TRAY_CONTROL_H))
        .rounded(px(theme::RADIUS_MD))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .bg(if selected {
            theme::panel_lift()
        } else {
            clear()
        })
        .text_color(if selected {
            theme::bone()
        } else {
            theme::smoke()
        })
        .when(enabled, |button| {
            button
                .tab_index(0)
                .cursor_pointer()
                .hover(|button| button.bg(theme::panel_lift()).text_color(theme::bone_dim()))
                .focus(|button| button.text_color(theme::focus()))
                .active(|button| button.bg(theme::panel_hover()))
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .tooltip(controls::text_tooltip(tooltip_label, tooltip_hint))
        .child(
            svg()
                .path(icon_path)
                .size(px(13.0))
                .text_color(if selected {
                    theme::bone()
                } else if enabled {
                    theme::smoke()
                } else {
                    theme::edge_hard()
                }),
        )
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

/// The primary affordance: a round send button. Filled while it can act,
/// quiet while the draft is empty.
fn submit_orb(
    id: impl Into<SharedString>,
    running: bool,
    can_submit: bool,
    on_click: controls::ClickHandler,
) -> impl IntoElement {
    div()
        .id(id.into())
        .size(px(TRAY_CONTROL_H))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .bg(if can_submit {
            theme::signal()
        } else {
            theme::panel_lift()
        })
        .when(can_submit, |button| {
            button
                .tab_index(0)
                .cursor_pointer()
                .hover(|button| button.bg(theme::signal_hot()))
                .focus(|button| button.bg(theme::signal_hot()))
                .active(|button| button.bg(theme::signal_deep()))
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .tooltip(controls::text_tooltip(
            if can_submit {
                if running { "Steer" } else { "Send" }
            } else {
                "Write a prompt or attach a file first"
            },
            Some("Enter"),
        ))
        .child(
            svg()
                .path("icons/arrow-up.svg")
                .size(px(13.0))
                .text_color(if can_submit {
                    theme::canvas()
                } else {
                    theme::smoke()
                }),
        )
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
    format!("Think: {value}").into()
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
