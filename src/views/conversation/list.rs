use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use gpui::{
    AnyElement, App, Entity, FontWeight, IntoElement, ListState, SharedString, div, prelude::*, px,
};

use super::{
    ActivityDisclosureState, BandFingerprint, StreamBandCache, TranscriptText, TranscriptTextCache,
};
use crate::controller::ConversationProjection;
use crate::services::git_diff::WorkspaceDiff;
use crate::state::runtime::{FacetStatus, MessageRole};
use crate::theme;

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConversationItem {
    Header,
    Preamble {
        message_index: usize,
        key: String,
    },
    Turn {
        number: usize,
        user_index: usize,
        body: Range<usize>,
        key: String,
    },
    Trailing,
}

#[derive(Clone)]
pub(in crate::views) struct ConversationDiffSummary {
    pub(in crate::views) snapshot: Option<Arc<WorkspaceDiff>>,
    pub(in crate::views) files_expanded: bool,
    pub(in crate::views) root: Entity<crate::views::RootView>,
}

/// Shared per-stream entities every list item render needs; bundling keeps
/// the render callback inside GPUI's arity limits.
pub(in crate::views) struct ConversationStreamEntities {
    pub(in crate::views) transcript_cache: Entity<TranscriptTextCache>,
    pub(in crate::views) band_cache: Entity<StreamBandCache>,
    pub(in crate::views) disclosures: Entity<ActivityDisclosureState>,
    pub(in crate::views) diff_summary: ConversationDiffSummary,
}

#[derive(Debug, Clone)]
pub(in crate::views) struct ConversationListModel {
    items: Arc<Vec<ConversationItem>>,
    turn_count: usize,
    revision: u64,
    message_structure_revision: u64,
}

impl ConversationListModel {
    pub(in crate::views) fn new(projection: &ConversationProjection) -> Self {
        let mut items = vec![ConversationItem::Header];
        let mut message_index = 0;
        let mut turn = 0;
        while message_index < projection.messages.len() {
            let message = &projection.messages[message_index];
            if message.role != MessageRole::User {
                items.push(ConversationItem::Preamble {
                    message_index,
                    key: message.key.0.clone(),
                });
                message_index += 1;
                continue;
            }

            turn += 1;
            let user_index = message_index;
            message_index += 1;
            let body_start = message_index;
            while message_index < projection.messages.len()
                && projection.messages[message_index].role != MessageRole::User
            {
                message_index += 1;
            }
            items.push(ConversationItem::Turn {
                number: turn,
                user_index,
                body: body_start..message_index,
                key: projection.messages[user_index].key.0.clone(),
            });
        }
        items.push(ConversationItem::Trailing);
        Self {
            items: Arc::new(items),
            turn_count: turn,
            revision: projection.revision,
            message_structure_revision: projection.message_structure_revision,
        }
    }

    pub(in crate::views) fn updated(
        previous: &Self,
        projection: &ConversationProjection,
        epoch_changed: bool,
    ) -> Self {
        if !epoch_changed
            && previous.message_structure_revision == projection.message_structure_revision
        {
            let mut current = previous.clone();
            current.revision = projection.revision;
            return current;
        }
        Self::new(projection)
    }

    pub(in crate::views) fn item_count(&self) -> usize {
        self.items.len()
    }

    pub(in crate::views) fn refresh_trailing(&self, state: &ListState) {
        let index = self.items.len().saturating_sub(1);
        state.splice(index..self.items.len(), 1);
    }

    pub(in crate::views) fn reconcile(
        &self,
        previous: &Self,
        state: &ListState,
        epoch_changed: bool,
    ) {
        if epoch_changed {
            state.reset(self.items.len());
            return;
        }
        if self.revision == previous.revision && Arc::ptr_eq(&self.items, &previous.items) {
            return;
        }

        let structure_stable = Arc::ptr_eq(&self.items, &previous.items);
        let stable_prefix = if structure_stable {
            self.items.len()
        } else {
            previous
                .items
                .iter()
                .zip(self.items.iter())
                .take_while(|(left, right)| left == right)
                .count()
        };
        let last_segment = self.items.len().saturating_sub(2);
        let refresh_from = stable_prefix.min(last_segment);
        let anchor = structure_stable.then(|| state.logical_scroll_top());
        state.splice(
            refresh_from..previous.items.len(),
            self.items.len().saturating_sub(refresh_from),
        );
        if let Some(anchor) = anchor {
            state.scroll_to(anchor);
        }
    }

    pub(in crate::views) fn render_item(
        &self,
        item_index: usize,
        projection: &ConversationProjection,
        stream: &ConversationStreamEntities,
        cx: &mut App,
    ) -> AnyElement {
        let Some(item) = self.items.get(item_index) else {
            return div().into_any_element();
        };
        match item {
            ConversationItem::Header => {
                header(self.turn_count + projection.accepted_user_inputs.len()).into_any_element()
            }
            ConversationItem::Preamble { message_index, key } => {
                let fingerprint = BandFingerprint::capture(
                    projection,
                    std::slice::from_ref(&projection.messages[*message_index]),
                );
                let model = stream.band_cache.update(cx, |bands, cx| {
                    bands.model_for(format!("preamble:{key}"), fingerprint, cx, |cx| {
                        super::build_preamble_model(
                            projection,
                            *message_index,
                            &stream.transcript_cache,
                            cx,
                        )
                    })
                });
                let render = super::ConversationRenderContext {
                    projection,
                    texts: &model.texts,
                    disclosures: &stream.disclosures,
                    root: &stream.diff_summary.root,
                };
                row(super::preamble(
                    &projection.messages[*message_index],
                    &model,
                    &render,
                    cx,
                ))
            }
            ConversationItem::Turn {
                number,
                user_index,
                body,
                key,
            } => {
                let fingerprint = BandFingerprint::capture(
                    projection,
                    &projection.messages[*user_index..body.end],
                );
                let model = stream.band_cache.update(cx, |bands, cx| {
                    bands.model_for(format!("turn:{key}"), fingerprint, cx, |cx| {
                        super::build_turn_model(
                            projection,
                            *user_index,
                            body.clone(),
                            &stream.transcript_cache,
                            cx,
                        )
                    })
                });
                let render = super::ConversationRenderContext {
                    projection,
                    texts: &model.texts,
                    disclosures: &stream.disclosures,
                    root: &stream.diff_summary.root,
                };
                let links_above = item_index > 0
                    && matches!(self.items[item_index - 1], ConversationItem::Turn { .. });
                let links_below = match self.items.get(item_index + 1) {
                    Some(ConversationItem::Turn { .. }) => true,
                    Some(ConversationItem::Trailing) => !projection.accepted_user_inputs.is_empty(),
                    _ => false,
                };
                turn_row(
                    super::turn_card(
                        *number,
                        &projection.messages[*user_index],
                        &model,
                        &render,
                        links_above,
                        links_below,
                        cx,
                    )
                    .into_any_element(),
                )
            }
            ConversationItem::Trailing => {
                let texts =
                    super::cached_optimistic_texts(projection, &stream.transcript_cache, cx);
                let connects_above = item_index > 0
                    && matches!(self.items[item_index - 1], ConversationItem::Turn { .. });
                trailing(
                    projection,
                    self.turn_count,
                    connects_above,
                    &texts,
                    stream,
                    cx,
                )
                .into_any_element()
            }
        }
    }
}

fn header(turn_count: usize) -> impl IntoElement {
    stream_gutter().pb(px(16.0)).child(
        div()
            .w_full()
            .flex()
            .flex_row()
            .items_baseline()
            .justify_between()
            .child(
                div()
                    .font_family(theme::sans())
                    .text_size(theme::text_size(theme::T_UI_SM))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::bone_dim())
                    .child("Conversation"),
            )
            .child(
                div()
                    .font_family(theme::mono())
                    .text_size(theme::text_size(theme::T_TINY))
                    .text_color(theme::smoke())
                    .child(format!(
                        "{turn_count} turn{}",
                        if turn_count == 1 { "" } else { "s" }
                    )),
            ),
    )
}

fn row(content: AnyElement) -> AnyElement {
    stream_gutter()
        .pb(px(super::TURN_GAP))
        .child(content)
        .into_any_element()
}

/// Turns carry their own spacing (`TURN_GAP` inside `turn_card`); the gutter
/// only keeps the thread clear of the side rails.
fn turn_row(content: AnyElement) -> AnyElement {
    stream_gutter().child(content).into_any_element()
}

/// Keep stream chrome clear of the side rails. Padding lives on each list item
/// because GPUI `List` does not reliably inset item widths from container `px`.
fn stream_gutter() -> gpui::Div {
    div().w_full().px(px(theme::STREAM_PAD_X))
}

fn trailing(
    projection: &ConversationProjection,
    completed_turns: usize,
    connects_above: bool,
    texts: &HashMap<String, Entity<TranscriptText>>,
    stream: &ConversationStreamEntities,
    cx: &mut App,
) -> impl IntoElement {
    let render = super::ConversationRenderContext {
        projection,
        texts,
        disclosures: &stream.disclosures,
        root: &stream.diff_summary.root,
    };
    let tail = super::tail_activity(&render, &stream.band_cache, cx);
    let has_tail = tail.is_some();
    let input_count = projection.accepted_user_inputs.len();
    // Pending prompts and the live tail form one chain on the rail: a queued
    // or steering prompt keeps the thread alive while work streams below it.
    let has_chain = input_count > 0 || has_tail;
    stream_gutter()
        .flex()
        .flex_col()
        .gap(px(super::TURN_GAP))
        .when(has_chain, |list| {
            list.child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .children(projection.accepted_user_inputs.iter().enumerate().map(
                        |(index, input)| {
                            let continues = index + 1 < input_count || has_tail;
                            super::optimistic_turn(
                                completed_turns + index + 1,
                                input,
                                texts,
                                connects_above || index > 0,
                                continues,
                            )
                        },
                    ))
                    .when_some(tail, |chain, activity| chain.child(activity)),
            )
        })
        .when_some(stream.diff_summary.snapshot.clone(), |tail, snapshot| {
            tail.child(crate::views::diff_summary::summary_card(
                &snapshot,
                stream.diff_summary.files_expanded,
                stream.diff_summary.root.clone(),
            ))
        })
        .when(
            projection.messages.is_empty()
                && projection.accepted_user_inputs.is_empty()
                && !matches!(projection.status, FacetStatus::Loading),
            |tail| tail.child(super::empty_state(projection)),
        )
        .child(
            div()
                .id(SharedString::from("conversation-bottom-padding"))
                .h(px(14.0)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{ListAlignment, ListOffset};

    #[test]
    fn content_updates_preserve_an_anchor_inside_the_streaming_turn() {
        let items = Arc::new(vec![
            ConversationItem::Header,
            ConversationItem::Turn {
                number: 1,
                user_index: 0,
                body: 1..2,
                key: "user:1".to_owned(),
            },
            ConversationItem::Trailing,
        ]);
        let previous = ConversationListModel {
            items: Arc::clone(&items),
            turn_count: 1,
            revision: 1,
            message_structure_revision: 1,
        };
        let current = ConversationListModel {
            items,
            turn_count: 1,
            revision: 2,
            message_structure_revision: 1,
        };
        let state = ListState::new(3, ListAlignment::Top, px(800.0));
        state.scroll_to(ListOffset {
            item_ix: 1,
            offset_in_item: px(500.0),
        });

        current.reconcile(&previous, &state, false);

        let anchor = state.logical_scroll_top();
        assert_eq!(anchor.item_ix, 1);
        assert_eq!(anchor.offset_in_item, px(500.0));
    }
}
