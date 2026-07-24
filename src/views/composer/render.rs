use gpui::{
    AnyElement, Context, CursorStyle, FontWeight, IntoElement, MouseButton, div, prelude::*, px,
};

use super::element::ComposerTextElement;
use super::{Composer, ComposerAvailability, ComposerChrome, ComposerEvent, ComposerFeedback};
use crate::theme;

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
                            .text_size(px(theme::T_UI_SM))
                            .line_height(px(20.0))
                            .text_color(if self.disabled {
                                theme::smoke()
                            } else {
                                theme::bone()
                            })
                            .focus(|input| {
                                input.border_color(theme::focus()).bg(if self.disabled {
                                    theme::canvas()
                                } else {
                                    theme::panel_lift()
                                })
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
                                        .focus(|button| {
                                            button.border_1().border_color(theme::focus())
                                        })
                                        .on_click(cx.listener(|view, _, _, cx| {
                                            view.emit_accept(false, cx);
                                        }))
                                })
                                .font_family(theme::CONTROL)
                                .text_size(px(theme::T_TINY))
                                .font_weight(FontWeight::BOLD)
                                .child(self.action_label.clone()),
                        )
                    }),
            )
            .when(show_status, |col| {
                col.child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_TINY))
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

        let mut input = div()
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
            .h(px(if panel { 64.0 } else { 88.0 }))
            .px(px(12.0))
            .py(px(if panel { 8.0 } else { 10.0 }))
            .overflow_hidden()
            .text_size(px(theme::T_BODY_SM))
            .line_height(px(21.0))
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
                })
                .focus(|input| {
                    input.border_color(theme::focus()).bg(if self.disabled {
                        theme::canvas()
                    } else {
                        theme::panel_lift()
                    })
                });
        } else {
            // Desk: focus ring via subtle lift so the shared prompt card stays clean.
            input = input.focus(|input| input.bg(theme::panel_lift()));
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

        let attachments = (!self.images.is_empty()).then(|| self.render_attachments(cx));

        div()
            .id(self.id_prefix.clone())
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
                    .pb(px(if desk { 8.0 } else { 0.0 }))
                    .pt(px(if desk { 6.0 } else { 0.0 }))
                    .when(desk, |footer| {
                        footer.border_t_1().border_color(theme::edge_soft())
                    })
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(1.0))
                            .when(show_status, |col| {
                                col.child(
                                    div()
                                        .font_family(theme::SANS)
                                        .text_size(px(theme::T_UI_SM))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(status_color)
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(status),
                                )
                            })
                            .when(!panel, |col| {
                                col.child(
                                    div()
                                        .font_family(theme::MONO)
                                        .text_size(px(theme::T_TINY))
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
                            .gap(px(8.0))
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
                                            .h(px(32.0))
                                            .px(px(8.0))
                                            .rounded(px(theme::RADIUS_SM))
                                            .flex()
                                            .items_center()
                                            .font_family(theme::CONTROL)
                                            .text_size(px(theme::T_UI_SM))
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
                                                .h(px(32.0))
                                                .px(px(8.0))
                                                .rounded(px(theme::RADIUS_SM))
                                                .flex()
                                                .items_center()
                                                .font_family(theme::CONTROL)
                                                .text_size(px(theme::T_UI_SM))
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
                                    .h(px(32.0))
                                    .min_w(px(68.0))
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
                                            .focus(|button| {
                                                button.border_1().border_color(theme::focus())
                                            })
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.emit_accept(false, cx);
                                            }))
                                    })
                                    .font_family(theme::CONTROL)
                                    .text_size(px(theme::T_UI_SM))
                                    .font_weight(FontWeight::BOLD)
                                    .child(primary_label),
                            ),
                    ),
            )
    }

    fn render_attachments(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let rows = self
            .images
            .iter()
            .enumerate()
            .map(|(index, image)| {
                let format = image
                    .mime_type
                    .strip_prefix("image/")
                    .unwrap_or(&image.mime_type)
                    .to_ascii_uppercase();
                let bytes = super::decoded_image_len(&image.data);
                div()
                    .id(gpui::SharedString::from(format!(
                        "{}-image-{index}",
                        self.id_prefix
                    )))
                    .w_full()
                    .min_h(px(28.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_row()
                            .items_baseline()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .font_family(theme::SANS)
                                    .text_size(px(theme::T_UI_SM))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::bone_dim())
                                    .child(format!("Image {}", index + 1)),
                            )
                            .child(
                                div()
                                    .font_family(theme::MONO)
                                    .text_size(px(theme::T_TINY))
                                    .text_color(theme::smoke())
                                    .child(format!("{format} · {}", format_image_bytes(bytes))),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .id(gpui::SharedString::from(format!(
                                        "{}-image-{index}-preview",
                                        self.id_prefix
                                    )))
                                    .tab_index(0)
                                    .cursor_pointer()
                                    .px(px(6.0))
                                    .py(px(3.0))
                                    .text_color(theme::bone_dim())
                                    .hover(|button| button.text_color(theme::bone()))
                                    .focus(|button| button.text_color(theme::focus()))
                                    .on_key_down(cx.listener(
                                        move |view, event: &gpui::KeyDownEvent, _, cx| {
                                            if matches!(
                                                event.keystroke.key.as_str(),
                                                "enter" | "space"
                                            ) {
                                                cx.stop_propagation();
                                                view.preview_image(index, cx);
                                            }
                                        },
                                    ))
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.preview_image(index, cx);
                                    }))
                                    .child(
                                        div()
                                            .font_family(theme::CONTROL)
                                            .text_size(px(theme::T_TINY))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("View"),
                                    ),
                            )
                            .child(
                                div()
                                    .id(gpui::SharedString::from(format!(
                                        "{}-image-{index}-remove",
                                        self.id_prefix
                                    )))
                                    .tab_index(0)
                                    .cursor_pointer()
                                    .px(px(6.0))
                                    .py(px(3.0))
                                    .text_color(theme::ash())
                                    .hover(|button| button.text_color(theme::error()))
                                    .focus(|button| button.text_color(theme::focus()))
                                    .on_key_down(cx.listener(
                                        move |view, event: &gpui::KeyDownEvent, _, cx| {
                                            if matches!(
                                                event.keystroke.key.as_str(),
                                                "enter" | "space"
                                            ) {
                                                cx.stop_propagation();
                                                view.remove_image(index, cx);
                                            }
                                        },
                                    ))
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.remove_image(index, cx);
                                    }))
                                    .child(
                                        div()
                                            .font_family(theme::CONTROL)
                                            .text_size(px(theme::T_TINY))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("Remove"),
                                    ),
                            ),
                    )
            })
            .collect::<Vec<_>>();

        div()
            .w_full()
            .px(px(10.0))
            .py(px(5.0))
            .bg(theme::canvas())
            .flex()
            .flex_col()
            .children(rows)
            .into_any_element()
    }
}

fn format_image_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}
