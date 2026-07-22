//! Focus-visible controls for runtime recovery.

use gpui::{
    ClickEvent, FontWeight, IntoElement, SharedString, Window, div, prelude::*, px, relative,
};

use crate::actions::RECOVERY_BUTTON_CONTEXT;
use crate::theme;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlTone {
    Normal,
    Danger,
}

pub fn recovery_button(
    id: &'static str,
    label: String,
    shortcut: &'static str,
    enabled: bool,
    on_click: ClickHandler,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(40.0))
        .min_w(px(116.0))
        .px(px(16.0))
        .border_2()
        .border_color(if enabled {
            theme::signal()
        } else {
            theme::edge_hard()
        })
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(10.0))
        .bg(if enabled {
            theme::signal()
        } else {
            theme::panel()
        })
        .text_color(if enabled {
            theme::canvas()
        } else {
            theme::smoke()
        })
        .when(enabled, |button| {
            button
                .tab_index(0)
                .key_context(RECOVERY_BUTTON_CONTEXT)
                .cursor_pointer()
                .hover(|button| {
                    button
                        .bg(theme::signal_hot())
                        .border_color(theme::signal_hot())
                })
                .active(|button| {
                    button
                        .bg(theme::signal_deep())
                        .border_color(theme::signal_deep())
                })
                .focus(|button| button.border_color(theme::focus()))
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI))
                .font_weight(FontWeight::BOLD)
                .child(label),
        )
        .child(
            div()
                .font_family(theme::MONO)
                .text_size(px(theme::T_TINY))
                .font_weight(FontWeight::MEDIUM)
                .opacity(if enabled { 0.72 } else { 0.5 })
                .child(shortcut),
        )
}

pub fn operation_button(
    id: impl Into<String>,
    label: impl Into<String>,
    detail: impl Into<String>,
    enabled: bool,
    tone: ControlTone,
    on_click: ClickHandler,
) -> impl IntoElement {
    let (fill, fill_hover, fill_active, text) = match tone {
        ControlTone::Normal => (
            theme::panel(),
            theme::panel_hover(),
            theme::panel_lift(),
            theme::bone_dim(),
        ),
        ControlTone::Danger => (
            theme::signal_deep(),
            theme::signal(),
            theme::panel_hover(),
            theme::bone(),
        ),
    };

    div()
        .id(SharedString::from(id.into()))
        .min_h(px(38.0))
        .px(px(11.0))
        .py(px(8.0))
        .border_1()
        .border_color(if enabled {
            theme::edge_hard()
        } else {
            theme::edge_soft()
        })
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .bg(if enabled { fill } else { theme::floor() })
        .text_color(if enabled { text } else { theme::smoke() })
        .when(enabled, |button| {
            button
                .tab_index(0)
                .cursor_pointer()
                .hover(move |button| button.bg(fill_hover).border_color(theme::focus()))
                .active(move |button| button.bg(fill_active))
                .focus(|button| button.border_color(theme::focus()))
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI_SM))
                .font_weight(FontWeight::SEMIBOLD)
                .child(label.into()),
        )
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_TINY))
                .line_height(relative(1.25))
                .text_color(if enabled {
                    theme::ash()
                } else {
                    theme::smoke()
                })
                .child(detail.into()),
        )
}

pub fn metric(label: &'static str, value: String) -> impl IntoElement {
    div()
        .min_h(px(44.0))
        .py(px(8.0))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(16.0))
        .border_b_1()
        .border_color(theme::edge_soft())
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI_SM))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::ash())
                .child(label),
        )
        .child(
            div()
                .min_w_0()
                .max_w(px(174.0))
                .font_family(theme::MONO)
                .text_size(px(theme::T_MONO))
                .line_height(relative(1.3))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::bone_dim())
                .text_right()
                .child(value),
        )
}
