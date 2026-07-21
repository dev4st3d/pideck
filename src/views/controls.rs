//! Interactive controls for the harness desk.

use gpui::{
    ClickEvent, FontWeight, IntoElement, SharedString, Window, div, prelude::*, px, relative,
};

use crate::{state::SideSection, theme};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static>;

fn clear() -> gpui::Rgba {
    gpui::rgba(0x0000_0000)
}

pub fn side_tabs(
    active: SideSection,
    on_select: impl Fn(SideSection) -> ClickHandler,
) -> impl IntoElement {
    div()
        .px(px(10.0))
        .pt(px(12.0))
        .pb(px(10.0))
        .border_b_1()
        .border_color(theme::edge_soft())
        .flex()
        .flex_row()
        .gap(px(2.0))
        .children(SideSection::ALL.into_iter().map(move |section| {
            let selected = active == section;
            let handler = on_select(section);
            div()
                .id(SharedString::from(format!("side-{}", section.label())))
                .flex_1()
                .px(px(6.0))
                .py(px(7.0))
                .rounded(px(theme::RADIUS_SM))
                .cursor_pointer()
                .bg(if selected {
                    theme::panel_lift()
                } else {
                    clear()
                })
                .hover(|tab| {
                    if selected {
                        tab
                    } else {
                        tab.bg(theme::panel())
                    }
                })
                .active(|tab| tab.opacity(0.92))
                .on_click(move |event, window, cx| handler(event, window, cx))
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .font_weight(if selected {
                            FontWeight::BOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(if selected {
                            theme::bone()
                        } else {
                            theme::ash()
                        })
                        .text_center()
                        .child(section.label()),
                )
        }))
}

pub fn primary_button(
    id: &'static str,
    label: &'static str,
    on_click: ClickHandler,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(42.0))
        .min_w(px(76.0))
        .px(px(16.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(theme::signal())
        .hover(|button| button.bg(theme::signal_hot()))
        .active(|button| button.bg(theme::signal_deep()))
        .on_click(move |event, window, cx| on_click(event, window, cx))
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI_SM))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::canvas())
                .child(label),
        )
}

pub fn status_button(
    id: &'static str,
    label: &'static str,
    color: gpui::Rgba,
    on_click: ClickHandler,
) -> impl IntoElement {
    div()
        .id(id)
        .px(px(8.0))
        .py(px(5.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(7.0))
        .cursor_pointer()
        .hover(|button| button.bg(theme::panel_lift()))
        .active(|button| button.bg(theme::panel()))
        .on_click(move |event, window, cx| on_click(event, window, cx))
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
                .child(label),
        )
}

pub fn list_row(child: impl IntoElement) -> impl IntoElement {
    row_shell().child(child)
}

pub fn interactive_list_row(
    id: SharedString,
    on_click: ClickHandler,
    child: impl IntoElement,
) -> impl IntoElement {
    row_shell()
        .id(id)
        .cursor_pointer()
        .hover(|row| row.bg(theme::panel_hover()))
        .active(|row| row.bg(theme::panel()))
        .on_click(move |event, window, cx| on_click(event, window, cx))
        .child(child)
}

fn row_shell() -> gpui::Div {
    div().border_b_1().border_color(theme::edge_soft())
}

pub fn meta_text(text: &'static str) -> impl IntoElement {
    div()
        .font_family(theme::MONO)
        .text_size(px(theme::T_MONO_SM))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme::ash())
        .child(text)
}

pub fn meta_sep() -> impl IntoElement {
    div()
        .w(px(1.0))
        .h(px(11.0))
        .bg(theme::edge_hard())
        .flex_shrink_0()
}

pub fn composer_field(placeholder: bool, text: String) -> impl IntoElement {
    div()
        .id("composer-field")
        .flex_1()
        .min_w_0()
        .h(px(42.0))
        .px(px(14.0))
        .flex()
        .items_center()
        .rounded(px(theme::RADIUS_SM))
        .bg(theme::canvas())
        .border_1()
        .border_color(theme::edge())
        .hover(|field| field.border_color(theme::edge_hard()).bg(theme::floor()))
        .child(
            div()
                .w_full()
                .font_family(theme::SANS)
                .text_size(px(theme::T_BODY_SM))
                .font_weight(FontWeight::MEDIUM)
                .line_height(relative(1.4))
                .text_color(if placeholder {
                    theme::ash()
                } else {
                    theme::bone()
                })
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(text),
        )
}
