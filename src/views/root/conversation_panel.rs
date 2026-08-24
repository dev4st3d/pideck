//! Virtualized conversation viewport and custom scrollbar.

use gpui::StatefulInteractiveElement;

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

pub(super) struct ConversationAreaParams {
    pub(super) projection: Arc<ConversationProjection>,
    pub(super) list: Arc<ConversationListModel>,
    pub(super) list_state: ListState,
    pub(super) transcript_cache: Entity<TranscriptTextCache>,
    pub(super) stream_bands: Entity<StreamBandCache>,
    pub(super) activity_disclosures: Entity<ActivityDisclosureState>,
    pub(super) workspace_diff: Option<Arc<WorkspaceDiff>>,
    pub(super) workspace_diff_files_expanded: bool,
    pub(super) follow: bool,
    pub(super) root: Entity<RootView>,
}

pub(super) fn conversation_area(params: ConversationAreaParams) -> impl IntoElement {
    let ConversationAreaParams {
        projection,
        list: conversation_list,
        list_state: conversation_list_state,
        transcript_cache,
        stream_bands,
        activity_disclosures,
        workspace_diff,
        workspace_diff_files_expanded,
        follow,
        root,
    } = params;
    let wheel_root = root.clone();
    let jump_root = root.clone();
    let scrollbar_state = conversation_list_state.clone();
    let stream_entities = ConversationStreamEntities {
        transcript_cache,
        band_cache: stream_bands,
        disclosures: activity_disclosures,
        diff_summary: ConversationDiffSummary {
            snapshot: workspace_diff,
            files_expanded: workspace_diff_files_expanded,
            root: root.clone(),
        },
    };

    // The transcript shares the center column with the composer and must yield
    // height to it on every lifecycle-driven rerender.
    div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(theme::floor())
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
                        conversation_list.render_item(item_index, &projection, &stream_entities, cx)
                    })
                    .size_full()
                    .min_w_0()
                    .pt(px(12.0))
                    .pb(px(12.0)),
                )
                .bg(theme::canvas())
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
                )
                .when(!follow, |viewport| {
                    viewport.child(
                        div()
                            .id("conversation-jump-latest")
                            .absolute()
                            .right(px(12.0))
                            .bottom(px(10.0))
                            .h(px(28.0))
                            .px(px(9.0))
                            .rounded(px(theme::RADIUS_SM))
                            .border_1()
                            .border_color(theme::edge())
                            .bg(theme::panel_lift())
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(5.0))
                            .cursor_pointer()
                            .tab_index(0)
                            .font_family(theme::main())
                            .text_size(theme::text_size(theme::T_UI_SM))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::bone_dim())
                            .hover(|button| {
                                button.bg(theme::panel_hover()).text_color(theme::bone())
                            })
                            .focus(|button| button.border_color(theme::focus()))
                            .active(|button| button.bg(theme::canvas()))
                            .on_click(move |_, _, cx| {
                                jump_root.update(cx, |view, cx| view.jump_to_latest(cx));
                            })
                            .child(
                                svg()
                                    .path("icons/chevron-down.svg")
                                    .size(px(10.0))
                                    .text_color(theme::data()),
                            )
                            .child("Jump to latest"),
                    )
                }),
        )
}

impl RootView {
    pub(super) fn on_conversation_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if event.delta.precise() {
            // Pixel deltas already carry the platform's touchpad precision and
            // momentum. Never layer synthetic motion on top of them.
            self.conversation_scroll_motion.cancel();
            return false;
        }

        let distance = -event.delta.pixel_delta(px(20.0)).y;
        if distance == px(0.0) {
            return false;
        }
        if distance > px(0.0) && self.conversation_follow.get() {
            self.conversation_scroll_motion.cancel();
            self.conversation_list_state.scroll_to(ListOffset {
                item_ix: self.conversation_list.item_count(),
                offset_in_item: px(0.0),
            });
            return true;
        }

        self.conversation_follow.set(false);
        let now = Instant::now();
        if self.conversation_scroll_motion.push(distance, now) {
            self.advance_conversation_scroll(now, cx);
            self.schedule_conversation_scroll_frame(window, cx);
        }
        true
    }

    pub(super) fn jump_to_latest(&mut self, cx: &mut Context<Self>) {
        self.conversation_scroll_motion.cancel();
        self.conversation_follow.set(true);
        self.conversation_list_state.scroll_to(ListOffset {
            item_ix: self.conversation_list.item_count(),
            offset_in_item: px(0.0),
        });
        cx.notify();
    }

    pub(super) fn advance_conversation_scroll(&mut self, now: Instant, cx: &mut Context<Self>) {
        let Some(step) = self.conversation_scroll_motion.advance(now) else {
            return;
        };
        let before = self.conversation_list_state.logical_scroll_top();
        self.conversation_list_state.scroll_by(step);
        let after = self.conversation_list_state.logical_scroll_top();
        let stalled = before.item_ix == after.item_ix
            && (f32::from(before.offset_in_item) - f32::from(after.offset_in_item)).abs() < 0.01;
        let at_top =
            step < px(0.0) && after.item_ix == 0 && f32::from(after.offset_in_item) <= 0.01;
        let at_bottom =
            step > px(0.0) && (after.item_ix >= self.conversation_list.item_count() || stalled);

        if at_bottom {
            self.conversation_list_state.scroll_to(ListOffset {
                item_ix: self.conversation_list.item_count(),
                offset_in_item: px(0.0),
            });
            self.conversation_follow.set(true);
            self.conversation_scroll_motion.cancel();
        } else if at_top || stalled {
            self.conversation_scroll_motion.cancel();
        }
        cx.notify();
    }

    pub(super) fn schedule_conversation_scroll_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.conversation_scroll_motion.schedule_frame() {
            return;
        }
        cx.on_next_frame(window, |view, window, cx| {
            view.conversation_scroll_motion.begin_frame();
            view.advance_conversation_scroll(Instant::now(), cx);
            view.schedule_conversation_scroll_frame(window, cx);
        });
    }

}
