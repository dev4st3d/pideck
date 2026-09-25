//! Adjustable workspace terminal panel backed by a real operating-system PTY.

use std::{
    ops::Range,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use gpui::{
    ClipboardItem, Context, CursorStyle, Entity, EventEmitter, FocusHandle, FontWeight,
    IntoElement, Keystroke, MouseButton, Render, SharedString, Subscription, Task, Window, div,
    prelude::*, px, svg,
};

use super::file_editor::{FileEditor, FileEditorEvent};
use super::image_viewer::{ImageViewer, ImageViewerEvent};
use super::terminal_manager::text_tooltip;
use crate::services::terminal::{TerminalEvent, TerminalSize, TerminalWorker};
use crate::services::terminal_engine::TerminalEngine;
use crate::theme;
use crate::theme::terminal_manager as chrome;

mod diff;
mod element;
use diff::DiffView;
mod input;
use element::{TerminalElement, TerminalGeometry};

pub(crate) const TERMINAL_LINE_HEIGHT: f32 = 18.0;
const OUTPUT_BATCH_INTERVAL: Duration = Duration::from_millis(8);
const MAX_TERMINAL_TABS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TerminalPanelEvent {
    CloseRequested,
    LayoutChanged,
    ContentChanged,
    FilesSaved,
    Review {
        file: crate::services::project_git::ReviewFile,
        action: crate::services::project_git::ReviewAction,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TerminalStatus {
    Dormant,
    Starting,
    Running,
    Exited(u32),
    Failed(String),
}

struct TerminalSession {
    workspace: PathBuf,
    shell: String,
    worker: Option<TerminalWorker>,
    engine: TerminalEngine,
    size: TerminalSize,
    status: TerminalStatus,
    generation: u64,
    focus_handle: FocusHandle,
    geometry: Option<TerminalGeometry>,
    selecting: bool,
    pressed_button: Option<crate::services::terminal_engine::MouseButton>,
    last_mouse_cell: Option<(usize, usize)>,
    composition: String,
    composition_selection: Range<usize>,
    cursor_visible: bool,
    cursor_blinking: bool,
    _focus_subscriptions: Option<(Subscription, Subscription)>,
    _event_task: Option<Task<()>>,
    _sync_task: Option<Task<()>>,
    _cursor_task: Option<Task<()>>,
}

impl TerminalSession {
    fn new(workspace: PathBuf, size: TerminalSize, cx: &mut Context<Self>) -> Self {
        Self {
            workspace,
            shell: String::new(),
            worker: None,
            engine: new_engine(size),
            size,
            status: TerminalStatus::Dormant,
            generation: 1,
            focus_handle: cx.focus_handle(),
            geometry: None,
            selecting: false,
            pressed_button: None,
            last_mouse_cell: None,
            composition: String::new(),
            composition_selection: 0..0,
            cursor_visible: true,
            cursor_blinking: false,
            _focus_subscriptions: None,
            _event_task: None,
            _sync_task: None,
            _cursor_task: None,
        }
    }

    fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    fn activate(&mut self, cx: &mut Context<Self>) {
        if matches!(self.status, TerminalStatus::Dormant) {
            self.restart(cx);
        }
    }

    fn resize(&mut self, size: TerminalSize, cx: &mut Context<Self>) {
        if self.size == size {
            return;
        }
        self.size = size;
        self.engine.resize(size.rows, size.cols);
        if let Some(worker) = &self.worker {
            let _ = worker.resize(size);
        }
        cx.notify();
    }

    fn send_input(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        if bytes.is_empty() || !matches!(self.status, TerminalStatus::Running) {
            return;
        }
        self.engine.scroll_to_bottom();
        self.engine.clear_selection();
        self.start_cursor_blink(cx);
        self.write_pty(bytes, cx);
        cx.notify();
    }

    // Protocol replies and mouse/focus events must not reset the user's viewport.
    fn write_pty(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        if bytes.is_empty() || !matches!(self.status, TerminalStatus::Running) {
            return;
        }
        if !self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.write_bytes(bytes))
        {
            self.status = TerminalStatus::Failed(
                "The terminal is no longer accepting input. Restart it to continue.".to_owned(),
            );
            cx.notify();
        }
    }

    fn pump_events(
        events: async_channel::Receiver<TerminalEvent>,
        generation: u64,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        cx.spawn(async move |view, cx| {
            while let Ok(first) = events.recv().await {
                // ConPTY can split a clear-and-redraw across separate reads.
                // Give the rest of that burst one bounded window to arrive;
                // never extend the deadline under continuous output.
                cx.background_executor().timer(OUTPUT_BATCH_INTERVAL).await;
                let mut batch = Vec::with_capacity(8);
                batch.push(first);
                while batch.len() < 64 {
                    let Ok(event) = events.try_recv() else {
                        break;
                    };
                    batch.push(event);
                }
                let keep_pumping = view
                    .update(cx, |view, cx| {
                        if view.generation != generation {
                            return false;
                        }
                        view.apply_events(batch, cx);
                        true
                    })
                    .unwrap_or(false);
                if !keep_pumping {
                    break;
                }
            }
        })
    }

    fn apply_events(&mut self, events: Vec<TerminalEvent>, cx: &mut Context<Self>) {
        for event in events {
            match event {
                TerminalEvent::Started { shell, .. } => {
                    self.shell = shell;
                    self.status = TerminalStatus::Running;
                }
                TerminalEvent::Output(bytes) => {
                    let replies = self.engine.feed(&bytes);
                    self.write_pty(replies, cx);
                }
                TerminalEvent::Exited { code } => self.status = TerminalStatus::Exited(code),
                TerminalEvent::Error { summary } => {
                    if !matches!(self.status, TerminalStatus::Exited(_)) {
                        self.status = TerminalStatus::Failed(summary);
                    }
                }
            }
        }
        self.schedule_sync_timeout(cx);
        cx.notify();
    }

    fn schedule_sync_timeout(&mut self, cx: &mut Context<Self>) {
        self._sync_task.take();
        let Some(deadline) = self.engine.sync_deadline() else {
            return;
        };
        let generation = self.generation;
        self._sync_task = Some(cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(deadline.saturating_duration_since(Instant::now()))
                .await;
            let _ = view.update(cx, |view, cx| {
                if view.generation != generation {
                    return;
                }
                let replies = view.engine.flush_sync_timeout(Instant::now());
                view.write_pty(replies, cx);
                view.schedule_sync_timeout(cx);
                cx.notify();
            });
        }));
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.worker = Some(TerminalWorker::spawn(self.workspace.clone(), self.size));
        self.engine = new_engine(self.size);
        self._sync_task.take();
        self.composition.clear();
        self.composition_selection = 0..0;
        self.selecting = false;
        self.pressed_button = None;
        self.status = TerminalStatus::Starting;
        if let Some(worker) = &self.worker {
            self._event_task = Some(Self::pump_events(worker.events(), self.generation, cx));
        }
        cx.notify();
    }

    fn copy_selection(&self, cx: &mut Context<Self>) {
        let contents = self
            .engine
            .selected_text()
            .unwrap_or_else(|| self.engine.visible_text());
        if !contents.trim().is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(contents));
        }
    }
}

fn new_engine(size: TerminalSize) -> TerminalEngine {
    let mut engine = TerminalEngine::new(size.rows, size.cols);
    apply_engine_appearance(&mut engine);
    engine
}

fn apply_engine_appearance(engine: &mut TerminalEngine) {
    let rgb = |color: gpui::Rgba| {
        let color = u32::from(color);
        ((color >> 24) as u8, (color >> 16) as u8, (color >> 8) as u8)
    };
    engine.set_default_colors(rgb(theme::bone_dim()), rgb(theme::canvas()));
}

enum FileTab {
    Editor(Entity<FileEditor>),
    Image(Entity<ImageViewer>),
}

enum TabContent {
    Terminal(Entity<TerminalSession>),
    File {
        path: PathBuf,
        tab: FileTab,
    },
    Diff {
        title: String,
        view: Entity<DiffView>,
    },
}

impl FileTab {
    fn editor(&self) -> Option<&Entity<FileEditor>> {
        match self {
            Self::Editor(editor) => Some(editor),
            Self::Image(_) => None,
        }
    }

    fn title(&self, cx: &gpui::App) -> String {
        match self {
            Self::Editor(editor) => editor.read(cx).title(),
            Self::Image(viewer) => viewer.read(cx).title(),
        }
    }

    fn is_busy(&self, cx: &gpui::App) -> bool {
        match self {
            Self::Editor(editor) => editor.read(cx).is_busy(),
            Self::Image(viewer) => viewer.read(cx).is_busy(),
        }
    }

    fn is_dirty(&self, cx: &gpui::App) -> bool {
        match self {
            Self::Editor(editor) => editor.read(cx).is_dirty(),
            Self::Image(_) => false,
        }
    }

    fn focus(&self, window: &mut Window, cx: &mut gpui::App) {
        match self {
            Self::Editor(editor) => editor.update(cx, |editor, cx| editor.focus(window, cx)),
            Self::Image(viewer) => viewer.update(cx, |viewer, cx| viewer.focus(window, cx)),
        }
    }

    fn notify(&self, cx: &mut gpui::App) {
        match self {
            Self::Editor(editor) => editor.update(cx, |_, cx| cx.notify()),
            Self::Image(viewer) => viewer.update(cx, |_, cx| cx.notify()),
        }
    }

    fn retarget(&self, path: PathBuf, cx: &mut gpui::App) {
        match self {
            Self::Editor(editor) => editor.update(cx, |editor, cx| editor.retarget(path, cx)),
            Self::Image(viewer) => viewer.update(cx, |viewer, cx| viewer.retarget(path, cx)),
        }
    }

    fn reload_clean(&self, window: &mut Window, cx: &mut gpui::App) {
        match self {
            Self::Editor(editor) => editor.update(cx, |editor, cx| editor.reload_clean(window, cx)),
            Self::Image(viewer) => viewer.update(cx, |viewer, cx| viewer.reload_clean(window, cx)),
        }
    }

    fn into_any_view(&self) -> gpui::AnyView {
        match self {
            Self::Editor(editor) => editor.clone().into(),
            Self::Image(viewer) => viewer.clone().into(),
        }
    }
}

impl TabContent {
    fn editor(&self) -> Option<&Entity<FileEditor>> {
        match self {
            Self::File { tab, .. } => tab.editor(),
            Self::Terminal(_) | Self::Diff { .. } => None,
        }
    }
}

struct WorkspaceTab {
    id: u64,
    content: TabContent,
    _subscription: Subscription,
}

pub(crate) struct TerminalView {
    workspace: PathBuf,
    tabs: Vec<WorkspaceTab>,
    active: usize,
    last_terminal_id: Option<u64>,
    next_id: u64,
    fallback_focus: FocusHandle,
    close_pending: bool,
    files_locked: bool,
}

impl TerminalView {
    pub(crate) fn new(workspace: PathBuf, cx: &mut Context<Self>) -> Self {
        let size = TerminalSize::default();
        let session = cx.new(|cx| TerminalSession::new(workspace.clone(), size, cx));
        let subscription = cx.observe(&session, |_, _, cx| cx.notify());
        Self {
            workspace,
            tabs: vec![WorkspaceTab {
                id: 1,
                content: TabContent::Terminal(session),
                _subscription: subscription,
            }],
            active: 0,
            last_terminal_id: Some(1),
            next_id: 2,
            fallback_focus: cx.focus_handle(),
            close_pending: false,
            files_locked: false,
        }
    }

    pub(crate) fn apply_appearance(&mut self, cx: &mut Context<Self>) {
        for tab in &self.tabs {
            match &tab.content {
                TabContent::Terminal(session) => session.update(cx, |session, cx| {
                    // Recolor the existing screen and protocol defaults without replacing its PTY or state.
                    apply_engine_appearance(&mut session.engine);
                    cx.notify();
                }),
                TabContent::File { tab, .. } => tab.notify(cx),
                TabContent::Diff { view, .. } => view.update(cx, |_, cx| cx.notify()),
            }
        }
        cx.notify();
    }

    pub(crate) fn restore_layout(&mut self, count: usize, active: usize, cx: &mut Context<Self>) {
        let count = count.min(MAX_TERMINAL_TABS);
        let mut retained = 0;
        self.tabs.retain(|tab| {
            if matches!(tab.content, TabContent::Terminal(_)) {
                retained += 1;
                return retained <= count;
            }
            true
        });
        while self.terminal_count() < count {
            self.push_tab(cx);
        }
        self.active = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| matches!(tab.content, TabContent::Terminal(_)))
            .nth(active.min(count.saturating_sub(1)))
            .map_or(0, |(index, _)| index);
        self.last_terminal_id = self
            .tabs
            .get(self.active)
            .filter(|tab| matches!(tab.content, TabContent::Terminal(_)))
            .map(|tab| tab.id);
        cx.notify();
    }

    pub(crate) fn terminal_count(&self) -> usize {
        self.tabs
            .iter()
            .filter(|tab| matches!(tab.content, TabContent::Terminal(_)))
            .count()
    }

    pub(crate) fn layout_snapshot(&self) -> (usize, usize) {
        let terminals = self
            .tabs
            .iter()
            .filter(|tab| matches!(tab.content, TabContent::Terminal(_)));
        let active = terminals
            .clone()
            .position(|tab| Some(tab.id) == self.last_terminal_id)
            .unwrap_or(0);
        (terminals.count(), active)
    }

    pub(crate) fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self
            .tabs
            .get(self.active)
            .filter(|tab| matches!(tab.content, TabContent::Terminal(_)))
        {
            self.last_terminal_id = Some(tab.id);
        }
        self.activate(cx);
        match self.tabs.get(self.active).map(|tab| &tab.content) {
            Some(TabContent::Terminal(session)) => window.focus(&session.read(cx).focus_handle()),
            Some(TabContent::File { tab, .. }) => tab.focus(window, cx),
            Some(TabContent::Diff { view, .. }) => window.focus(&view.read(cx).focus),
            None => window.focus(&self.fallback_focus),
        }
    }

    pub(crate) fn has_running_sessions(&self, cx: &gpui::App) -> bool {
        self.tabs.iter().any(|tab| match &tab.content {
            TabContent::Terminal(session) => matches!(
                session.read(cx).status,
                TerminalStatus::Starting | TerminalStatus::Running
            ),
            TabContent::File { .. } | TabContent::Diff { .. } => false,
        })
    }

    pub(crate) fn has_dirty_files(&self, cx: &gpui::App) -> bool {
        self.tabs
            .iter()
            .filter_map(|tab| tab.content.editor())
            .any(|editor| editor.read(cx).is_dirty())
    }

    pub(crate) fn has_saving_files(&self, cx: &gpui::App) -> bool {
        self.tabs
            .iter()
            .filter_map(|tab| tab.content.editor())
            .any(|editor| editor.read(cx).is_saving())
    }

    pub(crate) fn paths_busy(&self, paths: &[PathBuf], deleting: bool, cx: &gpui::App) -> bool {
        self.tabs.iter().any(|tab| match &tab.content {
            TabContent::File { path, tab }
                if paths.iter().any(|parent| path.starts_with(parent)) =>
            {
                tab.is_busy(cx) || (deleting && tab.is_dirty(cx))
            }
            _ => false,
        })
    }

    pub(crate) fn begin_file_change(&mut self, allow_dirty: bool, cx: &mut Context<Self>) -> bool {
        if self.files_locked
            || self.tabs.iter().any(|tab| match &tab.content {
                TabContent::File { tab, .. } => {
                    tab.is_busy(cx) || (!allow_dirty && tab.is_dirty(cx))
                }
                TabContent::Terminal(_) | TabContent::Diff { .. } => false,
            })
        {
            return false;
        }
        self.files_locked = true;
        for tab in &self.tabs {
            if let TabContent::Diff { view, .. } = &tab.content {
                view.update(cx, |view, cx| view.set_operation_busy(true, cx));
            }
            if let Some(editor) = tab.content.editor() {
                editor.update(cx, |editor, cx| editor.lock_saves(true, cx));
            }
        }
        true
    }

    pub(crate) fn end_file_change(&mut self, cx: &mut Context<Self>) {
        self.files_locked = false;
        for tab in &self.tabs {
            if let TabContent::Diff { view, .. } = &tab.content {
                view.update(cx, |view, cx| view.set_operation_busy(false, cx));
            }
            if let Some(editor) = tab.content.editor() {
                editor.update(cx, |editor, cx| editor.lock_saves(false, cx));
            }
        }
        cx.notify();
    }

    pub(crate) fn files_moved(&mut self, moves: &[(PathBuf, PathBuf)], cx: &mut Context<Self>) {
        for tab in &mut self.tabs {
            if let TabContent::File { path, tab } = &mut tab.content {
                for (from, to) in moves {
                    if let Ok(relative) = path.strip_prefix(from) {
                        *path = to.join(relative);
                        tab.retarget(path.clone(), cx);
                        break;
                    }
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn reload_clean_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stale_diffs: Vec<_> = self
            .tabs
            .iter()
            .filter(|tab| matches!(tab.content, TabContent::Diff { .. }))
            .map(|tab| tab.id)
            .collect();
        for id in stale_diffs {
            self.remove_tab(id, window, cx);
        }
        for tab in &self.tabs {
            if let TabContent::File { tab, .. } = &tab.content {
                tab.reload_clean(window, cx);
            }
        }
    }

    #[cfg(test)]
    fn active_file(&self) -> Option<PathBuf> {
        match self.tabs.get(self.active).map(|tab| &tab.content) {
            Some(TabContent::File { path, .. }) => Some(path.clone()),
            Some(TabContent::Terminal(_) | TabContent::Diff { .. }) | None => None,
        }
    }

    pub(crate) fn save_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editors = self
            .tabs
            .iter()
            .filter_map(|tab| tab.content.editor())
            .cloned()
            .collect::<Vec<_>>();
        for editor in editors {
            if editor.read(cx).is_dirty() && !editor.read(cx).is_saving() {
                editor.update(cx, |editor, cx| editor.save(window, cx));
            }
        }
    }

    pub(crate) fn focus_dirty_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self
            .tabs
            .iter()
            .find(|tab| {
                tab.content
                    .editor()
                    .is_some_and(|editor| editor.read(cx).is_dirty())
            })
            .map(|tab| tab.id);
        if let Some(id) = id {
            self.select_tab(id, window, cx);
        }
    }

    pub(crate) fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.files_locked {
            return;
        }
        let path = if path.is_absolute() {
            path
        } else {
            self.workspace.join(path)
        };
        let path = crate::services::paths::without_windows_verbatim_prefix(&path);
        if let Some(id) = self
            .tabs
            .iter()
            .find(|tab| match &tab.content {
                TabContent::File { path: existing, .. } => same_file_path(existing, &path),
                TabContent::Terminal(_) | TabContent::Diff { .. } => false,
            })
            .map(|tab| tab.id)
        {
            self.select_tab(id, window, cx);
            return;
        }
        let project = self.workspace.clone();
        let tab = if crate::services::project_files::is_image_path(&path) {
            FileTab::Image(cx.new(|cx| ImageViewer::open(project, path.clone(), window, cx)))
        } else {
            FileTab::Editor(cx.new(|cx| FileEditor::open(project, path.clone(), window, cx)))
        };
        self.push_file(path, tab, window, cx);
    }

    pub(crate) fn open_diff(
        &mut self,
        file: crate::services::project_git::ReviewFile,
        content: crate::services::project_git::DiffContent,
        activate: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = file
            .commit
            .as_ref()
            .map(|commit| commit.summary.subject.clone())
            .unwrap_or_else(|| {
                file.path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            });
        if let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| matches!(tab.content, TabContent::Diff { .. }))
        {
            if let TabContent::Diff { title: label, view } = &mut tab.content {
                *label = title;
                view.update(cx, |view, cx| {
                    view.update_snapshot(file, content, cx);
                    view.set_operation_busy(self.files_locked, cx);
                });
            }
            let id = tab.id;
            if activate {
                self.select_tab(id, window, cx);
            }
            cx.notify();
            return;
        }
        if !activate {
            return;
        }
        let view = cx.new(|cx| DiffView::new(self.workspace.clone(), file, content, cx));
        view.update(cx, |view, cx| {
            view.set_operation_busy(self.files_locked, cx)
        });
        let subscription = cx.subscribe_in(
            &view,
            window,
            |pane, diff, event: &diff::DiffEvent, window, cx| {
                let file = diff.read(cx).file.clone();
                match event {
                    diff::DiffEvent::OpenFile => pane.open_file(file.path, window, cx),
                    diff::DiffEvent::Review(action) => cx.emit(TerminalPanelEvent::Review {
                        file,
                        action: action.clone(),
                    }),
                }
            },
        );
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.tabs.push(WorkspaceTab {
            id,
            content: TabContent::Diff { title, view },
            _subscription: subscription,
        });
        self.select_tab(id, window, cx);
    }

    fn push_file(
        &mut self,
        path: PathBuf,
        tab: FileTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let subscription = match &tab {
            FileTab::Editor(editor) => cx.subscribe_in(
                editor,
                window,
                move |view, _, event, window, cx| match event {
                    FileEditorEvent::Changed => {
                        cx.emit(TerminalPanelEvent::ContentChanged);
                        cx.notify();
                    }
                    FileEditorEvent::Saved => {
                        cx.emit(TerminalPanelEvent::FilesSaved);
                        cx.emit(TerminalPanelEvent::ContentChanged);
                        cx.notify();
                    }
                    FileEditorEvent::CloseReady => view.remove_tab(id, window, cx),
                },
            ),
            FileTab::Image(viewer) => {
                cx.subscribe_in(viewer, window, move |_, _, event, _, cx| match event {
                    ImageViewerEvent::Changed => {
                        cx.emit(TerminalPanelEvent::ContentChanged);
                        cx.notify();
                    }
                })
            }
        };
        self.tabs.push(WorkspaceTab {
            id,
            content: TabContent::File { path, tab },
            _subscription: subscription,
        });
        self.select_tab(id, window, cx);
    }

    pub(crate) fn close_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active) {
            self.close_tab(tab.id, window, cx);
        }
    }

    pub(crate) fn cycle_tab(&mut self, reverse: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        let index = next_tab_index(self.active, self.tabs.len(), reverse);
        self.select_tab(self.tabs[index].id, window, cx);
    }

    pub(crate) fn activate(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active)
            && let TabContent::Terminal(session) = &tab.content
        {
            session.update(cx, |session, cx| session.activate(cx));
        }
    }

    fn push_tab(&mut self, cx: &mut Context<Self>) -> bool {
        if self.terminal_count() >= MAX_TERMINAL_TABS {
            return false;
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let workspace = self.workspace.clone();
        let size = self
            .tabs
            .iter()
            .find_map(|tab| match &tab.content {
                TabContent::Terminal(session) => Some(session.read(cx).size),
                TabContent::File { .. } | TabContent::Diff { .. } => None,
            })
            .unwrap_or_default();
        let session = cx.new(|cx| TerminalSession::new(workspace, size, cx));
        let subscription = cx.observe(&session, |_, _, cx| cx.notify());
        self.tabs.push(WorkspaceTab {
            id,
            content: TabContent::Terminal(session),
            _subscription: subscription,
        });
        self.active = self.tabs.len() - 1;
        self.last_terminal_id = Some(id);
        true
    }

    pub(crate) fn add_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.push_tab(cx) {
            return;
        }
        self.focus(window, cx);
        cx.emit(TerminalPanelEvent::LayoutChanged);
        cx.notify();
    }

    fn select_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        self.active = index;
        if matches!(self.tabs[index].content, TabContent::Terminal(_)) {
            self.last_terminal_id = Some(id);
        }
        self.focus(window, cx);
        cx.emit(TerminalPanelEvent::LayoutChanged);
        cx.emit(TerminalPanelEvent::ContentChanged);
        cx.notify();
    }

    fn close_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.close_pending {
            return;
        }
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        if let Some(editor) = tab.content.editor().cloned() {
            editor.update(cx, |editor, cx| editor.request_close(window, cx));
            return;
        }
        let running = match &tab.content {
            TabContent::Terminal(session) => matches!(
                session.read(cx).status,
                TerminalStatus::Starting | TerminalStatus::Running
            ),
            TabContent::File { .. } | TabContent::Diff { .. } => false,
        };
        if !running {
            self.remove_tab(id, window, cx);
            return;
        }
        self.close_pending = true;
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            "Close this terminal?",
            Some("The shell and any commands running in this terminal will stop."),
            &["Cancel", "Close terminal"],
            cx,
        );
        let handle = window.window_handle();
        cx.spawn(async move |view, cx| {
            let close = answer.await.ok() == Some(1);
            let _ = handle.update(cx, |_, window, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.close_pending = false;
                    if close {
                        view.remove_tab(id, window, cx);
                    } else {
                        view.focus(window, cx);
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn remove_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        self.tabs.remove(index);
        if self.last_terminal_id == Some(id) {
            self.last_terminal_id = self
                .tabs
                .iter()
                .find(|tab| matches!(tab.content, TabContent::Terminal(_)))
                .map(|tab| tab.id);
        }
        if self.tabs.is_empty() {
            self.active = 0;
            cx.emit(TerminalPanelEvent::LayoutChanged);
            cx.emit(TerminalPanelEvent::ContentChanged);
            cx.emit(TerminalPanelEvent::CloseRequested);
            cx.notify();
            return;
        }
        self.active = active_index_after_close(self.active, index, self.tabs.len());
        self.focus(window, cx);
        cx.emit(TerminalPanelEvent::LayoutChanged);
        cx.emit(TerminalPanelEvent::ContentChanged);
        cx.notify();
    }
}

fn same_file_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.as_os_str()
            .as_encoded_bytes()
            .eq_ignore_ascii_case(right.as_os_str().as_encoded_bytes())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

impl TerminalStatus {
    fn label(&self) -> String {
        match self {
            Self::Dormant => "Ready".to_owned(),
            Self::Starting => "Starting…".to_owned(),
            Self::Running => "Running".to_owned(),
            Self::Exited(code) => format!("Exited ({code})"),
            Self::Failed(_) => "Error".to_owned(),
        }
    }
}

fn next_tab_index(active: usize, count: usize, reverse: bool) -> usize {
    if count == 0 {
        return 0;
    }
    if reverse {
        (active + count - 1) % count
    } else {
        (active + 1) % count
    }
}

fn active_index_after_close(active: usize, removed: usize, remaining: usize) -> usize {
    if removed < active {
        active - 1
    } else {
        active.min(remaining.saturating_sub(1))
    }
}

fn terminal_key_bytes(keystroke: &Keystroke, application_cursor: bool) -> Option<Vec<u8>> {
    let modifiers = keystroke.modifiers;
    // Project and tab shortcuts belong to the manager, as do F6 and Shift+F6.
    // Ordinary control keys (especially Ctrl+C), Tab, and Shift+Tab belong to
    // the running terminal program.
    if keystroke.key == "f6"
        || (modifiers.control
            && (matches!(keystroke.key.as_str(), "`" | "tab")
                || (modifiers.shift
                    && matches!(keystroke.key.as_str(), "t" | "w" | "o" | "b" | "l"))
                || (modifiers.alt && matches!(keystroke.key.as_str(), "up" | "down"))))
    {
        return None;
    }
    if modifiers.platform {
        return None;
    }
    let modifier = 1
        + u8::from(modifiers.shift)
        + 2 * u8::from(modifiers.alt)
        + 4 * u8::from(modifiers.control);
    if modifier > 1 {
        let cursor = match keystroke.key.as_str() {
            "up" => Some('A'),
            "down" => Some('B'),
            "right" => Some('C'),
            "left" => Some('D'),
            "home" => Some('H'),
            "end" => Some('F'),
            _ => None,
        };
        if let Some(cursor) = cursor {
            return Some(format!("\x1b[1;{modifier}{cursor}").into_bytes());
        }
    }

    let named = match keystroke.key.as_str() {
        "space" if modifiers.control => Some(b"\0".as_slice()),
        "space" => Some(b" ".as_slice()),
        "enter" => Some(b"\r".as_slice()),
        "backspace" => Some(b"\x7f".as_slice()),
        "tab" if modifiers.shift => Some(b"\x1b[Z".as_slice()),
        "tab" => Some(b"\t".as_slice()),
        "escape" => Some(b"\x1b".as_slice()),
        "up" if application_cursor => Some(b"\x1bOA".as_slice()),
        "down" if application_cursor => Some(b"\x1bOB".as_slice()),
        "right" if application_cursor => Some(b"\x1bOC".as_slice()),
        "left" if application_cursor => Some(b"\x1bOD".as_slice()),
        "up" => Some(b"\x1b[A".as_slice()),
        "down" => Some(b"\x1b[B".as_slice()),
        "right" => Some(b"\x1b[C".as_slice()),
        "left" => Some(b"\x1b[D".as_slice()),
        "home" => Some(b"\x1b[H".as_slice()),
        "end" => Some(b"\x1b[F".as_slice()),
        "delete" => Some(b"\x1b[3~".as_slice()),
        "insert" => Some(b"\x1b[2~".as_slice()),
        "pageup" => Some(b"\x1b[5~".as_slice()),
        "pagedown" => Some(b"\x1b[6~".as_slice()),
        "f1" => Some(b"\x1bOP".as_slice()),
        "f2" => Some(b"\x1bOQ".as_slice()),
        "f3" => Some(b"\x1bOR".as_slice()),
        "f4" => Some(b"\x1bOS".as_slice()),
        "f5" => Some(b"\x1b[15~".as_slice()),
        "f6" => Some(b"\x1b[17~".as_slice()),
        "f7" => Some(b"\x1b[18~".as_slice()),
        "f8" => Some(b"\x1b[19~".as_slice()),
        "f9" => Some(b"\x1b[20~".as_slice()),
        "f10" => Some(b"\x1b[21~".as_slice()),
        "f11" => Some(b"\x1b[23~".as_slice()),
        "f12" => Some(b"\x1b[24~".as_slice()),
        _ => None,
    };
    if let Some(named) = named {
        let mut bytes = Vec::with_capacity(named.len() + usize::from(modifiers.alt));
        if modifiers.alt {
            bytes.push(0x1b);
        }
        bytes.extend_from_slice(named);
        return Some(bytes);
    }

    if modifiers.control && !modifiers.alt && !modifiers.platform {
        let bytes = keystroke.key.as_bytes();
        if bytes.len() == 1 {
            let byte = bytes[0].to_ascii_lowercase();
            if byte.is_ascii_lowercase() {
                return Some(vec![byte - b'a' + 1]);
            }
            return match byte {
                b'[' => Some(vec![0x1b]),
                b'\\' => Some(vec![0x1c]),
                b']' => Some(vec![0x1d]),
                b'^' => Some(vec![0x1e]),
                b'_' => Some(vec![0x1f]),
                _ => None,
            };
        }
        return None;
    }
    let text = keystroke.key_char.as_ref()?;
    let mut bytes = Vec::with_capacity(text.len() + usize::from(modifiers.alt));
    if modifiers.alt {
        bytes.push(0x1b);
    }
    bytes.extend_from_slice(text.as_bytes());
    Some(bytes)
}

impl Render for TerminalSession {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_focus_tracking(window, cx);
        let restartable = matches!(
            self.status,
            TerminalStatus::Exited(_) | TerminalStatus::Failed(_)
        );
        let status = match &self.status {
            TerminalStatus::Failed(summary) => summary.clone(),
            status => status.label(),
        };

        div()
            .id("terminal-session")
            .track_focus(&self.focus_handle)
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(theme::canvas())
            .child(
                div()
                    .id("terminal-output")
                    .track_focus(&self.focus_handle)
                    .tab_index(0)
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .p(px(chrome::INSET))
                    .bg(theme::canvas())
                    .cursor(CursorStyle::IBeam)
                    .font_family(theme::mono())
                    .text_size(theme::text_size(theme::T_MONO))
                    .line_height(px(TERMINAL_LINE_HEIGHT))
                    .text_color(theme::bone_dim())
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                    .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
                    .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
                    .on_key_down(cx.listener(Self::on_key_down))
                    .on_scroll_wheel(cx.listener(Self::on_scroll))
                    .child(TerminalElement {
                        session: cx.entity(),
                    }),
            )
            .when(
                restartable || matches!(self.status, TerminalStatus::Starting),
                |panel| {
                    panel.child(
                        div()
                            .h(px(chrome::CONTROL_HEIGHT + chrome::SMALL_GAP * 2.0))
                            .px(px(chrome::CONTENT_INSET))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(chrome::GAP))
                            .border_t_1()
                            .border_color(theme::edge_soft())
                            .bg(theme::floor())
                            .font_family(theme::mono())
                            .text_size(px(chrome::DETAIL_TEXT_SIZE))
                            .text_color(theme::ash())
                            .child(div().flex_1().min_w_0().truncate().child(status))
                            .when(restartable, |bar| {
                                bar.child(
                                    div()
                                        .id("restart-terminal")
                                        .flex_shrink_0()
                                        .tab_index(0)
                                        .cursor_pointer()
                                        .h(px(chrome::CONTROL_HEIGHT))
                                        .px(px(chrome::INSET))
                                        .flex()
                                        .items_center()
                                        .rounded(px(chrome::CONTROL_RADIUS))
                                        .border_1()
                                        .border_color(theme::edge_soft())
                                        .font_family(chrome::CHROME_FONT)
                                        .text_size(px(chrome::CHROME_TEXT_SIZE))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme::bone())
                                        .hover(|button| button.bg(theme::panel_hover()))
                                        .focus(|button| {
                                            button
                                                .bg(theme::panel_hover())
                                                .text_color(theme::focus())
                                        })
                                        .on_click(cx.listener(|session, _, window, cx| {
                                            session.restart(cx);
                                            window.focus(&session.focus_handle);
                                        }))
                                        .child("Restart terminal"),
                                )
                            }),
                    )
                },
            )
    }
}

impl EventEmitter<TerminalPanelEvent> for TerminalView {}

impl Render for TerminalView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_content: Option<gpui::AnyView> =
            self.tabs.get(self.active).map(|tab| match &tab.content {
                TabContent::Terminal(session) => session.clone().into(),
                TabContent::File { tab, .. } => tab.into_any_view(),
                TabContent::Diff { view, .. } => view.clone().into(),
            });
        let mut terminal_index = 0;
        let tabs = self
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let (label, tooltip, icon, status) = match &tab.content {
                    TabContent::Terminal(session) => {
                        terminal_index += 1;
                        let session = session.read(cx);
                        let title = session.engine.title().trim();
                        let label = terminal_tab_title(
                            if title.is_empty() {
                                &session.shell
                            } else {
                                title
                            },
                            terminal_index,
                        );
                        let status_color = match session.status {
                            TerminalStatus::Starting => theme::working(),
                            TerminalStatus::Failed(_) => theme::error(),
                            TerminalStatus::Dormant
                            | TerminalStatus::Running
                            | TerminalStatus::Exited(_) => theme::ash(),
                        };
                        let tooltip = format!(
                            "{} · {}",
                            if title.is_empty() { &label } else { title },
                            session.status.label()
                        );
                        (
                            label,
                            tooltip,
                            "icons/terminal.svg",
                            matches!(
                                session.status,
                                TerminalStatus::Failed(_) | TerminalStatus::Exited(_)
                            )
                            .then(|| (session.status.label(), status_color)),
                        )
                    }
                    TabContent::File { path, tab } => {
                        let label = tab.title(cx);
                        let icon = match tab {
                            FileTab::Image(_) => "icons/image.svg",
                            FileTab::Editor(_) => "icons/file-code.svg",
                        };
                        (
                            label,
                            path.to_string_lossy().into_owned(),
                            icon,
                            tab.is_dirty(cx).then(|| ("●".to_owned(), theme::ash())),
                        )
                    }
                    TabContent::Diff { title, .. } => (
                        title.clone(),
                        format!("{title} · Read-only diff"),
                        "icons/file-code.svg",
                        None,
                    ),
                };
                workspace_tab(
                    tab.id,
                    label,
                    tooltip,
                    icon,
                    status,
                    index == self.active,
                    cx,
                )
                .into_any_element()
            })
            .collect::<Vec<_>>();

        div()
            .id("workspace-terminal")
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(theme::canvas())
            .child(
                div()
                    .h(px(chrome::HEADER_HEIGHT))
                    .pr(px(8.0))
                    .flex_shrink_0()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(chrome::SMALL_GAP))
                    .bg(theme::floor())
                    .border_b_1()
                    .border_color(theme::edge_soft())
                    .child(
                        div()
                            .id("terminal-tabs")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_x_scroll()
                            .flex()
                            .flex_row()
                            .items_center()
                            .children(tabs),
                    )
                    .child({
                        use gpui_component::{
                            button::{Button, ButtonVariants},
                            menu::{DropdownMenu, PopupMenuItem},
                        };
                        let owner = cx.weak_entity();
                        let items: Vec<_> = self
                            .tabs
                            .iter()
                            .map(|tab| {
                                (
                                    tab.id,
                                    match &tab.content {
                                        TabContent::Terminal(session) => {
                                            session.read(cx).shell.clone()
                                        }
                                        TabContent::File { tab, .. } => tab.title(cx),
                                        TabContent::Diff { title, .. } => title.clone(),
                                    },
                                )
                            })
                            .collect();
                        Button::new("tab-list")
                            .label("⌄")
                            .ghost()
                            .w(px(28.0))
                            .h(px(32.0))
                            .tooltip("Open tabs")
                            .dropdown_menu(move |mut menu, _, _| {
                                for (id, title) in &items {
                                    let owner = owner.clone();
                                    let id = *id;
                                    menu = menu.item(PopupMenuItem::new(title.clone()).on_click(
                                        move |_, window, cx| {
                                            let owner = owner.clone();
                                            window.defer(cx, move |window, cx| {
                                                let _ = owner.update(cx, |view, cx| {
                                                    view.select_tab(id, window, cx)
                                                });
                                            });
                                        },
                                    ));
                                }
                                menu
                            })
                    }),
            )
            .when_some(active_content, |panel, content| {
                panel.child(div().flex_1().min_w_0().min_h_0().child(content))
            })
            .when(self.tabs.is_empty(), |panel| {
                panel.child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(theme::ash())
                        .child("Open a file or create a terminal"),
                )
            })
    }
}

fn terminal_tab_title(title: &str, index: usize) -> String {
    let path = title.trim_matches('"');
    let basename = path.rsplit(['\\', '/']).next().unwrap_or(path);
    let shell = match basename.to_ascii_lowercase().as_str() {
        "cmd" | "cmd.exe" => Some("CMD"),
        "pwsh" | "pwsh.exe" | "powershell" | "powershell.exe" => Some("PowerShell"),
        _ => None,
    };
    if let Some(shell) = shell {
        return if index > 1 {
            format!("{shell} {index}")
        } else {
            shell.to_owned()
        };
    }
    let absolute = Path::new(path).is_absolute()
        || path.starts_with("\\\\")
        || path.as_bytes().get(1) == Some(&b':');
    if absolute && path.to_ascii_lowercase().ends_with(".exe") {
        return path.rsplit(['\\', '/']).next().unwrap_or(path).to_owned();
    }
    if title.is_empty() {
        format!("Terminal {index}")
    } else {
        title.to_owned()
    }
}

fn workspace_tab(
    id: u64,
    label: String,
    tooltip: String,
    icon: &'static str,
    status: Option<(String, gpui::Rgba)>,
    selected: bool,
    cx: &mut Context<TerminalView>,
) -> impl IntoElement {
    let select_id = id;
    let close_id = id;
    let dirty = status.as_ref().is_some_and(|(label, _)| label == "●");
    div()
        .id(SharedString::from(format!("terminal-tab-{id}")))
        .h_full()
        .max_w(px(250.0))
        .min_w(px(180.0))
        .px(px(chrome::TAB_INSET))
        .flex_shrink_0()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(chrome::COMPACT_GAP))
        .font_family(chrome::CHROME_FONT)
        .text_size(px(12.0))
        .line_height(px(chrome::CHROME_LINE_HEIGHT))
        .bg(if selected {
            theme::canvas()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .border_b_2()
        .border_color(if selected {
            theme::focus()
        } else {
            gpui::rgba(0x00000000)
        })
        .text_color(if selected {
            theme::bone()
        } else {
            theme::ash()
        })
        .tab_index(0)
        .cursor_pointer()
        .hover(|tab| {
            if selected {
                tab
            } else {
                tab.bg(theme::panel_hover()).text_color(theme::bone())
            }
        })
        .focus(|tab| tab.text_color(theme::focus()))
        .tooltip(text_tooltip(tooltip))
        .on_click(cx.listener(move |view, _, window, cx| view.select_tab(select_id, window, cx)))
        .child(if icon == "rs" {
            div()
                .font_family(theme::mono())
                .text_size(px(13.0))
                .text_color(theme::ash())
                .child("rs")
                .into_any_element()
        } else {
            svg()
                .path(icon)
                .size(px(chrome::ICON_SIZE))
                .flex_shrink_0()
                .text_color(if selected {
                    theme::focus()
                } else {
                    theme::ash()
                })
                .into_any_element()
        })
        .child(
            div()
                .min_w_0()
                .flex_1()
                .max_w(px(170.0))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .font_family(chrome::CHROME_FONT)
                .text_size(px(12.0))
                .font_weight(FontWeight::NORMAL)
                .child(label),
        )
        .when_some(
            status.filter(|(label, _)| label != "●"),
            |tab, (status, color)| {
                tab.child(
                    div()
                        .flex_shrink_0()
                        .max_w(px(70.0))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .font_family(theme::mono())
                        .text_size(px(chrome::DETAIL_TEXT_SIZE))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(color)
                        .child(status),
                )
            },
        )
        .child(
            div()
                .id(SharedString::from(format!("close-terminal-tab-{id}")))
                .size(px(chrome::CONTROL_HEIGHT))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(chrome::CONTROL_RADIUS))
                .font_family(chrome::CHROME_FONT)
                .text_size(px(chrome::DETAIL_TEXT_SIZE))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if selected {
                    theme::ash()
                } else {
                    theme::smoke()
                })
                .tab_index(0)
                .cursor_pointer()
                .hover(|button| button.bg(theme::panel_hover()).text_color(theme::bone()))
                .focus(|button| button.bg(theme::panel_hover()).text_color(theme::focus()))
                .on_click(cx.listener(move |view, _, window, cx| {
                    cx.stop_propagation();
                    view.close_tab(close_id, window, cx)
                }))
                .tooltip(text_tooltip("Close tab · Ctrl+Shift+W"))
                .child(if dirty {
                    div()
                        .size(px(10.0))
                        .rounded_full()
                        .bg(theme::ash())
                        .into_any_element()
                } else {
                    svg()
                        .path("icons/close.svg")
                        .size(px(chrome::ICON_SIZE))
                        .text_color(theme::ash())
                        .into_any_element()
                }),
        )
}

#[cfg(test)]
mod tests {
    use gpui::Modifiers;

    use super::*;

    #[gpui::test]
    fn terminal_output_burst_keeps_old_frame_until_redraw_arrives(cx: &mut gpui::TestAppContext) {
        let (sender, receiver) = async_channel::bounded(64);
        let session = cx.new(|cx| {
            TerminalSession::new(
                PathBuf::from("synthetic-project"),
                TerminalSize::default(),
                cx,
            )
        });
        session.update(cx, |session, cx| {
            session.engine.feed(b"old frame");
            session._event_task = Some(TerminalSession::pump_events(
                receiver,
                session.generation,
                cx,
            ));
        });
        sender
            .try_send(TerminalEvent::Output(b"\x1b[2J\x1b[H".to_vec()))
            .unwrap();
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(4));
        cx.run_until_parked();
        session.read_with(cx, |session, _| {
            assert!(session.engine.visible_text().starts_with("old frame"));
        });
        sender
            .try_send(TerminalEvent::Output(b"new frame".to_vec()))
            .unwrap();
        cx.executor().advance_clock(Duration::from_millis(4));
        cx.run_until_parked();
        session.read_with(cx, |session, _| {
            assert!(session.engine.visible_text().starts_with("new frame"));
        });

        // A single final chunk must also render, even when the channel closes.
        sender
            .try_send(TerminalEvent::Output(b"!".to_vec()))
            .unwrap();
        drop(sender);
        cx.run_until_parked();
        cx.executor().advance_clock(OUTPUT_BATCH_INTERVAL);
        cx.run_until_parked();
        session.read_with(cx, |session, _| {
            assert!(session.engine.visible_text().starts_with("new frame!"));
        });
    }

    #[gpui::test]
    fn terminal_output_burst_does_not_cross_session_generations(cx: &mut gpui::TestAppContext) {
        let (sender, receiver) = async_channel::bounded(64);
        let session = cx.new(|cx| {
            TerminalSession::new(
                PathBuf::from("synthetic-project"),
                TerminalSize::default(),
                cx,
            )
        });
        session.update(cx, |session, cx| {
            session._event_task = Some(TerminalSession::pump_events(
                receiver,
                session.generation,
                cx,
            ));
        });
        sender
            .try_send(TerminalEvent::Output(b"stale output".to_vec()))
            .unwrap();
        cx.run_until_parked();
        session.update(cx, |session, _| {
            session.generation += 1;
            session.engine.feed(b"current frame");
        });
        cx.executor().advance_clock(OUTPUT_BATCH_INTERVAL);
        cx.run_until_parked();
        session.read_with(cx, |session, _| {
            assert!(session.engine.visible_text().starts_with("current frame"));
            assert!(!session.engine.visible_text().contains("stale"));
        });
    }

    #[test]
    fn appearance_changes_preserve_terminal_history_selection_and_modes() {
        struct RestoreAppearance(theme::Appearance);
        impl Drop for RestoreAppearance {
            fn drop(&mut self) {
                theme::set_appearance(self.0);
            }
        }
        let _restore = RestoreAppearance(theme::appearance());
        let mut engine = new_engine(TerminalSize::new(3, 24));
        let _ =
            engine.feed(b"one\r\ntwo\r\nthree\r\nfour\r\nfive\x1b[?2004h\x1b]2;Retained title\x07");
        engine.scroll(2);
        engine.select_all();
        let rows = engine.snapshot().rows;
        let offset = engine.display_offset();
        let selection = engine.selected_text();
        let modes = engine.modes();
        assert!(offset > 0);
        assert!(selection.is_some());
        for appearance in theme::Appearance::ALL {
            theme::set_appearance(appearance);
            apply_engine_appearance(&mut engine);
            assert_eq!(engine.snapshot().rows, rows);
            assert_eq!(engine.display_offset(), offset);
            assert_eq!(engine.selected_text(), selection);
            assert_eq!(engine.modes(), modes);
            assert_eq!(engine.title(), "Retained title");
        }
    }

    fn key(key: &str, key_char: Option<&str>, modifiers: Modifiers) -> Keystroke {
        Keystroke {
            modifiers,
            key: key.to_owned(),
            key_char: key_char.map(ToOwned::to_owned),
        }
    }

    #[test]
    fn terminal_tab_labels_shorten_only_absolute_executable_titles() {
        assert_eq!(terminal_tab_title(r"C:\Windows\system32\cmd.exe", 1), "CMD");
        assert_eq!(
            terminal_tab_title(r#""C:\Program Files\PowerShell\pwsh.exe""#, 2),
            "PowerShell 2"
        );
        assert_eq!(terminal_tab_title("Claude Code", 1), "Claude Code");
        assert_eq!(
            terminal_tab_title("project/notes.txt", 1),
            "project/notes.txt"
        );
        assert_eq!(terminal_tab_title("", 3), "Terminal 3");
    }

    #[test]
    fn active_tab_stays_stable_when_other_tabs_close() {
        assert_eq!(active_index_after_close(2, 0, 2), 1);
        assert_eq!(active_index_after_close(1, 1, 2), 1);
        assert_eq!(active_index_after_close(2, 2, 2), 1);
        assert_eq!(MAX_TERMINAL_TABS, 8);
        assert_eq!(active_index_after_close(0, 0, 0), 0);
    }

    #[test]
    fn cycling_tabs_wraps_in_both_directions() {
        assert_eq!(next_tab_index(0, 3, true), 2);
        assert_eq!(next_tab_index(2, 3, false), 0);
        assert_eq!(next_tab_index(1, 3, true), 0);
        assert_eq!(next_tab_index(0, 1, false), 0);
        assert_eq!(next_tab_index(0, 0, true), 0);
    }

    #[gpui::test]
    fn restored_layout_bounds_tabs_without_starting_shells(cx: &mut gpui::TestAppContext) {
        let terminal = cx.new(|cx| TerminalView::new(PathBuf::from("synthetic-project"), cx));
        terminal.update(cx, |terminal, cx| {
            terminal.restore_layout(3, 1, cx);
            assert_eq!(terminal.layout_snapshot(), (3, 1));
            assert!(!terminal.has_running_sessions(cx));
            assert!(terminal.tabs.iter().all(|tab| match &tab.content {
                TabContent::Terminal(session) =>
                    session.read(cx).workspace.as_path()
                        == std::path::Path::new("synthetic-project")
                        && session.read(cx).worker.is_none(),
                TabContent::File { .. } | TabContent::Diff { .. } => false,
            }));
            terminal.restore_layout(usize::MAX, usize::MAX, cx);
            assert_eq!(terminal.layout_snapshot(), (8, 7));
            terminal.restore_layout(0, 7, cx);
            assert_eq!(terminal.layout_snapshot(), (0, 0));
        });
    }

    #[gpui::test]
    fn mixed_editor_tabs_retain_background_terminal_state(cx: &mut gpui::TestAppContext) {
        use std::io::Write;
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_SOURCE: AtomicU64 = AtomicU64::new(1);
        struct TestSource(PathBuf);
        impl Drop for TestSource {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let project = std::env::temp_dir();
        let source = TestSource(project.join(format!(
            "pideck-mixed-tabs-{}-{}.txt",
            std::process::id(),
            NEXT_SOURCE.fetch_add(1, Ordering::Relaxed)
        )));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&source.0)
            .unwrap();
        file.write_all(b"synthetic editor buffer\n").unwrap();
        drop(file);
        cx.update(FileEditor::initialize);
        let pane = cx.new(|cx| TerminalView::new(project, cx));
        let root_pane = pane.clone();
        let window =
            cx.add_window(move |window, cx| gpui_component::Root::new(root_pane, window, cx));
        window
            .update(cx, |_, window, cx| {
                pane.update(cx, |view, cx| {
                    let TabContent::Terminal(session) = &view.tabs[0].content else {
                        panic!("initial terminal");
                    };
                    let session = session.clone();
                    let original_id = session.entity_id();
                    session.update(cx, |session, _| {
                        // Model an already-running worker without starting a real shell in a UI test.
                        session.status = TerminalStatus::Running;
                        let _ = session.engine.feed(b"retained terminal output");
                    });
                    let generation = session.read(cx).generation;
                    view.open_file(source.0.clone(), window, cx);
                    let editor_id = view.tabs[1].content.editor().unwrap().entity_id();
                    let tab_id = view.tabs[1].id;
                    view.open_file(source.0.clone(), window, cx);
                    assert_eq!(view.tabs.len(), 2);
                    assert_eq!(
                        view.active_file(),
                        Some(crate::services::paths::without_windows_verbatim_prefix(
                            &source.0
                        ))
                    );
                    assert_eq!(view.layout_snapshot(), (1, 0));
                    assert!(view.has_running_sessions(cx));
                    view.select_tab(view.tabs[0].id, window, cx);
                    view.select_tab(tab_id, window, cx);
                    assert_eq!(
                        view.tabs[1].content.editor().unwrap().entity_id(),
                        editor_id
                    );
                    let TabContent::Terminal(retained) = &view.tabs[0].content else {
                        panic!("retained terminal");
                    };
                    assert_eq!(retained.entity_id(), original_id);
                    assert_eq!(retained.read(cx).generation, generation);
                    assert!(
                        retained
                            .read(cx)
                            .engine
                            .visible_text()
                            .contains("retained terminal output")
                    );
                    assert_eq!(view.terminal_count(), 1);
                    assert!(!view.has_dirty_files(cx));
                });
            })
            .unwrap();
        cx.run_until_parked();
    }

    #[gpui::test]
    fn image_files_open_in_a_viewer_tab(cx: &mut gpui::TestAppContext) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_SOURCE: AtomicU64 = AtomicU64::new(1);
        struct TestSource(PathBuf);
        impl Drop for TestSource {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        const TINY_PNG: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08,
            0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x18, 0xDD, 0x8D,
            0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        let project = std::env::temp_dir();
        let source = TestSource(project.join(format!(
            "pideck-image-tab-{}-{}.png",
            std::process::id(),
            NEXT_SOURCE.fetch_add(1, Ordering::Relaxed)
        )));
        std::fs::write(&source.0, TINY_PNG).unwrap();
        cx.update(FileEditor::initialize);
        let pane = cx.new(|cx| TerminalView::new(project, cx));
        let root_pane = pane.clone();
        let window =
            cx.add_window(move |window, cx| gpui_component::Root::new(root_pane, window, cx));
        window
            .update(cx, |_, window, cx| {
                pane.update(cx, |view, cx| {
                    view.open_file(source.0.clone(), window, cx);
                    assert_eq!(view.tabs.len(), 2);
                    assert!(matches!(
                        view.tabs[1].content,
                        TabContent::File {
                            tab: FileTab::Image(_),
                            ..
                        }
                    ));
                    assert!(view.tabs[1].content.editor().is_none());
                    let tab_id = view.tabs[1].id;
                    view.open_file(source.0.clone(), window, cx);
                    assert_eq!(view.tabs.len(), 2);
                    assert_eq!(view.tabs[1].id, tab_id);
                    assert_eq!(
                        view.active_file(),
                        Some(crate::services::paths::without_windows_verbatim_prefix(
                            &source.0
                        ))
                    );
                    assert!(!view.has_dirty_files(cx));
                    view.close_tab(tab_id, window, cx);
                    assert_eq!(view.tabs.len(), 1);
                    assert!(matches!(view.tabs[0].content, TabContent::Terminal(_)));
                });
            })
            .unwrap();
        cx.run_until_parked();
    }

    #[test]
    fn file_tab_identity_uses_the_full_path() {
        assert!(same_file_path(
            Path::new("project/src/main.rs"),
            Path::new("project/src/main.rs")
        ));
        assert!(!same_file_path(
            Path::new("project/src/main.rs"),
            Path::new("project/examples/main.rs")
        ));
        #[cfg(windows)]
        assert!(same_file_path(
            Path::new(r"C:\Project\src\main.rs"),
            Path::new(r"c:\project\SRC\main.rs")
        ));
    }

    #[test]
    fn manager_shortcuts_bubble_while_shell_shortcuts_reach_the_pty() {
        let control_shift = Modifiers {
            control: true,
            shift: true,
            ..Default::default()
        };
        for name in ["t", "w", "o", "b", "l", "tab"] {
            assert_eq!(
                terminal_key_bytes(&key(name, None, control_shift), false),
                None
            );
        }
        let control = Modifiers {
            control: true,
            ..Default::default()
        };
        assert_eq!(terminal_key_bytes(&key("tab", None, control), false), None);
        assert_eq!(
            terminal_key_bytes(&key("c", None, control), false),
            Some(vec![3])
        );
        assert_eq!(
            terminal_key_bytes(&key("tab", None, Modifiers::default()), false),
            Some(vec![9])
        );
        assert_eq!(
            terminal_key_bytes(
                &key(
                    "tab",
                    Some("\t"),
                    Modifiers {
                        shift: true,
                        ..Default::default()
                    },
                ),
                false,
            ),
            Some(b"\x1b[Z".to_vec())
        );
        for name in ["up", "down"] {
            assert_eq!(
                terminal_key_bytes(
                    &key(
                        name,
                        None,
                        Modifiers {
                            control: true,
                            alt: true,
                            ..Default::default()
                        }
                    ),
                    false
                ),
                None
            );
        }
        assert_eq!(
            terminal_key_bytes(&key("f6", None, Modifiers::default()), false),
            None
        );
        assert_eq!(
            terminal_key_bytes(
                &key(
                    "f6",
                    None,
                    Modifiers {
                        shift: true,
                        ..Default::default()
                    },
                ),
                false,
            ),
            None
        );
        assert_eq!(
            terminal_key_bytes(&key("left", None, control), true),
            Some(b"\x1b[1;5D".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(
                &key(
                    "enter",
                    None,
                    Modifiers {
                        alt: true,
                        ..Default::default()
                    }
                ),
                false
            ),
            Some(b"\x1b\r".to_vec())
        );
    }

    #[test]
    fn terminal_keymap_supports_text_navigation_and_control_input() {
        assert_eq!(
            terminal_key_bytes(&key("a", Some("a"), Modifiers::default()), false),
            Some(b"a".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(&key("space", None, Modifiers::default()), false),
            Some(b" ".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(&key("enter", None, Modifiers::default()), false),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(&key("up", None, Modifiers::default()), false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(&key("up", None, Modifiers::default()), true),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(
                &key(
                    "c",
                    None,
                    Modifiers {
                        control: true,
                        ..Default::default()
                    },
                ),
                false,
            ),
            Some(vec![0x03])
        );
        assert_eq!(
            terminal_key_bytes(
                &key(
                    "x",
                    Some("x"),
                    Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                ),
                false,
            ),
            Some(b"\x1bx".to_vec())
        );
    }

    #[test]
    fn new_terminal_shortcut_bubbles_to_workspace() {
        assert_eq!(
            terminal_key_bytes(
                &key(
                    "`",
                    Some("`"),
                    Modifiers {
                        control: true,
                        ..Default::default()
                    },
                ),
                false,
            ),
            None
        );
    }
}
