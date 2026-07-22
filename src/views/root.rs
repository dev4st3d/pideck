//! Minimal live shell for the supervised Pi runtime.

use gpui::{
    Context, Entity, FocusHandle, FontWeight, IntoElement, Render, Subscription, Window, div,
    prelude::*, px, relative,
};

use crate::actions::{ActivateRecovery, Connect, FocusNext, FocusPrevious, Retry, Stop};
use crate::controller::RuntimeController;
use crate::state::{RecoveryAction, ShellProjection};
use crate::theme;
use crate::views::controls;

pub struct RootView {
    controller: Entity<RuntimeController>,
    focus_handle: FocusHandle,
    _controller_observation: Subscription,
}

impl RootView {
    pub fn new(
        window: &mut Window,
        controller: Entity<RuntimeController>,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle);
        let controller_observation = cx.observe(&controller, |_, _, cx| cx.notify());
        Self {
            controller,
            focus_handle,
            _controller_observation: controller_observation,
        }
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        self.controller
            .update(cx, |controller, cx| controller.connect(cx));
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.controller
            .update(cx, |controller, cx| controller.stop(cx));
    }

    fn activate_recovery(&mut self, action: RecoveryAction, cx: &mut Context<Self>) {
        match action {
            RecoveryAction::Connect | RecoveryAction::Retry => self.connect(cx),
            RecoveryAction::Stop => self.stop(cx),
        }
    }

    fn on_connect(&mut self, _: &Connect, _: &mut Window, cx: &mut Context<Self>) {
        self.connect(cx);
    }

    fn on_retry(&mut self, _: &Retry, _: &mut Window, cx: &mut Context<Self>) {
        self.connect(cx);
    }

    fn on_stop(&mut self, _: &Stop, _: &mut Window, cx: &mut Context<Self>) {
        self.stop(cx);
    }

    fn on_activate_recovery(
        &mut self,
        _: &ActivateRecovery,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = self.controller.read(cx).projection().action;
        if let Some(action) = action {
            self.activate_recovery(action, cx);
        }
    }

    fn on_focus_next(&mut self, _: &FocusNext, window: &mut Window, _: &mut Context<Self>) {
        window.focus_next();
    }

    fn on_focus_previous(&mut self, _: &FocusPrevious, window: &mut Window, _: &mut Context<Self>) {
        window.focus_prev();
    }
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let projection = self.controller.read(cx).projection();

        div()
            .id("runtime-shell")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_connect))
            .on_action(cx.listener(Self::on_retry))
            .on_action(cx.listener(Self::on_stop))
            .on_action(cx.listener(Self::on_activate_recovery))
            .on_action(cx.listener(Self::on_focus_next))
            .on_action(cx.listener(Self::on_focus_previous))
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::canvas())
            .font_family(theme::SANS)
            .text_color(theme::bone())
            .child(titlebar(&projection))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .child(runtime_status(&projection, cx))
                    .child(inspector(&projection)),
            )
    }
}

fn titlebar(projection: &ShellProjection) -> impl IntoElement {
    div()
        .h(px(theme::TITLE_H))
        .px(px(theme::PAD_X))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(20.0))
        .bg(theme::floor())
        .border_b_1()
        .border_color(theme::edge_hard())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .min_w_0()
                .flex_1()
                .child(
                    div()
                        .font_family(theme::DISPLAY)
                        .text_size(px(theme::T_WORDMARK))
                        .font_weight(FontWeight::NORMAL)
                        .flex_shrink_0()
                        .child("pi"),
                )
                .child(
                    div()
                        .w(px(1.0))
                        .h(px(14.0))
                        .bg(theme::edge_hard())
                        .flex_shrink_0(),
                )
                .child(
                    div()
                        .min_w_0()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_TITLE))
                        .font_weight(FontWeight::BOLD)
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(projection.session.label()),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .min_w_0()
                .child(meta(projection.model.label()))
                .child(meta(projection.thinking.label()))
                .child(meta(projection.cost.label()))
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .font_weight(FontWeight::BOLD)
                        .text_color(lifecycle_color(projection))
                        .child(projection.lifecycle.clone()),
                ),
        )
}

fn meta(value: String) -> impl IntoElement {
    div()
        .font_family(theme::MONO)
        .text_size(px(theme::T_MONO_SM))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme::ash())
        .max_w(px(190.0))
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap()
        .child(value)
}

fn runtime_status(projection: &ShellProjection, cx: &mut Context<RootView>) -> impl IntoElement {
    let action = projection.action;
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .px(px(48.0))
        .py(px(36.0))
        .flex()
        .flex_col()
        .justify_between()
        .child(
            div()
                .max_w(px(620.0))
                .flex()
                .flex_col()
                .gap(px(14.0))
                .child(
                    div()
                        .font_family(theme::DISPLAY)
                        .text_size(px(30.0))
                        .line_height(relative(1.08))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(if projection.no_model {
                            theme::error()
                        } else {
                            theme::bone()
                        })
                        .child(projection.headline.clone()),
                )
                .child(
                    div()
                        .max_w(px(560.0))
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_BODY))
                        .line_height(relative(1.5))
                        .text_color(theme::bone_dim())
                        .child(projection.detail.clone()),
                )
                .when(projection.has_stale_values, |content| {
                    content.child(
                        div()
                            .font_family(theme::SANS)
                            .text_size(px(theme::T_UI_SM))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::data())
                            .child("Last valid values remain visible."),
                    )
                })
                .when_some(action, |content, action| {
                    content.child(div().mt(px(8.0)).child(controls::recovery_button(
                        action_id(action),
                        action.label().to_owned(),
                        action.shortcut(),
                        true,
                        Box::new(cx.listener(move |view, _, _, cx| {
                            view.activate_recovery(action, cx);
                        })),
                    )))
                }),
        )
        .child(
            div()
                .pt(px(24.0))
                .border_t_1()
                .border_color(theme::edge_soft())
                .flex()
                .flex_col()
                .gap(px(7.0))
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_LABEL))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::ash())
                        .child("Workspace"),
                )
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_MONO))
                        .line_height(relative(1.35))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::data())
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(projection.workspace.clone()),
                ),
        )
}

fn inspector(projection: &ShellProjection) -> impl IntoElement {
    div()
        .w(px(theme::INSPECT_W))
        .h_full()
        .px(px(18.0))
        .py(px(18.0))
        .flex()
        .flex_col()
        .bg(theme::floor())
        .border_l_1()
        .border_color(theme::edge_hard())
        .child(
            div()
                .pb(px(10.0))
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI_SM))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::bone_dim())
                .child("Runtime inspector"),
        )
        .child(controls::metric("Context", projection.context.label()))
        .child(controls::metric(
            "Input tokens",
            projection.input_tokens.label(),
        ))
        .child(controls::metric(
            "Output tokens",
            projection.output_tokens.label(),
        ))
        .child(controls::metric("Cache", projection.cache.label()))
        .child(controls::metric("Cost", projection.cost.label()))
        .child(controls::metric("Model", projection.model.label()))
        .child(controls::metric("Thinking", projection.thinking.label()))
}

fn action_id(action: RecoveryAction) -> &'static str {
    match action {
        RecoveryAction::Connect => "runtime-connect",
        RecoveryAction::Retry => "runtime-retry",
        RecoveryAction::Stop => "runtime-stop",
    }
}

fn lifecycle_color(projection: &ShellProjection) -> gpui::Rgba {
    match projection.lifecycle.as_str() {
        "Ready" => theme::live(),
        "Running" | "Loading" | "Connecting" | "Cancelling" | "Stopping" => theme::data(),
        "Connection error" | "No model" => theme::error(),
        "Not connected" | "Stopped" => theme::ash(),
        _ => theme::bone_dim(),
    }
}
