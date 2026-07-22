//! Live Pi shell with an authoritative streaming conversation.

use std::collections::{HashMap, HashSet};

use gpui::{
    Context, Entity, FocusHandle, Focusable, FontWeight, IntoElement, Render, ScrollHandle,
    Subscription, Window, div, prelude::*, px,
};

use crate::actions::{AbortRun, ActivateRecovery, Connect, FocusNext, FocusPrevious, Retry, Stop};
use crate::controller::{
    AcceptedSubmission, AcceptedSubmissionKind, ComposerRuntime, ConversationProjection,
    RuntimeController, SubmissionPreference,
};
use crate::state::runtime::{
    BashStatus, CompactionState, PromptDelivery, QueueContents, QueueDeliveryMode, RetryState,
    RuntimeLifecycle, RuntimeOperation,
};
use crate::state::{RecoveryAction, ShellProjection};
use crate::theme;
use crate::views::composer::{Composer, ComposerAvailability, ComposerEvent, ComposerFeedback};
use crate::views::controls;
use crate::views::conversation::{self, ScrollPinning, TranscriptText};
use crate::views::tool_card::{self, ToolCard};

struct PendingDraft {
    request: crate::services::rpc::RequestId,
    text: String,
}

pub struct RootView {
    controller: Entity<RuntimeController>,
    composer: Entity<Composer>,
    conversation: ConversationProjection,
    conversation_scroll: ScrollHandle,
    scroll_pinning: ScrollPinning,
    transcript_texts: HashMap<String, Entity<TranscriptText>>,
    tool_cards: HashMap<String, Entity<ToolCard>>,
    pending_draft: Option<PendingDraft>,
    pending_bash: Option<crate::services::rpc::RequestId>,
    focus_handle: FocusHandle,
    _controller_observation: Subscription,
    _composer_subscription: Subscription,
}

impl RootView {
    pub fn new(
        window: &mut Window,
        controller: Entity<RuntimeController>,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let composer = cx.new(Composer::new);
        let conversation = controller.read(cx).conversation_projection();
        let transcript_texts = conversation::text_fragments(&conversation)
            .into_iter()
            .map(|(key, text)| {
                let entity = cx.new(|cx| TranscriptText::new(key.clone(), text, cx));
                (key, entity)
            })
            .collect();
        let tool_cards = tool_card::cards_for_projection(&conversation)
            .into_iter()
            .map(|card| {
                let key = card.key.clone();
                (key, cx.new(|_| ToolCard::new(card)))
            })
            .collect();
        window.focus(&composer.read(cx).focus_handle(cx));
        let controller_observation = cx.observe_in(&controller, window, |view, _, window, cx| {
            view.sync_runtime(window, cx)
        });
        let composer_subscription = cx.subscribe_in(&composer, window, |view, _, event, _, cx| {
            view.on_composer_event(event, cx)
        });
        Self {
            controller,
            composer,
            conversation,
            conversation_scroll: ScrollHandle::new(),
            scroll_pinning: ScrollPinning::default(),
            transcript_texts,
            tool_cards,
            pending_draft: None,
            pending_bash: None,
            focus_handle,
            _controller_observation: controller_observation,
            _composer_subscription: composer_subscription,
        }
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        self.controller
            .update(cx, |controller, cx| controller.connect(cx));
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.controller
            .update(cx, |controller, cx| controller.stop(cx));
    }

    fn activate_recovery(&mut self, action: RecoveryAction, cx: &mut Context<Self>) {
        match action {
            RecoveryAction::Connect | RecoveryAction::Retry => self.connect(cx),
            RecoveryAction::Stop => self.stop(cx),
        }
    }

    fn on_connect(&mut self, _: &Connect, _: &mut Window, cx: &mut Context<Self>) {
        self.connect(cx);
    }

    fn on_retry(&mut self, _: &Retry, _: &mut Window, cx: &mut Context<Self>) {
        self.connect(cx);
    }

    fn on_stop(&mut self, _: &Stop, _: &mut Window, cx: &mut Context<Self>) {
        self.stop(cx);
    }

    fn on_abort_run(&mut self, _: &AbortRun, _: &mut Window, cx: &mut Context<Self>) {
        match self.controller.read(cx).composer_projection().runtime {
            ComposerRuntime::BashRunning | ComposerRuntime::BashCancelling => self.abort_bash(cx),
            ComposerRuntime::Running | ComposerRuntime::Cancelling => self.abort(cx),
            ComposerRuntime::Unavailable | ComposerRuntime::Idle => {}
        }
    }

    fn on_composer_event(&mut self, event: &ComposerEvent, cx: &mut Context<Self>) {
        match event {
            ComposerEvent::Accept { text } => {
                self.submit(text.clone(), SubmissionPreference::Default, cx)
            }
            ComposerEvent::FollowUp { text } => {
                self.submit(text.clone(), SubmissionPreference::FollowUp, cx)
            }
            ComposerEvent::Abort => self.abort(cx),
            ComposerEvent::AbortBash => self.abort_bash(cx),
        }
    }

    fn submit(&mut self, text: String, preference: SubmissionPreference, cx: &mut Context<Self>) {
        if self.pending_draft.is_some() {
            self.composer.update(cx, |composer, cx| {
                composer.set_feedback(
                    ComposerFeedback::Rejected(
                        "The previous acceptance is still pending.".to_owned(),
                    ),
                    cx,
                );
            });
            return;
        }

        let result = self.controller.update(cx, |controller, cx| {
            controller.submit(text.clone(), preference, cx)
        });
        match result {
            Ok(AcceptedSubmission {
                request,
                kind: AcceptedSubmissionKind::Prompt(kind),
            }) => {
                self.pending_draft = Some(PendingDraft { request, text });
                self.composer.update(cx, |composer, cx| {
                    composer.set_feedback(ComposerFeedback::Pending(kind), cx)
                });
            }
            Ok(AcceptedSubmission {
                request,
                kind:
                    AcceptedSubmissionKind::Bash {
                        exclude_from_context,
                    },
            }) => {
                self.pending_bash = Some(request);
                self.composer.update(cx, |composer, cx| {
                    composer.clear_bash_accepted(&text, exclude_from_context, cx);
                });
            }
            Err(rejection) => {
                self.composer.update(cx, |composer, cx| {
                    composer.set_feedback(
                        ComposerFeedback::Rejected(rejection.message().to_owned()),
                        cx,
                    )
                });
            }
        }
    }

    fn abort(&mut self, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.abort(cx);
        });
    }

    fn abort_bash(&mut self, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.abort_bash(cx);
        });
    }

    fn abort_retry(&mut self, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.abort_retry(cx);
        });
    }

    fn set_steering_mode(&mut self, mode: QueueDeliveryMode, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.set_steering_mode(mode, cx);
        });
    }

    fn set_follow_up_mode(&mut self, mode: QueueDeliveryMode, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.set_follow_up_mode(mode, cx);
        });
    }

    fn set_auto_compaction(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.set_auto_compaction(enabled, cx);
        });
    }

    fn set_auto_retry(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.set_auto_retry(enabled, cx);
        });
    }

    fn compact(&mut self, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.compact(None, cx);
        });
    }

    fn sync_runtime(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let conversation = self.controller.read(cx).conversation_projection();
        let epoch_changed = conversation.epoch != self.conversation.epoch;
        if epoch_changed {
            self.transcript_texts.clear();
            self.tool_cards.clear();
        }
        let fragments = conversation::text_fragments(&conversation);
        let active = fragments
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<HashSet<_>>();
        self.transcript_texts.retain(|key, _| active.contains(key));
        for (key, text) in fragments {
            if let Some(entity) = self.transcript_texts.get(&key) {
                entity.update(cx, |text_view, cx| text_view.set_text(text, cx));
            } else {
                let entity = cx.new(|cx| TranscriptText::new(key.clone(), text, cx));
                self.transcript_texts.insert(key, entity);
            }
        }
        let cards = tool_card::cards_for_projection(&conversation);
        let active_cards = cards
            .iter()
            .map(|card| card.key.clone())
            .collect::<HashSet<_>>();
        self.tool_cards.retain(|key, _| active_cards.contains(key));
        for card in cards {
            if let Some(entity) = self.tool_cards.get(&card.key) {
                entity.update(cx, |view, cx| view.set_data(card, cx));
            } else {
                let key = card.key.clone();
                self.tool_cards.insert(key, cx.new(|_| ToolCard::new(card)));
            }
        }
        self.conversation = conversation;

        let projection = self.controller.read(cx).composer_projection();
        let availability = match projection.runtime {
            ComposerRuntime::Unavailable => ComposerAvailability::Unavailable,
            ComposerRuntime::Idle => ComposerAvailability::Idle,
            ComposerRuntime::Running => ComposerAvailability::Running,
            ComposerRuntime::Cancelling => ComposerAvailability::Cancelling,
            ComposerRuntime::BashRunning => ComposerAvailability::BashRunning,
            ComposerRuntime::BashCancelling => ComposerAvailability::BashCancelling,
        };
        let was_available = matches!(
            self.composer.read(cx).availability(),
            ComposerAvailability::Idle | ComposerAvailability::Running
        );
        self.composer.update(cx, |composer, cx| {
            composer.set_availability(availability, cx)
        });
        if !was_available
            && matches!(
                availability,
                ComposerAvailability::Idle | ComposerAvailability::Running
            )
        {
            window.focus(&self.composer.read(cx).focus_handle(cx));
        }

        if epoch_changed
            && self.pending_bash.as_ref().is_some_and(|request| {
                !self
                    .conversation
                    .bash_executions
                    .iter()
                    .any(|execution| &execution.request == request)
            })
        {
            self.pending_bash = None;
            self.composer.update(cx, |composer, cx| {
                composer.set_feedback(
                    ComposerFeedback::Rejected(
                        "The session changed before Bash could be reconciled.".to_owned(),
                    ),
                    cx,
                )
            });
        }

        if let Some(request) = self.pending_bash.as_ref()
            && let Some(execution) = self
                .conversation
                .bash_executions
                .iter()
                .find(|execution| &execution.request == request)
        {
            match execution.status {
                BashStatus::Running | BashStatus::Cancelling => {}
                BashStatus::Succeeded | BashStatus::Cancelled => {
                    self.composer.update(cx, |composer, cx| {
                        composer.set_feedback(ComposerFeedback::BashCompleted, cx)
                    });
                    self.pending_bash = None;
                }
                BashStatus::Failed | BashStatus::Uncertain => {
                    let summary = execution.error.clone().unwrap_or_else(|| {
                        execution.exit_code.map_or_else(
                            || "Bash did not complete successfully.".to_owned(),
                            |code| format!("Bash exited with code {code}."),
                        )
                    });
                    self.composer.update(cx, |composer, cx| {
                        composer.set_feedback(ComposerFeedback::Rejected(summary), cx)
                    });
                    self.pending_bash = None;
                }
            }
        }

        let Some(pending) = self.pending_draft.as_ref() else {
            if matches!(projection.delivery, PromptDelivery::Uncertain { .. }) {
                self.composer.update(cx, |composer, cx| {
                    composer.set_feedback(ComposerFeedback::Uncertain, cx)
                });
            }
            cx.notify();
            return;
        };
        let request_matches = match &projection.delivery {
            PromptDelivery::Pending { request, .. }
            | PromptDelivery::Accepted { request, .. }
            | PromptDelivery::Rejected { request, .. }
            | PromptDelivery::Uncertain { request, .. } => request == &pending.request,
            PromptDelivery::None => false,
        };
        if !request_matches {
            cx.notify();
            return;
        }

        match projection.delivery {
            PromptDelivery::Pending { .. } => {}
            PromptDelivery::Accepted { kind, .. } => {
                let expected = pending.text.clone();
                self.composer.update(cx, |composer, cx| {
                    composer.clear_accepted(&expected, kind, cx);
                });
                self.pending_draft = None;
            }
            PromptDelivery::Rejected { summary, .. } => {
                self.composer.update(cx, |composer, cx| {
                    composer.set_feedback(ComposerFeedback::Rejected(summary), cx)
                });
                self.pending_draft = None;
            }
            PromptDelivery::Uncertain { .. } => {
                self.composer.update(cx, |composer, cx| {
                    composer.set_feedback(ComposerFeedback::Uncertain, cx)
                });
                self.pending_draft = None;
            }
            PromptDelivery::None => {}
        }
        cx.notify();
    }

    fn on_activate_recovery(
        &mut self,
        _: &ActivateRecovery,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = self.controller.read(cx).projection().action;
        if let Some(action) = action {
            self.activate_recovery(action, cx);
        }
    }

    fn on_focus_next(&mut self, _: &FocusNext, window: &mut Window, _: &mut Context<Self>) {
        window.focus_next();
    }

    fn on_focus_previous(&mut self, _: &FocusPrevious, window: &mut Window, _: &mut Context<Self>) {
        window.focus_prev();
    }
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let projection = self.controller.read(cx).projection();
        let selection_active = self
            .transcript_texts
            .values()
            .any(|text| text.read(cx).selection_active());
        if self.scroll_pinning.should_follow(
            self.conversation.epoch,
            self.conversation.revision,
            self.conversation_scroll.offset().y,
            self.conversation_scroll.max_offset().height,
            selection_active,
        ) {
            self.conversation_scroll.scroll_to_bottom();
        }

        div()
            .id("runtime-shell")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_connect))
            .on_action(cx.listener(Self::on_retry))
            .on_action(cx.listener(Self::on_stop))
            .on_action(cx.listener(Self::on_abort_run))
            .on_action(cx.listener(Self::on_activate_recovery))
            .on_action(cx.listener(Self::on_focus_next))
            .on_action(cx.listener(Self::on_focus_previous))
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::canvas())
            .font_family(theme::SANS)
            .text_color(theme::bone())
            .child(titlebar(&projection))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .child(conversation_area(
                        &projection,
                        &self.conversation,
                        &self.conversation_scroll,
                        &self.transcript_texts,
                        &self.tool_cards,
                        &self.composer,
                        cx,
                    ))
                    .child(inspector(&projection, &self.conversation, cx)),
            )
    }
}

fn titlebar(projection: &ShellProjection) -> impl IntoElement {
    div()
        .h(px(theme::TITLE_H))
        .px(px(theme::PAD_X))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(20.0))
        .bg(theme::floor())
        .border_b_1()
        .border_color(theme::edge_hard())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .min_w_0()
                .flex_1()
                .child(
                    div()
                        .font_family(theme::DISPLAY)
                        .text_size(px(theme::T_WORDMARK))
                        .font_weight(FontWeight::NORMAL)
                        .flex_shrink_0()
                        .child("pi"),
                )
                .child(
                    div()
                        .w(px(1.0))
                        .h(px(14.0))
                        .bg(theme::edge_hard())
                        .flex_shrink_0(),
                )
                .child(
                    div()
                        .min_w_0()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_TITLE))
                        .font_weight(FontWeight::BOLD)
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(projection.session.label()),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .min_w_0()
                .child(meta(projection.model.label()))
                .child(meta(projection.thinking.label()))
                .child(meta(projection.cost.label()))
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .font_weight(FontWeight::BOLD)
                        .text_color(lifecycle_color(projection))
                        .child(projection.lifecycle.clone()),
                ),
        )
}

fn meta(value: String) -> impl IntoElement {
    div()
        .font_family(theme::MONO)
        .text_size(px(theme::T_MONO_SM))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme::ash())
        .max_w(px(190.0))
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap()
        .child(value)
}

fn conversation_area(
    projection: &ShellProjection,
    conversation_projection: &ConversationProjection,
    scroll: &ScrollHandle,
    transcript_texts: &HashMap<String, Entity<TranscriptText>>,
    tool_cards: &HashMap<String, Entity<ToolCard>>,
    composer: &Entity<Composer>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let action = projection.action;
    let scroll = scroll.clone();

    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(theme::canvas())
        .child(
            div()
                .min_h(px(62.0))
                .px(px(theme::STREAM_PAD_X))
                .py(px(10.0))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(18.0))
                .bg(theme::floor())
                .border_b_1()
                .border_color(theme::edge_soft())
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(
                            div()
                                .font_family(theme::SANS)
                                .text_size(px(theme::T_LABEL))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::ash())
                                .child(if projection.has_stale_values {
                                    "Workspace · showing last valid values"
                                } else {
                                    "Workspace"
                                }),
                        )
                        .child(
                            div()
                                .max_w(px(560.0))
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .font_family(theme::MONO)
                                .text_size(px(theme::T_MONO))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme::data())
                                .child(projection.workspace.clone()),
                        ),
                )
                .when_some(action, |header, action| {
                    header.child(controls::recovery_button(
                        action_id(action),
                        action.label().to_owned(),
                        action.shortcut(),
                        true,
                        Box::new(cx.listener(move |view, _, _, cx| {
                            view.activate_recovery(action, cx);
                        })),
                    ))
                }),
        )
        .child(
            div()
                .id("conversation-scroll")
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scroll()
                .track_scroll(&scroll)
                .scrollbar_width(px(theme::SCROLLBAR))
                .child(
                    div()
                        .w_full()
                        .px(px(theme::STREAM_PAD_X))
                        .pt(px(16.0))
                        .pb(px(32.0))
                        .child(conversation::stream(
                            conversation_projection,
                            transcript_texts,
                            tool_cards,
                        )),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .px(px(theme::STREAM_PAD_X))
                .pt(px(12.0))
                .pb(px(10.0))
                .bg(theme::floor())
                .border_t_1()
                .border_color(theme::edge_hard())
                .child(composer.clone()),
        )
}

fn inspector(
    projection: &ShellProjection,
    conversation: &ConversationProjection,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    div()
        .w(px(theme::INSPECT_W))
        .flex_shrink_0()
        .h_full()
        .px(px(18.0))
        .py(px(18.0))
        .flex()
        .flex_col()
        .bg(theme::floor())
        .border_l_1()
        .border_color(theme::edge_hard())
        .child(
            div()
                .pb(px(10.0))
                .font_family(theme::SANS)
                .text_size(px(theme::T_UI_SM))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::bone_dim())
                .child("Runtime inspector"),
        )
        .child(controls::metric("Context", projection.context.label()))
        .child(controls::metric(
            "Input tokens",
            projection.input_tokens.label(),
        ))
        .child(controls::metric(
            "Output tokens",
            projection.output_tokens.label(),
        ))
        .child(controls::metric("Cache", projection.cache.label()))
        .child(controls::metric("Cost", projection.cost.label()))
        .child(controls::metric("Model", projection.model.label()))
        .child(controls::metric("Thinking", projection.thinking.label()))
        .child(run_controls(conversation, cx))
        .child(queue_panel(conversation))
}

fn run_controls(
    conversation: &ConversationProjection,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let locked = conversation.pending_operation.is_some();
    let can_run_controls = conversation.steering_mode.is_some();
    let compact_enabled = matches!(
        conversation.lifecycle,
        RuntimeLifecycle::Ready | RuntimeLifecycle::Settled
    ) && !locked
        && !matches!(conversation.compaction, CompactionState::Running { .. });
    let abort_enabled = conversation.lifecycle == RuntimeLifecycle::Running;
    let abort_retry_enabled = matches!(conversation.retry, RetryState::Waiting { .. });
    let bash_running = conversation
        .bash_executions
        .iter()
        .any(|execution| execution.status == BashStatus::Running);

    div()
        .mt(px(14.0))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(section_label("Run controls"))
        .when_some(
            conversation.pending_operation.as_ref(),
            |panel, operation| {
                panel.child(status_line(
                    "Pending",
                    operation_label(operation).to_owned(),
                ))
            },
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(controls::operation_button(
                    "abort-run",
                    "Abort run",
                    if abort_enabled {
                        "Current agent only"
                    } else {
                        "No active run"
                    },
                    abort_enabled,
                    controls::ControlTone::Danger,
                    Box::new(cx.listener(|view, _, _, cx| view.abort(cx))),
                ))
                .child(controls::operation_button(
                    "abort-bash",
                    "Abort Bash",
                    if bash_running {
                        "Direct Bash only"
                    } else {
                        "No Bash running"
                    },
                    bash_running,
                    controls::ControlTone::Danger,
                    Box::new(cx.listener(|view, _, _, cx| view.abort_bash(cx))),
                ))
                .child(controls::operation_button(
                    "abort-retry",
                    "Abort retry",
                    if abort_retry_enabled {
                        "Retry timer only"
                    } else {
                        "No retry timer"
                    },
                    abort_retry_enabled,
                    controls::ControlTone::Danger,
                    Box::new(cx.listener(|view, _, _, cx| view.abort_retry(cx))),
                ))
                .child(controls::operation_button(
                    "compact-now",
                    "Compact",
                    if compact_enabled {
                        "Manual context summary"
                    } else {
                        "Wait until idle"
                    },
                    compact_enabled,
                    controls::ControlTone::Normal,
                    Box::new(cx.listener(|view, _, _, cx| view.compact(cx))),
                )),
        )
        .child(mode_controls(
            "steering",
            "Steering mode",
            conversation.steering_mode,
            locked || !can_run_controls,
            |view, mode, cx| view.set_steering_mode(mode, cx),
            cx,
        ))
        .child(mode_controls(
            "follow-up",
            "Follow-up mode",
            conversation.follow_up_mode,
            locked || !can_run_controls,
            |view, mode, cx| view.set_follow_up_mode(mode, cx),
            cx,
        ))
        .child(toggle_row(
            "auto-compaction",
            "Auto compaction",
            conversation.auto_compaction_enabled,
            locked || !can_run_controls,
            |view, enabled, cx| view.set_auto_compaction(enabled, cx),
            cx,
        ))
        .child(toggle_row(
            "auto-retry",
            "Auto retry",
            conversation.auto_retry_enabled,
            locked || !can_run_controls,
            |view, enabled, cx| view.set_auto_retry(enabled, cx),
            cx,
        ))
}

fn mode_controls(
    prefix: &'static str,
    title: &'static str,
    current: Option<QueueDeliveryMode>,
    locked: bool,
    apply: fn(&mut RootView, QueueDeliveryMode, &mut Context<RootView>),
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let current_label = current.map(mode_label).unwrap_or("Unknown");
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(status_line(title, current_label.to_owned()))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(mode_button(
                    format!("{prefix}-all"),
                    "All",
                    QueueDeliveryMode::All,
                    current,
                    locked,
                    apply,
                    cx,
                ))
                .child(mode_button(
                    format!("{prefix}-one"),
                    "One at a time",
                    QueueDeliveryMode::OneAtATime,
                    current,
                    locked,
                    apply,
                    cx,
                )),
        )
}

fn mode_button(
    id: String,
    label: &'static str,
    mode: QueueDeliveryMode,
    current: Option<QueueDeliveryMode>,
    locked: bool,
    apply: fn(&mut RootView, QueueDeliveryMode, &mut Context<RootView>),
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let selected = current == Some(mode);
    let detail = if selected { "Selected" } else { "Switch mode" };
    controls::operation_button(
        id,
        label,
        detail,
        current.is_some() && !locked && !selected,
        controls::ControlTone::Normal,
        Box::new(cx.listener(move |view, _, _, cx| apply(view, mode, cx))),
    )
}

fn toggle_row(
    prefix: &'static str,
    title: &'static str,
    current: Option<bool>,
    locked: bool,
    apply: fn(&mut RootView, bool, &mut Context<RootView>),
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let current_label = current.map(on_off).unwrap_or("Unknown");
    let target = !current.unwrap_or(true);
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(status_line(title, current_label.to_owned()))
        .child(controls::operation_button(
            format!("{prefix}-toggle"),
            if target { "Enable" } else { "Disable" },
            "Protocol toggle",
            current.is_some() && !locked,
            controls::ControlTone::Normal,
            Box::new(cx.listener(move |view, _, _, cx| apply(view, target, cx))),
        ))
}

fn queue_panel(conversation: &ConversationProjection) -> impl IntoElement {
    div()
        .mt(px(14.0))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(section_label("Queued input"))
        .when(conversation.context_awaiting_fresh_usage, |panel| {
            panel.child(status_line(
                "Context",
                "Awaiting fresh usage after compaction".to_owned(),
            ))
        })
        .child(match &conversation.queue {
            QueueContents::Unknown { pending_count } => status_line(
                "Pending",
                format!(
                    "Pi reports {pending_count} queued item{}",
                    plural(*pending_count)
                ),
            )
            .into_any_element(),
            QueueContents::Known {
                steering,
                follow_up,
            } => div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(queue_group("Steering", steering))
                .child(queue_group("Follow-up", follow_up))
                .into_any_element(),
        })
}

fn queue_group(title: &'static str, items: &[String]) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .child(status_line(
            title,
            format!("{} item{}", items.len(), plural(items.len() as u64)),
        ))
        .when(items.is_empty(), |group| {
            group.child(
                div()
                    .px(px(10.0))
                    .py(px(8.0))
                    .rounded(px(theme::RADIUS_SM))
                    .bg(theme::canvas())
                    .font_family(theme::SANS)
                    .text_size(px(theme::T_UI_SM))
                    .text_color(theme::smoke())
                    .child("Empty"),
            )
        })
        .children(items.iter().enumerate().map(|(index, item)| {
            div()
                .px(px(10.0))
                .py(px(8.0))
                .rounded(px(theme::RADIUS_SM))
                .bg(theme::canvas())
                .border_1()
                .border_color(theme::edge_soft())
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::data())
                        .child(format!("{} #{:02}", title, index + 1)),
                )
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .line_height(gpui::relative(1.35))
                        .text_color(theme::bone_dim())
                        .child(item.clone()),
                )
        }))
}

fn section_label(text: &'static str) -> impl IntoElement {
    div()
        .font_family(theme::SANS)
        .text_size(px(theme::T_UI_SM))
        .font_weight(FontWeight::BOLD)
        .text_color(theme::bone_dim())
        .child(text)
}

fn status_line(label: &'static str, value: String) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_baseline()
        .justify_between()
        .gap(px(10.0))
        .child(
            div()
                .font_family(theme::SANS)
                .text_size(px(theme::T_TINY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::ash())
                .child(label),
        )
        .child(
            div()
                .min_w_0()
                .font_family(theme::MONO)
                .text_size(px(theme::T_TINY))
                .text_color(theme::bone_dim())
                .text_right()
                .child(value),
        )
}

fn operation_label(operation: &RuntimeOperation) -> &'static str {
    match operation {
        RuntimeOperation::SetSteeringMode(_) => "Changing steering mode",
        RuntimeOperation::SetFollowUpMode(_) => "Changing follow-up mode",
        RuntimeOperation::Compact => "Compacting",
        RuntimeOperation::SetAutoCompaction(_) => "Changing auto compaction",
        RuntimeOperation::SetAutoRetry(_) => "Changing auto retry",
    }
}

fn mode_label(mode: QueueDeliveryMode) -> &'static str {
    match mode {
        QueueDeliveryMode::All => "All",
        QueueDeliveryMode::OneAtATime => "One at a time",
    }
}

fn on_off(enabled: bool) -> &'static str {
    if enabled { "On" } else { "Off" }
}

fn plural(count: u64) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn action_id(action: RecoveryAction) -> &'static str {
    match action {
        RecoveryAction::Connect => "runtime-connect",
        RecoveryAction::Retry => "runtime-retry",
        RecoveryAction::Stop => "runtime-stop",
    }
}

fn lifecycle_color(projection: &ShellProjection) -> gpui::Rgba {
    match projection.lifecycle.as_str() {
        "Ready" => theme::live(),
        "Running" | "Loading" | "Connecting" | "Cancelling" | "Stopping" => theme::data(),
        "Connection error" | "No model" => theme::error(),
        "Not connected" | "Stopped" => theme::ash(),
        _ => theme::bone_dim(),
    }
}
