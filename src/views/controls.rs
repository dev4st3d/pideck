//! Shared pier-style controls for the live desk shell.

use gpui::{
    ClickEvent, FontWeight, IntoElement, SharedString, Window, div, prelude::*, px, relative, rgba,
};

use crate::actions::RECOVERY_BUTTON_CONTEXT;
use crate::theme;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static>;

fn clear() -> gpui::Rgba {
    rgba(0x0000_0000)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlTone {
    Normal,
    Danger,
}

pub fn meta_text(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .font_family(theme::MONO)
        .text_size(px(theme::T_MONO_SM))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme::ash())
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap()
        .child(text.into())
}

pub fn meta_sep() -> impl IntoElement {
    div()
        .w(px(1.0))
        .h(px(11.0))
        .bg(theme::edge_hard())
        .flex_shrink_0()
}

pub fn section_label(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .font_family(theme::SANS)
        .text_size(px(theme::T_LABEL))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::ash())
        .child(text.into())
}

pub fn status_pill(label: impl Into<SharedString>, color: gpui::Rgba) -> impl IntoElement {
    div()
        .px(px(8.0))
        .py(px(5.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(7.0))
        .child(
            div()
                .w(px(6.0))
                .h(px(6.0))
                .rounded_full()
                .bg(color)
                .flex_shrink_0(),
        )
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI_SM))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(label.into()),
        )
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
        .h(px(34.0))
        .min_w(px(96.0))
        .px(px(12.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .bg(if enabled {
            theme::signal()
        } else {
            theme::panel()
        })
        .border_1()
        .border_color(if enabled {
            theme::signal()
        } else {
            theme::edge_hard()
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
                .text_size(px(theme::T_UI_SM))
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

pub fn quiet_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    enabled: bool,
    on_click: ClickHandler,
) -> impl IntoElement {
    div()
        .id(id.into())
        .h(px(28.0))
        .px(px(8.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .text_color(if enabled {
            theme::bone_dim()
        } else {
            theme::smoke()
        })
        .when(enabled, |button| {
            button
                .tab_index(0)
                .cursor_pointer()
                .hover(|button| button.bg(theme::panel()).text_color(theme::bone()))
                .active(|button| button.bg(theme::panel_lift()))
                .focus(|button| button.border_1().border_color(theme::focus()))
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI_SM))
                .font_weight(FontWeight::SEMIBOLD)
                .child(label.into()),
        )
}

pub fn chip_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    selected: bool,
    enabled: bool,
    on_click: ClickHandler,
) -> impl IntoElement {
    div()
        .id(id.into())
        .h(px(28.0))
        .px(px(10.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .bg(if selected {
            theme::panel_lift()
        } else {
            clear()
        })
        .border_1()
        .border_color(if selected {
            theme::edge_hard()
        } else {
            theme::edge_soft()
        })
        .text_color(if selected {
            theme::bone()
        } else if enabled {
            theme::ash()
        } else {
            theme::smoke()
        })
        .when(enabled, |button| {
            button
                .tab_index(0)
                .cursor_pointer()
                .hover(|button| {
                    if selected {
                        button.bg(theme::panel_hover()).text_color(theme::bone())
                    } else {
                        button.bg(theme::panel()).text_color(theme::bone())
                    }
                })
                .active(|button| button.bg(theme::panel_lift()))
                .focus(|button| button.border_color(theme::focus()))
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_TINY))
                .font_weight(if selected {
                    FontWeight::BOLD
                } else {
                    FontWeight::SEMIBOLD
                })
                .child(label.into()),
        )
}

pub fn panel_note(text: impl Into<SharedString>, tone: ControlTone) -> impl IntoElement {
    let (border, color) = match tone {
        ControlTone::Danger => (theme::error(), theme::error()),
        ControlTone::Normal => (theme::edge_soft(), theme::bone_dim()),
    };
    div()
        .px(px(10.0))
        .py(px(8.0))
        .rounded(px(theme::RADIUS_SM))
        .bg(theme::panel())
        .border_1()
        .border_color(border)
        .font_family(theme::SANS)
        .text_size(px(theme::T_TINY))
        .line_height(relative(1.4))
        .text_color(color)
        .child(text.into())
}

pub fn panel_footer_status(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .px(px(12.0))
        .py(px(8.0))
        .border_t_1()
        .border_color(theme::edge_soft())
        .font_family(theme::MONO)
        .text_size(px(theme::T_TINY))
        .line_height(relative(1.35))
        .text_color(theme::smoke())
        .child(text.into())
}

pub fn action_row(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    enabled: bool,
    tone: ControlTone,
    on_click: ClickHandler,
) -> impl IntoElement {
    let (label_color, detail_color) = match (enabled, tone) {
        (true, ControlTone::Danger) => (theme::error(), theme::ash()),
        (true, _) => (theme::bone(), theme::ash()),
        (false, _) => (theme::smoke(), theme::smoke()),
    };

    div()
        .id(id.into())
        .px(px(11.0))
        .py(px(9.0))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .when(enabled, |row| {
            row.tab_index(0)
                .cursor_pointer()
                .hover(|row| row.bg(theme::panel_hover()))
                .active(|row| row.bg(theme::panel()))
                .focus(|row| row.bg(theme::panel_lift()))
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
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
                        .text_color(label_color)
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(label.into()),
                )
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_TINY))
                        .text_color(detail_color)
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(detail.into()),
                ),
        )
}

pub fn interactive_list_row(
    id: SharedString,
    enabled: bool,
    on_click: ClickHandler,
    child: impl IntoElement,
) -> impl IntoElement {
    row_shell()
        .id(id)
        .when(enabled, |row| {
            row.tab_index(0)
                .cursor_pointer()
                .hover(|row| row.bg(theme::panel_hover()))
                .active(|row| row.bg(theme::panel()))
                .focus(|row| row.bg(theme::panel_lift()))
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .child(child)
}

fn row_shell() -> gpui::Div {
    div().border_b_1().border_color(theme::edge_soft())
}

pub fn session_row(
    name: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    active: bool,
) -> impl IntoElement {
    div()
        .px(px(14.0))
        .py(px(10.0))
        .border_l_2()
        .border_color(if active { theme::signal() } else { clear() })
        .bg(if active { theme::panel_lift() } else { clear() })
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI))
                .font_weight(if active {
                    FontWeight::BOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(if active {
                    theme::bone()
                } else {
                    theme::bone_dim()
                })
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(name.into()),
        )
        .child(
            div()
                .font_family(theme::MONO)
                .text_size(px(theme::T_TINY))
                .text_color(if active {
                    theme::signal()
                } else {
                    theme::ash()
                })
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(detail.into()),
        )
}

pub fn divider_list() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .border_1()
        .border_color(theme::edge_soft())
        .rounded(px(theme::RADIUS_SM))
        .overflow_hidden()
        .bg(theme::canvas())
}

pub fn context_block(
    label: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    pct: Option<f32>,
    tokens_in: impl Into<SharedString>,
    tokens_out: impl Into<SharedString>,
    cache: impl Into<SharedString>,
) -> impl IntoElement {
    let pct = pct.unwrap_or(0.0).clamp(0.0, 1.0);
    let fill_color = if pct > 0.85 {
        theme::signal()
    } else if pct > 0.0 {
        theme::live()
    } else {
        theme::edge_hard()
    };

    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .px(px(2.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .gap(px(8.0))
                .child(section_label(label))
                .child(
                    div()
                        .min_w_0()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::bone_dim())
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(detail.into()),
                ),
        )
        .child(
            div()
                .h(px(4.0))
                .w_full()
                .rounded(px(2.0))
                .bg(theme::canvas())
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(relative(pct))
                        .when(pct > 0.0, |fill| fill.min_w(px(2.0)))
                        .bg(fill_color),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .gap(px(8.0))
                .child(stat("in", tokens_in))
                .child(stat("out", tokens_out))
                .child(stat("cache", cache)),
        )
}

fn stat(k: &'static str, v: impl Into<SharedString>) -> impl IntoElement {
    div()
        .min_w_0()
        .flex_1()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_TINY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::ash())
                .child(k),
        )
        .child(
            div()
                .font_family(theme::MONO)
                .text_size(px(theme::T_MONO_SM))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::bone_dim())
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(v.into()),
        )
}

pub fn metric_row(label: &'static str, value: impl Into<SharedString>) -> impl IntoElement {
    div()
        .min_h(px(34.0))
        .py(px(6.0))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
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
                .flex_1()
                .font_family(theme::MONO)
                .text_size(px(theme::T_MONO_SM))
                .line_height(relative(1.3))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::bone_dim())
                .text_right()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(value.into()),
        )
}

pub fn queue_row(
    mode: impl Into<SharedString>,
    preview: impl Into<SharedString>,
) -> impl IntoElement {
    div()
        .px(px(11.0))
        .py(px(10.0))
        .flex()
        .flex_row()
        .items_start()
        .gap(px(10.0))
        .child(
            div()
                .font_family(theme::MONO)
                .text_size(px(theme::T_TINY))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::data())
                .child(mode.into()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI_SM))
                .line_height(relative(1.45))
                .text_color(theme::bone_dim())
                .child(preview.into()),
        )
}

pub fn empty_list_note(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .px(px(11.0))
        .py(px(10.0))
        .font_family(theme::SANS)
        .text_size(px(theme::T_UI_SM))
        .text_color(theme::smoke())
        .child(text.into())
}
