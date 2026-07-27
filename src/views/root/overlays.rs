use gpui::{AnyElement, StatefulInteractiveElement, relative};

use super::shared::{plural, popup_sheet};
use super::*;

const CONVERSATION_SCROLLBAR_MIN_THUMB: f32 = 28.0;

#[derive(Debug, Clone, Copy, PartialEq)]
struct ConversationScrollbarGeometry {
    thumb_top: Pixels,
    thumb_height: Pixels,
    travel: Pixels,
    max_offset: Pixels,
}

impl ConversationScrollbarGeometry {
    fn new(track_height: Pixels, max_offset: Pixels, current_offset: Pixels) -> Option<Self> {
        if track_height <= Pixels::ZERO || max_offset <= Pixels::ZERO {
            return None;
        }

        let content_height = track_height + max_offset;
        let thumb_height = (track_height * (track_height / content_height))
            .max(px(CONVERSATION_SCROLLBAR_MIN_THUMB))
            .min(track_height);
        let travel = track_height - thumb_height;
        let thumb_top = travel * (current_offset / max_offset).clamp(0.0, 1.0);

        Some(Self {
            thumb_top,
            thumb_height,
            travel,
            max_offset,
        })
    }

    fn offset_for_pointer(self, pointer_y: Pixels, drag_offset: Pixels) -> Pixels {
        if self.travel <= Pixels::ZERO {
            return Pixels::ZERO;
        }
        let thumb_top = (pointer_y - drag_offset).clamp(Pixels::ZERO, self.travel);
        self.max_offset * (thumb_top / self.travel)
    }
}

fn conversation_scrollbar_geometry(
    state: &ListState,
    track_height: Pixels,
) -> Option<ConversationScrollbarGeometry> {
    ConversationScrollbarGeometry::new(
        track_height,
        state.max_offset_for_scrollbar().height,
        -state.scroll_px_offset_for_scrollbar().y,
    )
}

fn scroll_conversation_from_scrollbar(
    view: &mut RootView,
    state: &ListState,
    geometry: ConversationScrollbarGeometry,
    pointer_y: Pixels,
    drag_offset: Pixels,
    cx: &mut Context<RootView>,
) {
    let offset = geometry.offset_for_pointer(pointer_y, drag_offset);
    state.set_offset_from_scrollbar(point(Pixels::ZERO, -offset));
    view.conversation_follow.set(offset >= geometry.max_offset);
    view.conversation_scroll_motion.cancel();
    cx.notify();
}

#[cfg(test)]
mod conversation_scrollbar_tests {
    use super::*;

    #[test]
    fn geometry_tracks_scroll_range() {
        let geometry = ConversationScrollbarGeometry::new(px(200.0), px(600.0), px(300.0)).unwrap();

        assert_eq!(geometry.thumb_height, px(50.0));
        assert_eq!(geometry.thumb_top, px(75.0));
        assert_eq!(geometry.offset_for_pointer(px(100.0), px(25.0)), px(300.0));
    }

    #[test]
    fn hidden_without_overflow() {
        assert_eq!(
            ConversationScrollbarGeometry::new(px(200.0), Pixels::ZERO, Pixels::ZERO),
            None
        );
    }
}

#[cfg(test)]
mod extension_dialog_layout_tests {
    use super::*;

    #[test]
    fn split_dialog_title_keeps_question_and_scrolls_detail() {
        let (headline, detail) = split_dialog_title(
            "[UI] Which layout?\n\n--- 1. Compact preview ---\n```rs\nfn main() {}\n```\n\nType numbers",
        );
        assert_eq!(headline, "[UI] Which layout?");
        assert!(detail.as_deref().is_some_and(|body| {
            body.contains("Compact preview") && body.contains("Type numbers")
        }));
    }

    #[test]
    fn split_dialog_title_handles_single_line_and_empty() {
        assert_eq!(
            split_dialog_title("Pick one?"),
            ("Pick one?".to_owned(), None)
        );
        assert_eq!(split_dialog_title("   "), (String::new(), None));
        let (headline, detail) = split_dialog_title("Line one\nline two");
        assert_eq!(headline, "Line one");
        assert_eq!(detail.as_deref(), Some("line two"));
    }

    #[test]
    fn split_select_option_separates_label_and_description() {
        assert_eq!(
            split_select_option("1. Compact — Fewer chrome rows"),
            (
                "1. Compact".to_owned(),
                Some("Fewer chrome rows".to_owned())
            )
        );
        assert_eq!(
            split_select_option("2. Type something."),
            ("2. Type something.".to_owned(), None)
        );
        assert_eq!(
            split_select_option("3. Nested - with hyphen still splits"),
            (
                "3. Nested".to_owned(),
                Some("with hyphen still splits".to_owned())
            )
        );
    }

    #[test]
    fn keyboard_hint_mentions_digit_range_for_select() {
        assert!(extension_dialog_keyboard_hint("select", 4).contains("1–4"));
        assert!(extension_dialog_keyboard_hint("select", 12).contains("1–9"));
        assert!(!extension_dialog_keyboard_hint("input", 0).contains("shortcut"));
    }
}

pub(super) struct ConversationAreaParams {
    pub(super) projection: Arc<ConversationProjection>,
    pub(super) list: Arc<ConversationListModel>,
    pub(super) list_state: ListState,
    pub(super) transcript_cache: Entity<TranscriptTextCache>,
    pub(super) activity_disclosures: Entity<ActivityDisclosureState>,
    pub(super) workspace_diff: Option<Arc<WorkspaceDiff>>,
    pub(super) workspace_diff_files_expanded: bool,
    pub(super) root: Entity<RootView>,
}

pub(super) fn conversation_area(params: ConversationAreaParams) -> impl IntoElement {
    let ConversationAreaParams {
        projection,
        list: conversation_list,
        list_state: conversation_list_state,
        transcript_cache,
        activity_disclosures,
        workspace_diff,
        workspace_diff_files_expanded,
        root,
    } = params;
    let wheel_root = root.clone();
    let scrollbar_state = conversation_list_state.clone();
    let diff_summary = ConversationDiffSummary {
        snapshot: workspace_diff,
        files_expanded: workspace_diff_files_expanded,
        root: root.clone(),
    };

    // The transcript shares the center column with the composer and must yield
    // height to it on every lifecycle-driven rerender.
    div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(theme::canvas())
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
                        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                        move |_, hitbox, window, _| {
                            window.on_mouse_event(
                                move |event: &ScrollWheelEvent, phase, window, cx| {
                                    if phase != DispatchPhase::Capture
                                        || !hitbox.should_handle_scroll(window)
                                    {
                                        return;
                                    }
                                    let handled = wheel_root.update(cx, |view, cx| {
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
                            &projection,
                            &transcript_cache,
                            &activity_disclosures,
                            &diff_summary,
                            cx,
                        )
                    })
                    .size_full()
                    .min_w_0()
                    .pt(px(16.0))
                    .pb(px(16.0)),
                )
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            let Some(geometry) = conversation_scrollbar_geometry(
                                &scrollbar_state,
                                bounds.size.height,
                            ) else {
                                return;
                            };

                            let track = Bounds::new(
                                point(bounds.right() - px(2.0), bounds.top()),
                                size(px(2.0), bounds.size.height),
                            );
                            let thumb = Bounds::new(
                                point(bounds.right() - px(6.0), bounds.top() + geometry.thumb_top),
                                size(px(5.0), geometry.thumb_height),
                            );
                            window.paint_quad(fill(track, theme::edge_soft()));
                            window.paint_quad(fill(thumb, theme::edge_hard()));

                            let mouse_down_root = root.clone();
                            let mouse_down_state = scrollbar_state.clone();
                            window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                                if phase != DispatchPhase::Capture
                                    || event.button != MouseButton::Left
                                    || !bounds.contains(&event.position)
                                {
                                    return;
                                }

                                let drag_offset = if thumb.contains(&event.position) {
                                    event.position.y - thumb.top()
                                } else {
                                    geometry.thumb_height / 2.0
                                };
                                mouse_down_state.scrollbar_drag_started();
                                mouse_down_root.update(cx, |view, cx| {
                                    view.conversation_scrollbar_drag_offset = Some(drag_offset);
                                    scroll_conversation_from_scrollbar(
                                        view,
                                        &mouse_down_state,
                                        geometry,
                                        event.position.y - bounds.top(),
                                        drag_offset,
                                        cx,
                                    );
                                });
                                cx.stop_propagation();
                            });

                            let mouse_move_root = root.clone();
                            let mouse_move_state = scrollbar_state.clone();
                            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                                if phase != DispatchPhase::Capture {
                                    return;
                                }
                                let handled = mouse_move_root.update(cx, |view, cx| {
                                    let Some(drag_offset) = view.conversation_scrollbar_drag_offset
                                    else {
                                        return false;
                                    };
                                    scroll_conversation_from_scrollbar(
                                        view,
                                        &mouse_move_state,
                                        geometry,
                                        event.position.y - bounds.top(),
                                        drag_offset,
                                        cx,
                                    );
                                    true
                                });
                                if handled {
                                    cx.stop_propagation();
                                }
                            });

                            let mouse_up_root = root.clone();
                            let mouse_up_state = scrollbar_state.clone();
                            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                                if phase != DispatchPhase::Capture
                                    || event.button != MouseButton::Left
                                {
                                    return;
                                }
                                let handled = mouse_up_root.update(cx, |view, cx| {
                                    if view.conversation_scrollbar_drag_offset.take().is_none() {
                                        return false;
                                    }
                                    mouse_up_state.scrollbar_drag_ended();
                                    cx.notify();
                                    true
                                });
                                if handled {
                                    cx.stop_propagation();
                                }
                            });
                        },
                    )
                    .absolute()
                    .right_0()
                    .top_0()
                    .bottom_0()
                    .w(px(theme::SCROLLBAR + 4.0)),
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
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_TINY))
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

pub(super) fn file_suggestion_sheet(
    entries: &[FileMatch],
    selected: usize,
    scroll: &ScrollHandle,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let rows = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| file_row(entry, index, index == selected, cx))
        .collect::<Vec<_>>();
    popup_sheet()
        .max_w(px(560.0))
        .max_h(px(220.0))
        .child(
            div()
                .px(px(8.0))
                .py(px(5.0))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(theme::edge_soft())
                .child(controls::section_label("Files"))
                .child(
                    div()
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child("↑↓ · Enter · Esc"),
                ),
        )
        .child(
            div()
                .id("file-completion-results")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .scrollbar_width(px(theme::SCROLLBAR))
                .children(rows),
        )
}

fn file_row(
    entry: &FileMatch,
    index: usize,
    selected: bool,
    cx: &mut Context<RootView>,
) -> gpui::AnyElement {
    let path = entry.path.clone();
    div()
        .id(gpui::SharedString::from(format!("file-row-{index}-{path}")))
        .px(px(8.0))
        .py(px(4.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .border_b_1()
        .border_color(theme::edge_soft())
        .when(selected, |row| row.bg(theme::panel_hover()))
        .cursor_pointer()
        .hover(|row| row.bg(theme::panel_hover()))
        .on_click(cx.listener(move |view, _, window, cx| view.choose_file_match(index, window, cx)))
        .child(
            div()
                .w(px(2.0))
                .h(px(12.0))
                .rounded(px(1.0))
                .flex_shrink_0()
                .bg(if selected {
                    theme::signal()
                } else {
                    theme::edge_soft()
                }),
        )
        .child(
            div()
                .w(px(10.0))
                .flex_shrink_0()
                .font_family(theme::mono())
                .text_size(theme::text_size(theme::T_TINY))
                .text_color(if entry.is_directory {
                    theme::data()
                } else {
                    theme::smoke()
                })
                .child(if entry.is_directory { "/" } else { "·" }),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .font_family(theme::mono())
                .text_size(theme::text_size(theme::T_TINY))
                .text_color(if selected {
                    theme::bone()
                } else {
                    theme::bone_dim()
                })
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(path),
        )
        .into_any_element()
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
                .font_family(theme::mono())
                .text_size(theme::text_size(theme::T_TINY))
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
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_BODY))
                        .text_color(theme::bone())
                        .child(format!("/{}{}", entry.name, hint)),
                )
                .child(
                    div()
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_TINY))
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
                .font_family(theme::mono())
                .text_size(theme::text_size(theme::T_TINY))
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
                                .font_family(theme::mono())
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(theme::text_size(theme::T_TINY))
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
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_UI_SM))
                        .line_height(theme::text_size(18.0))
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
                                .font_family(theme::mono())
                                .text_size(theme::text_size(theme::T_TINY))
                                .text_color(theme::data())
                                .child(sanitize_untrusted_text(key)),
                        )
                        .children(widget.lines.iter().map(|line| {
                            div()
                                .font_family(theme::mono())
                                .text_size(theme::text_size(theme::T_MONO_SM))
                                .line_height(theme::text_size(17.0))
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
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_TINY))
                        .text_color(theme::data())
                        .child(sanitize_untrusted_text(key)),
                )
                .child(
                    div()
                        .min_w_0()
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_TINY))
                        .text_color(theme::ash())
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(status.text.clone()),
                )
        }))
}

pub(super) fn activity_detail_overlay(
    detail: &ActivityDetail,
    focus: &FocusHandle,
    scroll: &ScrollHandle,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let record_count = detail.records.len();
    let records = detail
        .records
        .iter()
        .enumerate()
        .map(|(index, record)| activity_detail_record(record, index, record_count))
        .collect::<Vec<_>>();

    div()
        .id("activity-detail-overlay")
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .left_0()
        .occlude()
        .bg(gpui::rgba(0x0b0a_09e6))
        .p(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .key_context("ActivityDetail")
        .on_key_down(cx.listener(RootView::on_activity_detail_key_down))
        .child(
            div()
                .id("activity-detail-dialog")
                .w_full()
                .h_full()
                .max_w(px(980.0))
                .max_h(px(760.0))
                .rounded(px(theme::RADIUS))
                .overflow_hidden()
                .border_1()
                .border_color(theme::edge_hard())
                .bg(theme::floor())
                .flex()
                .flex_col()
                .child(
                    div()
                        .min_h(px(52.0))
                        .px(px(16.0))
                        .py(px(9.0))
                        .bg(theme::panel())
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(16.0))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .child(
                                    div()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .font_family(theme::sans())
                                        .text_size(theme::text_size(theme::T_BODY))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme::bone())
                                        .child(detail.title.clone()),
                                )
                                .child(
                                    div()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .font_family(theme::sans())
                                        .text_size(theme::text_size(theme::T_TINY))
                                        .text_color(theme::smoke())
                                        .child(format!(
                                            "Prompt, request parameters, metadata, and {}",
                                            if record_count == 1 {
                                                "result"
                                            } else {
                                                "results"
                                            }
                                        )),
                                ),
                        )
                        .child(activity_detail_close_button(focus, cx)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .relative()
                        .child(controls::scroll_wheel_capture(scroll))
                        .child(
                            div()
                                .id("activity-detail-scroll")
                                .size_full()
                                .overflow_y_scroll()
                                .track_scroll(scroll)
                                .scrollbar_width(px(theme::SCROLLBAR))
                                .p(px(16.0))
                                .flex()
                                .flex_col()
                                .gap(px(16.0))
                                .when_some(detail.prompt.clone(), |body, prompt| {
                                    body.child(activity_detail_section("Prompt", prompt, false))
                                })
                                .children(records),
                        ),
                ),
        )
}

fn activity_detail_close_button(focus: &FocusHandle, cx: &mut Context<RootView>) -> AnyElement {
    div()
        .id("close-activity-detail")
        .track_focus(focus)
        .h(px(28.0))
        .px(px(8.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_SM))
        .tab_index(0)
        .cursor_pointer()
        .whitespace_nowrap()
        .font_family(theme::main())
        .text_size(theme::text_size(theme::T_TINY))
        .font_weight(FontWeight::SEMIBOLD)
        .bg(theme::canvas())
        .text_color(theme::ash())
        .hover(|button| button.bg(theme::panel_hover()).text_color(theme::bone()))
        .active(|button| button.bg(theme::panel_lift()))
        .focus(|button| button.bg(theme::panel_hover()).text_color(theme::focus()))
        .on_click(cx.listener(|view, _, window, cx| view.close_activity_detail(window, cx)))
        .on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                view.close_activity_detail(window, cx);
            }
        }))
        .child("Close · Esc")
        .into_any_element()
}

fn activity_detail_record(
    record: &crate::views::conversation::ActivityDetailRecord,
    index: usize,
    record_count: usize,
) -> AnyElement {
    div()
        .w_full()
        .p(px(14.0))
        .rounded(px(theme::RADIUS_SM))
        .bg(theme::panel())
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .gap(px(12.0))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_UI))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::bone_dim())
                        .child(if record_count == 1 {
                            record.label.clone()
                        } else {
                            format!("{:02} · {}", index + 1, record.label)
                        }),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .whitespace_nowrap()
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_TINY))
                        .text_color(theme::ash())
                        .child(record.kind),
                ),
        )
        .when_some(record.parameters.clone(), |body, parameters| {
            body.child(activity_detail_section("Parameters", parameters, true))
        })
        .child(activity_detail_section(
            "Result",
            record.result.clone(),
            true,
        ))
        .when(!record.metadata.is_empty(), |body| {
            body.child(activity_detail_metadata(&record.metadata))
        })
        .into_any_element()
}

fn activity_detail_section(label: &'static str, text: String, mono: bool) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .child(
            div()
                .font_family(theme::sans())
                .text_size(theme::text_size(theme::T_TINY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::ash())
                .child(label),
        )
        .child(
            div()
                .w_full()
                .min_w_0()
                .p(px(10.0))
                .rounded(px(theme::RADIUS_SM))
                .bg(theme::canvas())
                .font_family(if mono { theme::mono() } else { theme::sans() })
                .text_size(theme::text_size(if mono {
                    theme::T_MONO
                } else {
                    theme::T_UI_SM
                }))
                .line_height(relative(1.5))
                .text_color(theme::bone_dim())
                .child(sanitize_untrusted_text(&text)),
        )
        .into_any_element()
}

fn activity_detail_metadata(rows: &[(String, String)]) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .child(
            div()
                .font_family(theme::sans())
                .text_size(theme::text_size(theme::T_TINY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::ash())
                .child("Metadata"),
        )
        .child(
            div()
                .w_full()
                .p(px(10.0))
                .rounded(px(theme::RADIUS_SM))
                .bg(theme::canvas())
                .flex()
                .flex_col()
                .gap(px(6.0))
                .children(rows.iter().map(|(label, value)| {
                    div()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .font_family(theme::sans())
                                .text_size(theme::text_size(theme::T_TINY))
                                .text_color(theme::smoke())
                                .child(sanitize_untrusted_text(label)),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .font_family(theme::mono())
                                .text_size(theme::text_size(theme::T_MONO_SM))
                                .text_color(theme::bone_dim())
                                .child(sanitize_untrusted_text(value)),
                        )
                })),
        )
        .into_any_element()
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
    let (headline, detail) = split_dialog_title(dialog.title());
    let request = dialog.request.clone();
    let kind = dialog.kind();
    let keyboard_hint = extension_dialog_keyboard_hint(
        kind,
        match &request {
            DialogRequest::Select { options, .. } => options.len(),
            DialogRequest::Confirm { .. } => 2,
            _ => 0,
        },
    );
    let body = match &request {
        DialogRequest::Select { options, .. } => div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .when(options.is_empty(), |list| {
                list.child(
                    div()
                        .text_size(theme::text_size(theme::T_UI_SM))
                        .text_color(theme::error())
                        .child("This request has no selectable options."),
                )
            })
            .children(options.iter().enumerate().map(|(index, option)| {
                let option_answer = option.clone();
                let (label, description) = split_select_option(option);
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
                            .flex()
                            .flex_col()
                            .gap(px(3.0))
                            .min_w_0()
                            .child(
                                div()
                                    .font_family(theme::sans())
                                    .text_size(theme::text_size(theme::T_UI))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::bone())
                                    .child(label),
                            )
                            .when_some(description, |row, description| {
                                row.child(
                                    div()
                                        .font_family(theme::sans())
                                        .text_size(theme::text_size(theme::T_UI_SM))
                                        .line_height(theme::text_size(18.0))
                                        .text_color(theme::bone_dim())
                                        .child(description),
                                )
                            }),
                    )
            }))
            .into_any_element(),
        DialogRequest::Confirm { message, .. } => div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .font_family(theme::sans())
                    .text_size(theme::text_size(theme::T_BODY_SM))
                    .line_height(theme::text_size(21.0))
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
        DialogRequest::Input { .. } => div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(input.clone())
            .child(
                div()
                    .font_family(theme::mono())
                    .text_size(theme::text_size(theme::T_TINY))
                    .text_color(theme::smoke())
                    .child("Enter submits · Esc cancels · empty is allowed"),
            )
            .into_any_element(),
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
                .max_w(px(720.0))
                .max_h(px(720.0))
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
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(4.0))
                                .child(
                                    div()
                                        .font_family(theme::mono())
                                        .text_size(theme::text_size(theme::T_TINY))
                                        .text_color(theme::focus())
                                        .child(format!("Extension {kind} · untrusted UI")),
                                )
                                .child(
                                    div()
                                        .font_family(theme::sans())
                                        .text_size(theme::text_size(theme::T_TITLE))
                                        .font_weight(FontWeight::BOLD)
                                        .line_height(theme::text_size(26.0))
                                        .text_color(theme::bone())
                                        .child(headline),
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
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_UI_SM))
                        .line_height(theme::text_size(18.0))
                        .text_color(theme::smoke())
                        .child(
                            "This content comes from an extension. It is not a secure permission prompt and has no verified provenance.",
                        ),
                )
                .when_some(detail, |card, detail| {
                    card.child(
                        div()
                            .id("extension-dialog-detail")
                            .w_full()
                            .max_h(px(220.0))
                            .overflow_y_scroll()
                            .scrollbar_width(px(theme::SCROLLBAR))
                            .px(px(12.0))
                            .py(px(10.0))
                            .rounded(px(theme::RADIUS_SM))
                            .bg(theme::canvas())
                            .border_1()
                            .border_color(theme::edge_soft())
                            .font_family(theme::mono())
                            .text_size(theme::text_size(theme::T_UI_SM))
                            .line_height(theme::text_size(18.0))
                            .text_color(theme::bone_dim())
                            .child(detail),
                    )
                })
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
                                .font_family(theme::mono())
                                .text_size(theme::text_size(theme::T_TINY))
                                .text_color(theme::smoke())
                                .child(match queued_dialogs {
                                    0 => keyboard_hint,
                                    count => format!(
                                        "{keyboard_hint} · {count} queued dialog{}",
                                        plural(count as u64)
                                    ),
                                }),
                        )
                        .when_some(deadline_copy, |row, deadline| {
                            row.child(
                                div()
                                    .font_family(theme::mono())
                                    .text_size(theme::text_size(theme::T_TINY))
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
        .hover(|button| {
            button
                .bg(theme::panel_lift())
                .border_color(theme::edge_hard())
        })
        .on_click(move |event, window, cx| on_click(event, window, cx))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .font_family(theme::main())
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(theme::text_size(theme::T_UI_SM))
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

/// Split an extension dialog title into a short headline and optional detail body.
///
/// `ask_user_question`'s RPC walker folds previews, option lists, and instructions into
/// the stock `select`/`input` title with `\n\n` separators. Keep the first block as the
/// title and scroll the remainder so long multi-select prompts stay usable.
pub(super) fn split_dialog_title(title: &str) -> (String, Option<String>) {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return (String::new(), None);
    }
    let (head, rest) = match trimmed.split_once("\n\n") {
        Some((head, rest)) => (head.trim(), rest.trim()),
        None => {
            // Single newlines still appear in free-form titles; first line is the headline.
            match trimmed.split_once('\n') {
                Some((head, rest)) if !rest.trim().is_empty() => (head.trim(), rest.trim()),
                _ => (trimmed, ""),
            }
        }
    };
    let headline = if head.is_empty() {
        single_line_title(trimmed)
    } else {
        head.to_owned()
    };
    let detail = (!rest.is_empty()).then(|| rest.to_owned());
    (headline, detail)
}

/// Split `N. Label — description` option lines for readable select rows.
///
/// The full original string remains the answer value; this only affects display.
pub(super) fn split_select_option(option: &str) -> (String, Option<String>) {
    let trimmed = option.trim();
    if trimmed.is_empty() {
        return (String::new(), None);
    }
    for separator in [" — ", " – ", " - "] {
        if let Some((label, description)) = trimmed.split_once(separator) {
            let label = label.trim();
            let description = description.trim();
            if !label.is_empty() && !description.is_empty() {
                return (label.to_owned(), Some(description.to_owned()));
            }
        }
    }
    (trimmed.to_owned(), None)
}

fn extension_dialog_keyboard_hint(kind: &str, option_count: usize) -> String {
    match kind {
        "select" if option_count > 0 => {
            let max_digit = option_count.min(9);
            if max_digit == 1 {
                "↑↓ move · Enter choose · 1 shortcut · Esc cancel".to_owned()
            } else {
                format!("↑↓ move · Enter choose · 1–{max_digit} shortcut · Esc cancel")
            }
        }
        "confirm" => "←→ choose · Enter confirm · Esc cancel".to_owned(),
        "input" => "Enter submit · Esc cancel".to_owned(),
        "editor" => "Enter submit · Esc cancel".to_owned(),
        _ => "Esc cancel".to_owned(),
    }
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
        digit
            if matches!(kind, Some("select"))
                && digit.len() == 1
                && digit.as_bytes()[0].is_ascii_digit()
                && digit != "0" =>
        {
            let index = (digit.as_bytes()[0] - b'1') as usize;
            Some(ExtensionDialogKey::SelectIndex(index))
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

#[derive(Debug, Clone, Copy, PartialEq)]
struct PastedImageLayout {
    bounds: Bounds<Pixels>,
    image_width: u32,
    image_height: u32,
    scale: f32,
}

impl PastedImageLayout {
    fn new(frame: Bounds<Pixels>, image_width: u32, image_height: u32) -> Option<Self> {
        if image_width == 0
            || image_height == 0
            || frame.size.width <= Pixels::ZERO
            || frame.size.height <= Pixels::ZERO
        {
            return None;
        }
        let scale = (f32::from(frame.size.width) / image_width as f32)
            .min(f32::from(frame.size.height) / image_height as f32)
            .min(1.0);
        let drawn_width = px(image_width as f32 * scale);
        let drawn_height = px(image_height as f32 * scale);
        Some(Self {
            bounds: Bounds::new(
                point(
                    frame.left() + (frame.size.width - drawn_width) / 2.0,
                    frame.top() + (frame.size.height - drawn_height) / 2.0,
                ),
                size(drawn_width, drawn_height),
            ),
            image_width,
            image_height,
            scale,
        })
    }

    fn image_point(self, position: gpui::Point<Pixels>, clamp: bool) -> Option<ImagePoint> {
        if !clamp && !self.bounds.contains(&position) {
            return None;
        }
        let x = (f32::from(position.x - self.bounds.left()) / self.scale)
            .clamp(0.0, self.image_width.saturating_sub(1) as f32);
        let y = (f32::from(position.y - self.bounds.top()) / self.scale)
            .clamp(0.0, self.image_height.saturating_sub(1) as f32);
        Some(ImagePoint { x, y })
    }

    fn screen_point(self, image: ImagePoint) -> gpui::Point<Pixels> {
        point(
            self.bounds.left() + px(image.x * self.scale),
            self.bounds.top() + px(image.y * self.scale),
        )
    }
}

fn decoded_pasted_image(image: &PromptImage) -> Option<(Vec<u8>, u32, u32)> {
    let bytes = STANDARD.decode(&image.data).ok()?;
    let decoded = image::load_from_memory(&bytes).ok()?;
    Some((bytes, decoded.width(), decoded.height()))
}

fn pasted_image_source(image: &PromptImage) -> Option<(Arc<Image>, u32, u32)> {
    let format = ImageFormat::from_mime_type(&image.mime_type)?;
    let (bytes, width, height) = decoded_pasted_image(image)?;
    (!bytes.is_empty()).then(|| (Arc::new(Image::from_bytes(format, bytes)), width, height))
}

fn draw_brush_disc(
    pixels: &mut image::RgbaImage,
    center: ImagePoint,
    diameter: u16,
    color: [u8; 4],
) {
    let radius = diameter as f32 / 2.0;
    let radius_squared = radius * radius;
    let min_x = (center.x - radius).floor().max(0.0) as u32;
    let max_x = (center.x + radius).ceil().min(pixels.width() as f32 - 1.0) as u32;
    let min_y = (center.y - radius).floor().max(0.0) as u32;
    let max_y = (center.y + radius).ceil().min(pixels.height() as f32 - 1.0) as u32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x as f32 + 0.5 - center.x;
            let dy = y as f32 + 0.5 - center.y;
            if dx * dx + dy * dy <= radius_squared {
                pixels.put_pixel(x, y, image::Rgba(color));
            }
        }
    }
}

fn draw_brush_segment(
    pixels: &mut image::RgbaImage,
    from: ImagePoint,
    to: ImagePoint,
    diameter: u16,
    color: [u8; 4],
) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let distance = (dx * dx + dy * dy).sqrt();
    let spacing = (diameter as f32 / 4.0).max(0.5);
    let steps = (distance / spacing).ceil().max(1.0) as usize;
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        draw_brush_disc(
            pixels,
            ImagePoint {
                x: from.x + dx * t,
                y: from.y + dy * t,
            },
            diameter,
            color,
        );
    }
}

pub(super) fn annotate_prompt_image(
    image: &PromptImage,
    stroke: &PencilStroke,
) -> Result<PromptImage, String> {
    let bytes = STANDARD
        .decode(&image.data)
        .map_err(|_| "The image data could not be decoded.".to_owned())?;
    let mut pixels = image::load_from_memory(&bytes)
        .map_err(|_| "The image could not be opened for drawing.".to_owned())?
        .to_rgba8();
    let color = stroke.color.rgba8();
    if let Some(first) = stroke.points.first().copied() {
        draw_brush_disc(&mut pixels, first, stroke.size, color);
        for pair in stroke.points.windows(2) {
            draw_brush_segment(&mut pixels, pair[0], pair[1], stroke.size, color);
        }
    }

    let (format, mime_type) = match image.mime_type.as_str() {
        "image/jpeg" => (image::ImageFormat::Jpeg, "image/jpeg"),
        "image/webp" => (image::ImageFormat::WebP, "image/webp"),
        _ => (image::ImageFormat::Png, "image/png"),
    };
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(pixels)
        .write_to(&mut encoded, format)
        .map_err(|_| "The edited image could not be encoded.".to_owned())?;
    Ok(PromptImage {
        data: STANDARD.encode(encoded.into_inner()),
        mime_type: mime_type.to_owned(),
    })
}

#[cfg(test)]
mod pasted_image_tests {
    use super::*;

    #[test]
    fn scale_down_layout_centers_and_maps_image_pixels() {
        let frame = Bounds::new(point(px(10.0), px(20.0)), size(px(400.0), px(300.0)));
        let layout = PastedImageLayout::new(frame, 800, 400).unwrap();

        assert_eq!(layout.scale, 0.5);
        assert_eq!(layout.bounds.origin, point(px(10.0), px(70.0)));
        assert_eq!(layout.bounds.size, size(px(400.0), px(200.0)));
        assert_eq!(
            layout.image_point(point(px(210.0), px(170.0)), false),
            Some(ImagePoint { x: 400.0, y: 200.0 })
        );
        assert_eq!(layout.image_point(point(px(0.0), px(0.0)), false), None);
    }

    #[test]
    fn annotation_changes_pixels_and_keeps_a_supported_payload() {
        let source = image::RgbaImage::from_pixel(12, 12, image::Rgba([0, 0, 0, 0]));
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let prompt = PromptImage {
            data: STANDARD.encode(encoded.into_inner()),
            mime_type: "image/png".to_owned(),
        };
        let stroke = PencilStroke {
            image_index: 0,
            points: vec![ImagePoint { x: 2.0, y: 6.0 }, ImagePoint { x: 9.0, y: 6.0 }],
            color: PencilColor::Red,
            size: 3,
        };

        let edited = annotate_prompt_image(&prompt, &stroke).unwrap();
        let bytes = STANDARD.decode(&edited.data).unwrap();
        let pixels = image::load_from_memory(&bytes).unwrap().to_rgba8();

        assert_eq!(edited.mime_type, "image/png");
        assert_eq!(pixels.get_pixel(5, 6).0, PencilColor::Red.rgba8());
        assert_eq!(pixels.get_pixel(0, 0).0, [0, 0, 0, 0]);
    }
}

fn paint_pencil_stroke(stroke: &PencilStroke, layout: PastedImageLayout, window: &mut Window) {
    let Some(first) = stroke.points.first().copied() else {
        return;
    };
    let color = gpui::rgb(stroke.color.rgb());
    if stroke.points.len() == 1 {
        let center = layout.screen_point(first);
        let radius = px(stroke.size as f32 * layout.scale / 2.0);
        let mut dot = PathBuilder::fill();
        dot.move_to(point(center.x + radius, center.y));
        dot.arc_to(
            point(radius, radius),
            px(0.0),
            false,
            false,
            point(center.x - radius, center.y),
        );
        dot.arc_to(
            point(radius, radius),
            px(0.0),
            false,
            false,
            point(center.x + radius, center.y),
        );
        dot.close();
        if let Ok(path) = dot.build() {
            window.paint_path(path, color);
        }
        return;
    }

    let mut path = PathBuilder::stroke(px((stroke.size as f32 * layout.scale).max(1.0)));
    path.move_to(layout.screen_point(first));
    for point in stroke.points.iter().skip(1).copied() {
        path.line_to(layout.screen_point(point));
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

pub(super) struct PastedImageOverlayParams<'a> {
    pub prompt_image: &'a PromptImage,
    pub index: usize,
    pub count: usize,
    pub pencil_enabled: bool,
    pub pencil_color: PencilColor,
    pub pencil_size: u16,
    pub pencil_stroke: Option<PencilStroke>,
    pub can_undo: bool,
    pub pencil_error: Option<&'a str>,
}

fn pencil_toolbar(
    pencil_enabled: bool,
    pencil_color: PencilColor,
    pencil_size: u16,
    can_undo: bool,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    // Brush preview scales with size without dominating the bar.
    let brush_dot = (4.0 + (pencil_size as f32 / 64.0) * 10.0).clamp(4.0, 14.0);

    div()
        .min_h(px(44.0))
        .px(px(12.0))
        .py(px(6.0))
        .flex_shrink_0()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.0))
        .border_b_1()
        .border_color(theme::edge_soft())
        .bg(theme::floor())
        // Pencil toggle — icon-only selected state.
        .child(
            div()
                .id("pasted-image-pencil")
                .tab_index(0)
                .size(px(30.0))
                .rounded(px(theme::RADIUS_SM))
                .border_1()
                .border_color(if pencil_enabled {
                    theme::edge_hard()
                } else {
                    theme::edge_soft()
                })
                .bg(if pencil_enabled {
                    theme::panel_lift()
                } else {
                    theme::canvas()
                })
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|button| button.bg(theme::panel_lift()).border_color(theme::edge()))
                .on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        view.toggle_pencil(cx);
                    }
                }))
                .on_click(cx.listener(|view, _, _, cx| view.toggle_pencil(cx)))
                .child(svg().path("icons/pencil.svg").size(px(14.0)).text_color(
                    if pencil_enabled {
                        theme::data()
                    } else {
                        theme::ash()
                    },
                )),
        )
        .when(pencil_enabled, |toolbar| {
            toolbar
                .child(pencil_toolbar_sep())
                // Color swatches — selection ring only, no redundant "Color: Red" copy.
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.0))
                        .children(PencilColor::ALL.into_iter().map(|color| {
                            let selected = color == pencil_color;
                            div()
                                .id(gpui::SharedString::from(format!(
                                    "pencil-color-{}",
                                    color.label().to_ascii_lowercase()
                                )))
                                .tab_index(0)
                                .size(px(20.0))
                                .rounded(px(999.0))
                                .border_1()
                                .border_color(if selected {
                                    theme::bone()
                                } else {
                                    theme::edge_hard()
                                })
                                .bg(gpui::rgb(color.rgb()))
                                .cursor_pointer()
                                .hover(|swatch| swatch.border_color(theme::bone_dim()))
                                .on_key_down(cx.listener(
                                    move |view, event: &gpui::KeyDownEvent, _, cx| {
                                        if matches!(event.keystroke.key.as_str(), "enter" | "space")
                                        {
                                            cx.stop_propagation();
                                            view.set_pencil_color(color, cx);
                                        }
                                    },
                                ))
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.set_pencil_color(color, cx)
                                }))
                        })),
                )
                .child(pencil_toolbar_sep())
                // Size stepper with live brush preview.
                .child(
                    div()
                        .h(px(30.0))
                        .px(px(4.0))
                        .rounded(px(theme::RADIUS_SM))
                        .border_1()
                        .border_color(theme::edge_soft())
                        .bg(theme::canvas())
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(2.0))
                        .child(
                            div()
                                .id("pencil-size-decrease")
                                .tab_index(0)
                                .size(px(26.0))
                                .rounded(px(theme::RADIUS_SM))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(theme::ash())
                                .cursor_pointer()
                                .hover(|button| {
                                    button.bg(theme::panel_lift()).text_color(theme::bone())
                                })
                                .on_key_down(cx.listener(
                                    |view, event: &gpui::KeyDownEvent, _, cx| {
                                        if matches!(event.keystroke.key.as_str(), "enter" | "space")
                                        {
                                            cx.stop_propagation();
                                            view.adjust_pencil_size(-1, cx);
                                        }
                                    },
                                ))
                                .on_click(
                                    cx.listener(|view, _, _, cx| view.adjust_pencil_size(-1, cx)),
                                )
                                .child(
                                    div()
                                        .font_family(theme::main())
                                        .text_size(theme::text_size(theme::T_UI_SM))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("−"),
                                ),
                        )
                        .child(
                            div()
                                .w(px(52.0))
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_center()
                                .gap(px(6.0))
                                .child(
                                    div()
                                        .size(px(brush_dot))
                                        .rounded(px(999.0))
                                        .bg(gpui::rgb(pencil_color.rgb()))
                                        .flex_shrink_0(),
                                )
                                .child(
                                    div()
                                        .font_family(theme::mono())
                                        .text_size(theme::text_size(theme::T_TINY))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme::bone_dim())
                                        .child(format!("{pencil_size}")),
                                ),
                        )
                        .child(
                            div()
                                .id("pencil-size-increase")
                                .tab_index(0)
                                .size(px(26.0))
                                .rounded(px(theme::RADIUS_SM))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(theme::ash())
                                .cursor_pointer()
                                .hover(|button| {
                                    button.bg(theme::panel_lift()).text_color(theme::bone())
                                })
                                .on_key_down(cx.listener(
                                    |view, event: &gpui::KeyDownEvent, _, cx| {
                                        if matches!(event.keystroke.key.as_str(), "enter" | "space")
                                        {
                                            cx.stop_propagation();
                                            view.adjust_pencil_size(1, cx);
                                        }
                                    },
                                ))
                                .on_click(
                                    cx.listener(|view, _, _, cx| view.adjust_pencil_size(1, cx)),
                                )
                                .child(
                                    div()
                                        .font_family(theme::main())
                                        .text_size(theme::text_size(theme::T_UI_SM))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("+"),
                                ),
                        ),
                )
                .child(pencil_toolbar_sep())
                // Undo — icon button, dimmed when empty.
                .child(
                    div()
                        .id("pencil-undo")
                        .tab_index(if can_undo { 0 } else { -1 })
                        .size(px(30.0))
                        .rounded(px(theme::RADIUS_SM))
                        .border_1()
                        .border_color(theme::edge_soft())
                        .bg(theme::canvas())
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(if can_undo {
                            theme::bone_dim()
                        } else {
                            theme::smoke()
                        })
                        .cursor(if can_undo {
                            gpui::CursorStyle::PointingHand
                        } else {
                            gpui::CursorStyle::Arrow
                        })
                        .opacity(if can_undo { 1.0 } else { 0.45 })
                        .when(can_undo, |button| {
                            button
                                .hover(|button| {
                                    button
                                        .bg(theme::panel_lift())
                                        .border_color(theme::edge())
                                        .text_color(theme::bone())
                                })
                                .focus(|button| button.border_color(theme::focus()))
                                .on_key_down(cx.listener(
                                    |view, event: &gpui::KeyDownEvent, _, cx| {
                                        if matches!(event.keystroke.key.as_str(), "enter" | "space")
                                        {
                                            cx.stop_propagation();
                                            view.undo_pencil_stroke(cx);
                                        }
                                    },
                                ))
                                .on_click(cx.listener(|view, _, _, cx| view.undo_pencil_stroke(cx)))
                        })
                        .child(svg().path("icons/undo.svg").size(px(14.0)).text_color(
                            if can_undo {
                                theme::ash()
                            } else {
                                theme::smoke()
                            },
                        )),
                )
        })
}

fn pencil_toolbar_sep() -> impl IntoElement {
    div()
        .w(px(1.0))
        .h(px(16.0))
        .bg(theme::edge_soft())
        .flex_shrink_0()
}

pub(super) fn pasted_image_overlay(
    params: PastedImageOverlayParams<'_>,
    cx: &mut Context<RootView>,
) -> gpui::AnyElement {
    let PastedImageOverlayParams {
        prompt_image,
        index,
        count,
        pencil_enabled,
        pencil_color,
        pencil_size,
        pencil_stroke,
        can_undo,
        pencil_error,
    } = params;
    let image = pasted_image_source(prompt_image);
    let image_missing = image.is_none();
    let image_dimensions = image.as_ref().map(|(_, width, height)| (*width, *height));
    let image_source = image.map(|(source, _, _)| source);
    let root = cx.entity();
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
                                .font_family(theme::sans())
                                .text_size(theme::text_size(theme::T_UI_SM))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::bone_dim())
                                .child(if count > 1 {
                                    format!("Image {} of {} · {format}", index + 1, count)
                                } else {
                                    format!("Image 1 · {format}")
                                }),
                        )
                        .child(controls::chrome_action(
                            "pasted-image-close",
                            "Close",
                            true,
                            Box::new(cx.listener(|view, _, window, cx| {
                                view.close_pasted_image(window, cx)
                            })),
                        )),
                )
                .child(pencil_toolbar(
                    pencil_enabled,
                    pencil_color,
                    pencil_size,
                    can_undo,
                    cx,
                ))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .mx(px(14.0))
                        .relative()
                        .bg(theme::canvas())
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when_some(image_source, |frame, source| {
                            frame.child(img(source).size_full().object_fit(ObjectFit::ScaleDown))
                        })
                        .when(pencil_enabled && image_dimensions.is_some(), |frame| {
                            let (image_width, image_height) = image_dimensions.unwrap();
                            let paint_stroke = pencil_stroke.clone();
                            let paint_root = root.clone();
                            frame.child(
                                canvas(
                                    |_, _, _| (),
                                    move |bounds, _, window, _| {
                                        let Some(layout) = PastedImageLayout::new(
                                            bounds,
                                            image_width,
                                            image_height,
                                        ) else {
                                            return;
                                        };
                                        if let Some(stroke) = paint_stroke
                                            .as_ref()
                                            .filter(|stroke| stroke.image_index == index)
                                        {
                                            paint_pencil_stroke(stroke, layout, window);
                                        }

                                        let down_root = paint_root.clone();
                                        window.on_mouse_event(
                                            move |event: &MouseDownEvent, phase, _, cx| {
                                                if phase != DispatchPhase::Capture
                                                    || event.button != MouseButton::Left
                                                {
                                                    return;
                                                }
                                                let Some(image_point) =
                                                    layout.image_point(event.position, false)
                                                else {
                                                    return;
                                                };
                                                down_root.update(cx, |view, cx| {
                                                    view.start_pencil_stroke(index, image_point, cx)
                                                });
                                                cx.stop_propagation();
                                            },
                                        );

                                        let move_root = paint_root.clone();
                                        window.on_mouse_event(
                                            move |event: &MouseMoveEvent, phase, _, cx| {
                                                if phase != DispatchPhase::Capture {
                                                    return;
                                                }
                                                let Some(image_point) =
                                                    layout.image_point(event.position, true)
                                                else {
                                                    return;
                                                };
                                                let handled = move_root.update(cx, |view, cx| {
                                                    view.continue_pencil_stroke(
                                                        index,
                                                        image_point,
                                                        cx,
                                                    )
                                                });
                                                if handled {
                                                    cx.stop_propagation();
                                                }
                                            },
                                        );

                                        let up_root = paint_root.clone();
                                        window.on_mouse_event(
                                            move |event: &MouseUpEvent, phase, _, cx| {
                                                if phase != DispatchPhase::Capture
                                                    || event.button != MouseButton::Left
                                                {
                                                    return;
                                                }
                                                if up_root.update(cx, |view, cx| {
                                                    view.finish_pencil_stroke(cx)
                                                }) {
                                                    cx.stop_propagation();
                                                }
                                            },
                                        );
                                    },
                                )
                                .absolute()
                                .top_0()
                                .right_0()
                                .bottom_0()
                                .left_0()
                                .cursor_crosshair(),
                            )
                        })
                        .when(image_missing, |frame| {
                            frame.child(
                                div()
                                    .px(px(24.0))
                                    .text_size(theme::text_size(theme::T_BODY))
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
                        .gap(px(18.0))
                        .when_some(pencil_error, |footer, message| {
                            footer.child(
                                div()
                                    .font_family(theme::sans())
                                    .text_size(theme::text_size(theme::T_TINY))
                                    .text_color(theme::error())
                                    .child(message.to_owned()),
                            )
                        })
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
                                            .font_family(theme::sans())
                                            .text_size(theme::text_size(theme::T_BODY))
                                            .text_color(theme::bone())
                                            .hover(|button| button.bg(theme::panel_hover()))
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
                                            .font_family(theme::mono())
                                            .text_size(theme::text_size(theme::T_TINY))
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
                                            .font_family(theme::sans())
                                            .text_size(theme::text_size(theme::T_BODY))
                                            .text_color(theme::bone())
                                            .hover(|button| button.bg(theme::panel_hover()))
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
                                .font_family(theme::sans())
                                .font_weight(FontWeight::BOLD)
                                .text_size(theme::text_size(theme::T_BODY))
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
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_UI_SM))
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
                    .font_family(theme::sans())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(theme::text_size(theme::T_TINY))
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
                                .font_family(theme::sans())
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(theme::text_size(theme::T_BODY))
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
                                    .text_size(theme::text_size(theme::T_BODY))
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
        ("Toggle workspace sidebar", "Ctrl+B"),
        ("Toggle inspector", "Ctrl+I"),
        ("Increase font size", "Ctrl++"),
        ("Decrease font size", "Ctrl+-"),
        ("Send / steer", "Enter"),
        ("Queue follow-up", "Alt+Enter"),
        ("Insert newline", "Shift+Enter"),
        ("@ file / command menus", "↑ ↓ Enter Esc"),
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
                                .text_size(theme::text_size(theme::T_BODY))
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
                                .text_size(theme::text_size(theme::T_BODY))
                                .text_color(theme::bone())
                                .child(label),
                        )
                        .child(
                            div()
                                .font_family(theme::mono())
                                .text_size(theme::text_size(theme::T_UI_SM))
                                .text_color(theme::data())
                                .child(keys),
                        )
                })),
        )
}
