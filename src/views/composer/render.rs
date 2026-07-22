use gpui::{Context, CursorStyle, FontWeight, IntoElement, MouseButton, div, prelude::*, px};

use super::element::ComposerTextElement;
use super::{Composer, ComposerAvailability, ComposerEvent, ComposerFeedback};
use crate::theme;

impl Composer {
    pub(super) fn render_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let can_submit = !self.disabled && !self.buffer.text().trim().is_empty();
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

        div()
            .id(self.id_prefix.clone())
            .flex()
            .flex_col()
            .gap(px(8.0))
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
                    .h(px(112.0))
                    .p(px(12.0))
                    .overflow_hidden()
                    .rounded(px(theme::RADIUS))
                    .border_2()
                    .border_color(if self.disabled {
                        theme::edge_soft()
                    } else {
                        theme::edge_hard()
                    })
                    .bg(if self.disabled {
                        theme::floor()
                    } else {
                        theme::panel()
                    })
                    .text_size(px(theme::T_BODY_SM))
                    .line_height(px(21.0))
                    .text_color(if self.disabled {
                        theme::smoke()
                    } else {
                        theme::bone()
                    })
                    .focus(|input| input.border_color(theme::focus()))
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
            .child(
                div()
                    .min_h(px(34.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(16.0))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .font_family(theme::SANS)
                                    .text_size(px(theme::T_UI_SM))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(status_color)
                                    .child(self.status_text()),
                            )
                            .child(
                                div()
                                    .font_family(theme::SANS)
                                    .text_size(px(theme::T_TINY))
                                    .text_color(theme::smoke())
                                    .child(self.hint_text()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(12.0))
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
                                            .min_h(px(34.0))
                                            .px(px(4.0))
                                            .flex()
                                            .items_center()
                                            .font_family(theme::SANS)
                                            .text_size(px(theme::T_UI_SM))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme::bone_dim())
                                            .hover(|button| button.text_color(theme::error()))
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
                                                .when(can_submit, |button| {
                                                    button
                                                        .tab_index(0)
                                                        .cursor_pointer()
                                                        .hover(|button| {
                                                            button.text_color(theme::bone())
                                                        })
                                                        .focus(|button| {
                                                            button.text_color(theme::focus())
                                                        })
                                                        .on_click(cx.listener(|view, _, _, cx| {
                                                            view.emit_accept(true, cx);
                                                        }))
                                                })
                                                .font_family(theme::SANS)
                                                .text_size(px(theme::T_UI_SM))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(if can_submit {
                                                    theme::bone_dim()
                                                } else {
                                                    theme::smoke()
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
                                    .h(px(34.0))
                                    .min_w(px(72.0))
                                    .px(px(14.0))
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
                                            .focus(|button| button.text_color(theme::focus()))
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.emit_accept(false, cx);
                                            }))
                                    })
                                    .font_family(theme::SANS)
                                    .text_size(px(theme::T_UI))
                                    .font_weight(FontWeight::BOLD)
                                    .child(primary_label),
                            ),
                    ),
            )
    }
}
