//! Headerless project outline. The manager owns saving; this view owns editing and undo.

use gpui::{
    AnyElement, Context, Div, ElementId, Entity, EventEmitter, FocusHandle, KeyDownEvent, Render,
    ScrollHandle, SharedString, Stateful, Subscription, Window, div, prelude::*, px, svg,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
};

use crate::theme::terminal_manager as chrome;
use crate::{
    services::checklist::{Checklist, Section},
    theme,
};

gpui::actions!(checklist, [IndentReminder, OutdentReminder]);

/// Left padding shared by every row so labels, editors, and add controls align.
const ROW_INSET: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Selection(usize, Option<usize>);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Edit {
    Add(usize, Option<usize>),
    Task(usize, usize),
    Section(usize),
    NewSection,
}

#[derive(Clone, Copy)]
enum Command {
    Rename,
    Add,
    Child,
    Indent,
    Outdent,
    Delete,
    Undo,
}

pub(super) struct ChecklistChanged;

pub(super) struct ChecklistView {
    data: Checklist,
    focus: FocusHandle,
    input: Entity<InputState>,
    _input_subscription: Subscription,
    editing: Option<Edit>,
    selected: Option<Selection>,
    undo: Vec<Checklist>,
    /// Banner copy for the delete that `Undo` can still reverse.
    deleted: Option<&'static str>,
    scroll: ScrollHandle,
}

impl EventEmitter<ChecklistChanged> for ChecklistView {}

impl ChecklistView {
    pub(super) fn bind_keys(cx: &mut gpui::App) {
        cx.bind_keys([
            gpui::KeyBinding::new("tab", IndentReminder, Some("Checklist")),
            gpui::KeyBinding::new("shift-tab", OutdentReminder, Some("Checklist")),
        ]);
    }

    pub(super) fn new(data: Checklist, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Add a reminder…"));
        let subscription = cx.subscribe_in(&input, window, |view, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                view.submit(window, cx);
            }
        });
        Self {
            data,
            focus: cx.focus_handle(),
            input,
            _input_subscription: subscription,
            editing: None,
            selected: None,
            undo: Vec::new(),
            deleted: None,
            scroll: ScrollHandle::new(),
        }
    }

    pub(super) fn snapshot(&self) -> Checklist {
        self.data.clone()
    }

    pub(super) fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            self.input.update(cx, |input, cx| input.focus(window, cx));
        } else {
            window.focus(&self.focus);
        }
    }

    fn checkpoint(&mut self) {
        if self.undo.len() == 50 {
            self.undo.remove(0);
        }
        self.undo.push(self.data.clone());
        self.deleted = None;
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(ChecklistChanged);
        cx.notify();
    }

    fn begin(&mut self, edit: Edit, window: &mut Window, cx: &mut Context<Self>) {
        let value = match edit {
            Edit::Task(s, t) => self.data.sections[s].tasks[t].label.clone(),
            Edit::Section(s) => self.data.sections[s].label.clone(),
            Edit::Add(..) | Edit::NewSection => String::new(),
        };
        self.editing = Some(edit);
        self.input.update(cx, |input, cx| {
            input.set_placeholder(placeholder(edit), window, cx);
            input.set_value(value, window, cx);
            input.focus(window, cx);
        });
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.editing else { return };
        let value = self.input.read(cx).value().trim().to_owned();
        if value.is_empty() {
            return;
        }
        self.checkpoint();
        match edit {
            Edit::Add(s, parent) => {
                if self.data.sections.is_empty() {
                    self.data.sections.push(Section::new("Workspace".into()));
                }
                let index = self.data.sections[s].add(value, parent);
                self.selected = Some(Selection(s, Some(index)));
            }
            Edit::Task(s, t) => self.data.sections[s].tasks[t].label = value,
            Edit::Section(s) => self.data.sections[s].label = value,
            Edit::NewSection => {
                self.data.sections.push(Section::new(value));
                let s = self.data.sections.len() - 1;
                self.selected = Some(Selection(s, None));
                self.editing = Some(Edit::Add(s, None));
            }
        }
        let next = self.editing;
        self.input.update(cx, |input, cx| {
            if let Some(next) = next {
                input.set_placeholder(placeholder(next), window, cx);
            }
            input.set_value("", window, cx);
        });
        if matches!(edit, Edit::Task(..) | Edit::Section(_)) {
            self.editing = None;
            window.focus(&self.focus);
        }
        self.changed(cx);
    }

    fn rows(&self) -> Vec<Selection> {
        self.data
            .sections
            .iter()
            .enumerate()
            .flat_map(|(s, section)| {
                std::iter::once(Selection(s, None)).chain(
                    section
                        .visible()
                        .into_iter()
                        .map(move |t| Selection(s, Some(t))),
                )
            })
            .collect()
    }

    fn select(&mut self, selection: Selection, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(selection);
        window.focus(&self.focus);
        cx.notify();
    }

    fn collapse(&mut self, Selection(s, t): Selection, cx: &mut Context<Self>) {
        match t {
            Some(t) => {
                let task = &mut self.data.sections[s].tasks[t];
                task.collapsed = !task.collapsed;
            }
            None => {
                let section = &mut self.data.sections[s];
                section.collapsed = !section.collapsed;
            }
        }
        self.changed(cx);
    }

    fn toggle(&mut self, selection: Selection, cx: &mut Context<Self>) {
        if let Selection(s, Some(t)) = selection {
            self.checkpoint();
            self.data.sections[s].toggle(t);
            self.changed(cx);
        } else {
            self.collapse(selection, cx);
        }
    }

    fn command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(command, Command::Undo) {
            if let Some(data) = self.undo.pop() {
                self.data = data;
                self.selected = None;
                self.editing = None;
                self.deleted = None;
                window.focus(&self.focus);
                self.changed(cx);
            }
            return;
        }
        let Selection(s, task) = self.selected.unwrap_or(Selection(0, None));
        if self.data.sections.is_empty() {
            if matches!(command, Command::Add | Command::Child) {
                self.begin(Edit::Add(0, None), window, cx);
            }
            return;
        }
        match command {
            Command::Rename => self.begin(
                task.map_or(Edit::Section(s), |t| Edit::Task(s, t)),
                window,
                cx,
            ),
            Command::Add => self.begin(Edit::Add(s, None), window, cx),
            Command::Child => {
                if task.is_none_or(|t| self.data.sections[s].tasks[t].depth < 8) {
                    self.begin(Edit::Add(s, task), window, cx);
                }
            }
            Command::Indent | Command::Outdent => {
                if let Some(t) = task {
                    let before = self.data.clone();
                    if let Some(t) =
                        self.data.sections[s].indent(t, matches!(command, Command::Outdent))
                    {
                        if self.undo.len() == 50 {
                            self.undo.remove(0);
                        }
                        self.undo.push(before);
                        self.selected = Some(Selection(s, Some(t)));
                        self.changed(cx);
                    }
                }
            }
            Command::Delete => {
                self.checkpoint();
                if let Some(t) = task {
                    self.data.sections[s].remove(t);
                } else {
                    self.data.sections.remove(s);
                }
                self.selected = None;
                self.editing = None;
                self.deleted = Some(if task.is_some() {
                    "Reminder deleted"
                } else {
                    "Section deleted"
                });
                self.changed(cx);
            }
            Command::Undo => {}
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        if key.key == "escape" && self.editing.is_some() {
            self.editing = None;
            window.focus(&self.focus);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if !self.focus.is_focused(window) {
            return;
        }
        match key.key.as_str() {
            "up" | "down" | "home" | "end" => {
                let rows = self.rows();
                if rows.is_empty() {
                    return;
                }
                let current = self
                    .selected
                    .and_then(|s| rows.iter().position(|r| *r == s));
                let next = match key.key.as_str() {
                    "home" => 0,
                    "end" => rows.len() - 1,
                    "up" => current.unwrap_or(0).saturating_sub(1),
                    _ => current.map_or(0, |i| (i + 1).min(rows.len() - 1)),
                };
                self.selected = Some(rows[next]);
                self.scroll.scroll_to_item(next);
                cx.notify();
            }
            "space" => {
                if let Some(s) = self.selected {
                    self.toggle(s, cx);
                }
            }
            "left" | "right" => {
                if let Some(Selection(s, t)) = self.selected {
                    let collapsed = t.map_or(self.data.sections[s].collapsed, |t| {
                        self.data.sections[s].tasks[t].collapsed
                    });
                    if collapsed == (key.key == "right") {
                        self.collapse(Selection(s, t), cx);
                    }
                }
            }
            "enter" => self.command(
                if key.modifiers.control {
                    Command::Child
                } else {
                    Command::Add
                },
                window,
                cx,
            ),
            "f2" => self.command(Command::Rename, window, cx),
            "delete" => self.command(Command::Delete, window, cx),
            "z" if key.modifiers.control => self.command(Command::Undo, window, cx),
            "n" if key.modifiers.control && key.modifiers.shift => {
                self.begin(Edit::NewSection, window, cx)
            }
            _ => return,
        }
        window.prevent_default();
        cx.stop_propagation();
    }

    fn menu(&self, selection: Selection, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = cx.weak_entity();
        let depth_limit = selection
            .1
            .is_some_and(|t| self.data.sections[selection.0].tasks[t].depth >= 8);
        Button::new(SharedString::from(format!(
            "checklist-menu-{}-{:?}",
            selection.0, selection.1
        )))
        .ghost()
        .child(icon("overflow", 14.0))
        .size(px(chrome::CHECKLIST_CONTROL_SIZE))
        .p_0()
        .tooltip(if selection.1.is_some() {
            "Reminder actions"
        } else {
            "Section actions"
        })
        .dropdown_menu(move |mut menu, _, _| {
            for (label, command) in [
                ("Add subtask", Command::Child),
                ("Rename", Command::Rename),
                ("Delete", Command::Delete),
            ] {
                if matches!(command, Command::Child) && (selection.1.is_none() || depth_limit) {
                    continue;
                }
                let owner = owner.clone();
                menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    let owner = owner.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = owner.update(cx, |view, cx| {
                            view.selected = Some(selection);
                            window.focus(&view.focus);
                            view.command(command, window, cx);
                        });
                    });
                }));
            }
            menu
        })
    }

    fn row(&self, selection: Selection, focused: bool, cx: &mut Context<Self>) -> AnyElement {
        let Selection(s, task_index) = selection;
        let section = &self.data.sections[s];
        let task = task_index.map(|t| &section.tasks[t]);
        let label = task.map_or(&section.label, |t| &t.label).clone();
        let collapsed = task.map_or(section.collapsed, |t| t.collapsed);
        let progress = task_index.map_or((0, 0), |t| section.progress(t));
        let has_children = task.is_none() || progress.1 > 0;
        let selected = self.selected == Some(selection);
        div()
            .id(SharedString::from(format!(
                "checklist-row-{s}-{task_index:?}"
            )))
            .debug_selector(move || format!("checklist-row-{s}-{task_index:?}").into())
            .h(px(if task.is_none() {
                chrome::CHECKLIST_HEADING_HEIGHT
            } else {
                chrome::CHECKLIST_ROW_HEIGHT
            }))
            .relative()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(chrome::CHECKLIST_GAP))
            .pl(px(ROW_INSET
                + task.map_or(0.0, |t| {
                    t.depth as f32 * chrome::CHECKLIST_INDENT
                })))
            .pr(px(4.0))
            .rounded(px(3.0))
            .when(task.is_none() && s > 0, |r| r.mt(px(16.0)))
            .when(selected, |r| r.bg(theme::selection()))
            .hover(|r| r.bg(theme::panel_hover()))
            .on_click(cx.listener(move |view, _, window, cx| view.select(selection, window, cx)))
            // Keyboard position marker; it takes no space, so rows never shift.
            .when(selected && focused, |r| {
                r.child(
                    div()
                        .absolute()
                        .left_0()
                        .top(px(7.0))
                        .bottom(px(7.0))
                        .w(px(2.0))
                        .rounded(px(1.0))
                        .bg(theme::focus()),
                )
            })
            .child(if task.is_none() {
                div()
                    .w(px(chrome::CHECKLIST_DISCLOSURE_WIDTH))
                    .flex_shrink_0()
                    .text_align(gpui::TextAlign::Center)
                    .text_size(px(10.0))
                    .line_height(px(chrome::CONTROL_LINE_HEIGHT))
                    .font_family(theme::mono())
                    .text_color(theme::ash())
                    .child(format!("{:02}", s + 1))
                    .into_any_element()
            } else {
                div()
                    .w(px(chrome::CHECKLIST_DISCLOSURE_WIDTH))
                    .h(px(chrome::CHECKLIST_CONTROL_SIZE))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(has_children, |r| {
                        r.child(
                            div()
                                .id(SharedString::from(format!(
                                    "checklist-fold-{s}-{task_index:?}"
                                )))
                                .debug_selector(move || {
                                    format!("checklist-fold-{s}-{task_index:?}").into()
                                })
                                .w(px(chrome::CHECKLIST_DISCLOSURE_WIDTH))
                                .h(px(chrome::CHECKLIST_CONTROL_SIZE))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(3.0))
                                .tab_index(0)
                                .cursor_pointer()
                                .hover(|s| s.bg(theme::panel_lift()))
                                .border_1()
                                .border_color(gpui::transparent_black())
                                .focus(|r| r.border_color(theme::focus()))
                                .tooltip(super::terminal_manager::text_tooltip(
                                    "Expand or collapse subtasks · Left/Right",
                                ))
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.collapse(selection, cx);
                                    cx.stop_propagation();
                                }))
                                .child(icon(
                                    if collapsed {
                                        "chevron-right"
                                    } else {
                                        "chevron-down"
                                    },
                                    12.0,
                                )),
                        )
                    })
                    .into_any_element()
            })
            .when_some(task, |r, task| {
                r.child(
                    div()
                        .id(SharedString::from(format!(
                            "checklist-check-{s}-{task_index:?}"
                        )))
                        .debug_selector(move || {
                            format!("checklist-check-{s}-{task_index:?}").into()
                        })
                        .tab_index(0)
                        .cursor_pointer()
                        .size(px(14.0))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.0))
                        .border_1()
                        .border_color(if task.done {
                            theme::success()
                        } else {
                            theme::ash()
                        })
                        .when(task.done, |r| r.bg(theme::success()))
                        .focus(|r| r.border_color(theme::focus()))
                        .tooltip(super::terminal_manager::text_tooltip(if task.done {
                            "Mark incomplete · Space"
                        } else {
                            "Complete reminder and subtasks · Space"
                        }))
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.toggle(selection, cx);
                            cx.stop_propagation();
                        }))
                        .when(task.done, |r| {
                            r.child(
                                svg()
                                    .path("icons/check.svg")
                                    .size(px(12.0))
                                    .text_color(theme::floor()),
                            )
                        }),
                )
            })
            .child(
                div()
                    .id(SharedString::from(format!(
                        "checklist-label-{s}-{task_index:?}"
                    )))
                    .debug_selector(move || format!("checklist-label-{s}-{task_index:?}").into())
                    .flex_1()
                    .min_w_0()
                    .line_height(px(if task.is_none() {
                        28.0
                    } else {
                        chrome::CONTROL_LINE_HEIGHT
                    }))
                    .truncate()
                    .tooltip(super::terminal_manager::text_tooltip(label.clone()))
                    .text_size(px(if task.is_none() {
                        chrome::CHECKLIST_HEADING_SIZE
                    } else {
                        13.0
                    }))
                    .font_family(if task.is_none() {
                        chrome::HEADING_FONT
                    } else {
                        chrome::CHROME_FONT
                    })
                    .text_color(if task.is_some_and(|t| t.done) {
                        theme::smoke()
                    } else {
                        theme::bone()
                    })
                    .when(task.is_some_and(|t| t.done), |r| r.line_through())
                    .child(label),
            )
            .when(progress.1 > 0, |r| {
                r.child(
                    div()
                        .text_size(px(10.0))
                        .line_height(px(chrome::CONTROL_LINE_HEIGHT))
                        .flex_shrink_0()
                        .text_color(theme::ash())
                        .child(format!("{} / {}", progress.0, progress.1)),
                )
            })
            .when(task.is_none(), |r| {
                r.child(
                    div()
                        .id(SharedString::from(format!("checklist-section-fold-{s}")))
                        .w(px(chrome::CHECKLIST_DISCLOSURE_WIDTH))
                        .h(px(chrome::CHECKLIST_CONTROL_SIZE))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.0))
                        .tab_index(0)
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::panel_lift()))
                        .border_1()
                        .border_color(gpui::transparent_black())
                        .focus(|r| r.border_color(theme::focus()))
                        .tooltip(super::terminal_manager::text_tooltip(
                            "Expand or collapse section · Left/Right",
                        ))
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.collapse(selection, cx);
                            cx.stop_propagation();
                        }))
                        .child(icon(
                            if collapsed {
                                "chevron-right"
                            } else {
                                "chevron-down"
                            },
                            12.0,
                        )),
                )
            })
            .child(self.menu(selection, cx))
            .into_any_element()
    }
}

fn icon(name: &str, size: f32) -> impl IntoElement {
    svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(size))
        .text_color(theme::ash())
}

fn placeholder(edit: Edit) -> &'static str {
    match edit {
        Edit::Add(_, Some(_)) => "Add a subtask…",
        Edit::Add(..) => "Add a reminder…",
        Edit::Task(..) => "Rename reminder",
        Edit::Section(_) => "Rename section",
        Edit::NewSection => "Name the section…",
    }
}

/// Quiet, left-aligned text control. The transparent border reserves room for the
/// focus ring, so focusing never shifts the label.
fn control(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .rounded(px(3.0))
        .border_1()
        .border_color(gpui::transparent_black())
        .font_family(chrome::CHROME_FONT)
        .text_size(px(13.0))
        .line_height(px(chrome::CONTROL_LINE_HEIGHT))
        .text_color(theme::ash())
        .tab_index(0)
        .cursor_pointer()
        .hover(|s| s.bg(theme::panel_hover()).text_color(theme::bone()))
        .active(|s| s.bg(theme::selection()))
        .focus(|s| s.border_color(theme::focus()).text_color(theme::bone()))
}

/// Footer action lined up with the checkbox column, with its shortcut spelled out.
fn footer_action(
    id: &'static str,
    label: &'static str,
    shortcut: &'static str,
    tooltip: &'static str,
) -> Stateful<Div> {
    control(id)
        .debug_selector(move || id.into())
        .h(px(chrome::CHECKLIST_ROW_HEIGHT))
        .gap(px(chrome::CHECKLIST_GAP))
        // The 1px border sits inside the box, so trim the inset to keep labels aligned.
        .pl(px(ROW_INSET - 1.0))
        .pr(px(8.0))
        .tooltip(super::terminal_manager::text_tooltip(tooltip))
        .child(
            div()
                .w(px(chrome::CHECKLIST_DISCLOSURE_WIDTH))
                .flex_shrink_0(),
        )
        .child(
            svg()
                .path("icons/plus.svg")
                .size(px(14.0))
                .flex_shrink_0()
                .text_color(theme::ash()),
        )
        .child(label)
        .child(
            div()
                .ml_auto()
                .flex_shrink_0()
                .text_size(px(11.0))
                .opacity(0.7)
                .child(shortcut),
        )
}

impl ChecklistView {
    fn empty_hint(&self) -> impl IntoElement {
        let no_sections = self.data.sections.is_empty();
        div()
            .flex()
            .flex_col()
            .justify_center()
            .font_family(chrome::CHROME_FONT)
            .text_size(px(12.0))
            .line_height(px(18.0))
            .text_color(theme::ash())
            .when(no_sections, |hint| {
                hint.px(px(ROW_INSET)).pt(px(4.0)).pb(px(8.0)).child(
                    div()
                        .text_size(px(13.0))
                        .line_height(px(chrome::CONTROL_LINE_HEIGHT))
                        .text_color(theme::bone())
                        .child("No reminders yet"),
                )
            })
            .when(!no_sections, |hint| {
                hint.h(px(chrome::CHECKLIST_ROW_HEIGHT))
                    .pl(px(chrome::CHECKLIST_TEXT_INSET))
            })
            .child("Keep the next thing to do here.")
    }

    fn editor(&self, edit: Edit, cx: &mut Context<Self>) -> AnyElement {
        let (caption, glyph, save_tooltip) = match edit {
            Edit::Add(_, Some(_)) => (Some("New subtask"), "plus", "Add subtask · Enter"),
            Edit::Add(..) => (None, "plus", "Add reminder · Enter"),
            Edit::Task(..) => (Some("Rename reminder"), "pencil", "Save name · Enter"),
            Edit::Section(_) => (Some("Rename section"), "pencil", "Save name · Enter"),
            Edit::NewSection => (Some("New section"), "plus", "Create section · Enter"),
        };
        div()
            .mt(px(4.0))
            .pr(px(4.0))
            .when_some(caption, |editor, caption| {
                editor.child(
                    div()
                        .pl(px(chrome::CHECKLIST_TEXT_INSET))
                        .pt(px(4.0))
                        .font_family(chrome::CHROME_FONT)
                        .text_size(px(chrome::DETAIL_TEXT_SIZE))
                        .line_height(px(chrome::DETAIL_LINE_HEIGHT))
                        .text_color(theme::ash())
                        .child(caption),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(chrome::CHECKLIST_GAP))
                    .pl(px(ROW_INSET
                        + chrome::CHECKLIST_DISCLOSURE_WIDTH
                        + chrome::CHECKLIST_GAP))
                    .child(
                        div()
                            .size(px(14.0))
                            .flex_shrink_0()
                            .child(icon(glyph, 14.0)),
                    )
                    .child(
                        div()
                            .id("checklist-editor-input")
                            .debug_selector(|| "checklist-editor-input".into())
                            .flex_1()
                            .min_w_0()
                            .child(
                                Input::new(&self.input)
                                    .appearance(false)
                                    .bordered(false)
                                    .px_0()
                                    .py_0()
                                    .h(px(chrome::CHECKLIST_ROW_HEIGHT))
                                    .font_family(chrome::CHROME_FONT)
                                    .text_size(px(13.0)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap(px(2.0))
                            .child(
                                Button::new("checklist-save")
                                    .ghost()
                                    .child(icon("queue-return", 14.0))
                                    .size(px(chrome::CHECKLIST_CONTROL_SIZE))
                                    .p_0()
                                    .tooltip(save_tooltip)
                                    .on_click(
                                        cx.listener(|view, _, window, cx| view.submit(window, cx)),
                                    ),
                            )
                            .child(
                                Button::new("checklist-cancel")
                                    .ghost()
                                    .child(icon("close", 12.0))
                                    .size(px(chrome::CHECKLIST_CONTROL_SIZE))
                                    .p_0()
                                    .tooltip("Cancel · Escape")
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.editing = None;
                                        window.focus(&view.focus);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn footer(&self, focused: bool, cx: &mut Context<Self>) -> AnyElement {
        // With no row selected, Enter adds to the first section, so mark that target.
        let default_target = focused && self.selected.is_none();
        div()
            .child(
                footer_action(
                    "checklist-add",
                    "Add a reminder",
                    "Enter",
                    "Add a reminder · Enter · Ctrl+Enter adds a subtask",
                )
                .mt(px(4.0))
                .when(default_target, |action| action.border_color(theme::focus()))
                .on_click(cx.listener(|view, _, window, cx| {
                    view.command(Command::Add, window, cx);
                })),
            )
            .child(
                footer_action(
                    "checklist-add-section",
                    "New section",
                    "Ctrl+Shift+N",
                    "Add a section · Ctrl+Shift+N",
                )
                .mt(px(12.0))
                .on_click(cx.listener(|view, _, window, cx| {
                    view.begin(Edit::NewSection, window, cx);
                })),
            )
            .into_any_element()
    }

    /// Docked below the list so the notice never covers reminders or shifts them.
    fn undo_bar(&self, label: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("checklist-undo-bar")
            .flex_shrink_0()
            .h(px(40.0))
            .pl(px(16.0))
            .pr(px(8.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .border_t_1()
            .border_color(theme::edge())
            .bg(theme::chrome())
            .font_family(chrome::CHROME_FONT)
            .text_size(px(13.0))
            .text_color(theme::bone())
            .child(div().flex_1().min_w_0().truncate().child(label))
            .child(
                control("checklist-undo")
                    .h(px(28.0))
                    .px(px(7.0))
                    .gap(px(6.0))
                    .text_color(theme::bone())
                    .tooltip(super::terminal_manager::text_tooltip("Undo · Ctrl+Z"))
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.command(Command::Undo, window, cx);
                    }))
                    .child(
                        svg()
                            .path("icons/undo.svg")
                            .size(px(12.0))
                            .text_color(theme::ash()),
                    )
                    .child("Undo"),
            )
            .child(
                control("checklist-undo-dismiss")
                    .size(px(28.0))
                    .justify_center()
                    .tooltip(super::terminal_manager::text_tooltip("Dismiss"))
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.deleted = None;
                        window.focus(&view.focus);
                        cx.notify();
                    }))
                    .child(icon("close", 12.0)),
            )
    }
}

impl Render for ChecklistView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.contains_focused(window, cx);
        let rows = self.rows();
        let no_reminders = self.data.sections.iter().all(|s| s.tasks.is_empty());
        let footer = match self.editing {
            Some(edit) => self.editor(edit, cx),
            None => self.footer(focused, cx),
        };
        div()
            .id("project-checklist")
            .debug_selector(|| "project-checklist".into())
            .track_focus(&self.focus)
            .key_context("Checklist")
            .tab_index(0)
            .tab_group()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(theme::panel())
            .text_color(theme::bone())
            .border_l_1()
            .border_color(theme::edge())
            .on_action(cx.listener(|view, _: &IndentReminder, window, cx| {
                if view.focus.is_focused(window) {
                    view.command(Command::Indent, window, cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|view, _: &OutdentReminder, window, cx| {
                if view.focus.is_focused(window) {
                    view.command(Command::Outdent, window, cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(
                cx.listener(|view, _: &gpui_component::input::Enter, _, cx| {
                    // Single-line Input propagates Enter after emitting PressEnter. Consume
                    // it here so the platform cannot insert a newline into the editor.
                    if view.editing.is_some() {
                        cx.stop_propagation();
                    } else {
                        cx.propagate();
                    }
                }),
            )
            .on_action(
                cx.listener(|view, _: &gpui_component::input::Escape, window, cx| {
                    if view.editing.take().is_some() {
                        window.focus(&view.focus);
                        cx.stop_propagation();
                        cx.notify();
                    } else {
                        cx.propagate();
                    }
                }),
            )
            .on_key_down(cx.listener(Self::key_down))
            .child(
                div()
                    .id("checklist-scroll")
                    .track_scroll(&self.scroll)
                    .overflow_y_scroll()
                    .flex_1()
                    .min_h_0()
                    .p(px(12.0))
                    .children(rows.into_iter().map(|row| self.row(row, focused, cx)))
                    .when(no_reminders, |body| body.child(self.empty_hint()))
                    .child(footer),
            )
            .when_some(self.deleted, |panel, label| {
                panel.child(self.undo_bar(label, cx))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component::Root;

    #[gpui::test]
    fn checklist_keyboard_editing_nesting_and_undo(cx: &mut gpui::TestAppContext) {
        cx.update(super::super::file_editor::FileEditor::initialize);
        cx.update(ChecklistView::bind_keys);
        let (root, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| {
                let mut view = ChecklistView::new(Checklist::default(), window, cx);
                view.begin(Edit::Add(0, None), window, cx);
                view
            });
            Root::new(view, window, cx)
        });
        let view = root.read_with(cx, |root, _| {
            root.view()
                .clone()
                .downcast::<ChecklistView>()
                .ok()
                .unwrap()
        });
        cx.simulate_input("First reminder");
        cx.simulate_keystrokes("enter");
        assert_eq!(view.read_with(cx, |v, _| v.data.sections[0].tasks.len()), 1);
        assert_eq!(
            cx.debug_bounds("checklist-editor-input").unwrap().left(),
            cx.debug_bounds("checklist-label-0-Some(0)").unwrap().left(),
        );
        cx.simulate_input("Second reminder");
        cx.simulate_keystrokes("enter escape tab");
        assert_eq!(
            view.read_with(cx, |v, _| v.data.sections[0].tasks[1].depth),
            1
        );
        let row = cx.debug_bounds("checklist-row-0-Some(0)").unwrap();
        let fold = cx.debug_bounds("checklist-fold-0-Some(0)").unwrap();
        let checkbox = cx.debug_bounds("checklist-check-0-Some(0)").unwrap();
        let label = cx.debug_bounds("checklist-label-0-Some(0)").unwrap();
        assert_eq!(row.size.height, px(chrome::CHECKLIST_ROW_HEIGHT));
        assert_eq!(fold.center().y, row.center().y);
        assert_eq!(checkbox.center().y, row.center().y);
        assert_eq!(label.center().y, row.center().y);
        cx.simulate_keystrokes("space");
        assert!(view.read_with(cx, |v, _| v.data.sections[0].tasks[0].done));
        cx.simulate_keystrokes("f2 ctrl-a");
        cx.simulate_input("Renamed child");
        cx.simulate_keystrokes("enter");
        assert_eq!(
            view.read_with(cx, |v, _| v.data.sections[0].tasks[1].label.clone()),
            "Renamed child"
        );
        cx.simulate_keystrokes("delete");
        assert_eq!(view.read_with(cx, |v, _| v.data.sections[0].tasks.len()), 1);
        assert_eq!(
            view.read_with(cx, |v, _| v.deleted),
            Some("Reminder deleted")
        );
        cx.simulate_keystrokes("ctrl-z");
        assert_eq!(view.read_with(cx, |v, _| v.data.sections[0].tasks.len()), 2);
        assert_eq!(view.read_with(cx, |v, _| v.deleted), None);
        assert!(cx.debug_bounds("project-checklist").is_some());
        // The add control spans the list like a row instead of shrinking to its label.
        let add = cx.debug_bounds("checklist-add").unwrap();
        let first_row = cx.debug_bounds("checklist-row-0-Some(0)").unwrap();
        assert_eq!(add.left(), first_row.left());
        assert_eq!(add.size.width, first_row.size.width);
        assert_eq!(add.size.height, px(chrome::CHECKLIST_ROW_HEIGHT));
    }

    #[gpui::test]
    fn checklist_sections_and_empty_submission_are_recoverable(cx: &mut gpui::TestAppContext) {
        cx.update(super::super::file_editor::FileEditor::initialize);
        cx.update(ChecklistView::bind_keys);
        let (root, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| {
                let mut view = ChecklistView::new(Checklist::default(), window, cx);
                view.begin(Edit::NewSection, window, cx);
                view
            });
            Root::new(view, window, cx)
        });
        let view = root.read_with(cx, |root, _| {
            root.view()
                .clone()
                .downcast::<ChecklistView>()
                .ok()
                .unwrap()
        });
        cx.simulate_keystrokes("enter");
        assert_eq!(view.read_with(cx, |v, _| v.data.sections.len()), 1);
        cx.simulate_input("Next release");
        cx.simulate_keystrokes("enter");
        cx.simulate_input("Test small windows");
        cx.simulate_keystrokes("enter escape home delete");
        assert_eq!(
            view.read_with(cx, |v, _| v.data.sections[0].label.clone()),
            "Next release"
        );
        cx.simulate_keystrokes("ctrl-z");
        assert_eq!(view.read_with(cx, |v, _| v.data.sections.len()), 2);
        assert_eq!(
            view.read_with(cx, |v, _| v.data.sections[1].tasks[0].label.clone()),
            "Test small windows"
        );
    }
}
