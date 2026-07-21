use gpui::{Context, FontWeight, IntoElement, Render, Window, div, prelude::*, px};

use crate::{state::AppState, theme};

pub struct RootView {
    state: AppState,
}

impl RootView {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            state: AppState::default(),
        }
    }
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme::canvas())
            .font_family(theme::UI_FONT_FAMILY)
            .text_color(theme::text_primary())
            .child(
                div()
                    .w(px(440.0))
                    .flex()
                    .flex_col()
                    .gap_6()
                    .p_8()
                    .rounded_xl()
                    .border_1()
                    .border_color(theme::border())
                    .bg(theme::surface())
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::accent())
                            .child("RUST + GPUI"),
                    )
                    .child(
                        div()
                            .text_3xl()
                            .font_weight(FontWeight::BOLD)
                            .child("Pi GUI is ready."),
                    )
                    .child(div().text_base().text_color(theme::text_secondary()).child(
                        "A clean native starter with separate app, state, theme, and view modules.",
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format!("Count: {}", self.state.count())),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .id("reset-counter")
                                            .px_4()
                                            .py_2()
                                            .rounded_lg()
                                            .cursor_pointer()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .bg(theme::surface_hover())
                                            .hover(|button| button.bg(theme::border()))
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.state.reset();
                                                cx.notify();
                                            }))
                                            .child("Reset"),
                                    )
                                    .child(
                                        div()
                                            .id("increment-counter")
                                            .px_4()
                                            .py_2()
                                            .rounded_lg()
                                            .cursor_pointer()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme::canvas())
                                            .bg(theme::accent())
                                            .hover(|button| button.bg(theme::accent_hover()))
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.state.increment();
                                                cx.notify();
                                            }))
                                            .child("Increment"),
                                    ),
                            ),
                    ),
            )
    }
}
