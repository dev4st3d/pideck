use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use gpui::{
    AnyElement, App, Entity, FontWeight, IntoElement, ListState, SharedString, div, prelude::*, px,
};

use super::{ActivityDisclosureState, TranscriptText, TranscriptTextCache};
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
        cache: &Entity<TranscriptTextCache>,
        disclosures: &Entity<ActivityDisclosureState>,
        diff_summary: &ConversationDiffSummary,
        cx: &mut App,
    ) -> AnyElement {
        let Some(item) = self.items.get(item_index) else {
            return div().into_any_element();
        };
        match item {
            ConversationItem::Header => {
                header(self.turn_count + projection.accepted_user_inputs.len()).into_any_element()
            }
            ConversationItem::Preamble { message_index, .. } => {
                let texts = super::cached_message_texts(
                    projection,
                    *message_index..*message_index + 1,
                    cache,
                    cx,
                );
                row(super::preamble(
                    &projection.messages[*message_index],
                    projection,
                    &texts,
                    disclosures,
                    cx,
                ))
            }
            ConversationItem::Turn {
                number,
                user_index,
                body,
                ..
            } => {
                let texts =
                    super::cached_message_texts(projection, *user_index..body.end, cache, cx);
                let messages = &projection.messages[body.clone()];
                connected_row(
                    super::turn_card(
                        super::TurnPosition {
                            index: *number,
                            is_last: *number == self.turn_count,
                        },
                        &projection.messages[*user_index],
                        messages,
                        projection,
                        &texts,
                        disclosures,
                        cx,
                    )
                    .into_any_element(),
                )
            }
            ConversationItem::Trailing => {
                let texts = super::cached_optimistic_texts(projection, cache, cx);
                trailing(
                    projection,
                    self.turn_count,
                    &texts,
                    disclosures,
                    diff_summary,
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
            .pb(px(10.0))
            .border_b_1()
            .border_color(theme::edge_soft())
            .child(
                div()
                    .font_family(theme::sans())
                    .text_size(theme::text_size(theme::T_UI_SM))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme::ash())
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
        .pb(px(16.0))
        .child(content)
        .into_any_element()
}

fn connected_row(content: AnyElement) -> AnyElement {
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
    texts: &HashMap<String, Entity<TranscriptText>>,
    disclosures: &Entity<ActivityDisclosureState>,
    diff_summary: &ConversationDiffSummary,
    cx: &mut App,
) -> impl IntoElement {
    stream_gutter()
        .flex()
        .flex_col()
        .gap(px(16.0))
        .children(
            projection
                .accepted_user_inputs
                .iter()
                .enumerate()
                .map(|(index, input)| {
                    super::optimistic_turn(completed_turns + index + 1, input, texts)
                }),
        )
        .when_some(
            super::tail_activity(projection, disclosures, cx),
            |tail, activity| tail.child(activity),
        )
        .when_some(diff_summary.snapshot.clone(), |tail, snapshot| {
            tail.child(
                div()
                    .pt(px(14.0))
                    .child(crate::views::diff_summary::summary_card(
                        &snapshot,
                        diff_summary.files_expanded,
                        diff_summary.root.clone(),
                    )),
            )
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
                .h(px(16.0)),
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
