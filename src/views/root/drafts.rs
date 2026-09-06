//! Session-owned local checkpoints and a non-blocking, loss-aware close barrier.

use super::*;
use crate::services::draft_store::DraftOwner;
use crate::state::drafts::{EditorStamp, StoredDraft, StoredScroll};
use crate::views::composer::ComposerDraftSnapshot;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct CheckpointStamp {
    editor: EditorStamp,
    scroll: Option<(usize, u32)>,
    following: bool,
    recovery_revision: u64,
    uncertain: bool,
}

#[derive(PartialEq, Eq)]
struct CloseStamp {
    active: u64,
    fonts: u64,
    projects: u64,
    sessions: Vec<(u64, Option<DraftOwner>, Option<CheckpointStamp>)>,
}

fn owner_for(slot: &ThreadRuntimeSlot) -> Option<DraftOwner> {
    if slot.projection.pending_session_file.is_some()
        || slot.projection.lifecycle == RuntimeLifecycle::Loading {
        return None;
    }
    let session = slot.projection.session_file.as_ref()?;
    Some(DraftOwner { project: project_key(&slot.project_path), session: project_key(session) })
}

fn scroll_stamp(scroll: Option<ListOffset>) -> Option<(usize, u32)> {
    scroll.map(|scroll| (scroll.item_ix, f32::from(scroll.offset_in_item).to_bits()))
}

fn slot_stamp(slot: &ThreadRuntimeSlot) -> Option<CheckpointStamp> {
    Some(CheckpointStamp {
        editor: slot.ui.editor.as_ref()?.stamp(),
        scroll: scroll_stamp(slot.ui.scroll),
        following: slot.ui.following.unwrap_or(true),
        recovery_revision: slot.ui.recovery_revision,
        uncertain: slot.ui.pending_draft.is_some(),
    })
}

impl RootView {
    pub(super) fn start_draft_checkpoints(&mut self, cx: &mut Context<Self>) {
        // Throttled rather than trailing-edge debounce: continuous typing still
        // reaches disk, with at most one latest queued write per session.
        self.draft_checkpoint_task = Some(cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                if view.update(cx, |view, cx| {
                    if crate::services::accessibility::refresh_motion_preference() {
                        view.conversation_scroll_motion.cancel();
                        view.sessions_scroll_motion.cancel();
                        cx.notify();
                    }
                    view.checkpoint_all(cx);
                    if let Some(error) = view.draft_store.as_ref().and_then(|store| store.last_error())
                        && view.draft_feedback.as_deref() != Some(error.as_str()) {
                        view.draft_feedback = Some(error);
                        cx.notify();
                    }
                }).is_err() { break; }
            }
        }));
    }

    fn active_checkpoint_stamp(&self, cx: &Context<Self>) -> Option<CheckpointStamp> {
        let slot = self.runtime_slots.iter().find(|slot| slot.id == self.active_runtime_id)?;
        Some(CheckpointStamp {
            editor: self.composer.read(cx).draft_stamp(),
            scroll: scroll_stamp(Some(self.conversation_list_state.logical_scroll_top())),
            following: self.conversation_follow.get(),
            recovery_revision: slot.ui.recovery_revision,
            uncertain: self.pending_draft.is_some(),
        })
    }

    pub(super) fn checkpoint_all(&mut self, cx: &mut Context<Self>) {
        let stamp = self.active_checkpoint_stamp(cx);
        let previous = self.runtime_slots.iter().find(|slot| slot.id == self.active_runtime_id)
            .and_then(|slot| slot.last_checkpoint.as_ref());
        // No document, attachment or undo-journal clones when nothing changed.
        if stamp.as_ref() != previous { self.save_active_thread_ui(cx); }
        let ids: Vec<_> = self.runtime_slots.iter().filter(|slot| slot.id != self.active_runtime_id)
            .map(|slot| slot.id).collect();
        for id in ids { self.checkpoint_slot(id); }
    }

    pub(super) fn checkpoint_slot(&mut self, id: u64) {
        let Some(store) = &self.draft_store else { return; };
        let Some(slot) = self.runtime_slots.iter_mut().find(|slot| slot.id == id) else { return; };
        if !slot.draft_ready || slot.draft_loading || slot.draft_load_error.is_some()
            || owner_for(slot) != slot.draft_owner { return; }
        let Some(owner) = slot.draft_owner.clone() else { return; };
        let Some(stamp) = slot_stamp(slot) else { return; };
        if slot.last_checkpoint.as_ref() == Some(&stamp) { return; }
        let Some(editor) = slot.ui.editor.as_ref() else { return; };
        let draft = StoredDraft {
            editor: editor.stored(), images: slot.ui.images.clone(), files: slot.ui.files.clone(),
            scroll: slot.ui.scroll.map(|scroll| StoredScroll {
                item: scroll.item_ix, offset: f32::from(scroll.offset_in_item),
            }),
            following: slot.ui.following.unwrap_or(true),
            saved_inputs: slot.ui.saved_inputs.clone(),
            acceptance_uncertain: slot.ui.pending_draft.is_some(),
        };
        match store.enqueue(owner, draft) {
            Ok(()) => slot.last_checkpoint = Some(stamp),
            Err(error) => self.draft_feedback = Some(error),
        }
    }

    pub(super) fn maybe_restore_draft(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = self.draft_store.clone() else { return; };
        let Some(index) = self.runtime_slots.iter().position(|slot| slot.id == id) else { return; };
        let Some(owner) = owner_for(&self.runtime_slots[index]) else { return; };
        let slot = &self.runtime_slots[index];
        if slot.draft_loading || (slot.draft_owner.as_ref() == Some(&owner)
            && (slot.draft_ready || slot.draft_load_error.is_some())) { return; }
        let baseline = if id == self.active_runtime_id {
            Some(self.composer.read(cx).draft_stamp())
        } else { slot.ui.editor.as_ref().map(ComposerDraftSnapshot::stamp) };
        let initially_empty = if id == self.active_runtime_id {
            self.composer.read(cx).draft().is_empty() && !self.composer.read(cx).has_attachments()
                && self.pending_draft.is_none()
        } else {
            slot.ui.draft.is_empty() && slot.ui.images.is_empty() && slot.ui.files.is_empty()
                && slot.ui.pending_draft.is_none()
        };
        let slot = &mut self.runtime_slots[index];
        slot.draft_owner = Some(owner.clone());
        slot.draft_ready = false;
        slot.draft_loading = true;
        slot.draft_load_error = None;
        slot.last_checkpoint = None;
        let requested = owner.clone();
        let load = cx.background_executor().spawn(async move {
            store.load(&requested).map(|draft| draft.map(|draft| {
                let editor = ComposerDraftSnapshot::from_stored(draft.editor.clone(), &draft.images);
                (draft, editor)
            }))
        });
        let handle = window.window_handle();
        cx.spawn(async move |view, cx| {
            let result = load.await;
            let _ = handle.update(cx, |_, window, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.finish_draft_restore(id, owner, baseline, initially_empty, result, window, cx);
                });
            });
        }).detach();
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_draft_restore(
        &mut self, id: u64, owner: DraftOwner, baseline: Option<EditorStamp>, initially_empty: bool,
        result: Result<Option<(StoredDraft, ComposerDraftSnapshot)>, String>,
        window: &mut Window, cx: &mut Context<Self>,
    ) {
        let Some(index) = self.runtime_slots.iter().position(|slot| slot.id == id) else { return; };
        self.runtime_slots[index].draft_loading = false;
        // The callback's runtime and complete owner must still agree. No active
        // session globals are used to interpret a background read's result.
        if owner_for(&self.runtime_slots[index]).as_ref() != Some(&owner) {
            self.maybe_restore_draft(id, window, cx);
            return;
        }
        match result {
            Err(error) => {
                self.runtime_slots[index].draft_load_error = Some(error.clone());
                self.draft_feedback = Some(error);
            }
            Ok(restored) => {
                self.runtime_slots[index].draft_ready = true;
                if let Some((mut draft, editor)) = restored {
                    let current = if id == self.active_runtime_id {
                        Some(self.composer.read(cx).draft_stamp())
                    } else { self.runtime_slots[index].ui.editor.as_ref().map(ComposerDraftSnapshot::stamp) };
                    let untouched = initially_empty && (current == baseline
                        || (baseline.is_none() && current.as_ref().is_some_and(|stamp| {
                            stamp.revision.text == 0 && stamp.revision.attachments == 0
                        })));
                    let slot = &mut self.runtime_slots[index];
                    slot.ui.saved_inputs.extend(std::mem::take(&mut draft.saved_inputs));
                    slot.ui.recovery_revision = slot.ui.recovery_revision.wrapping_add(1);
                    if untouched {
                        slot.ui.draft = draft.editor.buffer.text().to_owned();
                        slot.ui.images = draft.images;
                        slot.ui.files = draft.files;
                        slot.ui.editor = Some(editor);
                        slot.ui.scroll = draft.scroll.map(|scroll| ListOffset {
                            item_ix: scroll.item, offset_in_item: px(scroll.offset),
                        });
                        slot.ui.following = Some(draft.following);
                        if id == self.active_runtime_id {
                            self.restore_active_thread_ui(cx);
                            self.restore_thread_scroll();
                            if draft.acceptance_uncertain {
                                self.composer.update(cx, |composer, cx| {
                                    composer.set_feedback(ComposerFeedback::Uncertain, cx);
                                });
                            }
                        }
                    } else if !draft.editor.buffer.text().is_empty() || !draft.images.is_empty() || !draft.files.is_empty() {
                        // Typing always wins. Offer the older text/attachments as
                        // explicit additive recovery, never replace the new draft.
                        slot.ui.saved_inputs.push_back(draft.into_recovered_input());
                        self.draft_feedback = Some("An earlier local draft is available under Restore next. Your current input was kept.".into());
                    }
                }
            }
        }
        if self.close_after_restore && !self.runtime_slots.iter().any(|slot| slot.draft_loading) {
            self.close_after_restore = false;
            self.request_close(window, cx);
        }
        cx.notify();
    }

    pub(super) fn retry_draft_storage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft_feedback = None;
        if self.draft_store.is_none() {
            let directory = self.font_catalog.settings_path.parent()
                .unwrap_or(std::path::Path::new(".")).join("drafts-v1");
            match crate::services::draft_store::DraftStore::new(directory) {
                Ok(store) => self.draft_store = Some(Arc::new(store)),
                Err(error) => {
                    self.draft_feedback = Some(format!("Local draft storage is unavailable: {error}"));
                    cx.notify();
                    return;
                }
            }
        }
        let ids: Vec<_> = self.runtime_slots.iter_mut().filter_map(|slot| {
            slot.last_checkpoint = None;
            let failed = slot.draft_load_error.take().is_some();
            (failed || !slot.draft_ready).then_some(slot.id)
        }).collect();
        for id in ids { self.maybe_restore_draft(id, window, cx); }
        self.checkpoint_all(cx);
        if let Some(store) = self.draft_store.clone() {
            let flush = cx.background_executor().spawn(async move { store.flush(Duration::from_secs(10)) });
            cx.spawn(async move |view, cx| {
                let result = flush.await;
                let _ = view.update(cx, |view, cx| {
                    if let Err(error) = result { view.draft_feedback = Some(error); }
                    cx.notify();
                });
            }).detach();
        }
        cx.notify();
    }

    fn close_stamp(&self, cx: &Context<Self>) -> CloseStamp {
        CloseStamp {
            active: self.active_runtime_id, fonts: self.font_save_generation, projects: self.project_save_generation,
            sessions: self.runtime_slots.iter().map(|slot| {
                let mut stamp = if slot.id == self.active_runtime_id { self.active_checkpoint_stamp(cx) }
                    else { slot_stamp(slot) };
                // Streaming can move the transcript every frame. It must not
                // hold close open forever; only edits/recovery/ownership do that.
                if let Some(stamp) = stamp.as_mut() {
                    stamp.scroll = None;
                    stamp.following = false;
                }
                (slot.id, owner_for(slot), stamp)
            }).collect(),
        }
    }

    /// Window close is a save barrier, not a synchronous filesystem operation.
    /// Returning false lets GPUI keep dispatching input while the writer flushes.
    pub(crate) fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.close_pending { return false; }
        if self.runtime_slots.iter().any(|slot| slot.draft_loading) {
            self.close_after_restore = true;
            self.draft_feedback = Some("Finishing local draft recovery before closing.".into());
            cx.notify();
            return false;
        }
        self.checkpoint_all(cx);
        if self.runtime_slots.iter().any(|slot| !slot.ui.can_evict()
            && (owner_for(slot).is_none() || (!slot.draft_ready && slot.draft_load_error.is_none()))) {
            self.draft_feedback = Some("An input does not yet have a confirmed Pi session. Let the session finish opening, or copy and clear that input before closing. Nothing was sent or discarded.".into());
            cx.notify();
            return false;
        }
        if let Some(error) = self.runtime_slots.iter().find_map(|slot| {
            (slot.draft_load_error.is_some() && !slot.ui.can_evict())
                .then(|| slot.draft_load_error.clone()).flatten()
        }) {
            self.draft_feedback = Some(format!("The window was kept open: {error} Repair the local drafts-v1 file, then select Retry storage. The original file has not been replaced."));
            cx.notify();
            return false;
        }
        let Some(store) = self.draft_store.clone() else {
            let safe = self.composer.read(cx).draft().is_empty() && !self.composer.read(cx).has_attachments()
                && self.runtime_slots.iter().all(|slot| slot.ui.can_evict());
            if !safe {
                self.draft_feedback = Some("Draft storage is unavailable. Copy your draft before closing; the editor was kept open.".into());
                cx.notify();
            } else { self.shutdown_runtime_pool(cx); }
            return safe;
        };
        // Retry failed checkpoints on close even when their editor stamps match.
        for slot in &mut self.runtime_slots { slot.last_checkpoint = None; }
        self.checkpoint_all(cx);
        let stamp = self.close_stamp(cx);
        let attachments = self.attachment_task.take();
        let fonts = self.font_save_task.take();
        let projects = self.project_save_task.take();
        self.close_pending = true;
        self.draft_feedback = Some("Saving local drafts before closing…".into());
        let handle = window.window_handle();
        cx.spawn(async move |view, cx| {
            if let Some(task) = attachments { task.await; }
            if let Some(task) = fonts { task.await; }
            if let Some(task) = projects { task.await; }
            let result = cx.background_executor().spawn(async move {
                store.flush(Duration::from_secs(10))
            }).await;
            let _ = handle.update(cx, |_, window, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.close_pending = false;
                    match result {
                        Err(error) => {
                            view.draft_feedback = Some(format!("The window was kept open. {error}"));
                            cx.notify();
                        }
                        Ok(()) if view.close_stamp(cx) != stamp => {
                            // Typing, late attachment completion or navigation
                            // during the flush must be included before closing.
                            view.request_close(window, cx);
                        }
                        Ok(()) => {
                            view.shutdown_runtime_pool(cx);
                            window.remove_window();
                        }
                    }
                });
            });
        }).detach();
        cx.notify();
        false
    }

    fn shutdown_runtime_pool(&mut self, cx: &mut Context<Self>) {
        self.draft_checkpoint_task.take();
        for slot in &self.runtime_slots {
            slot.controller.update(cx, |controller, _| controller.shutdown());
        }
    }
}
