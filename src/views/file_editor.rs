//! Native file buffers backed by the Apache-licensed GPUI Component editor.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    App, Context, Entity, EventEmitter, FontWeight, IntoElement, KeyBinding, Render, Subscription,
    Task, Window, actions, div, prelude::*, px,
};
use gpui_component::input::{Input, InputEvent, InputState, Rope};
use gpui_component::{Theme, ThemeMode};

use crate::services::project_files::{self, FileError, FileSnapshot, LineEnding};
use crate::theme;
use crate::theme::terminal_manager as chrome;

actions!(file_editor, [SaveFile]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileEditorEvent {
    Changed,
    Saved,
    CloseReady,
}

struct Source {
    project: PathBuf,
    path: PathBuf,
}

pub(crate) struct FileEditor {
    source: Source,
    input: Entity<InputState>,
    snapshot: Option<FileSnapshot>,
    saved_text: Rope,
    dirty: bool,
    loading: bool,
    saving: bool,
    error: Option<FileError>,
    error_during_load: bool,
    prompt_pending: bool,
    close_after_save: bool,
    generation: u64,
    cursor_position: gpui_component::input::Position,
    _input_subscription: Subscription,
    _input_observation: Subscription,
    _load_task: Option<Task<()>>,
    _save_task: Option<Task<()>>,
}

impl FileEditor {
    pub(crate) fn initialize(cx: &mut App) {
        gpui_component::init(cx);
        Self::apply_appearance(cx);
        cx.bind_keys([KeyBinding::new("ctrl-s", SaveFile, Some("FileEditor"))]);
    }

    pub(crate) fn apply_appearance(cx: &mut App) {
        Theme::change(
            if theme::appearance().is_dark() {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            },
            None,
            cx,
        );
        let component_theme = Theme::global_mut(cx);
        component_theme.font_family = chrome::CHROME_FONT.into();
        // Root uses this as the rem base; the existing workbench uses 16px.
        component_theme.font_size = px(16.0);
        component_theme.mono_font_family = theme::mono();
        component_theme.mono_font_size = px(theme::T_MONO);
        component_theme.background = theme::canvas().into();
        component_theme.foreground = theme::bone().into();
        component_theme.border = theme::edge().into();
        component_theme.input = theme::edge().into();
        component_theme.caret = theme::focus().into();
        component_theme.ring = theme::focus().into();
        component_theme.selection = theme::selection().into();
        component_theme.muted = theme::panel().into();
        component_theme.muted_foreground = theme::ash().into();
        component_theme.popover = theme::panel_lift().into();
        component_theme.popover_foreground = theme::bone().into();
        component_theme.scrollbar = theme::canvas().into();
        component_theme.scrollbar_thumb = theme::edge().into();
        component_theme.scrollbar_thumb_hover = theme::ash().into();
        component_theme.radius = px(chrome::CONTROL_RADIUS);
        component_theme.shadow = false;
        let highlight = Arc::make_mut(&mut component_theme.highlight_theme);
        highlight.style.editor_background = Some(theme::canvas().into());
        highlight.style.editor_foreground = Some(theme::bone().into());
        highlight.style.editor_active_line = Some(theme::panel().into());
        highlight.style.editor_line_number = Some(theme::ash().into());
        highlight.style.editor_active_line_number = Some(theme::bone().into());
    }

    pub(crate) fn open(
        project: PathBuf,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut editor = Self::new(Source { project, path }, "", window, cx);
        editor.load(window, cx);
        editor
    }

    fn new(source: Source, text: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let language = if source
            .path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            "json"
        } else {
            "text"
        };
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor(language)
                .line_number(true)
                .soft_wrap(false)
                .default_value(text.to_owned())
        });
        let saved_text = input.read(cx).text().clone();
        let cursor_position = input.read(cx).cursor_position();
        let subscription = cx.subscribe(&input, |editor, input, event, cx| {
            if matches!(event, InputEvent::Change) {
                editor.dirty =
                    editor.snapshot.is_some() && input.read(cx).text() != &editor.saved_text;
                cx.emit(FileEditorEvent::Changed);
                cx.notify();
            }
        });
        let observation = cx.observe(&input, |editor, input, cx| {
            let position = input.read(cx).cursor_position();
            if editor.cursor_position != position {
                editor.cursor_position = position;
                cx.notify();
            }
        });
        Self {
            source,
            input,
            snapshot: None,
            saved_text,
            dirty: false,
            loading: false,
            saving: false,
            error: None,
            error_during_load: false,
            prompt_pending: false,
            close_after_save: false,
            generation: 0,
            cursor_position,
            _input_subscription: subscription,
            _input_observation: observation,
            _load_task: None,
            _save_task: None,
        }
    }

    pub(crate) fn title(&self) -> String {
        self.source
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "File".to_owned())
    }

    pub(crate) fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty
    }
    pub(crate) fn is_saving(&self) -> bool {
        self.saving
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading || self.saving {
            return;
        }
        let project = self.source.project.clone();
        let path = self.source.path.clone();
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.loading = true;
        self.error = None;
        self.error_during_load = true;
        let work = cx
            .background_executor()
            .spawn(async move { project_files::load_text_file(&project, &path) });
        let handle = window.window_handle();
        self._load_task = Some(cx.spawn(async move |editor, cx| {
            let result = work.await;
            let _ = handle.update(cx, |_, window, cx| {
                let _ = editor.update(cx, |editor, cx| {
                    if editor.generation != generation {
                        return;
                    }
                    editor.loading = false;
                    match result {
                        Ok(snapshot) => {
                            editor.input.update(cx, |input, cx| {
                                input.set_value(snapshot.text.clone(), window, cx)
                            });
                            editor.saved_text = editor.input.read(cx).text().clone();
                            editor.snapshot = Some(snapshot);
                            editor.dirty = false;
                            editor.error = None;
                        }
                        Err(error) => editor.error = Some(error),
                    }
                    cx.emit(FileEditorEvent::Changed);
                    cx.notify();
                });
            });
        }));
        cx.emit(FileEditorEvent::Changed);
        cx.notify();
    }

    pub(crate) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading || self.saving || !self.dirty {
            return;
        }
        let Some(snapshot) = self.snapshot.clone() else {
            return;
        };
        let project = self.source.project.clone();
        let text = self.input.read(cx).value().to_string();
        // Keep the cheap, persistent Rope matching these exact bytes. A later
        // edit remains dirty when this background save finishes.
        let saved_text = self.input.read(cx).text().clone();
        self.saving = true;
        self.error = None;
        self.error_during_load = false;
        let generation = self.generation;
        let work = cx
            .background_executor()
            .spawn(async move { project_files::save_text_file(&project, &snapshot, &text) });
        let handle = window.window_handle();
        self._save_task = Some(cx.spawn(async move |editor, cx| {
            let result = work.await;
            let _ = handle.update(cx, |_, window, cx| {
                let _ = editor.update(cx, |editor, cx| {
                    if editor.generation != generation {
                        return;
                    }
                    editor.complete_save(result, saved_text, window, cx);
                });
            });
        }));
        cx.emit(FileEditorEvent::Changed);
        cx.notify();
    }

    fn complete_save(
        &mut self,
        result: Result<FileSnapshot, FileError>,
        saved_text: Rope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.saving = false;
        match result {
            Ok(snapshot) => {
                self.snapshot = Some(snapshot);
                self.saved_text = saved_text;
                self.dirty = self.input.read(cx).text() != &self.saved_text;
                self.error = None;
                cx.emit(FileEditorEvent::Saved);
                if self.close_after_save {
                    self.close_after_save = false;
                    if self.dirty {
                        self.request_close(window, cx);
                    } else {
                        cx.emit(FileEditorEvent::CloseReady);
                    }
                }
            }
            Err(error) => {
                self.error = Some(error);
                self.close_after_save = false;
            }
        }
        cx.emit(FileEditorEvent::Changed);
        cx.notify();
    }

    pub(crate) fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.prompt_pending {
            return;
        }
        if self.saving {
            self.close_after_save = true;
            return;
        }
        if !self.dirty {
            cx.emit(FileEditorEvent::CloseReady);
            return;
        }
        self.prompt_pending = true;
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &format!("Save changes to {}?", self.title()),
            Some("Your unsaved edits will be lost if you discard them."),
            &["Cancel", "Save", "Discard"],
            cx,
        );
        let handle = window.window_handle();
        cx.spawn(async move |editor, cx| {
            let answer = answer.await.ok();
            let _ = handle.update(cx, |_, window, cx| {
                let _ = editor.update(cx, |editor, cx| {
                    editor.prompt_pending = false;
                    match answer {
                        Some(1) if editor.dirty => {
                            editor.close_after_save = true;
                            editor.save(window, cx);
                        }
                        Some(1 | 2) => cx.emit(FileEditorEvent::CloseReady),
                        _ => editor.focus(window, cx),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn request_reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.prompt_pending || self.loading || self.saving {
            return;
        }
        if !self.dirty {
            self.load(window, cx);
            return;
        }
        self.prompt_pending = true;
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            "Reload this file from disk?",
            Some("Reloading discards your unsaved edits. Copy any changes you want to keep first."),
            &["Cancel", "Reload"],
            cx,
        );
        let handle = window.window_handle();
        cx.spawn(async move |editor, cx| {
            let reload = answer.await.ok() == Some(1);
            let _ = handle.update(cx, |_, window, cx| {
                let _ = editor.update(cx, |editor, cx| {
                    editor.prompt_pending = false;
                    if reload {
                        editor.load(window, cx);
                    } else {
                        editor.focus(window, cx);
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn status(&self) -> String {
        if self.loading {
            return "Opening file…".to_owned();
        }
        if self.saving {
            return "Saving…".to_owned();
        }
        if self.dirty {
            return "Unsaved changes".to_owned();
        }
        if let Some(snapshot) = &self.snapshot {
            let ending = match snapshot.line_ending {
                LineEnding::Lf => "LF",
                LineEnding::CrLf => "CRLF",
            };
            return format!(
                "Saved · UTF-8{} · {ending}",
                if snapshot.bom { " BOM" } else { "" }
            );
        }
        "File unavailable".to_owned()
    }
}

impl EventEmitter<FileEditorEvent> for FileEditor {}

impl Render for FileEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ready = self.snapshot.is_some();
        let can_save = self.dirty && !self.loading && !self.saving;
        let position = self.cursor_position;
        div()
            .id("file-editor")
            .key_context("FileEditor")
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(theme::canvas())
            .on_action(cx.listener(|editor, _: &SaveFile, window, cx| editor.save(window, cx)))
            .when(ready, |view| {
                view.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .overflow_hidden()
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_MONO))
                        .child(
                            Input::new(&self.input)
                                .h_full()
                                .appearance(false)
                                .bordered(false)
                                .focus_bordered(false)
                                .disabled(self.loading)
                                .p(px(chrome::CONTENT_INSET))
                                .font_family(theme::mono())
                                .text_size(px(theme::T_MONO))
                                .line_height(px(chrome::TECH_LINE_HEIGHT))
                                .text_color(theme::bone()),
                        ),
                )
            })
            .when(!ready, |view| {
                view.child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .items_center()
                        .p(px(chrome::CONTENT_INSET))
                        .font_family(chrome::CHROME_FONT)
                        .text_size(px(chrome::CHROME_TEXT_SIZE))
                        .text_color(theme::ash())
                        .child(if self.loading {
                            "Opening file…"
                        } else {
                            "This file could not be opened."
                        }),
                )
            })
            .when_some(self.error.as_ref(), |view, error| {
                let conflict = *error == FileError::Conflict;
                view.child(
                    div()
                        .flex_shrink_0()
                        .px(px(chrome::CONTENT_INSET))
                        .py(px(chrome::INSET))
                        .flex()
                        .items_center()
                        .gap(px(chrome::GAP))
                        .bg(theme::error_wash())
                        .font_family(chrome::CHROME_FONT)
                        .text_size(px(chrome::CHROME_TEXT_SIZE))
                        .text_color(theme::error())
                        .child(div().flex_1().min_w_0().child(error.to_string()))
                        .child(
                            div()
                                .id("recover-file")
                                .tab_index(0)
                                .cursor_pointer()
                                .flex_shrink_0()
                                .h(px(chrome::CONTROL_HEIGHT))
                                .px(px(chrome::INSET))
                                .flex()
                                .items_center()
                                .rounded(px(chrome::CONTROL_RADIUS))
                                .border_1()
                                .border_color(theme::edge_soft())
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme::bone())
                                .hover(|button| button.bg(theme::panel_hover()))
                                .focus(|button| {
                                    button.bg(theme::panel_hover()).text_color(theme::focus())
                                })
                                .on_click(cx.listener(move |editor, _, window, cx| {
                                    if conflict
                                        || (editor.error_during_load && editor.snapshot.is_some())
                                    {
                                        editor.request_reload(window, cx);
                                    } else if editor.snapshot.is_some() {
                                        editor.save(window, cx);
                                    } else {
                                        editor.load(window, cx);
                                    }
                                }))
                                .child(if conflict {
                                    "Reload from disk"
                                } else {
                                    "Retry"
                                }),
                        ),
                )
            })
            .child(
                div()
                    .h(px(chrome::CONTROL_HEIGHT + chrome::SMALL_GAP * 2.0))
                    .flex_shrink_0()
                    .px(px(chrome::CONTENT_INSET))
                    .flex()
                    .items_center()
                    .gap(px(chrome::GAP))
                    .border_t_1()
                    .border_color(theme::edge_soft())
                    .bg(theme::floor())
                    .font_family(theme::mono())
                    .text_size(px(chrome::DETAIL_TEXT_SIZE))
                    .text_color(theme::ash())
                    .child(div().flex_1().min_w_0().truncate().child(self.status()))
                    .when(ready, |bar| {
                        bar.child(format!(
                            "Ln {}, Col {}",
                            position.line + 1,
                            position.character + 1
                        ))
                    })
                    .child(
                        div()
                            .id("save-file")
                            .h(px(chrome::CONTROL_HEIGHT))
                            .px(px(chrome::INSET))
                            .flex()
                            .items_center()
                            .flex_shrink_0()
                            .rounded(px(chrome::CONTROL_RADIUS))
                            .border_1()
                            .border_color(if can_save {
                                theme::focus()
                            } else {
                                theme::edge_soft()
                            })
                            .bg(if can_save {
                                theme::focus()
                            } else {
                                theme::canvas()
                            })
                            .font_family(chrome::CHROME_FONT)
                            .text_size(px(chrome::CHROME_TEXT_SIZE))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if can_save {
                                theme::canvas()
                            } else {
                                theme::ash()
                            })
                            .when(can_save, |button| {
                                button
                                    .tab_index(0)
                                    .cursor_pointer()
                                    .hover(|button| button.border_color(theme::bone()))
                                    .focus(|button| {
                                        button
                                            .border_color(theme::bone())
                                            .bg(theme::bone())
                                            .text_color(theme::canvas())
                                    })
                                    .on_click(
                                        cx.listener(|editor, _, window, cx| {
                                            editor.save(window, cx)
                                        }),
                                    )
                            })
                            .child("Save · Ctrl+S"),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use gpui_component::Root;

    fn snapshot(text: &str) -> FileSnapshot {
        FileSnapshot {
            path: PathBuf::from("synthetic-project/file.txt"),
            text: text.to_owned(),
            baseline: text.as_bytes().to_vec(),
            line_ending: LineEnding::Lf,
            bom: false,
        }
    }

    #[gpui::test]
    fn editor_tracks_unicode_edits_undo_and_late_save_completion(cx: &mut TestAppContext) {
        cx.update(FileEditor::initialize);
        let (root, cx) = cx.add_window_view(|window, cx| {
            let editor = cx.new(|cx| {
                let mut editor = FileEditor::new(
                    Source {
                        project: PathBuf::from("synthetic-project"),
                        path: PathBuf::from("synthetic-project/file.txt"),
                    },
                    "original",
                    window,
                    cx,
                );
                editor.snapshot = Some(snapshot("original"));
                editor.focus(window, cx);
                editor
            });
            Root::new(editor, window, cx)
        });
        let editor = root.read_with(cx, |root, _| {
            root.view().clone().downcast::<FileEditor>().ok().unwrap()
        });
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input("😀 edited");
        assert!(editor.read_with(cx, |editor, _| editor.is_dirty()));
        cx.simulate_keystrokes("ctrl-z");
        assert!(!editor.read_with(cx, |editor, _| editor.is_dirty()));

        let mut events = cx.events(&editor);
        cx.update(|window, app| {
            editor.update(app, |editor, cx| {
                editor
                    .input
                    .update(cx, |input, cx| input.set_value("first edit", window, cx));
                let saved = editor.input.read(cx).text().clone();
                editor
                    .input
                    .update(cx, |input, cx| input.set_value("newer edit", window, cx));
                editor.complete_save(Ok(snapshot("first edit")), saved.clone(), window, cx);
                assert!(editor.is_dirty());
                assert_eq!(editor.input.read(cx).value().as_ref(), "newer edit");
                assert_eq!(editor.snapshot.as_ref().unwrap().text, "first edit");
            });
        });
        let mut saved_events = 0;
        while let Ok(event) = events.try_recv() {
            if event == FileEditorEvent::Saved {
                saved_events += 1;
            }
        }
        assert_eq!(saved_events, 1);

        cx.update(|window, app| {
            editor.update(app, |editor, cx| {
                editor.close_after_save = true;
                editor.complete_save(
                    Err(FileError::Conflict),
                    editor.saved_text.clone(),
                    window,
                    cx,
                );
                assert!(editor.is_dirty());
                assert!(!editor.close_after_save);
                assert_eq!(editor.error, Some(FileError::Conflict));
            });
        });
        assert_eq!(events.try_recv().unwrap(), FileEditorEvent::Changed);
        assert!(events.try_recv().is_err());

        cx.update(|window, app| {
            editor.update(app, |editor, cx| {
                editor.close_after_save = true;
                editor.complete_save(
                    Ok(snapshot("newer edit")),
                    editor.input.read(cx).text().clone(),
                    window,
                    cx,
                );
                assert!(!editor.is_dirty());
            });
        });
        assert_eq!(events.try_recv().unwrap(), FileEditorEvent::Saved);
        assert_eq!(events.try_recv().unwrap(), FileEditorEvent::CloseReady);
        assert_eq!(events.try_recv().unwrap(), FileEditorEvent::Changed);
        assert!(events.try_recv().is_err());
    }
}
