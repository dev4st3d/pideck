//! Settings dialog for the terminal font and keyboard shortcuts.
//!
//! Choices apply immediately. The dialog only reports them as events; the
//! manager owns persistence and rebinding so a failed save never blocks typing.

use gpui::{
    App, Context, Div, EventEmitter, FocusHandle, FontWeight, IntoElement, KeyBinding,
    KeyDownEvent, Keystroke, MouseButton, Render, ScrollHandle, SharedString, Stateful,
    Subscription, Window, actions, div, prelude::*, px, svg,
};
use gpui_component::scroll::{Scrollbar, ScrollbarShow};

use super::terminal_manager::{button, check_mark, text_tooltip};
use crate::fonts;
use crate::services::keymap::{self, Keymap, Shortcut, ShortcutGroup};
use crate::services::terminal_font::{
    MAX_LINE_HEIGHT, MAX_SIZE, MIN_LINE_HEIGHT, MIN_SIZE, TerminalFont,
};
use crate::theme;
use crate::theme::terminal_manager as chrome;

actions!(settings, [FocusNextControl]);

const DIALOG_CONTEXT: &str = "SettingsDialog";

/// Shortcut table columns, shared by the header and every row so they align.
const CHIP_COLUMN_WIDTH: f32 = 236.0;
const ROW_BUTTON_WIDTH: f32 = 60.0;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SettingsEvent {
    FontChanged(TerminalFont),
    ShortcutChanged(Shortcut, Keystroke),
    ShortcutReset(Shortcut),
    AllShortcutsReset,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsTab {
    Terminal,
    Keyboard,
}

pub(crate) struct SettingsPanel {
    tab: SettingsTab,
    families: Vec<SharedString>,
    family_cursor: usize,
    shortcut_cursor: usize,
    recording: Option<Shortcut>,
    rejection: Option<String>,
    focus: FocusHandle,
    first_stop: FocusHandle,
    last_stop: FocusHandle,
    family_focus: FocusHandle,
    shortcut_focus: FocusHandle,
    family_scroll: ScrollHandle,
    shortcut_scroll: ScrollHandle,
    _recorder: Option<Subscription>,
    _observers: [Subscription; 2],
}

impl EventEmitter<SettingsEvent> for SettingsPanel {}

impl SettingsPanel {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let mut families = fonts::terminal_families(cx);
        let current = TerminalFont::global(cx).family.clone();
        if !families.contains(&current) {
            families.push(current.clone());
        }
        let family_cursor = families
            .iter()
            .position(|family| *family == current)
            .unwrap_or(0);
        Self {
            tab: SettingsTab::Terminal,
            families,
            family_cursor,
            shortcut_cursor: 0,
            recording: None,
            rejection: None,
            focus: cx.focus_handle(),
            first_stop: cx.focus_handle(),
            last_stop: cx.focus_handle(),
            family_focus: cx.focus_handle(),
            shortcut_focus: cx.focus_handle(),
            family_scroll: ScrollHandle::new(),
            shortcut_scroll: ScrollHandle::new(),
            _recorder: None,
            _observers: [
                cx.observe_global::<Keymap>(|_, cx| cx.notify()),
                cx.observe_global::<TerminalFont>(|_, cx| cx.notify()),
            ],
        }
    }

    /// GPUI Component's Root binds Tab to "focus next" ahead of any key
    /// handler. A deeper context outranks it, which is how the dialog keeps
    /// focus from leaking into the workbench behind it.
    pub(crate) fn bind_keys(cx: &mut App) {
        cx.bind_keys([KeyBinding::new(
            "tab",
            FocusNextControl,
            Some(DIALOG_CONTEXT),
        )]);
    }

    pub(crate) fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    fn select_tab(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.stop_recording();
            self.tab = tab;
            cx.notify();
        }
    }

    fn choose_family(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(family) = self.families.get(index).cloned() else {
            return;
        };
        self.family_cursor = index;
        let font = TerminalFont::global(cx);
        if font.family != family {
            cx.emit(SettingsEvent::FontChanged(font.with_family(family)));
        }
        cx.notify();
    }

    fn step_size(&mut self, direction: i8, cx: &mut Context<Self>) {
        if let Some(font) = TerminalFont::global(cx).stepped_size(direction) {
            cx.emit(SettingsEvent::FontChanged(font));
        }
    }

    fn step_line_height(&mut self, direction: i8, cx: &mut Context<Self>) {
        if let Some(font) = TerminalFont::global(cx).stepped_line_height(direction) {
            cx.emit(SettingsEvent::FontChanged(font));
        }
    }

    fn reset_font(&mut self, cx: &mut Context<Self>) {
        let font = TerminalFont::default();
        self.family_cursor = self
            .families
            .iter()
            .position(|family| *family == font.family)
            .unwrap_or(0);
        self.family_scroll.scroll_to_item(self.family_cursor);
        cx.emit(SettingsEvent::FontChanged(font));
        cx.notify();
    }

    /// Capture the next chord before GPUI resolves it, so pressing an existing
    /// shortcut records it instead of running it.
    fn start_recording(&mut self, shortcut: Shortcut, cx: &mut Context<Self>) {
        self.recording = Some(shortcut);
        self.rejection = None;
        let panel = cx.weak_entity();
        self._recorder = Some(cx.intercept_keystrokes(move |event, _, cx| {
            cx.stop_propagation();
            let keystroke = event.keystroke.clone();
            let _ = panel.update(cx, |panel, cx| panel.record(keystroke, cx));
        }));
        cx.notify();
    }

    fn stop_recording(&mut self) {
        self.recording = None;
        self.rejection = None;
        self._recorder = None;
    }

    fn record(&mut self, keystroke: Keystroke, cx: &mut Context<Self>) {
        let Some(shortcut) = self.recording else {
            return;
        };
        // A modifier on its own is only the start of a chord.
        if keymap::is_modifier_key(&keystroke.key) {
            return;
        }
        if keystroke.key == "escape" && !keystroke.modifiers.modified() {
            self.stop_recording();
            cx.notify();
            return;
        }
        match Keymap::global(cx).check(shortcut, &keystroke) {
            Ok(()) => {
                self.stop_recording();
                cx.emit(SettingsEvent::ShortcutChanged(shortcut, keystroke));
            }
            Err(rejection) => self.rejection = Some(rejection.message()),
        }
        cx.notify();
    }

    fn toggle_recording(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(shortcut) = Shortcut::ALL.get(index).copied() else {
            return;
        };
        self.shortcut_cursor = index;
        if self.recording == Some(shortcut) {
            self.stop_recording();
            cx.notify();
        } else {
            self.start_recording(shortcut, cx);
        }
    }

    fn reset_shortcut(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(shortcut) = Shortcut::ALL.get(index).copied() else {
            return;
        };
        self.shortcut_cursor = index;
        if self.recording == Some(shortcut) {
            self.stop_recording();
        }
        if Keymap::global(cx).is_customized(shortcut) {
            cx.emit(SettingsEvent::ShortcutReset(shortcut));
        }
        cx.notify();
    }

    fn reset_all_shortcuts(&mut self, cx: &mut Context<Self>) {
        self.stop_recording();
        cx.emit(SettingsEvent::AllShortcutsReset);
        cx.notify();
    }

    fn move_shortcut_cursor(&mut self, index: usize) {
        self.shortcut_cursor = index.min(Shortcut::ALL.len() - 1);
        self.shortcut_scroll
            .scroll_to_item(shortcut_child_index(self.shortcut_cursor));
    }

    fn on_family_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let last = self.families.len().saturating_sub(1);
        match event.keystroke.key.as_str() {
            "up" => self.family_cursor = self.family_cursor.saturating_sub(1),
            "down" => self.family_cursor = (self.family_cursor + 1).min(last),
            "home" => self.family_cursor = 0,
            "end" => self.family_cursor = last,
            "enter" | "space" => self.choose_family(self.family_cursor, cx),
            _ => return,
        }
        self.family_scroll.scroll_to_item(self.family_cursor);
        cx.stop_propagation();
        cx.notify();
    }

    fn on_shortcut_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let last = Shortcut::ALL.len() - 1;
        match event.keystroke.key.as_str() {
            "up" => self.move_shortcut_cursor(self.shortcut_cursor.saturating_sub(1)),
            "down" => self.move_shortcut_cursor((self.shortcut_cursor + 1).min(last)),
            "home" => self.move_shortcut_cursor(0),
            "end" => self.move_shortcut_cursor(last),
            "enter" => self.toggle_recording(self.shortcut_cursor, cx),
            "delete" | "backspace" => self.reset_shortcut(self.shortcut_cursor, cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Keep focus movement inside the dialog: Tab and Shift+F6 wrap at either
    /// end instead of reaching the workbench. Shift+Tab never moves focus.
    fn focus_next_control(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_next();
        if !self.focus.contains_focused(window, cx) {
            window.focus(&self.first_stop);
        }
    }

    fn on_dialog_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => cx.emit(SettingsEvent::Closed),
            "f6" if event.keystroke.modifiers.shift => {
                window.focus_prev();
                if !self.focus.contains_focused(window, cx) {
                    window.focus(&self.last_stop);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(chrome::TOOLBAR_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .pl(px(chrome::TOOLBAR_INSET))
            .pr(px(chrome::COMPACT_GAP))
            .border_b_1()
            .border_color(theme::edge())
            .child(
                div()
                    .font_family(chrome::HEADING_FONT)
                    .font_weight(FontWeight::NORMAL)
                    .text_size(px(chrome::CHECKLIST_HEADING_SIZE))
                    .text_color(theme::bone())
                    .child("Settings"),
            )
            .child(
                div()
                    .id("settings-close")
                    .size(px(chrome::CONTROL_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(chrome::CONTROL_RADIUS))
                    .border_1()
                    .border_color(gpui::rgba(0))
                    .tab_index(0)
                    .cursor_pointer()
                    .tooltip(text_tooltip("Close settings · Esc"))
                    .hover(|style| style.bg(theme::panel_hover()))
                    .active(|style| style.bg(theme::selection()))
                    .focus(|style| style.border_color(theme::focus()))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Closed)))
                    .child(
                        svg()
                            .path("icons/close.svg")
                            .size(px(chrome::ICON_SIZE))
                            .text_color(theme::ash()),
                    ),
            )
    }

    fn tab_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = |id: &'static str, label: &'static str, which: SettingsTab| {
            let active = self.tab == which;
            div()
                .id(id)
                .h_full()
                .px(px(chrome::CONTROL_INSET))
                .flex()
                .items_center()
                .border_b_2()
                .border_color(if active {
                    theme::focus()
                } else {
                    gpui::rgba(0)
                })
                .font_weight(if active {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if active { theme::bone() } else { theme::ash() })
                .tab_index(0)
                .cursor_pointer()
                .hover(|style| style.text_color(theme::bone()))
                .focus(|style| style.bg(theme::selection()))
                .on_click(cx.listener(move |view, _, _, cx| view.select_tab(which, cx)))
                .child(label)
        };
        div()
            .h(px(chrome::CONTROL_HEIGHT + chrome::SMALL_GAP))
            .flex_shrink_0()
            .flex()
            .px(px(chrome::INSET))
            .border_b_1()
            .border_color(theme::edge())
            .child(
                tab("settings-tab-terminal", "Terminal", SettingsTab::Terminal)
                    .track_focus(&self.first_stop),
            )
            .child(tab(
                "settings-tab-keyboard",
                "Keyboard",
                SettingsTab::Keyboard,
            ))
    }

    fn terminal_tab(
        &self,
        font: &TerminalFont,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let list_focused = self.family_focus.is_focused(window);
        let family_count = self.families.len();
        let at = |step: Option<TerminalFont>| step.is_some();
        let (can_shrink, can_grow) = (at(font.stepped_size(-1)), at(font.stepped_size(1)));
        let (can_tighten, can_loosen) = (
            at(font.stepped_line_height(-1)),
            at(font.stepped_line_height(1)),
        );
        div()
            .id("settings-terminal")
            .flex_1()
            .min_h_0()
            .px(px(chrome::CONTENT_INSET))
            .py(px(chrome::INSET))
            .flex()
            .gap(px(chrome::CONTENT_INSET))
            .child(
                div()
                    .w(px(chrome::FONT_LIST_WIDTH))
                    .flex_shrink_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap(px(chrome::SMALL_GAP))
                    .child(section_label("Font family"))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h(px(chrome::FONT_ROW_HEIGHT * 4.0))
                            .child(
                                div()
                                    .id("settings-families")
                                    .track_focus(&self.family_focus)
                                    .tab_index(0)
                                    .absolute()
                                    .inset_0()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.family_scroll)
                                    .border_1()
                                    .border_color(theme::edge())
                                    .rounded(px(chrome::CONTROL_RADIUS))
                                    .focus(|style| style.border_color(theme::focus()))
                                    .on_key_down(cx.listener(
                                        |view, event: &KeyDownEvent, _, cx| {
                                            view.on_family_key(event, cx)
                                        },
                                    ))
                                    .children(self.families.iter().enumerate().map(
                                        |(index, family)| {
                                            let current = *family == font.family;
                                            let cursor = self.family_cursor == index;
                                            div()
                                                .id(("settings-family", index))
                                                .relative()
                                                .h(px(chrome::FONT_ROW_HEIGHT))
                                                .pl(px(chrome::COMPACT_GAP))
                                                .pr(px(chrome::SCROLLBAR_GUTTER))
                                                .flex()
                                                .items_center()
                                                .gap(px(chrome::COMPACT_GAP))
                                                .cursor_pointer()
                                                .when(cursor, |row| row.bg(theme::selection()))
                                                .hover(|style| style.bg(theme::panel_hover()))
                                                .on_click(cx.listener(
                                                    move |view, _, window, cx| {
                                                        window.focus(&view.family_focus);
                                                        view.choose_family(index, cx)
                                                    },
                                                ))
                                                .when(cursor && list_focused, |row| {
                                                    row.child(focus_bar())
                                                })
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .truncate()
                                                        .font_family(family.clone())
                                                        .text_size(px(14.0))
                                                        .child(family.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .w(px(chrome::ICON_SIZE))
                                                        .flex_shrink_0()
                                                        .text_color(theme::focus())
                                                        .when(current, |mark| {
                                                            mark.child(check_mark())
                                                        }),
                                                )
                                        },
                                    )),
                            )
                            .child(scroll_indicator(&self.family_scroll)),
                    )
                    .child(hint(if family_count == 1 {
                        "Only IBM Plex Mono was found. Installed monospace fonts appear here."
                            .to_owned()
                    } else {
                        format!("{family_count} fonts · ↑ ↓ then Enter to choose")
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap(px(chrome::GAP))
                    .child(
                        div()
                            .flex()
                            .gap(px(chrome::CONTENT_INSET))
                            .child(stepper(
                                "Size",
                                format!("{MIN_SIZE}–{MAX_SIZE} px"),
                                format!("{} px", font.size),
                                step_button("size-down", "−", "Smaller text", can_shrink)
                                    .on_click(cx.listener(|view, _, _, cx| view.step_size(-1, cx))),
                                step_button("size-up", "+", "Larger text", can_grow)
                                    .on_click(cx.listener(|view, _, _, cx| view.step_size(1, cx))),
                            ))
                            .child(stepper(
                                "Line height",
                                format!("{MIN_LINE_HEIGHT:.1}–{MAX_LINE_HEIGHT:.1}×"),
                                format!("{:.1}×", font.line_height),
                                step_button("line-height-down", "−", "Tighter rows", can_tighten)
                                    .on_click(
                                        cx.listener(|view, _, _, cx| view.step_line_height(-1, cx)),
                                    ),
                                step_button("line-height-up", "+", "Looser rows", can_loosen)
                                    .on_click(
                                        cx.listener(|view, _, _, cx| view.step_line_height(1, cx)),
                                    ),
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .gap(px(chrome::SMALL_GAP))
                            .child(section_label("Preview"))
                            .child(preview(font)),
                    ),
            )
    }

    fn keyboard_tab(
        &self,
        keymap: &Keymap,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let list_focused = self.shortcut_focus.is_focused(window);
        let mut rows = Vec::new();
        let mut previous_group = None;
        for (index, shortcut) in Shortcut::ALL.into_iter().enumerate() {
            if previous_group != Some(shortcut.group()) {
                previous_group = Some(shortcut.group());
                rows.push(
                    div()
                        .id(("settings-group", index))
                        .h(px(chrome::SHORTCUT_ROW_HEIGHT))
                        .px(px(chrome::COMPACT_GAP))
                        .pt(px(chrome::COMPACT_GAP))
                        .flex()
                        .items_center()
                        .border_b_1()
                        .border_color(theme::edge())
                        .font_family(chrome::HEADING_FONT)
                        .font_weight(FontWeight::NORMAL)
                        .text_size(px(18.0))
                        .text_color(theme::bone())
                        .child(shortcut.group().label())
                        .into_any_element(),
                );
            }
            rows.push(
                self.shortcut_row(index, shortcut, keymap, list_focused, cx)
                    .into_any_element(),
            );
        }
        div()
            .id("settings-keyboard")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .px(px(chrome::INSET))
            .pt(px(chrome::COMPACT_GAP))
            .child(
                div()
                    .h(px(chrome::DETAIL_LINE_HEIGHT + chrome::SMALL_GAP))
                    .px(px(chrome::COMPACT_GAP))
                    .pr(px(chrome::SCROLLBAR_GUTTER + chrome::COMPACT_GAP))
                    .flex()
                    .items_center()
                    .gap(px(chrome::GAP))
                    .child(div().flex_1().child(section_label("Action")))
                    .child(
                        div()
                            .w(px(CHIP_COLUMN_WIDTH))
                            .flex_shrink_0()
                            .child(section_label("Shortcut")),
                    )
                    .child(
                        div()
                            .w(px(ROW_BUTTON_WIDTH * 2.0 + chrome::GAP))
                            .flex_shrink_0(),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("settings-shortcuts")
                            .track_focus(&self.shortcut_focus)
                            .tab_index(0)
                            .absolute()
                            .inset_0()
                            .overflow_y_scroll()
                            .track_scroll(&self.shortcut_scroll)
                            .border_1()
                            .border_color(gpui::rgba(0))
                            .rounded(px(chrome::CONTROL_RADIUS))
                            .focus(|style| style.border_color(theme::focus()))
                            .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                                view.on_shortcut_key(event, cx)
                            }))
                            .children(rows),
                    )
                    .child(scroll_indicator(&self.shortcut_scroll)),
            )
    }

    fn shortcut_row(
        &self,
        index: usize,
        shortcut: Shortcut,
        keymap: &Keymap,
        list_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let recording = self.recording == Some(shortcut);
        let customized = keymap.is_customized(shortcut);
        let selected = self.shortcut_cursor == index;
        div()
            .id(("settings-shortcut", index))
            .relative()
            .min_h(px(chrome::SHORTCUT_ROW_HEIGHT))
            .pl(px(chrome::COMPACT_GAP))
            .pr(px(chrome::SCROLLBAR_GUTTER + chrome::COMPACT_GAP))
            .py(px(chrome::SMALL_GAP / 2.0))
            .flex()
            .flex_col()
            .justify_center()
            .border_b_1()
            .border_color(theme::edge_soft())
            .when(selected, |row| row.bg(theme::selection()))
            .hover(|style| style.bg(theme::panel_hover()))
            .when(selected && list_focused, |row| row.child(focus_bar()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(chrome::GAP))
                    .child(div().flex_1().min_w_0().truncate().child(shortcut.label()))
                    .child(if recording {
                        capture_field().into_any_element()
                    } else {
                        div()
                            .w(px(CHIP_COLUMN_WIDTH))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(chrome::SMALL_GAP))
                            .children(
                                keymap
                                    .bindings(shortcut)
                                    .iter()
                                    .map(|binding| key_chip(keymap::display(binding), customized)),
                            )
                            .into_any_element()
                    })
                    .child(
                        row_button(
                            ("shortcut-change", index),
                            if recording { "Cancel" } else { "Change" },
                        )
                        .tooltip(text_tooltip(if recording {
                            "Stop recording · Esc".to_owned()
                        } else {
                            format!("Change “{}”", shortcut.label())
                        }))
                        .on_click(cx.listener(
                            move |view, _, window, cx| {
                                window.focus(&view.shortcut_focus);
                                view.toggle_recording(index, cx)
                            },
                        )),
                    )
                    .child(div().w(px(ROW_BUTTON_WIDTH)).flex_shrink_0().when(
                        customized,
                        |slot| {
                            slot.child(
                                row_button(("shortcut-reset", index), "Reset")
                                    .tooltip(text_tooltip(format!(
                                        "Restore {}",
                                        default_hint(shortcut)
                                    )))
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        window.focus(&view.shortcut_focus);
                                        view.reset_shortcut(index, cx)
                                    })),
                            )
                        },
                    )),
            )
            .when_some(
                self.rejection.clone().filter(|_| recording),
                |row, message| {
                    row.child(
                        div()
                            .mt(px(chrome::SMALL_GAP / 2.0))
                            .px(px(chrome::COMPACT_GAP))
                            .py(px(chrome::SMALL_GAP / 2.0))
                            .rounded(px(chrome::CONTROL_RADIUS))
                            .bg(theme::error_wash())
                            .text_size(px(chrome::DETAIL_TEXT_SIZE))
                            .line_height(px(chrome::DETAIL_LINE_HEIGHT))
                            .text_color(theme::error())
                            .child(message),
                    )
                },
            )
    }

    fn footer(
        &self,
        font: &TerminalFont,
        keymap: &Keymap,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let keyboard = self.tab == SettingsTab::Keyboard;
        let resettable = if keyboard {
            keymap.has_customizations()
        } else {
            !font.is_default()
        };
        div()
            .h(px(chrome::TOOLBAR_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(chrome::GAP))
            .px(px(chrome::TOOLBAR_INSET))
            .border_t_1()
            .border_color(theme::edge())
            .child(
                button(
                    "settings-reset",
                    if keyboard {
                        "Reset all shortcuts"
                    } else {
                        "Reset font"
                    },
                    resettable,
                )
                .when(resettable, |reset| {
                    reset.on_click(cx.listener(move |view, _, _, cx| {
                        if keyboard {
                            view.reset_all_shortcuts(cx)
                        } else {
                            view.reset_font(cx)
                        }
                    }))
                }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(chrome::DETAIL_TEXT_SIZE))
                    .text_color(theme::ash())
                    .child(if keyboard {
                        "↑ ↓ select · Enter change · Delete reset"
                    } else {
                        "Changes apply to every terminal immediately."
                    }),
            )
            .child(
                done_button()
                    .track_focus(&self.last_stop)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Closed))),
            )
    }
}

impl Render for SettingsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let width = chrome::DIALOG_WIDTH
            .min((f32::from(viewport.width) - chrome::DIALOG_MARGIN).max(320.0));
        let height = chrome::DIALOG_HEIGHT
            .min((f32::from(viewport.height) - chrome::DIALOG_MARGIN * 2.0).max(320.0));
        let font = TerminalFont::global(cx).clone();
        let keymap = Keymap::global(cx).clone();
        div()
            .id("settings-scrim")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme::scrim())
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    if view.recording.is_none() {
                        cx.emit(SettingsEvent::Closed);
                    }
                }),
            )
            .child(
                div()
                    .id("settings-dialog")
                    .key_context(DIALOG_CONTEXT)
                    .track_focus(&self.focus)
                    .w(px(width))
                    .h(px(height))
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .bg(theme::floor())
                    .border_1()
                    .border_color(theme::edge_hard())
                    .rounded(px(chrome::CONTROL_RADIUS))
                    .shadow_lg()
                    .font_family(chrome::CHROME_FONT)
                    .font_weight(FontWeight::NORMAL)
                    .text_size(px(chrome::CONTROL_TEXT_SIZE))
                    .line_height(px(chrome::CONTROL_LINE_HEIGHT))
                    .text_color(theme::bone())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_action(cx.listener(|view, _: &FocusNextControl, window, cx| {
                        view.focus_next_control(window, cx)
                    }))
                    .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                        view.on_dialog_key(event, window, cx)
                    }))
                    .child(self.header(cx))
                    .child(self.tab_strip(cx))
                    .child(match self.tab {
                        SettingsTab::Terminal => {
                            self.terminal_tab(&font, window, cx).into_any_element()
                        }
                        SettingsTab::Keyboard => {
                            self.keyboard_tab(&keymap, window, cx).into_any_element()
                        }
                    })
                    .child(self.footer(&font, &keymap, cx)),
            )
    }
}

/// Position of a shortcut among its list's direct children, counting group headings.
fn shortcut_child_index(shortcut_index: usize) -> usize {
    let mut children = 0;
    let mut previous_group: Option<ShortcutGroup> = None;
    for (index, shortcut) in Shortcut::ALL.into_iter().enumerate() {
        if previous_group != Some(shortcut.group()) {
            previous_group = Some(shortcut.group());
            children += 1;
        }
        if index == shortcut_index {
            break;
        }
        children += 1;
    }
    children
}

fn default_hint(shortcut: Shortcut) -> String {
    shortcut
        .defaults()
        .iter()
        .filter_map(|source| Keystroke::parse(source).ok())
        .map(|keystroke| keymap::display(&keystroke))
        .collect::<Vec<_>>()
        .join(" or ")
}

/// Primary action. Built from scratch because GPUI 0.2.2 panics when a hover,
/// active, or focus style is set twice, as it would be on top of `button`.
fn done_button() -> Stateful<Div> {
    div()
        .id("settings-done")
        .h(px(chrome::CONTROL_HEIGHT))
        .px(px(chrome::CONTROL_INSET))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(chrome::CONTROL_RADIUS))
        .border_1()
        .border_color(theme::focus())
        .bg(theme::focus())
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::on_accent())
        .tab_index(0)
        .cursor_pointer()
        .hover(|style| style.bg(theme::accent_hover()))
        .active(|style| style.bg(theme::accent_pressed()))
        .focus(|style| style.border_color(theme::bone()))
        .child("Done")
}

fn section_label(text: &'static str) -> Div {
    div()
        .text_size(px(chrome::DETAIL_TEXT_SIZE))
        .line_height(px(chrome::DETAIL_LINE_HEIGHT))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::ash())
        .child(text)
}

fn hint(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(chrome::DETAIL_TEXT_SIZE))
        .line_height(px(chrome::DETAIL_LINE_HEIGHT))
        .text_color(theme::ash())
        .child(text.into())
}

/// Always-visible scrollbar, so hidden rows are discoverable without scrolling first.
fn scroll_indicator(handle: &ScrollHandle) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .child(Scrollbar::vertical(handle).scrollbar_show(ScrollbarShow::Always))
}

/// Marks the keyboard-selected row while its list has focus, beyond the tint.
fn focus_bar() -> Div {
    div()
        .absolute()
        .left_0()
        .top(px(chrome::SMALL_GAP))
        .bottom(px(chrome::SMALL_GAP))
        .w(px(2.0))
        .rounded_full()
        .bg(theme::focus())
}

/// A "1 – value – 2" control with its range, for settings that move in fixed steps.
fn stepper(
    title: &'static str,
    range: String,
    value: String,
    down: Stateful<Div>,
    up: Stateful<Div>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(chrome::SMALL_GAP))
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(px(chrome::COMPACT_GAP))
                .child(section_label(title))
                .child(hint(range)),
        )
        .child(
            div()
                .h(px(chrome::STEPPER_BUTTON))
                .flex()
                .items_center()
                .rounded(px(chrome::CONTROL_RADIUS))
                .border_1()
                .border_color(theme::edge())
                .overflow_hidden()
                .child(down)
                .child(
                    div()
                        .min_w(px(76.0))
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .border_l_1()
                        .border_r_1()
                        .border_color(theme::edge())
                        .font_family(theme::mono())
                        .text_color(theme::bone())
                        .child(value),
                )
                .child(up),
        )
}

fn step_button(
    id: &'static str,
    glyph: &'static str,
    tooltip: &'static str,
    enabled: bool,
) -> Stateful<Div> {
    // Always focusable: a button that vanishes from the tab order at its limit
    // would strand keyboard focus. At the limit it is dimmed and does nothing.
    div()
        .id(id)
        .size(px(chrome::STEPPER_BUTTON))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .border_1()
        .border_color(gpui::rgba(0))
        .text_size(px(16.0))
        .text_color(if enabled {
            theme::bone()
        } else {
            theme::smoke()
        })
        .tab_index(0)
        .tooltip(text_tooltip(tooltip))
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|style| style.bg(theme::panel_hover()))
                .active(|style| style.bg(theme::selection()))
        })
        .focus(|style| style.border_color(theme::focus()).bg(theme::panel_lift()))
        .child(glyph)
}

fn key_chip(text: String, customized: bool) -> Div {
    div()
        .h(px(24.0))
        .px(px(chrome::COMPACT_GAP))
        .flex()
        .items_center()
        .flex_shrink_0()
        .rounded(px(chrome::CONTROL_RADIUS))
        .border_1()
        .border_color(if customized {
            theme::focus()
        } else {
            theme::edge_hard()
        })
        .bg(theme::panel_lift())
        .font_family(theme::mono())
        .text_size(px(12.0))
        .text_color(theme::bone())
        .child(text)
}

/// Stands in for the chips while a new chord is being recorded.
fn capture_field() -> Div {
    div()
        .w(px(CHIP_COLUMN_WIDTH))
        .h(px(24.0))
        .px(px(chrome::COMPACT_GAP))
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(chrome::SMALL_GAP))
        .rounded(px(chrome::CONTROL_RADIUS))
        .border_1()
        .border_color(theme::focus())
        .bg(theme::canvas())
        .text_size(px(12.0))
        .text_color(theme::focus())
        .font_weight(FontWeight::MEDIUM)
        .child("Press the new shortcut")
        .child(div().text_color(theme::ash()).child("Esc cancels"))
}

/// Mouse-first row action; the row's list owns keyboard access to Change and Reset.
fn row_button(id: (&'static str, usize), label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(ROW_BUTTON_WIDTH))
        .h(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .rounded(px(chrome::CONTROL_RADIUS))
        .border_1()
        .border_color(theme::edge())
        .text_size(px(12.0))
        .text_color(theme::bone_dim())
        .cursor_pointer()
        .hover(|style| style.bg(theme::panel_lift()).text_color(theme::bone()))
        .active(|style| style.bg(theme::selection()))
        .child(label)
}

/// Sample output in the chosen font, with ambiguous glyphs that expose a poor fit.
fn preview(font: &TerminalFont) -> impl IntoElement {
    div()
        .id("settings-preview")
        .flex_1()
        .min_h(px(96.0))
        .p(px(chrome::INSET))
        .overflow_hidden()
        .rounded(px(chrome::CONTROL_RADIUS))
        .border_1()
        .border_color(theme::edge())
        .bg(theme::canvas())
        .font_family(font.family.clone())
        .text_size(theme::text_size(font.size))
        .line_height(px(font.line_height_px()))
        .text_color(theme::bone_dim())
        .whitespace_nowrap()
        .child(
            div()
                .flex()
                .child(div().text_color(theme::focus()).child("PS C:\\project> "))
                .child("cargo test"),
        )
        .child(
            div()
                .text_color(theme::success())
                .child("test result: ok. 128 passed; 0 failed"),
        )
        .child("0O 1lI |!  {} [] () -> => != ==")
        .child("The quick brown fox jumps over the lazy dog")
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn open(
        cx: &mut TestAppContext,
    ) -> (
        gpui::Entity<SettingsPanel>,
        Rc<RefCell<Vec<SettingsEvent>>>,
        &mut gpui::VisualTestContext,
    ) {
        cx.update(crate::views::file_editor::FileEditor::initialize);
        cx.update(SettingsPanel::bind_keys);
        let events = Rc::new(RefCell::new(Vec::new()));
        let sink = events.clone();
        let (panel, cx) = cx.add_window_view(move |window, cx| {
            let panel = SettingsPanel::new(cx);
            window.focus(&panel.focus);
            panel
        });
        cx.update(|_, app| {
            let sink = sink.clone();
            app.subscribe(&panel, move |_, event: &SettingsEvent, _| {
                sink.borrow_mut().push(event.clone())
            })
            .detach();
        });
        (panel, events, cx)
    }

    #[test]
    fn child_indexes_count_group_headings() {
        // Workbench heading, then its first row.
        assert_eq!(shortcut_child_index(0), 1);
        let editor = Shortcut::ALL
            .iter()
            .position(|shortcut| shortcut.group() == ShortcutGroup::Editor)
            .unwrap();
        let workbench_rows = editor;
        // Two headings (Workbench, Editor) precede the first editor row.
        assert_eq!(shortcut_child_index(editor), workbench_rows + 2);
        // Three group headings precede or sit among the rows; the last row is last.
        assert_eq!(
            shortcut_child_index(Shortcut::ALL.len() - 1),
            Shortcut::ALL.len() + 2
        );
    }

    #[gpui::test]
    fn font_steps_and_family_choice_are_reported_not_applied(cx: &mut TestAppContext) {
        let (panel, events, cx) = open(cx);
        panel.update(cx, |panel, cx| {
            panel.step_size(1, cx);
            panel.step_line_height(-1, cx);
        });
        let defaults = TerminalFont::default();
        assert_eq!(
            events.borrow().as_slice(),
            [
                SettingsEvent::FontChanged(defaults.stepped_size(1).unwrap()),
                SettingsEvent::FontChanged(defaults.stepped_line_height(-1).unwrap()),
            ]
        );
        // Choosing the family already in use reports nothing.
        events.borrow_mut().clear();
        panel.update(cx, |panel, cx| panel.choose_family(0, cx));
        assert!(events.borrow().is_empty());
    }

    #[gpui::test]
    fn recording_captures_a_free_chord_and_explains_rejections(cx: &mut TestAppContext) {
        let (panel, events, cx) = open(cx);
        panel.update(cx, |panel, cx| {
            panel.toggle_recording(1, cx);
        });
        // A chord that belongs to another shortcut stays in recording mode with a reason.
        cx.simulate_keystrokes("ctrl-shift-w");
        assert!(events.borrow().is_empty());
        panel.read_with(cx, |panel, _| {
            assert_eq!(panel.recording, Some(Shortcut::NewTerminal));
            assert!(
                panel
                    .rejection
                    .as_deref()
                    .is_some_and(|message| message.contains("Close terminal tab"))
            );
        });
        cx.simulate_keystrokes("ctrl-alt-n");
        assert_eq!(
            events.borrow().as_slice(),
            [SettingsEvent::ShortcutChanged(
                Shortcut::NewTerminal,
                Keystroke::parse("ctrl-alt-n").unwrap()
            )]
        );
        panel.read_with(cx, |panel, _| assert!(panel.recording.is_none()));
    }

    #[gpui::test]
    fn escape_cancels_recording_before_it_can_close_the_dialog(cx: &mut TestAppContext) {
        let (panel, events, cx) = open(cx);
        panel.update(cx, |panel, cx| panel.toggle_recording(0, cx));
        cx.simulate_keystrokes("escape");
        assert!(events.borrow().is_empty());
        panel.read_with(cx, |panel, _| assert!(panel.recording.is_none()));
        cx.simulate_keystrokes("escape");
        assert_eq!(events.borrow().as_slice(), [SettingsEvent::Closed]);
    }

    #[gpui::test]
    fn shortcut_list_keys_select_record_and_reset(cx: &mut TestAppContext) {
        let (panel, events, cx) = open(cx);
        panel.update(cx, |panel, cx| {
            panel.select_tab(SettingsTab::Keyboard, cx);
        });
        cx.update(|window, cx| {
            let handle = panel.read(cx).shortcut_focus.clone();
            window.focus(&handle);
        });
        cx.simulate_keystrokes("down down enter");
        panel.read_with(cx, |panel, _| {
            assert_eq!(panel.shortcut_cursor, 2);
            assert_eq!(panel.recording, Some(Shortcut::CloseTerminal));
        });
        cx.simulate_keystrokes("escape");
        // Nothing is customized, so Delete has nothing to reset.
        cx.simulate_keystrokes("delete");
        assert!(events.borrow().is_empty());
        cx.update(|_, cx| {
            let mut keymap = Keymap::default();
            keymap
                .assign(
                    Shortcut::CloseTerminal,
                    &Keystroke::parse("ctrl-alt-w").unwrap(),
                )
                .unwrap();
            cx.set_global(keymap);
        });
        cx.simulate_keystrokes("delete");
        assert_eq!(
            events.borrow().as_slice(),
            [SettingsEvent::ShortcutReset(Shortcut::CloseTerminal)]
        );
    }

    #[gpui::test]
    fn tab_wraps_inside_the_dialog(cx: &mut TestAppContext) {
        let (panel, _, cx) = open(cx);
        cx.update(|window, cx| {
            let last = panel.read(cx).last_stop.clone();
            window.focus(&last);
        });
        cx.simulate_keystrokes("tab");
        cx.update(|window, cx| {
            let panel = panel.read(cx);
            assert!(panel.focus.contains_focused(window, cx));
        });
        cx.simulate_keystrokes("shift-f6");
        cx.update(|window, cx| {
            let panel = panel.read(cx);
            assert!(panel.focus.contains_focused(window, cx));
        });
    }

    #[test]
    fn default_hint_lists_every_default() {
        assert_eq!(
            default_hint(Shortcut::NewTerminal),
            "Ctrl+Shift+T or Ctrl+`"
        );
        assert_eq!(default_hint(Shortcut::FocusProjects), "F6");
    }
}
