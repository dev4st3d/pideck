//! Shared pier-style controls for the live desk shell.

use std::{rc::Rc, time::Duration};

use gpui::{
    Animation, AnimationExt, ClickEvent, DispatchPhase, FontWeight, HitboxBehavior, IntoElement,
    ScrollHandle, ScrollWheelEvent, SharedString, Window, canvas, deferred, div, ease_out_quint,
    point, prelude::*, px, relative, rgba, svg,
};

use crate::actions::RECOVERY_BUTTON_CONTEXT;
use crate::theme;

pub(crate) type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static>;
pub type HoverHandler = Rc<dyn Fn(&bool, &mut Window, &mut gpui::App) + 'static>;

fn clear() -> gpui::Rgba {
    rgba(0x0000_0000)
}

/// Routes wheel input to a scroll handle based on pointer position rather than
/// keyboard focus. This is intentionally a capture-phase listener so popovers
/// and modal panes do not leak wheel events into the transcript underneath.
pub fn scroll_wheel_capture(scroll: &ScrollHandle) -> impl IntoElement {
    let scroll = scroll.clone();
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            let scroll = scroll.clone();
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture || !hitbox.should_handle_scroll(window) {
                    return;
                }

                let delta = event.delta.pixel_delta(window.line_height());
                if delta.x == px(0.0) && delta.y == px(0.0) {
                    return;
                }

                let before = scroll.offset();
                let max = scroll.max_offset();
                let next = point(
                    (before.x + delta.x).clamp(-max.width, px(0.0)),
                    (before.y + delta.y).clamp(-max.height, px(0.0)),
                );
                if next != before {
                    scroll.set_offset(next);
                    window.refresh();
                }
                cx.stop_propagation();
            });
        },
    )
    .absolute()
    .size_full()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlTone {
    Normal,
    Danger,
}

pub fn meta_text(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .font_family(theme::mono())
        .text_size(theme::text_size(theme::T_MONO_SM))
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
        .font_family(theme::main())
        .text_size(theme::text_size(theme::T_LABEL))
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
                .font_family(theme::main())
                .text_size(theme::text_size(theme::T_UI_SM))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(label.into()),
        )
}

/// Four-corner activity mark shared by task rows and the titlebar runtime state.
pub fn square_status_indicator(
    animation_key: usize,
    animated: bool,
    cycle: Duration,
    color: gpui::Rgba,
) -> gpui::AnyElement {
    if !animated {
        return div()
            .relative()
            .flex_shrink_0()
            .size(px(8.0))
            .child(
                div()
                    .absolute()
                    .left(px(1.0))
                    .top(px(1.0))
                    .size(px(6.0))
                    .bg(color),
            )
            .into_any_element();
    }

    div()
        .relative()
        .flex_shrink_0()
        .size(px(8.0))
        .children([
            square_status_dot(animation_key, 0, px(0.0), px(0.0), cycle, color),
            square_status_dot(animation_key, 1, px(6.0), px(0.0), cycle, color),
            square_status_dot(animation_key, 2, px(6.0), px(6.0), cycle, color),
            square_status_dot(animation_key, 3, px(0.0), px(6.0), cycle, color),
        ])
        .into_any_element()
}

fn square_status_dot(
    animation_key: usize,
    dot_index: usize,
    left: gpui::Pixels,
    top: gpui::Pixels,
    cycle: Duration,
    color: gpui::Rgba,
) -> gpui::AnyElement {
    div()
        .absolute()
        .left(left)
        .top(top)
        .size(px(2.0))
        .bg(color)
        .with_animation(
            (
                "square-status",
                animation_key.wrapping_mul(4).wrapping_add(dot_index),
            ),
            Animation::new(cycle).repeat(),
            move |dot, progress| {
                let target = dot_index as f32 * 0.25;
                let distance = ((progress - target + 0.5).rem_euclid(1.0) - 0.5).abs();
                dot.opacity(0.28 + (1.0 - distance / 0.25).max(0.0) * 0.72)
            },
        )
        .into_any_element()
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
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .child(
            div()
                .font_family(theme::main())
                .text_size(theme::text_size(theme::T_UI_SM))
                .font_weight(FontWeight::BOLD)
                .child(label),
        )
        .child(
            div()
                .font_family(theme::mono())
                .text_size(theme::text_size(theme::T_TINY))
                .font_weight(FontWeight::MEDIUM)
                .opacity(if enabled { 0.72 } else { 0.5 })
                .child(shortcut),
        )
}

pub fn icon_button(
    id: impl Into<SharedString>,
    glyph: impl Into<SharedString>,
    selected: bool,
    enabled: bool,
    on_click: ClickHandler,
) -> impl IntoElement {
    div()
        .id(id.into())
        .size(px(28.0))
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
            clear()
        })
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
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .child(
            div()
                .relative()
                .top(px(-1.0))
                .font_family(theme::sans())
                .text_size(theme::text_size(16.0))
                .line_height(gpui::relative(1.0))
                .font_weight(FontWeight::MEDIUM)
                .child(glyph.into()),
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
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .child(
            div()
                .font_family(theme::main())
                .text_size(theme::text_size(theme::T_UI_SM))
                .font_weight(FontWeight::SEMIBOLD)
                .child(label.into()),
        )
}

/// Dense toolbar button for inspector run actions.
pub fn tone_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    enabled: bool,
    tone: ControlTone,
    on_click: ClickHandler,
) -> impl IntoElement {
    let (idle_bg, idle_border, idle_text, hot_bg, hot_border, hot_text) = match (enabled, tone) {
        (true, ControlTone::Danger) => (
            theme::panel(),
            theme::edge_hard(),
            theme::error(),
            theme::panel_hover(),
            theme::error(),
            theme::error(),
        ),
        (true, ControlTone::Normal) => (
            theme::panel_lift(),
            theme::edge_hard(),
            theme::bone(),
            theme::panel_hover(),
            theme::edge(),
            theme::bone(),
        ),
        (false, _) => (
            clear(),
            theme::edge_soft(),
            theme::smoke(),
            clear(),
            theme::edge_soft(),
            theme::smoke(),
        ),
    };

    div()
        .id(id.into())
        .h(px(28.0))
        .px(px(10.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .flex_1()
        .min_w_0()
        .bg(idle_bg)
        .border_1()
        .border_color(idle_border)
        .text_color(idle_text)
        .when(enabled, |button| {
            button
                .tab_index(0)
                .cursor_pointer()
                .hover(move |button| {
                    button
                        .bg(hot_bg)
                        .border_color(hot_border)
                        .text_color(hot_text)
                })
                .active(|button| button.bg(theme::panel()))
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .child(
            div()
                .font_family(theme::main())
                .text_size(theme::text_size(theme::T_TINY))
                .font_weight(FontWeight::SEMIBOLD)
                .child(label.into()),
        )
}

/// Label + trailing control on one baseline, for inspector settings.
pub fn setting_row(
    label: impl Into<SharedString>,
    detail: Option<impl Into<SharedString>>,
    control: impl IntoElement,
) -> impl IntoElement {
    div()
        .min_h(px(32.0))
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
                .flex_col()
                .gap(px(1.0))
                .child(
                    div()
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_UI_SM))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::bone_dim())
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(label.into()),
                )
                .when_some(detail, |col, detail| {
                    col.child(
                        div()
                            .font_family(theme::mono())
                            .text_size(theme::text_size(theme::T_TINY))
                            .text_color(theme::smoke())
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(detail.into()),
                    )
                }),
        )
        .child(div().flex_shrink_0().child(control))
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
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .child(
            div()
                .font_family(theme::main())
                .text_size(theme::text_size(theme::T_TINY))
                .font_weight(if selected {
                    FontWeight::BOLD
                } else {
                    FontWeight::SEMIBOLD
                })
                .child(label.into()),
        )
}

/// Compact select trigger for model / thinking controls in the prompt chrome.
pub fn compact_select(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    open: bool,
    enabled: bool,
    max_width: f32,
    on_click: ClickHandler,
) -> impl IntoElement {
    div()
        .id(id.into())
        .h(px(26.0))
        .max_w(px(max_width))
        .px(px(8.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .flex_shrink_0()
        // The prompt already supplies the containing surface. Keep the resting
        // trigger quiet instead of stacking a dark bordered box inside it.
        .bg(if open {
            theme::panel_lift()
        } else {
            theme::panel()
        })
        .border_1()
        .border_color(if open {
            theme::edge_hard()
        } else {
            theme::panel()
        })
        .text_color(if !enabled {
            theme::smoke()
        } else if open {
            theme::bone()
        } else {
            theme::bone_dim()
        })
        .when(enabled, |button| {
            button
                .tab_index(0)
                .cursor_pointer()
                .hover(|button| {
                    button
                        .bg(theme::panel_lift())
                        .border_color(theme::panel_lift())
                        .text_color(theme::bone())
                })
                .active(|button| button.bg(theme::panel_hover()))
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .child(
            div()
                .min_w_0()
                .flex_1()
                .font_family(theme::main())
                .text_size(theme::text_size(theme::T_LABEL))
                .font_weight(FontWeight::SEMIBOLD)
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(label.into()),
        )
        .child(
            svg()
                .path(if open {
                    "icons/chevron-up.svg"
                } else {
                    "icons/chevron-down.svg"
                })
                .size(px(12.0))
                .text_color(if open { theme::data() } else { theme::smoke() })
                .flex_shrink_0(),
        )
}

/// Quiet icon-sized action for dense toolbars.
pub fn chrome_action(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    enabled: bool,
    on_click: ClickHandler,
) -> impl IntoElement {
    div()
        .id(id.into())
        .h(px(20.0))
        .px(px(6.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .text_color(theme::smoke())
        .when(enabled, |button| {
            button
                .tab_index(0)
                .cursor_pointer()
                .hover(|button| button.bg(theme::canvas()).text_color(theme::bone_dim()))
                .active(|button| button.bg(theme::panel_lift()))
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .child(
            div()
                .font_family(theme::main())
                .text_size(theme::text_size(10.0))
                .font_weight(FontWeight::MEDIUM)
                .child(label.into()),
        )
}

/// Bare SVG action for dense toolbars.
pub fn chrome_icon_action(
    id: impl Into<SharedString>,
    icon_path: impl Into<SharedString>,
    enabled: bool,
    on_click: ClickHandler,
) -> impl IntoElement {
    chrome_icon_button(id, icon_path, false, enabled, on_click)
}

/// Toolbar SVG toggle with a selected (pressed-in) appearance.
pub fn chrome_icon_toggle(
    id: impl Into<SharedString>,
    icon_path: impl Into<SharedString>,
    selected: bool,
    enabled: bool,
    on_click: ClickHandler,
) -> impl IntoElement {
    chrome_icon_button(id, icon_path, selected, enabled, on_click)
}

fn chrome_icon_button(
    id: impl Into<SharedString>,
    icon_path: impl Into<SharedString>,
    selected: bool,
    enabled: bool,
    on_click: ClickHandler,
) -> impl IntoElement {
    let icon_color = if selected {
        theme::bone_dim()
    } else {
        theme::smoke()
    };
    div()
        .id(id.into())
        .size(px(20.0))
        .rounded(px(theme::RADIUS_SM))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .bg(if selected { theme::canvas() } else { clear() })
        .text_color(icon_color)
        .when(enabled, |button| {
            button
                .tab_index(0)
                .cursor_pointer()
                .hover(|button| button.bg(theme::canvas()).text_color(theme::bone_dim()))
                .active(|button| button.bg(theme::panel_lift()))
                .on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .child(
            svg()
                .path(icon_path.into())
                .size(px(13.0))
                .text_color(icon_color),
        )
}

/// Segmented tab track for multi-section settings.
pub fn tab_track() -> gpui::Div {
    div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(2.0))
        .p(px(3.0))
        .rounded(px(theme::RADIUS_SM))
        .bg(theme::canvas())
        .border_1()
        .border_color(theme::edge_soft())
}

pub fn tab_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    selected: bool,
    on_click: ClickHandler,
) -> impl IntoElement {
    div()
        .id(id.into())
        .h(px(28.0))
        .px(px(12.0))
        .rounded(px(2.0))
        .flex()
        .flex_1()
        .min_w_0()
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
            clear()
        })
        .text_color(if selected {
            theme::bone()
        } else {
            theme::ash()
        })
        .tab_index(0)
        .cursor_pointer()
        .hover(|button| {
            if selected {
                button.bg(theme::panel_hover())
            } else {
                button.bg(theme::panel()).text_color(theme::bone())
            }
        })
        .active(|button| button.bg(theme::panel_lift()))
        .on_click(move |event, window, cx| on_click(event, window, cx))
        .child(
            div()
                .font_family(theme::main())
                .text_size(theme::text_size(theme::T_TINY))
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
        .font_family(theme::sans())
        .text_size(theme::text_size(theme::T_TINY))
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
        .font_family(theme::mono())
        .text_size(theme::text_size(theme::T_TINY))
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
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_UI_SM))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(label_color)
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(label.into()),
                )
                .child(
                    div()
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_TINY))
                        .text_color(detail_color)
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(detail.into()),
                ),
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

pub struct SessionUsageParams {
    pub context: SharedString,
    pub pct: Option<f32>,
    pub model: SharedString,
    pub thinking: SharedString,
    pub cost: SharedString,
    pub tokens_in: SharedString,
    pub tokens_out: SharedString,
    pub cache_read: SharedString,
    pub cache_write: SharedString,
    pub tooltip_visible: bool,
    pub tooltip_hovered: bool,
    pub tooltip_epoch: u64,
    pub on_hover: HoverHandler,
}

pub fn session_usage(params: SessionUsageParams) -> impl IntoElement {
    let SessionUsageParams {
        context,
        pct,
        model,
        thinking,
        cost,
        tokens_in,
        tokens_out,
        cache_read,
        cache_write,
        tooltip_visible,
        tooltip_hovered,
        tooltip_epoch,
        on_hover,
    } = params;
    let pct = pct.unwrap_or(0.0).clamp(0.0, 1.0);
    let fill_color = usage_fill(pct);
    let tooltip = SessionUsageTooltip {
        context: context.clone(),
        model: model.clone(),
        thinking: thinking.clone(),
        cost: cost.clone(),
        tokens_in,
        tokens_out,
        cache_read,
        cache_write,
    };

    let tooltip_animation = Animation::new(Duration::from_millis(if tooltip_hovered {
        110
    } else {
        90
    }))
    .with_easing(ease_out_quint());
    let summary_hover = Rc::clone(&on_hover);
    let tooltip_hover = Rc::clone(&on_hover);

    div()
        .id("session-usage")
        .relative()
        .w_full()
        .px(px(10.0))
        .py(px(9.0))
        .rounded(px(theme::RADIUS))
        .bg(theme::panel())
        .border_1()
        .border_color(theme::edge_soft())
        .hover(|summary| {
            summary
                .bg(theme::panel_lift())
                .border_color(theme::edge_hard())
        })
        .on_hover(move |hovered, window, cx| (summary_hover)(hovered, window, cx))
        .flex()
        .flex_col()
        .gap(px(7.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .gap(px(10.0))
                .child(section_label("Context"))
                .child(
                    div()
                        .min_w_0()
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_TINY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::bone_dim())
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(context),
                ),
        )
        .child(
            div()
                .h(px(3.0))
                .w_full()
                .rounded(px(2.0))
                .bg(theme::canvas())
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(relative(pct))
                        .when(pct > 0.0, |fill| fill.min_w(px(2.0)))
                        .rounded(px(2.0))
                        .bg(fill_color),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .font_family(theme::sans())
                        .text_size(theme::text_size(theme::T_TINY))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::bone_dim())
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(model),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_TINY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::ash())
                        .child(format!("{thinking} · {cost}")),
                ),
        )
        .when(tooltip_visible, |summary| {
            let appearing = tooltip_hovered;
            summary.child(deferred(
                tooltip
                    .element()
                    .id("session-usage-tooltip-surface")
                    .absolute()
                    .top(relative(1.0))
                    .left_0()
                    .right_0()
                    .mt(px(6.0))
                    .occlude()
                    .on_hover(move |hovered, window, cx| (tooltip_hover)(hovered, window, cx))
                    .with_animation(
                        ("session-usage-tooltip", tooltip_epoch),
                        tooltip_animation,
                        move |tooltip, progress| {
                            tooltip.opacity(if appearing { progress } else { 1.0 - progress })
                        },
                    ),
            ))
        })
}

fn usage_fill(pct: f32) -> gpui::Rgba {
    if pct > 0.85 {
        theme::signal()
    } else if pct > 0.0 {
        theme::live()
    } else {
        theme::edge_hard()
    }
}

#[derive(Clone)]
struct SessionUsageTooltip {
    context: SharedString,
    model: SharedString,
    thinking: SharedString,
    cost: SharedString,
    tokens_in: SharedString,
    tokens_out: SharedString,
    cache_read: SharedString,
    cache_write: SharedString,
}

impl SessionUsageTooltip {
    fn element(&self) -> gpui::Div {
        div()
            .w_full()
            .p(px(12.0))
            .rounded(px(theme::RADIUS))
            .bg(theme::panel_lift())
            .border_1()
            .border_color(theme::edge_hard())
            .flex()
            .flex_col()
            .gap(px(11.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_baseline()
                    .justify_between()
                    .gap(px(12.0))
                    .child(
                        div()
                            .font_family(theme::main())
                            .text_size(theme::text_size(theme::T_UI_SM))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::bone())
                            .child("Session usage"),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .font_family(theme::mono())
                            .text_size(theme::text_size(theme::T_TINY))
                            .text_color(theme::ash())
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(self.context.clone()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .font_family(theme::sans())
                            .text_size(theme::text_size(theme::T_UI_SM))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::bone_dim())
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(self.model.clone()),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_family(theme::mono())
                            .text_size(theme::text_size(theme::T_TINY))
                            .text_color(theme::ash())
                            .child(format!("{} · {}", self.thinking, self.cost)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(8.0))
                            .child(usage_stat("Input", self.tokens_in.clone()))
                            .child(usage_stat("Output", self.tokens_out.clone())),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(8.0))
                            .child(usage_stat("Cache read", self.cache_read.clone()))
                            .child(usage_stat("Cache write", self.cache_write.clone())),
                    ),
            )
    }
}

fn usage_stat(label: &'static str, value: SharedString) -> impl IntoElement {
    div()
        .min_w_0()
        .flex_1()
        .flex()
        .flex_col()
        .gap(px(2.0))
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
                .font_family(theme::mono())
                .text_size(theme::text_size(theme::T_MONO_SM))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::bone_dim())
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(value),
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
                .font_family(theme::sans())
                .text_size(theme::text_size(theme::T_UI_SM))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::ash())
                .child(label),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .font_family(theme::mono())
                .text_size(theme::text_size(theme::T_MONO_SM))
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
                .font_family(theme::mono())
                .text_size(theme::text_size(theme::T_TINY))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::data())
                .child(mode.into()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .font_family(theme::sans())
                .text_size(theme::text_size(theme::T_UI_SM))
                .line_height(relative(1.45))
                .text_color(theme::bone_dim())
                .child(preview.into()),
        )
}

pub fn empty_list_note(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .px(px(11.0))
        .py(px(10.0))
        .font_family(theme::sans())
        .text_size(theme::text_size(theme::T_UI_SM))
        .text_color(theme::smoke())
        .child(text.into())
}
