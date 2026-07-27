use std::sync::Arc;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, Context, CursorStyle, FontWeight, Image, IntoElement,
    MouseButton, ObjectFit, SharedString, div, ease_out_quint, img, prelude::*, px, rgba,
};

use super::element::ComposerTextElement;
use super::{
    Composer, ComposerAvailability, ComposerChrome, ComposerEvent, ComposerFeedback,
    INPUT_HEIGHT_MOTION_MS,
};
use crate::theme;

/// On-screen size of each attached-image chip (square).
const ATTACHMENT_CHIP: f32 = 56.0;
/// Soft pop when an image is pasted into the composer.
const ATTACHMENT_POP_MS: u64 = 220;
/// Whole strip fade/lift when the first attachment appears.
const ATTACHMENT_STRIP_MS: u64 = 180;
/// Corner remove control diameter.
const ATTACHMENT_REMOVE: f32 = 18.0;

impl Composer {
    pub(super) fn render_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        match self.chrome {
            ComposerChrome::Field => self.render_field(cx).into_any_element(),
            ComposerChrome::Full | ComposerChrome::Panel => {
                self.render_multiline(cx).into_any_element()
            }
        }
    }

    fn render_field(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let can_submit = !self.disabled
            && (self.allow_empty_submit
                || !self.buffer.text().trim().is_empty()
                || !self.images.is_empty());
        let handles_composer_keys = self.availability != ComposerAvailability::Unavailable;
        let field_height = self.field_height();
        let field_padding_y = ((field_height - 22.0) / 2.0).max(0.0);
        let status = self.status_text();
        let show_status = !status.is_empty();
        let status_color = match self.feedback {
            ComposerFeedback::Rejected(_) | ComposerFeedback::Uncertain => theme::error(),
            ComposerFeedback::Pending(_) | ComposerFeedback::BashRunning { .. } => theme::data(),
            ComposerFeedback::Accepted(_) | ComposerFeedback::BashCompleted => theme::live(),
            ComposerFeedback::Ready => theme::ash(),
        };

        div()
            .id(self.id_prefix.clone())
            .w_full()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        div()
                            .id(gpui::SharedString::from(format!(
                                "{}-input",
                                self.id_prefix
                            )))
                            .track_focus(&self.focus_handle)
                            .when(!self.disabled, |input| input.tab_index(0))
                            .when(handles_composer_keys, |input| input.key_context("Composer"))
                            .cursor(if self.disabled {
                                CursorStyle::Arrow
                            } else {
                                CursorStyle::IBeam
                            })
                            .flex_1()
                            .min_w_0()
                            .h(px(field_height))
                            .px(px(10.0))
                            .py(px(field_padding_y))
                            .overflow_hidden()
                            .rounded(px(theme::RADIUS_SM))
                            .border_1()
                            .border_color(if self.disabled {
                                theme::edge_soft()
                            } else {
                                theme::edge()
                            })
                            .bg(if self.disabled {
                                theme::canvas()
                            } else {
                                theme::panel_lift()
                            })
                            .text_size(theme::text_size(theme::T_UI_SM))
                            .line_height(theme::text_size(20.0))
                            .text_color(if self.disabled {
                                theme::smoke()
                            } else {
                                theme::bone()
                            })
                            .when(handles_composer_keys, |input| {
                                input
                                    .on_action(cx.listener(Self::backspace))
                                    .on_action(cx.listener(Self::delete))
                                    .on_action(cx.listener(Self::left))
                                    .on_action(cx.listener(Self::right))
                                    .on_action(cx.listener(Self::up))
                                    .on_action(cx.listener(Self::down))
                                    .on_action(cx.listener(Self::select_left))
                                    .on_action(cx.listener(Self::select_right))
                                    .on_action(cx.listener(Self::select_up))
                                    .on_action(cx.listener(Self::select_down))
                                    .on_action(cx.listener(Self::line_start))
                                    .on_action(cx.listener(Self::line_end))
                                    .on_action(cx.listener(Self::select_line_start))
                                    .on_action(cx.listener(Self::select_line_end))
                                    .on_action(cx.listener(Self::select_all))
                                    .on_action(cx.listener(Self::copy))
                                    .on_action(cx.listener(Self::cut))
                                    .on_action(cx.listener(Self::paste))
                                    .on_action(cx.listener(Self::undo))
                                    .on_action(cx.listener(Self::redo))
                                    .on_action(cx.listener(Self::insert_newline))
                                    .on_action(cx.listener(Self::accept))
                                    .on_action(cx.listener(Self::follow_up))
                            })
                            .on_action(cx.listener(Self::abort))
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                            .on_mouse_move(cx.listener(Self::on_mouse_move))
                            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
                            .child(ComposerTextElement { input: cx.entity() }),
                    )
                    .when(!self.action_label.is_empty(), |row| {
                        row.child(
                            div()
                                .id(gpui::SharedString::from(format!(
                                    "{}-submit",
                                    self.id_prefix
                                )))
                                .h(px(34.0))
                                .min_w(px(56.0))
                                .px(px(10.0))
                                .rounded(px(theme::RADIUS_SM))
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(if can_submit {
                                    theme::signal()
                                } else {
                                    theme::panel_lift()
                                })
                                .text_color(if can_submit {
                                    theme::canvas()
                                } else {
                                    theme::smoke()
                                })
                                .when(can_submit, |button| {
                                    button
                                        .tab_index(0)
                                        .cursor_pointer()
                                        .hover(|button| button.bg(theme::signal_hot()))
                                        .active(|button| button.bg(theme::signal_deep()))
                                        .on_click(cx.listener(|view, _, _, cx| {
                                            view.emit_accept(false, cx);
                                        }))
                                })
                                .font_family(theme::main())
                                .text_size(theme::text_size(theme::T_TINY))
                                .font_weight(FontWeight::BOLD)
                                .child(self.action_label.clone()),
                        )
                    }),
            )
            .when(show_status, |col| {
                col.child(
                    div()
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_TINY))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(status_color)
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(status),
                )
            })
    }

    fn render_multiline(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let can_submit = !self.disabled
            && (self.allow_empty_submit
                || !self.buffer.text().trim().is_empty()
                || !self.images.is_empty());
        let handles_composer_keys = self.availability != ComposerAvailability::Unavailable;
        let running = self.availability == ComposerAvailability::Running;
        let bash_running = self.availability == ComposerAvailability::BashRunning;
        let primary_label = if running {
            "Steer".into()
        } else {
            self.action_label.clone()
        };
        let status_color = match self.feedback {
            ComposerFeedback::Rejected(_) | ComposerFeedback::Uncertain => theme::error(),
            ComposerFeedback::Pending(_) | ComposerFeedback::BashRunning { .. } => theme::data(),
            ComposerFeedback::Accepted(_) | ComposerFeedback::BashCompleted => theme::live(),
            ComposerFeedback::Ready => theme::ash(),
        };
        let panel = self.chrome == ComposerChrome::Panel;
        // Full desk chrome sits inside the prompt card from RootView, so the input
        // has no outer border; Panel chrome keeps a self-contained field look.
        let desk = self.chrome == ComposerChrome::Full;
        let status = self.status_text();
        let show_status = !status.is_empty() || !panel;
        let input_padding_x = if panel { 12.0 } else { 10.0 };
        let input_padding_y = if panel { 8.0 } else { 6.0 };
        let input_line_height = if panel { 21.0 } else { 20.0 };
        // Idle: one row · focused: multi-line · user enlarge: taller pinned shell.
        let height_motion = self.input_height_motion();
        let settled_height = self.input_target_height();
        let action_height = if desk { 28.0 } else { 32.0 };
        let action_gap = if desk { 6.0 } else { 8.0 };
        let id_prefix = self.id_prefix.clone();

        let mut input = div()
            .id(gpui::SharedString::from(format!("{id_prefix}-input")))
            .track_focus(&self.focus_handle)
            .when(!self.disabled, |input| input.tab_index(0))
            .when(handles_composer_keys, |input| input.key_context("Composer"))
            .cursor(if self.disabled {
                CursorStyle::Arrow
            } else {
                CursorStyle::IBeam
            })
            .h(px(settled_height))
            .px(px(input_padding_x))
            .py(px(input_padding_y))
            .overflow_hidden()
            .text_size(theme::text_size(theme::T_BODY_SM))
            .line_height(theme::text_size(input_line_height))
            .text_color(if self.disabled {
                theme::smoke()
            } else {
                theme::bone()
            });

        if panel {
            input = input
                .rounded(px(theme::RADIUS_SM))
                .border_1()
                .border_color(if self.disabled {
                    theme::edge_soft()
                } else {
                    theme::edge()
                })
                .bg(if self.disabled {
                    theme::canvas()
                } else {
                    theme::panel()
                });
        } else {
            // Same surface when collapsed or focused so minimize is height-only.
            input = input.bg(theme::panel_lift());
        }

        input = input
            .when(handles_composer_keys, |input| {
                input
                    .on_action(cx.listener(Self::backspace))
                    .on_action(cx.listener(Self::delete))
                    .on_action(cx.listener(Self::left))
                    .on_action(cx.listener(Self::right))
                    .on_action(cx.listener(Self::up))
                    .on_action(cx.listener(Self::down))
                    .on_action(cx.listener(Self::select_left))
                    .on_action(cx.listener(Self::select_right))
                    .on_action(cx.listener(Self::select_up))
                    .on_action(cx.listener(Self::select_down))
                    .on_action(cx.listener(Self::line_start))
                    .on_action(cx.listener(Self::line_end))
                    .on_action(cx.listener(Self::select_line_start))
                    .on_action(cx.listener(Self::select_line_end))
                    .on_action(cx.listener(Self::select_all))
                    .on_action(cx.listener(Self::copy))
                    .on_action(cx.listener(Self::cut))
                    .on_action(cx.listener(Self::paste))
                    .on_action(cx.listener(Self::undo))
                    .on_action(cx.listener(Self::redo))
                    .on_action(cx.listener(Self::insert_newline))
                    .on_action(cx.listener(Self::accept))
                    .on_action(cx.listener(Self::follow_up))
            })
            .on_action(cx.listener(Self::abort))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .child(ComposerTextElement { input: cx.entity() });

        let input = if let Some(motion) = height_motion {
            let from = motion.from;
            let to = motion.to;
            let anim_id = SharedString::from(format!("{id_prefix}-input-height"));
            input
                .with_animation(
                    (anim_id, motion.generation as usize),
                    Animation::new(Duration::from_millis(INPUT_HEIGHT_MOTION_MS))
                        .with_easing(ease_out_quint()),
                    move |input, delta| {
                        let height = from + (to - from) * delta;
                        input.h(px(height.max(1.0)))
                    },
                )
                .into_any_element()
        } else {
            input.into_any_element()
        };

        let attachments = (!self.images.is_empty()).then(|| self.render_attachments(cx));

        div()
            .id(id_prefix)
            .flex()
            .flex_col()
            .gap(px(if panel { 6.0 } else { 0.0 }))
            .child(input)
            .when_some(attachments, |composer, attachments| {
                composer.child(attachments)
            })
            .child(
                div()
                    .min_h(px(if panel { 28.0 } else { 36.0 }))
                    .px(px(if desk { 10.0 } else { 0.0 }))
                    .py(px(if desk { 4.0 } else { 0.0 }))
                    .when(desk, |footer| {
                        footer.border_t_1().border_color(theme::edge_soft())
                    })
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(10.0))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_row()
                            .items_baseline()
                            .gap(px(8.0))
                            .overflow_hidden()
                            .when(show_status, |row| {
                                row.child(
                                    div()
                                        .min_w_0()
                                        .font_family(theme::sans())
                                        .text_size(theme::text_size(theme::T_UI_SM))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(status_color)
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(status),
                                )
                            })
                            .when(!panel, |row| {
                                row.child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .font_family(theme::mono())
                                        .text_size(theme::text_size(theme::T_TINY))
                                        .text_color(theme::smoke())
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(self.hint_text()),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(action_gap))
                            .when(running || bash_running, |actions| {
                                actions
                                    .child(
                                        div()
                                            .id(gpui::SharedString::from(format!(
                                                "{}-abort",
                                                self.id_prefix
                                            )))
                                            .tab_index(0)
                                            .cursor_pointer()
                                            .h(px(action_height))
                                            .px(px(if desk { 7.0 } else { 8.0 }))
                                            .rounded(px(theme::RADIUS_SM))
                                            .flex()
                                            .items_center()
                                            .font_family(theme::main())
                                            .text_size(theme::text_size(theme::T_UI_SM))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme::bone_dim())
                                            .hover(|button| {
                                                button.bg(theme::panel()).text_color(theme::error())
                                            })
                                            .focus(|button| button.text_color(theme::focus()))
                                            .on_click(cx.listener(move |_, _, _, cx| {
                                                cx.emit(if bash_running {
                                                    ComposerEvent::AbortBash
                                                } else {
                                                    ComposerEvent::Abort
                                                });
                                            }))
                                            .child(if bash_running {
                                                "Abort Bash"
                                            } else {
                                                "Abort"
                                            }),
                                    )
                                    .when(running, |actions| {
                                        actions.child(
                                            div()
                                                .id(gpui::SharedString::from(format!(
                                                    "{}-follow-up",
                                                    self.id_prefix
                                                )))
                                                .h(px(action_height))
                                                .px(px(if desk { 7.0 } else { 8.0 }))
                                                .rounded(px(theme::RADIUS_SM))
                                                .flex()
                                                .items_center()
                                                .font_family(theme::main())
                                                .text_size(theme::text_size(theme::T_UI_SM))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(if can_submit {
                                                    theme::bone_dim()
                                                } else {
                                                    theme::smoke()
                                                })
                                                .when(can_submit, |button| {
                                                    button
                                                        .tab_index(0)
                                                        .cursor_pointer()
                                                        .hover(|button| {
                                                            button
                                                                .bg(theme::panel())
                                                                .text_color(theme::bone())
                                                        })
                                                        .focus(|button| {
                                                            button.text_color(theme::focus())
                                                        })
                                                        .on_click(cx.listener(|view, _, _, cx| {
                                                            view.emit_accept(true, cx);
                                                        }))
                                                })
                                                .child("Follow up"),
                                        )
                                    })
                            })
                            .child(
                                div()
                                    .id(gpui::SharedString::from(format!(
                                        "{}-submit",
                                        self.id_prefix
                                    )))
                                    .h(px(action_height))
                                    .min_w(px(if desk { 64.0 } else { 68.0 }))
                                    .px(px(if desk { 12.0 } else { 14.0 }))
                                    .rounded(px(theme::RADIUS_SM))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(if can_submit {
                                        theme::signal()
                                    } else {
                                        theme::panel_lift()
                                    })
                                    .text_color(if can_submit {
                                        theme::canvas()
                                    } else {
                                        theme::smoke()
                                    })
                                    .when(can_submit, |button| {
                                        button
                                            .tab_index(0)
                                            .cursor_pointer()
                                            .hover(|button| button.bg(theme::signal_hot()))
                                            .active(|button| button.bg(theme::signal_deep()))
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.emit_accept(false, cx);
                                            }))
                                    })
                                    .font_family(theme::main())
                                    .text_size(theme::text_size(theme::T_UI_SM))
                                    .font_weight(FontWeight::BOLD)
                                    .child(primary_label),
                            ),
                    ),
            )
    }

    fn render_attachments(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let can_remove = !self.disabled;
        let strip_key = self.strip_motion_key();
        let id_prefix = self.id_prefix.clone();
        let count = self.images.len();
        let mut chips = Vec::with_capacity(count);
        for index in 0..count {
            let mime = self.images[index].mime_type.clone();
            let format = mime
                .strip_prefix("image/")
                .unwrap_or(&mime)
                .to_ascii_uppercase();
            let thumb = self.thumbnail(index);
            let attach_token = self.attach_token(index);

            // Fixed layout slot so the pop scale does not shove neighboring chips.
            chips.push(
                div()
                    .size(px(ATTACHMENT_CHIP))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(self.render_attachment_chip(
                        index,
                        attach_token,
                        format,
                        thumb,
                        can_remove,
                        cx,
                    ))
                    .into_any_element(),
            );
        }

        let strip = div()
            .id(SharedString::from(format!("{id_prefix}-attachments")))
            .w_full()
            // Extra top/side padding so the corner × can sit slightly outside the chip.
            .px(px(12.0))
            .pt(px(12.0))
            .pb(px(4.0))
            .flex()
            .flex_row()
            .flex_wrap()
            .items_start()
            .gap(px(12.0))
            .children(chips);

        if strip_key == 0 {
            strip.into_any_element()
        } else {
            let enter_id = SharedString::from(format!("{id_prefix}-attachments-enter"));
            strip
                .with_animation(
                    (enter_id, strip_key as usize),
                    Animation::new(Duration::from_millis(ATTACHMENT_STRIP_MS))
                        .with_easing(ease_out_quint()),
                    |strip, t| strip.opacity(0.4 + 0.6 * t).mt(px(5.0 * (1.0 - t))),
                )
                .into_any_element()
        }
    }

    fn render_attachment_chip(
        &mut self,
        index: usize,
        attach_token: u64,
        format: String,
        thumb: Option<Arc<Image>>,
        can_remove: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let has_thumb = thumb.is_some();
        let chip = div()
            .id(SharedString::from(format!(
                "{}-image-{index}",
                self.id_prefix
            )))
            .relative()
            .size(px(ATTACHMENT_CHIP))
            .rounded(px(theme::RADIUS))
            .border_1()
            .border_color(rgba(0x0000_0000))
            .tab_index(0)
            .cursor_pointer()
            .focus(|chip| chip.border_color(theme::focus()))
            .on_key_down(cx.listener(move |view, event: &gpui::KeyDownEvent, _, cx| {
                match event.keystroke.key.as_str() {
                    "enter" | "space" => {
                        cx.stop_propagation();
                        view.preview_image(index, cx);
                    }
                    "backspace" | "delete" if can_remove => {
                        cx.stop_propagation();
                        view.remove_image(index, cx);
                    }
                    _ => {}
                }
            }))
            .on_click(cx.listener(move |view, _, _, cx| {
                view.preview_image(index, cx);
            }))
            .child(
                div()
                    .size_full()
                    .rounded(px(theme::RADIUS))
                    .border_1()
                    .border_color(theme::edge_soft())
                    .bg(theme::panel())
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|frame| frame.border_color(theme::edge_hard()))
                    .when_some(thumb, |frame, source| {
                        frame.child(img(source).size_full().object_fit(ObjectFit::Cover))
                    })
                    .when(!has_thumb, |frame| {
                        frame.bg(theme::canvas()).child(
                            div()
                                .font_family(theme::mono())
                                .text_size(theme::text_size(theme::T_TINY))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::ash())
                                .child(format),
                        )
                    }),
            )
            .when(can_remove, |chip| {
                chip.child(
                    div()
                        .id(SharedString::from(format!(
                            "{}-image-{index}-remove",
                            self.id_prefix
                        )))
                        .absolute()
                        .top(px(-5.0))
                        .right(px(-5.0))
                        .size(px(ATTACHMENT_REMOVE))
                        .rounded(px(ATTACHMENT_REMOVE / 2.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        // Dark translucent disc stays readable on light and dark thumbs.
                        .bg(rgba(0x1210_0ed9))
                        .border_1()
                        .border_color(rgba(0xffff_ff33))
                        .text_color(rgba(0xffff_fff0))
                        .tab_index(0)
                        .cursor_pointer()
                        .hover(|button| {
                            button
                                .bg(theme::error())
                                .border_color(theme::error())
                                .text_color(theme::bone())
                        })
                        .focus(|button| {
                            button
                                .border_color(theme::focus())
                                .text_color(theme::focus())
                        })
                        .on_key_down(cx.listener(move |view, event: &gpui::KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                cx.stop_propagation();
                                view.remove_image(index, cx);
                            }
                        }))
                        .on_click(cx.listener(move |view, _, _, cx| {
                            cx.stop_propagation();
                            view.remove_image(index, cx);
                        }))
                        .child(
                            div()
                                .relative()
                                .top(px(-0.5))
                                .font_family(theme::sans())
                                .text_size(theme::text_size(theme::T_TINY))
                                .font_weight(FontWeight::BOLD)
                                .line_height(gpui::relative(1.0))
                                .child("×"),
                        ),
                )
            });

        if attach_token == 0 {
            // Restored drafts stay settled (no pop).
            chip.into_any_element()
        } else {
            // Soft pop on paste: fade, rise, and grow into the fixed slot.
            let pop_id = SharedString::from(format!("{}-image-pop", self.id_prefix));
            chip.with_animation(
                (pop_id, attach_token as usize),
                Animation::new(Duration::from_millis(ATTACHMENT_POP_MS))
                    .with_easing(ease_out_quint()),
                |chip, t| {
                    let scale = 0.86 + 0.14 * t;
                    let size = ATTACHMENT_CHIP * scale;
                    let lift = 8.0 * (1.0 - t);
                    chip.size(px(size)).opacity(0.18 + 0.82 * t).mt(px(lift))
                },
            )
            .into_any_element()
        }
    }
}
