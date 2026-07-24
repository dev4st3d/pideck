use super::inspector::runtime_error_notice;
use super::shared::{plural, popup_sheet};
use super::*;

pub(super) fn conversation_area(
    projection: &ShellProjection,
    conversation_projection: Arc<ConversationProjection>,
    conversation_list: Arc<ConversationListModel>,
    conversation_list_state: ListState,
    transcript_cache: Entity<TranscriptTextCache>,
    root: Entity<RootView>,
) -> impl IntoElement {
    // The transcript shares the center column with the composer and must yield
    // height to it on every lifecycle-driven rerender.
    div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(theme::canvas())
        .when(
            matches!(
                projection.lifecycle.as_str(),
                "Connection error" | "No model"
            ),
            |area| area.child(runtime_error_notice(projection)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .w_full()
                .relative()
                .overflow_hidden()
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            window.on_mouse_event(
                                move |event: &ScrollWheelEvent, phase, window, cx| {
                                    if phase != DispatchPhase::Capture
                                        || !bounds.contains(&event.position)
                                    {
                                        return;
                                    }
                                    let handled = root.update(cx, |view, cx| {
                                        view.on_conversation_scroll_wheel(event, window, cx)
                                    });
                                    if handled {
                                        cx.stop_propagation();
                                    }
                                },
                            );
                        },
                    )
                    .absolute()
                    .size_full(),
                )
                .child(
                    list(conversation_list_state, move |item_index, _, cx| {
                        conversation_list.render_item(
                            item_index,
                            &conversation_projection,
                            &transcript_cache,
                            cx,
                        )
                    })
                    .size_full()
                    .min_w_0()
                    .pt(px(16.0))
                    .pb(px(16.0)),
                ),
        )
}

pub(super) fn command_suggestion_sheet(
    entries: &[CommandEntry],
    selected: usize,
    scroll: &ScrollHandle,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let rows = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| command_row(entry, index == selected, cx))
        .collect::<Vec<_>>();
    popup_sheet()
        .max_w(px(920.0))
        .max_h(px(310.0))
        .child(
            div()
                .px(px(10.0))
                .py(px(7.0))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(theme::edge_soft())
                .child(controls::section_label("Commands"))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child("↑↓ choose · Enter run · Esc close"),
                ),
        )
        .child(
            div()
                .id("slash-command-results")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .scrollbar_width(px(theme::SCROLLBAR))
                .children(rows),
        )
}

fn command_row(
    entry: &CommandEntry,
    selected: bool,
    cx: &mut Context<RootView>,
) -> gpui::AnyElement {
    let entry = entry.clone();
    let click_entry = entry.clone();
    let provenance = entry.provenance_label();
    let hint = entry
        .argument_hint
        .as_deref()
        .map(|hint| format!(" {hint}"))
        .unwrap_or_default();
    div()
        .id(gpui::SharedString::from(format!(
            "command-row-{}",
            entry.id
        )))
        .px(px(10.0))
        .py(px(7.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .border_b_1()
        .border_color(theme::edge_soft())
        .when(selected, |row| row.bg(theme::panel_hover()))
        .when(entry.enabled, |row| {
            row.cursor_pointer()
                .hover(|row| row.bg(theme::panel_hover()))
        })
        .when(!entry.enabled, |row| row.opacity(0.55))
        .on_click(cx.listener(move |view, _, window, cx| {
            view.choose_command_entry(click_entry.clone(), window, cx)
        }))
        .child(
            div()
                .w(px(76.0))
                .flex_shrink_0()
                .font_family(theme::MONO)
                .text_size(px(theme::T_TINY))
                .text_color(theme::data())
                .child(entry.group.label()),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_BODY))
                        .text_color(theme::bone())
                        .child(format!("/{}{}", entry.name, hint)),
                )
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(entry.description.clone()),
                ),
        )
        .child(
            div()
                .max_w(px(260.0))
                .font_family(theme::MONO)
                .text_size(px(theme::T_TINY))
                .text_color(theme::ash())
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(provenance),
        )
        .into_any_element()
}

pub(super) fn runtime_notification_stack(
    notifications: &VecDeque<RuntimeNotification>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let cards = notifications
        .iter()
        .enumerate()
        .map(|(index, notification)| {
            let (label, color) = match notification.kind {
                NotificationKind::Info => ("Pi", theme::data()),
                NotificationKind::Warning => ("Pi warning", theme::focus()),
                NotificationKind::Error => ("Pi error", theme::error()),
            };
            div()
                .occlude()
                .w_full()
                .p(px(10.0))
                .rounded(px(theme::RADIUS_SM))
                .border_1()
                .border_color(color)
                .bg(theme::panel_lift())
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(12.0))
                        .child(
                            div()
                                .font_family(theme::MONO)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(px(theme::T_TINY))
                                .text_color(color)
                                .child(label),
                        )
                        .child(controls::chrome_action(
                            format!("dismiss-runtime-notification-{index}"),
                            "Dismiss",
                            true,
                            Box::new(cx.listener(move |view, _, _, cx| {
                                view.dismiss_runtime_notification(index, cx)
                            })),
                        )),
                )
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .line_height(px(18.0))
                        .text_color(theme::bone_dim())
                        .child(notification.message.clone()),
                )
        })
        .collect::<Vec<_>>();

    div()
        .absolute()
        .top(px(theme::TITLE_H + 12.0))
        .right(px(18.0))
        .w(px(420.0))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .children(cards)
}

pub(super) fn extension_widgets(
    extension_ui: &ExtensionUiProjection,
    placement: WidgetPlacement,
) -> impl IntoElement {
    div()
        .px(px(12.0))
        .py(px(8.0))
        .border_b_1()
        .border_color(theme::edge_soft())
        .flex()
        .flex_col()
        .gap(px(7.0))
        .children(
            extension_ui
                .widgets
                .iter()
                .filter(move |(_, widget)| widget.placement == placement)
                .map(|(key, widget)| {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(
                            div()
                                .font_family(theme::MONO)
                                .text_size(px(theme::T_TINY))
                                .text_color(theme::data())
                                .child(sanitize_untrusted_text(key)),
                        )
                        .children(widget.lines.iter().map(|line| {
                            div()
                                .font_family(theme::MONO)
                                .text_size(px(theme::T_MONO_SM))
                                .line_height(px(17.0))
                                .text_color(theme::bone_dim())
                                .child(line.clone())
                        }))
                }),
        )
}

pub(super) fn extension_status_bar(extension_ui: &ExtensionUiProjection) -> impl IntoElement {
    div()
        .px(px(12.0))
        .py(px(6.0))
        .border_t_1()
        .border_color(theme::edge_soft())
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .children(extension_ui.statuses.iter().map(|(key, status)| {
            div()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(5.0))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::data())
                        .child(sanitize_untrusted_text(key)),
                )
                .child(
                    div()
                        .min_w_0()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::ash())
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(status.text.clone()),
                )
        }))
}

pub(super) fn extension_dialog_overlay(
    dialog: &crate::state::runtime::ExtensionDialog,
    queued_dialogs: usize,
    selected: usize,
    focus: &FocusHandle,
    input: &Entity<Composer>,
    editor: &Entity<Composer>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let deadline_copy = dialog.deadline.map(|deadline| {
        let seconds = deadline
            .saturating_duration_since(std::time::Instant::now())
            .as_secs_f32();
        format!("Auto-closes in {:.1}s", seconds.max(0.0))
    });
    let request = dialog.request.clone();
    let body = match &request {
        DialogRequest::Select { options, .. } => div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .when(options.is_empty(), |list| {
                list.child(
                    div()
                        .text_size(px(theme::T_UI_SM))
                        .text_color(theme::error())
                        .child("This request has no selectable options."),
                )
            })
            .children(options.iter().enumerate().map(|(index, option)| {
                let option_answer = option.clone();
                div()
                    .id(("extension-dialog-option", index))
                    .px(px(12.0))
                    .py(px(9.0))
                    .rounded(px(theme::RADIUS_SM))
                    .border_1()
                    .border_color(if index == selected {
                        theme::focus()
                    } else {
                        theme::edge_soft()
                    })
                    .bg(if index == selected {
                        theme::panel_hover()
                    } else {
                        theme::canvas()
                    })
                    .cursor_pointer()
                    .hover(|row| row.bg(theme::panel_lift()).border_color(theme::edge()))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.answer_extension_dialog(
                            DialogAnswer::Value(option_answer.clone()),
                            window,
                            cx,
                        )
                    }))
                    .child(
                        div()
                            .font_family(theme::SANS)
                            .text_size(px(theme::T_UI))
                            .text_color(theme::bone())
                            .child(option.clone()),
                    )
            }))
            .into_any_element(),
        DialogRequest::Confirm { message, .. } => div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .font_family(theme::SANS)
                    .text_size(px(theme::T_BODY_SM))
                    .line_height(px(21.0))
                    .text_color(theme::bone_dim())
                    .child(message.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap(px(8.0))
                    .child(extension_dialog_button(
                        "extension-confirm-no",
                        "No",
                        selected == 0,
                        Box::new(cx.listener(|view, _, window, cx| {
                            view.answer_extension_dialog(DialogAnswer::Confirmed(false), window, cx)
                        })),
                    ))
                    .child(extension_dialog_button(
                        "extension-confirm-yes",
                        "Confirm",
                        selected == 1,
                        Box::new(cx.listener(|view, _, window, cx| {
                            view.answer_extension_dialog(DialogAnswer::Confirmed(true), window, cx)
                        })),
                    )),
            )
            .into_any_element(),
        DialogRequest::Input { .. } => input.clone().into_any_element(),
        DialogRequest::Editor { .. } => editor.clone().into_any_element(),
    };

    div()
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .left_0()
        .occlude()
        .bg(gpui::rgba(0x0b0a_09e6))
        .flex()
        .items_center()
        .justify_center()
        .p(px(24.0))
        .track_focus(focus)
        .tab_index(0)
        .on_key_down(cx.listener(RootView::on_extension_dialog_key_down))
        .child(
            div()
                .id("extension-dialog-card")
                .w_full()
                .max_w(px(620.0))
                .max_h(px(620.0))
                .overflow_y_scroll()
                .p(px(18.0))
                .rounded(px(theme::RADIUS))
                .bg(theme::panel())
                .border_1()
                .border_color(theme::edge_hard())
                .flex()
                .flex_col()
                .gap(px(14.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_start()
                        .justify_between()
                        .gap(px(12.0))
                        .child(
                            div()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(4.0))
                                .child(
                                    div()
                                        .font_family(theme::MONO)
                                        .text_size(px(theme::T_TINY))
                                        .text_color(theme::focus())
                                        .child(format!(
                                            "Extension {} · untrusted UI",
                                            dialog.kind()
                                        )),
                                )
                                .child(
                                    div()
                                        .font_family(theme::SANS)
                                        .text_size(px(theme::T_TITLE))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme::bone())
                                        .child(dialog.title().to_owned()),
                                ),
                        )
                        .child(controls::chrome_action(
                            "cancel-extension-dialog",
                            "Cancel · Esc",
                            true,
                            Box::new(cx.listener(|view, _, window, cx| {
                                view.cancel_extension_dialog(window, cx);
                            })),
                        )),
                )
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .line_height(px(18.0))
                        .text_color(theme::smoke())
                        .child(
                            "This content comes from an extension. It is not a secure permission prompt and has no verified provenance.",
                        ),
                )
                .child(body)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(10.0))
                        .child(
                            div()
                                .font_family(theme::MONO)
                                .text_size(px(theme::T_TINY))
                                .text_color(theme::smoke())
                                .child(match queued_dialogs {
                                    0 => "No queued extension dialogs".to_owned(),
                                    count => format!(
                                        "{count} queued extension dialog{}",
                                        plural(count as u64)
                                    ),
                                }),
                        )
                        .when_some(deadline_copy, |row, deadline| {
                            row.child(
                                div()
                                    .font_family(theme::MONO)
                                    .text_size(px(theme::T_TINY))
                                    .text_color(theme::data())
                                    .child(deadline),
                            )
                        }),
                ),
        )
}

fn extension_dialog_button(
    id: impl Into<gpui::SharedString>,
    label: impl Into<gpui::SharedString>,
    selected: bool,
    on_click: RootClickHandler,
) -> impl IntoElement {
    div()
        .id(id.into())
        .h(px(32.0))
        .px(px(14.0))
        .rounded(px(theme::RADIUS_SM))
        .border_1()
        .border_color(if selected {
            theme::focus()
        } else {
            theme::edge()
        })
        .bg(if selected {
            theme::panel_hover()
        } else {
            theme::canvas()
        })
        .cursor_pointer()
        .hover(|button| button.bg(theme::panel_lift()).border_color(theme::focus()))
        .on_click(move |event, window, cx| on_click(event, window, cx))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .font_family(theme::CONTROL)
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(theme::T_UI_SM))
                .text_color(theme::bone())
                .child(label.into()),
        )
}

pub(super) fn single_line_title(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(160)
        .collect()
}

pub(super) fn extension_dialog_key(kind: Option<&str>, key: &str) -> Option<ExtensionDialogKey> {
    match key {
        "escape" => Some(ExtensionDialogKey::Cancel),
        "tab" => Some(ExtensionDialogKey::ContainFocus),
        "up" | "left" if matches!(kind, Some("select" | "confirm")) => {
            Some(ExtensionDialogKey::Move(-1))
        }
        "down" | "right" if matches!(kind, Some("select" | "confirm")) => {
            Some(ExtensionDialogKey::Move(1))
        }
        "enter" | "space" if matches!(kind, Some("select" | "confirm")) => {
            Some(ExtensionDialogKey::AcceptSelection)
        }
        _ => None,
    }
}

pub(super) fn wrapped_index(current: usize, count: usize, delta: isize) -> usize {
    if count == 0 {
        return 0;
    }
    (current as isize + delta).rem_euclid(count as isize) as usize
}

fn pasted_image_source(image: &PromptImage) -> Option<Arc<Image>> {
    let format = ImageFormat::from_mime_type(&image.mime_type)?;
    let bytes = STANDARD.decode(&image.data).ok()?;
    (!bytes.is_empty()).then(|| Arc::new(Image::from_bytes(format, bytes)))
}

pub(super) fn pasted_image_overlay(
    prompt_image: &PromptImage,
    index: usize,
    count: usize,
    cx: &mut Context<RootView>,
) -> gpui::AnyElement {
    let image = pasted_image_source(prompt_image);
    let image_missing = image.is_none();
    let format = prompt_image
        .mime_type
        .strip_prefix("image/")
        .unwrap_or(&prompt_image.mime_type)
        .to_ascii_uppercase();

    div()
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .left_0()
        .occlude()
        .bg(gpui::rgba(0x0807_06f2))
        .p(px(28.0))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .id("pasted-image-viewer")
                .w_full()
                .h_full()
                .max_w(px(1040.0))
                .max_h(px(720.0))
                .min_h_0()
                .flex()
                .flex_col()
                .rounded(px(theme::RADIUS_SM))
                .bg(theme::panel())
                .overflow_hidden()
                .child(
                    div()
                        .h(px(48.0))
                        .px(px(14.0))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .font_family(theme::SANS)
                                .text_size(px(theme::T_UI_SM))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::bone_dim())
                                .child(if count > 1 {
                                    format!("Image {} of {} · {format}", index + 1, count)
                                } else {
                                    format!("Image 1 · {format}")
                                }),
                        )
                        .child(
                            div()
                                .id("pasted-image-close")
                                .tab_index(0)
                                .cursor_pointer()
                                .px(px(8.0))
                                .py(px(5.0))
                                .text_color(theme::bone_dim())
                                .hover(|button| {
                                    button.bg(theme::panel_lift()).text_color(theme::bone())
                                })
                                .focus(|button| button.text_color(theme::focus()))
                                .on_key_down(cx.listener(
                                    |view, event: &gpui::KeyDownEvent, window, cx| {
                                        if matches!(event.keystroke.key.as_str(), "enter" | "space")
                                        {
                                            cx.stop_propagation();
                                            view.close_pasted_image(window, cx);
                                        }
                                    },
                                ))
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.close_pasted_image(window, cx)
                                }))
                                .child(
                                    div()
                                        .font_family(theme::CONTROL)
                                        .text_size(px(theme::T_UI_SM))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Close · Esc"),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .mx(px(14.0))
                        .bg(theme::canvas())
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when_some(image, |frame, source| {
                            frame.child(img(source).size_full().object_fit(ObjectFit::ScaleDown))
                        })
                        .when(image_missing, |frame| {
                            frame.child(
                                div()
                                    .px(px(24.0))
                                    .text_size(px(theme::T_BODY))
                                    .text_color(theme::error())
                                    .child("This pasted image could not be decoded."),
                            )
                        }),
                )
                .child(
                    div()
                        .h(px(54.0))
                        .px(px(14.0))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(count > 1, |footer| {
                            footer.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(14.0))
                                    .child(
                                        div()
                                            .id("pasted-image-previous")
                                            .tab_index(0)
                                            .cursor_pointer()
                                            .w(px(44.0))
                                            .h(px(32.0))
                                            .rounded(px(theme::RADIUS_SM))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .bg(theme::panel_lift())
                                            .font_family(theme::SANS)
                                            .text_size(px(theme::T_BODY))
                                            .text_color(theme::bone())
                                            .hover(|button| button.bg(theme::panel_hover()))
                                            .focus(|button| {
                                                button.border_1().border_color(theme::focus())
                                            })
                                            .on_key_down(cx.listener(
                                                |view, event: &gpui::KeyDownEvent, _, cx| {
                                                    if matches!(
                                                        event.keystroke.key.as_str(),
                                                        "enter" | "space"
                                                    ) {
                                                        cx.stop_propagation();
                                                        view.move_pasted_image(-1, cx);
                                                    }
                                                },
                                            ))
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.move_pasted_image(-1, cx)
                                            }))
                                            .child("←"),
                                    )
                                    .child(
                                        div()
                                            .font_family(theme::MONO)
                                            .text_size(px(theme::T_TINY))
                                            .text_color(theme::smoke())
                                            .child(format!("{} / {}", index + 1, count)),
                                    )
                                    .child(
                                        div()
                                            .id("pasted-image-next")
                                            .tab_index(0)
                                            .cursor_pointer()
                                            .w(px(44.0))
                                            .h(px(32.0))
                                            .rounded(px(theme::RADIUS_SM))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .bg(theme::panel_lift())
                                            .font_family(theme::SANS)
                                            .text_size(px(theme::T_BODY))
                                            .text_color(theme::bone())
                                            .hover(|button| button.bg(theme::panel_hover()))
                                            .focus(|button| {
                                                button.border_1().border_color(theme::focus())
                                            })
                                            .on_key_down(cx.listener(
                                                |view, event: &gpui::KeyDownEvent, _, cx| {
                                                    if matches!(
                                                        event.keystroke.key.as_str(),
                                                        "enter" | "space"
                                                    ) {
                                                        cx.stop_propagation();
                                                        view.move_pasted_image(1, cx);
                                                    }
                                                },
                                            ))
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.move_pasted_image(1, cx)
                                            }))
                                            .child("→"),
                                    ),
                            )
                        }),
                ),
        )
        .into_any_element()
}

pub(super) fn compaction_dialog(
    composer: &Entity<Composer>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left_0()
        .right_0()
        .occlude()
        .bg(theme::canvas())
        .flex()
        .items_center()
        .justify_center()
        .child(
            popup_sheet()
                .w(px(560.0))
                .child(
                    div()
                        .px(px(14.0))
                        .py(px(12.0))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(12.0))
                        .child(
                            div()
                                .font_family(theme::SANS)
                                .font_weight(FontWeight::BOLD)
                                .text_size(px(theme::T_BODY))
                                .text_color(theme::bone())
                                .child("Compact context"),
                        )
                        .child(controls::chrome_action(
                            "close-compaction-dialog",
                            "Cancel · Esc",
                            composer.read(cx).availability()
                                != ComposerAvailability::Unavailable,
                            Box::new(cx.listener(|view, _, window, cx| {
                                view.close_compaction_modal(window, cx)
                            })),
                        )),
                )
                .child(
                    div()
                        .px(px(14.0))
                        .pb(px(10.0))
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .line_height(gpui::relative(1.4))
                        .text_color(theme::smoke())
                        .child(
                            "Optionally tell Pi what the compacted summary should preserve. Leave it blank to compact normally.",
                        ),
                )
                .child(div().px(px(14.0)).pb(px(14.0)).child(composer.clone())),
        )
}

pub(super) fn command_palette_overlay(
    matches: &[CommandEntry],
    search: &Entity<Composer>,
    selected: usize,
    scroll: &ScrollHandle,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let mut rows = Vec::new();
    let mut previous_group = None;
    for (index, entry) in matches.iter().take(60).enumerate() {
        if previous_group != Some(entry.group) {
            previous_group = Some(entry.group);
            rows.push(
                div()
                    .px(px(10.0))
                    .pt(px(10.0))
                    .pb(px(5.0))
                    .font_family(theme::SANS)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(theme::T_TINY))
                    .text_color(theme::data())
                    .child(entry.group.label())
                    .into_any_element(),
            );
        }
        rows.push(command_row(entry, index == selected, cx).into_any_element());
    }

    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left_0()
        .right_0()
        .occlude()
        .bg(theme::canvas())
        .pt(px(76.0))
        .items_center()
        .child(
            div()
                .w(px(760.0))
                .h_full()
                .max_h(px(620.0))
                .flex()
                .flex_col()
                .rounded(px(theme::RADIUS_SM))
                .border_1()
                .border_color(theme::edge_hard())
                .bg(theme::panel())
                .overflow_hidden()
                .child(
                    div()
                        .px(px(12.0))
                        .py(px(10.0))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .font_family(theme::SANS)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(px(theme::T_BODY))
                                .child("Command palette"),
                        )
                        .child(controls::chrome_action(
                            "close-command-palette",
                            "Esc",
                            true,
                            Box::new(cx.listener(|view, _, window, cx| {
                                view.close_command_palette(window, cx)
                            })),
                        )),
                )
                .child(search.clone())
                .child(
                    div()
                        .id("command-palette-results")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(scroll)
                        .scrollbar_width(px(theme::SCROLLBAR))
                        .children(rows)
                        .when(matches.is_empty(), |list| {
                            list.child(
                                div()
                                    .p(px(18.0))
                                    .text_size(px(theme::T_BODY))
                                    .text_color(theme::smoke())
                                    .child("No matching commands."),
                            )
                        }),
                ),
        )
}

pub(super) fn hotkey_help_overlay(cx: &mut Context<RootView>) -> impl IntoElement {
    let shortcuts = [
        ("Command palette", "Ctrl+Shift+P"),
        ("Hotkey help", "Ctrl+/"),
        ("Send / steer", "Enter"),
        ("Queue follow-up", "Alt+Enter"),
        ("Insert newline", "Shift+Enter"),
        ("Abort run or Bash", "Esc"),
        ("Move focus", "Tab / Shift+Tab"),
        ("Copy transcript selection", "Ctrl+C"),
        ("History navigation", "↑ ↓ ← → Home End"),
    ];
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left_0()
        .right_0()
        .occlude()
        .bg(theme::canvas())
        .pt(px(96.0))
        .items_center()
        .child(
            popup_sheet()
                .w(px(560.0))
                .child(
                    div()
                        .px(px(12.0))
                        .py(px(10.0))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(px(theme::T_BODY))
                                .child("Native hotkeys"),
                        )
                        .child(controls::chrome_action(
                            "close-hotkey-help",
                            "Close",
                            true,
                            Box::new(cx.listener(|view, _, window, cx| {
                                view.hotkey_help_open = false;
                                window.focus(&view.composer.read(cx).focus_handle(cx));
                                cx.notify();
                            })),
                        )),
                )
                .children(shortcuts.into_iter().map(|(label, keys)| {
                    div()
                        .px(px(12.0))
                        .py(px(8.0))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .border_t_1()
                        .border_color(theme::edge_soft())
                        .child(
                            div()
                                .text_size(px(theme::T_BODY))
                                .text_color(theme::bone())
                                .child(label),
                        )
                        .child(
                            div()
                                .font_family(theme::MONO)
                                .text_size(px(theme::T_UI_SM))
                                .text_color(theme::data())
                                .child(keys),
                        )
                })),
        )
}
