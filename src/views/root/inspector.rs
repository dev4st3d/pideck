use super::shared::runtime_operation_label;
use super::shared::{plural, short_path};
use super::*;
use crate::views::conversation::{TranscriptText, TranscriptTextCache};

pub(super) struct InspectorParams<'a> {
    pub(super) projection: &'a ShellProjection,
    pub(super) conversation: &'a ConversationProjection,
    pub(super) orchestration: &'a OrchestrationProjection,
    pub(super) selected_task_id: Option<&'a str>,
    pub(super) goal_edit_composer: &'a Entity<Composer>,
    pub(super) delivery_focus: DeliveryFocus,
    pub(super) usage_tooltip_hovered: bool,
    pub(super) usage_tooltip_visible: bool,
    pub(super) usage_tooltip_epoch: u64,
}

pub(super) fn inspector(
    params: InspectorParams<'_>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let InspectorParams {
        projection,
        conversation,
        orchestration,
        selected_task_id,
        goal_edit_composer,
        delivery_focus,
        usage_tooltip_hovered,
        usage_tooltip_visible,
        usage_tooltip_epoch,
    } = params;
    div()
        .w(px(theme::INSPECT_W))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(theme::floor())
        .border_l_1()
        .border_color(theme::edge_hard())
        .child(
            div()
                .px(px(16.0))
                .h(px(theme::TITLE_H))
                .flex()
                .items_center()
                .border_b_1()
                .border_color(theme::edge_soft())
                .child(
                    div()
                        .font_family(theme::sans())
                        .text_size(px(theme::T_UI_SM))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::bone_dim())
                        .child("Inspector"),
                ),
        )
        .child(
            div()
                .id("inspector-scroll")
                .w_full()
                .min_w_0()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .scrollbar_width(px(theme::SCROLLBAR))
                .child(
                    div()
                        .w_full()
                        .px(px(14.0))
                        .pt(px(14.0))
                        .pb(px(22.0))
                        .flex()
                        .flex_col()
                        .gap(px(18.0))
                        .child(controls::session_usage(controls::SessionUsageParams {
                            context: projection.context.label().into(),
                            pct: context_pct(&projection.context.label()),
                            model: projection.model.label().into(),
                            thinking: projection.thinking.label().into(),
                            cost: projection.cost.label().into(),
                            tokens_in: projection.input_tokens.label().into(),
                            tokens_out: projection.output_tokens.label().into(),
                            cache_read: projection.cache_read.label().into(),
                            cache_write: projection.cache_write.label().into(),
                            tooltip_visible: usage_tooltip_visible,
                            tooltip_hovered: usage_tooltip_hovered,
                            tooltip_epoch: usage_tooltip_epoch,
                            on_hover: Rc::new(cx.listener(|view, hovered, _, cx| {
                                view.set_usage_tooltip_hovered(*hovered, cx)
                            })),
                        }))
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .gap(px(18.0))
                                .child(orchestration_panel(
                                    orchestration,
                                    selected_task_id,
                                    goal_edit_composer,
                                    cx,
                                ))
                                .child(run_controls(conversation, delivery_focus, cx))
                                .child(queue_panel(conversation)),
                        ),
                ),
        )
}

fn orchestration_panel(
    orchestration: &OrchestrationProjection,
    selected_task_id: Option<&str>,
    goal_edit_composer: &Entity<Composer>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let body = match (&orchestration.phase, orchestration.snapshot.as_ref()) {
        (OrchestrationPhase::Loading, None) => orchestration_state_note(
            "Connecting to Pi tasks, subagents, and goal…",
            theme::data(),
            false,
            cx,
        )
        .into_any_element(),
        (OrchestrationPhase::Error, None) => orchestration_state_note(
            orchestration
                .feedback
                .as_deref()
                .unwrap_or("Pi's orchestration state could not be loaded."),
            theme::error(),
            true,
            cx,
        )
        .into_any_element(),
        (OrchestrationPhase::Disconnected, None) => orchestration_state_note(
            "The orchestration adapter is disconnected.",
            theme::error(),
            true,
            cx,
        )
        .into_any_element(),
        (OrchestrationPhase::Empty, Some(_)) => controls::empty_list_note(
            "No task, subagent, schedule, or active goal in this Pi session.",
        )
        .into_any_element(),
        (_, Some(snapshot)) => {
            div()
                .flex()
                .flex_col()
                .gap(px(16.0))
                .when(
                    matches!(
                        orchestration.phase,
                        OrchestrationPhase::Stale | OrchestrationPhase::Disconnected
                    ),
                    |panel| {
                        panel.child(orchestration_state_note(
                            orchestration
                                .feedback
                                .as_deref()
                                .unwrap_or("Showing the last authoritative snapshot."),
                            theme::signal(),
                            true,
                            cx,
                        ))
                    },
                )
                .child(task_list(snapshot.tasks.as_slice(), selected_task_id, cx))
                .child(subagent_list(snapshot.subagents.as_slice(), cx))
                .when(!snapshot.schedules.is_empty(), |panel| {
                    panel.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(7.0))
                            .child(controls::section_label("Schedules"))
                            .child(controls::divider_list().children(
                                snapshot.schedules.iter().map(|schedule| {
                                    controls::queue_row(
                                        if schedule.enabled { "ON" } else { "OFF" },
                                        format!(
                                            "{} · {} · {}",
                                            schedule.name,
                                            schedule.schedule,
                                            schedule.subagent_type
                                        ),
                                    )
                                }),
                            )),
                    )
                })
                .child(goal_panel(snapshot.goal.as_ref(), goal_edit_composer, cx))
                .into_any_element()
        }
        _ => controls::empty_list_note("Waiting for Pi orchestration state.").into_any_element(),
    };

    div()
        .flex()
        .flex_col()
        .gap(px(9.0))
        .child(
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .child(controls::section_label("Work"))
                .when(orchestration.pending_actions > 0, |row| {
                    row.child(
                        div()
                            .font_family(theme::mono())
                            .text_size(px(theme::T_TINY))
                            .text_color(theme::data())
                            .child(format!("{} pending", orchestration.pending_actions)),
                    )
                }),
        )
        .child(body)
}

fn orchestration_state_note(
    message: impl Into<String>,
    color: gpui::Rgba,
    reconnect: bool,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    div()
        .p(px(10.0))
        .rounded(px(theme::RADIUS_SM))
        .bg(theme::panel())
        .border_1()
        .border_color(theme::edge_soft())
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .font_family(theme::sans())
                .text_size(px(theme::T_UI_SM))
                .line_height(gpui::relative(1.4))
                .text_color(color)
                .child(message.into()),
        )
        .when(reconnect, |note| {
            note.child(controls::quiet_button(
                "orchestration-reconnect",
                "Reconnect",
                true,
                Box::new(cx.listener(|view, _, _, cx| {
                    view.controller
                        .update(cx, |controller, cx| controller.restart_bridge(cx));
                })),
            ))
        })
}

fn task_list(
    tasks: &[TaskSnapshot],
    selected_task_id: Option<&str>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let completed = tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Completed)
        .map(|task| task.id.as_str())
        .collect::<HashSet<_>>();
    div()
        .flex()
        .flex_col()
        .gap(px(9.0))
        .child(
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .child(controls::section_label("Tasks"))
                .child(
                    div()
                        .font_family(theme::mono())
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(format!("{}/{}", completed.len(), tasks.len())),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                // Keep the former bordered list's footprint without boxing the tasks in.
                .px(px(4.0))
                .pt(px(3.0))
                .pb(px(6.0))
                .when(tasks.is_empty(), |list| {
                    list.child(controls::empty_list_note("No tasks in this session."))
                })
                .children(tasks.iter().enumerate().map(|(index, task)| {
                    task_row(
                        index,
                        task,
                        selected_task_id == Some(task.id.as_str()),
                        index + 1 == tasks.len(),
                        task.blocked_by
                            .iter()
                            .filter(|id| !completed.contains(id.as_str()))
                            .count(),
                        cx,
                    )
                })),
        )
}

fn task_row(
    index: usize,
    task: &TaskSnapshot,
    selected: bool,
    is_last: bool,
    open_blockers: usize,
    cx: &mut Context<RootView>,
) -> gpui::AnyElement {
    let id = task.id.clone();
    let keyboard_id = task.id.clone();
    let action_id = task.id.clone();
    let status_color = task_status_color(task.status, open_blockers);
    let can_execute = task.status == TaskStatus::Pending && open_blockers == 0;
    let can_stop = task.status == TaskStatus::InProgress;
    div()
        .relative()
        // Reserve a two-pixel structural inset for the connector rail.
        .border_l_2()
        .border_color(gpui::rgba(0x0000_0000))
        .rounded(px(theme::RADIUS_SM))
        .bg(if selected {
            theme::panel_lift()
        } else {
            gpui::rgba(0x0000_0000)
        })
        .child(
            div()
                .absolute()
                .left(px(1.0))
                .top_0()
                .when(is_last, |line| line.h(px(20.0)))
                .when(!is_last, |line| line.bottom_0())
                .w(px(1.0))
                .bg(theme::smoke())
                .opacity(0.48),
        )
        .child(
            div()
                .absolute()
                .left(px(1.0))
                .top(px(19.0))
                .w(px(6.0))
                .h(px(2.0))
                .rounded(px(1.0))
                .bg(theme::smoke())
                .opacity(0.48),
        )
        .child(
            div()
                .absolute()
                .left(px(7.0))
                .top(px(18.0))
                .size(px(4.0))
                .bg(theme::ash()),
        )
        .child(
            div()
                .id(("task-row", index))
                .tab_index(0)
                .key_context(ORCHESTRATION_ROW_CONTEXT)
                .cursor_pointer()
                .pl(px(14.0))
                .pr(px(12.0))
                .py(px(11.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .hover(|row| row.bg(theme::panel_hover()))
                .focus(|row| row.bg(theme::panel_lift()))
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.selected_task_id = if view.selected_task_id.as_deref() == Some(&id) {
                        None
                    } else {
                        Some(id.clone())
                    };
                    cx.notify();
                }))
                .on_action(cx.listener(move |view, _: &OrchestrationActivate, _, cx| {
                    view.selected_task_id =
                        if view.selected_task_id.as_deref() == Some(&keyboard_id) {
                            None
                        } else {
                            Some(keyboard_id.clone())
                        };
                    cx.notify();
                }))
                .child(
                    div()
                        .h(px(18.0))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.0))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .font_family(theme::sans())
                                .text_size(px(theme::T_UI_SM))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::bone())
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(task.subject.clone()),
                        )
                        .child(task_status_widget(
                            index,
                            task.status,
                            open_blockers,
                            status_color,
                        )),
                )
                .child(
                    div()
                        .font_family(theme::mono())
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(format!(
                            "{} · {}",
                            task.id,
                            task.owner.as_deref().unwrap_or("unassigned")
                        )),
                ),
        )
        .when(selected, |row| {
            let task_id = action_id.clone();
            row.child(
                div()
                    .pl(px(14.0))
                    .pr(px(12.0))
                    .pb(px(10.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .font_family(theme::sans())
                            .text_size(px(theme::T_UI_SM))
                            .line_height(gpui::relative(1.45))
                            .text_color(theme::bone_dim())
                            .child(task.description.clone()),
                    )
                    .when(open_blockers > 0, |detail| {
                        detail.child(controls::empty_list_note(format!(
                            "Waiting on {}",
                            task.blocked_by.join(", ")
                        )))
                    })
                    .when_some(task.output.clone(), |detail, output| {
                        detail.child(
                            div()
                                .p(px(8.0))
                                .rounded(px(theme::RADIUS_SM))
                                .bg(theme::canvas())
                                .font_family(theme::mono())
                                .text_size(px(theme::T_TINY))
                                .text_color(theme::ash())
                                .child(output),
                        )
                    })
                    .when(
                        !task.metadata.is_null()
                            && task
                                .metadata
                                .as_object()
                                .is_none_or(|value| !value.is_empty()),
                        |detail| {
                            detail.child(
                                div()
                                    .font_family(theme::mono())
                                    .text_size(px(theme::T_TINY))
                                    .text_color(theme::smoke())
                                    .child(task.metadata.to_string()),
                            )
                        },
                    )
                    .child(controls::quiet_button(
                        format!("task-action-{task_id}"),
                        if can_stop {
                            "Stop task"
                        } else {
                            "Execute task"
                        },
                        can_stop || can_execute,
                        Box::new(cx.listener(move |view, _, _, cx| {
                            let action = if can_stop {
                                OrchestrationAction::TaskStop {
                                    task_id: task_id.clone(),
                                }
                            } else {
                                OrchestrationAction::TaskExecute {
                                    task_ids: vec![task_id.clone()],
                                    additional_context: None,
                                    model: None,
                                    max_turns: None,
                                    cascade: true,
                                }
                            };
                            view.dispatch_orchestration_action(action, cx);
                        })),
                    )),
            )
        })
        .into_any_element()
}

fn subagent_list(agents: &[SubagentSnapshot], cx: &mut Context<RootView>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(7.0))
        .child(
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .child(controls::section_label("Subagents"))
                .child(
                    div()
                        .font_family(theme::mono())
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(agents.len().to_string()),
                ),
        )
        .child(
            controls::divider_list()
                .when(agents.is_empty(), |list| {
                    list.child(controls::empty_list_note("No subagents in this session."))
                })
                .children(agents.iter().enumerate().map(|(index, agent)| {
                    let agent_id = agent.id.clone();
                    let keyboard_agent_id = agent.id.clone();
                    div()
                        .id(("subagent-row", index))
                        .tab_index(0)
                        .key_context(ORCHESTRATION_ROW_CONTEXT)
                        .cursor_pointer()
                        .px(px(10.0))
                        .py(px(9.0))
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .hover(|row| row.bg(theme::panel_hover()))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.open_subagent(agent_id.clone(), window, cx);
                        }))
                        .on_action(cx.listener(
                            move |view, _: &OrchestrationActivate, window, cx| {
                                view.open_subagent(keyboard_agent_id.clone(), window, cx);
                            },
                        ))
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .justify_between()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .font_family(theme::sans())
                                        .text_size(px(theme::T_UI_SM))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(agent_type_color(&agent.agent_type))
                                        .child(agent.agent_type.clone()),
                                )
                                .child(
                                    div()
                                        .font_family(theme::sans())
                                        .text_size(px(theme::T_TINY))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(subagent_status_color(agent.status))
                                        .child(agent.status.label()),
                                ),
                        )
                        .child(
                            div()
                                .font_family(theme::sans())
                                .text_size(px(theme::T_UI_SM))
                                .line_height(gpui::relative(1.4))
                                .text_color(theme::bone_dim())
                                .child(agent.description.clone()),
                        )
                        .child(
                            div()
                                .font_family(theme::mono())
                                .text_size(px(theme::T_TINY))
                                .text_color(theme::smoke())
                                .child(match agent.queue_position {
                                    Some(position) => format!(
                                        "{} · queue {} · limit {}",
                                        agent.id, position, agent.max_concurrent
                                    ),
                                    None => format!(
                                        "{} · {} tool use{}",
                                        agent.id,
                                        agent.tool_uses,
                                        plural(agent.tool_uses)
                                    ),
                                }),
                        )
                })),
        )
}

fn goal_panel(
    goal: Option<&crate::orchestration::GoalSnapshot>,
    goal_edit_composer: &Entity<Composer>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let Some(goal) = goal else {
        return div()
            .flex()
            .flex_col()
            .gap(px(7.0))
            .child(controls::section_label("Goal"))
            .child(controls::empty_list_note("No active goal."))
            .into_any_element();
    };
    let Some(active) = goal.active.as_ref() else {
        return div()
            .flex()
            .flex_col()
            .gap(px(7.0))
            .child(controls::section_label("Goal queue"))
            .child(controls::empty_list_note(
                "No active goal. Pi still has queued goal work.",
            ))
            .child(
                controls::divider_list().children(goal.queue.iter().enumerate().map(
                    |(index, item)| {
                        controls::queue_row(
                            format!("GOAL {:02}", index + 1),
                            item.objective.clone(),
                        )
                    },
                )),
            )
            .into_any_element();
    };
    let goal_id = active.id.clone();
    let pause_id = goal_id.clone();
    let resume_id = goal_id.clone();
    let clear_id = goal_id.clone();
    let resumable = matches!(
        active.status.as_str(),
        "paused" | "blocked" | "usage_limited" | "budget_limited"
    );
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .child(controls::section_label("Goal"))
                .child(
                    div()
                        .font_family(theme::mono())
                        .text_size(px(theme::T_TINY))
                        .text_color(goal_status_color(&active.status))
                        .child(active.status.clone()),
                ),
        )
        .child(
            div()
                .p(px(10.0))
                .rounded(px(theme::RADIUS_SM))
                .bg(theme::panel())
                .border_1()
                .border_color(theme::edge_soft())
                .flex()
                .flex_col()
                .gap(px(7.0))
                .child(
                    div()
                        .font_family(theme::sans())
                        .text_size(px(theme::T_UI))
                        .font_weight(FontWeight::SEMIBOLD)
                        .line_height(gpui::relative(1.45))
                        .text_color(theme::bone())
                        .child(active.objective.clone()),
                )
                .child(goal_metrics(active, goal.queue.len()))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap(px(6.0))
                        .child(controls::chip_button(
                            format!("goal-pause-{goal_id}"),
                            "Pause",
                            false,
                            active.status == "active",
                            Box::new(cx.listener(move |view, _, _, cx| {
                                view.dispatch_orchestration_action(
                                    OrchestrationAction::GoalPause {
                                        goal_id: pause_id.clone(),
                                    },
                                    cx,
                                );
                            })),
                        ))
                        .child(controls::chip_button(
                            format!("goal-resume-{goal_id}"),
                            "Resume",
                            false,
                            resumable,
                            Box::new(cx.listener(move |view, _, _, cx| {
                                view.dispatch_orchestration_action(
                                    OrchestrationAction::GoalResume {
                                        goal_id: resume_id.clone(),
                                    },
                                    cx,
                                );
                            })),
                        ))
                        .child(controls::chip_button(
                            format!("goal-clear-{goal_id}"),
                            "Clear",
                            false,
                            true,
                            Box::new(cx.listener(move |view, _, _, cx| {
                                view.dispatch_orchestration_action(
                                    OrchestrationAction::GoalClear {
                                        goal_id: clear_id.clone(),
                                    },
                                    cx,
                                );
                            })),
                        )),
                ),
        )
        .child(goal_edit_composer.clone())
        .when(!goal.queue.is_empty(), |panel| {
            panel.child(
                controls::divider_list().children(goal.queue.iter().enumerate().map(
                    |(index, item)| {
                        controls::queue_row(
                            format!("GOAL {:02}", index + 1),
                            item.objective.clone(),
                        )
                    },
                )),
            )
        })
        .into_any_element()
}

fn goal_metrics(goal: &GoalItemSnapshot, queued: usize) -> impl IntoElement {
    let budget = goal
        .token_budget
        .map(|budget| format!("{} / {} tokens", goal.tokens_used, budget))
        .unwrap_or_else(|| format!("{} tokens", goal.tokens_used));
    div()
        .font_family(theme::mono())
        .text_size(px(theme::T_TINY))
        .text_color(theme::ash())
        .child(format!(
            "{} · {} elapsed · iteration {} · {} queued",
            budget,
            format_elapsed(goal.time_used_seconds),
            goal.iteration,
            queued
        ))
}

fn task_status_widget(
    index: usize,
    status: TaskStatus,
    blockers: usize,
    color: gpui::Rgba,
) -> gpui::AnyElement {
    let animated = blockers == 0 && status != TaskStatus::Completed;
    let cycle = match status {
        TaskStatus::Pending => Duration::from_millis(1_100),
        TaskStatus::InProgress | TaskStatus::Completed => Duration::from_millis(720),
    };
    // Key zero belongs to the titlebar status indicator.
    controls::square_status_indicator(index + 1, animated, cycle, color)
}

fn task_status_color(status: TaskStatus, blockers: usize) -> gpui::Rgba {
    if blockers > 0 {
        return theme::error();
    }
    match status {
        TaskStatus::Pending => theme::data(),
        TaskStatus::InProgress => theme::working(),
        TaskStatus::Completed => theme::live(),
    }
}

fn subagent_status_color(status: SubagentStatus) -> gpui::Rgba {
    match status {
        SubagentStatus::Queued => theme::data(),
        SubagentStatus::Running => theme::live(),
        SubagentStatus::Completed | SubagentStatus::Steered => theme::smoke(),
        SubagentStatus::Aborted | SubagentStatus::Stopped => theme::signal(),
        SubagentStatus::Error => theme::error(),
    }
}

fn agent_type_color(agent_type: &str) -> gpui::Rgba {
    match agent_type {
        "explore" | "research" => theme::live(),
        "plan" | "review" => theme::data(),
        _ => theme::signal(),
    }
}

fn goal_status_color(status: &str) -> gpui::Rgba {
    match status {
        "active" => theme::live(),
        "paused" | "usage_limited" | "budget_limited" => theme::signal(),
        "complete" | "completed" => theme::smoke(),
        "blocked" | "error" => theme::error(),
        _ => theme::ash(),
    }
}

fn format_elapsed(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{}s", seconds % 60)
    }
}

pub(super) fn subagent_dialog(
    agent: Option<&SubagentSnapshot>,
    requested_id: &str,
    focus: &FocusHandle,
    scroll: &ScrollHandle,
    composer: &Entity<Composer>,
    transcript_cache: &Entity<TranscriptTextCache>,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let header = agent
        .map(|agent| {
            (
                agent.agent_type.clone(),
                agent.description.clone(),
                agent.status.label().to_owned(),
                subagent_status_color(agent.status),
            )
        })
        .unwrap_or_else(|| {
            (
                "Subagent".to_owned(),
                "This agent ID is no longer present in Pi's authoritative store.".to_owned(),
                "Stale ID".to_owned(),
                theme::error(),
            )
        });
    let active = agent.is_some_and(|agent| agent.status.is_active());
    let stop_id = requested_id.to_owned();
    let (transcript_texts, result_text, error_text) = agent.map_or_else(
        || (Vec::new(), None, None),
        |agent| {
            transcript_cache.update(cx, |cache, cx| {
                let transcript_texts = agent
                    .transcript
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| {
                        cache.entity_for(
                            format!("subagent:{}:entry:{index}", agent.id),
                            &entry.content,
                            cx,
                        )
                    })
                    .collect::<Vec<_>>();
                let result_text = agent.result.as_ref().map(|result| {
                    cache.entity_for(format!("subagent:{}:result", agent.id), result, cx)
                });
                let error_text = agent.error.as_ref().map(|error| {
                    cache.entity_for(format!("subagent:{}:error", agent.id), error, cx)
                });
                (transcript_texts, result_text, error_text)
            })
        },
    );

    div()
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .left_0()
        .occlude()
        .bg(gpui::rgba(0x0b0a_09ed))
        .flex()
        .items_center()
        .justify_center()
        .p(px(28.0))
        .track_focus(focus)
        .tab_index(0)
        .on_key_down(cx.listener(RootView::on_subagent_dialog_key_down))
        .child(
            div()
                .id("subagent-conversation-dialog")
                .w_full()
                .max_w(px(980.0))
                .h_full()
                .max_h(px(780.0))
                .rounded(px(theme::RADIUS))
                .bg(theme::floor())
                .border_1()
                .border_color(theme::edge_hard())
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(
                    div()
                        .h(px(62.0))
                        .px(px(18.0))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(16.0))
                        .border_b_1()
                        .border_color(theme::edge_hard())
                        .child(
                            div()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(3.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_baseline()
                                        .gap(px(9.0))
                                        .child(
                                            div()
                                                .font_family(theme::sans())
                                                .text_size(px(theme::T_TITLE))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(agent_type_color(&header.0))
                                                .child(header.0.clone()),
                                        )
                                        .child(
                                            div()
                                                .font_family(theme::mono())
                                                .text_size(px(theme::T_TINY))
                                                .text_color(theme::smoke())
                                                .child(requested_id.to_owned()),
                                        ),
                                )
                                .child(
                                    div()
                                        .font_family(theme::sans())
                                        .text_size(px(theme::T_UI_SM))
                                        .text_color(theme::ash())
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(header.1.clone()),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(controls::status_pill(header.2.clone(), header.3))
                                .when(active, |row| {
                                    row.child(controls::quiet_button(
                                        "subagent-stop",
                                        "Stop",
                                        true,
                                        Box::new(cx.listener(move |view, _, _, cx| {
                                            view.dispatch_orchestration_action(
                                                OrchestrationAction::SubagentStop {
                                                    agent_id: stop_id.clone(),
                                                },
                                                cx,
                                            );
                                        })),
                                    ))
                                })
                                .child(controls::quiet_button(
                                    "subagent-close",
                                    "Close · Esc",
                                    true,
                                    Box::new(cx.listener(|view, _, window, cx| {
                                        view.close_subagent(window, cx);
                                    })),
                                )),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_row()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .relative()
                                .child(controls::scroll_wheel_capture(scroll))
                                .child(
                                    div()
                                        .id("subagent-live-transcript")
                                        .size_full()
                                        .overflow_y_scroll()
                                        .scrollbar_width(px(theme::SCROLLBAR))
                                        .track_scroll(scroll)
                                        .child(
                                            div()
                                                .w_full()
                                                .max_w(px(760.0))
                                                .mx_auto()
                                                .px(px(24.0))
                                                .py(px(22.0))
                                                .flex()
                                                .flex_col()
                                                .gap(px(16.0))
                                                .when(agent.is_none(), |transcript| {
                                                    transcript.child(controls::empty_list_note(
                                                        "Close this view and open a current agent from the Inspector.",
                                                    ))
                                                })
                                                .when_some(agent, |transcript, agent| {
                                                    transcript
                                                        .when(
                                                            agent.transcript.is_empty()
                                                                && agent.result.is_none()
                                                                && agent.error.is_none(),
                                                            |transcript| {
                                                                transcript.child(
                                                                    controls::empty_list_note(
                                                                        if agent.status
                                                                            == SubagentStatus::Queued
                                                                        {
                                                                            "Queued. Live output will appear when Pi starts the agent."
                                                                        } else {
                                                                            "Pi has not emitted conversation output for this agent yet."
                                                                        },
                                                                    ),
                                                                )
                                                            },
                                                        )
                                                        .children(
                                                            agent
                                                                .transcript
                                                                .iter()
                                                                .zip(transcript_texts.iter())
                                                                .map(|(entry, text)| {
                                                                    subagent_transcript_entry(
                                                                        entry,
                                                                        text.clone(),
                                                                    )
                                                                }),
                                                        )
                                                        .when(
                                                            agent.transcript_truncated,
                                                            |transcript| {
                                                                transcript.child(
                                                                    div()
                                                                        .px(px(10.0))
                                                                        .py(px(7.0))
                                                                        .rounded(px(
                                                                            theme::RADIUS_SM,
                                                                        ))
                                                                        .bg(theme::panel())
                                                                        .font_family(theme::mono())
                                                                        .text_size(px(
                                                                            theme::T_TINY,
                                                                        ))
                                                                        .text_color(theme::smoke())
                                                                        .child(
                                                                            "Earlier output is truncated; the live tail is shown.",
                                                                        ),
                                                                )
                                                            },
                                                        )
                                                        .when_some(
                                                            result_text.clone(),
                                                            |transcript, result| {
                                                                transcript.child(
                                                                    subagent_result_panel(
                                                                        "Result", result, false,
                                                                    ),
                                                                )
                                                            },
                                                        )
                                                        .when_some(
                                                            error_text.clone(),
                                                            |transcript, error| {
                                                                transcript.child(
                                                                    subagent_result_panel(
                                                                        "Error", error, true,
                                                                    ),
                                                                )
                                                            },
                                                        )
                                                }),
                                        ),
                                ),
                        )
                        .when_some(agent, |layout, agent| {
                            layout.child(subagent_metadata_panel(agent))
                        }),
                )
                .when(agent.is_some(), |dialog| {
                    dialog.child(
                        div()
                            .flex_shrink_0()
                            .px(px(18.0))
                            .py(px(12.0))
                            .border_t_1()
                            .border_color(theme::edge_hard())
                            .bg(theme::panel())
                            .child(composer.clone()),
                    )
                }),
        )
}

fn subagent_transcript_entry(
    entry: &crate::orchestration::SubagentTranscriptEntry,
    content: Entity<TranscriptText>,
) -> impl IntoElement {
    let (label, color, background, border, max_width) = match entry.role {
        TranscriptRole::User => (
            "YOU",
            theme::signal(),
            theme::user_message(),
            theme::user_message_edge(),
            620.0,
        ),
        TranscriptRole::Assistant => (
            "AGENT",
            theme::live(),
            theme::canvas(),
            theme::edge_soft(),
            760.0,
        ),
        TranscriptRole::ToolResult => (
            "TOOL",
            theme::data(),
            theme::panel(),
            theme::edge_soft(),
            760.0,
        ),
        TranscriptRole::System => (
            "SYSTEM",
            theme::ash(),
            theme::panel(),
            theme::edge_soft(),
            760.0,
        ),
    };
    div()
        .w_full()
        .flex()
        .when(entry.role == TranscriptRole::User, |row| row.justify_end())
        .child(
            div()
                .w_full()
                .max_w(px(max_width))
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(8.0))
                        .child(
                            div()
                                .font_family(theme::mono())
                                .text_size(px(theme::T_TINY))
                                .font_weight(FontWeight::BOLD)
                                .text_color(if entry.is_error {
                                    theme::error()
                                } else {
                                    color
                                })
                                .child(label),
                        )
                        .when_some(entry.tool_name.clone(), |row, tool| {
                            row.child(
                                div()
                                    .font_family(theme::mono())
                                    .text_size(px(theme::T_TINY))
                                    .text_color(theme::smoke())
                                    .child(tool),
                            )
                        })
                        .when_some(entry.timestamp.clone(), |row, timestamp| {
                            row.child(
                                div()
                                    .ml_auto()
                                    .font_family(theme::mono())
                                    .text_size(px(theme::T_TINY))
                                    .text_color(theme::smoke())
                                    .child(timestamp),
                            )
                        }),
                )
                .child(
                    div()
                        .p(px(if entry.role == TranscriptRole::Assistant {
                            4.0
                        } else {
                            12.0
                        }))
                        .rounded(px(theme::RADIUS_SM))
                        .bg(background)
                        .when(entry.role != TranscriptRole::Assistant, |body| {
                            body.border_1().border_color(if entry.is_error {
                                theme::error()
                            } else {
                                border
                            })
                        })
                        .font_family(if entry.role == TranscriptRole::ToolResult {
                            theme::mono()
                        } else {
                            theme::sans()
                        })
                        .text_size(px(theme::T_UI))
                        .line_height(gpui::relative(1.55))
                        .text_color(theme::bone_dim())
                        .child(content),
                ),
        )
}

fn subagent_result_panel(
    label: &'static str,
    content: Entity<TranscriptText>,
    error: bool,
) -> impl IntoElement {
    let color = if error { theme::error() } else { theme::live() };
    div()
        .w_full()
        .mt(px(4.0))
        .rounded(px(theme::RADIUS_SM))
        .border_1()
        .border_color(color)
        .bg(theme::panel())
        .overflow_hidden()
        .child(
            div()
                .px(px(12.0))
                .py(px(8.0))
                .border_b_1()
                .border_color(theme::edge_soft())
                .font_family(theme::mono())
                .text_size(px(theme::T_TINY))
                .font_weight(FontWeight::BOLD)
                .text_color(color)
                .child(label),
        )
        .child(
            div()
                .px(px(14.0))
                .py(px(12.0))
                .font_family(theme::sans())
                .text_size(px(theme::T_UI))
                .line_height(gpui::relative(1.55))
                .text_color(if error {
                    theme::error()
                } else {
                    theme::bone_dim()
                })
                .child(content),
        )
}

fn subagent_metadata_panel(agent: &SubagentSnapshot) -> impl IntoElement {
    div()
        .id("subagent-metadata")
        .w(px(230.0))
        .flex_shrink_0()
        .h_full()
        .overflow_y_scroll()
        .px(px(14.0))
        .py(px(18.0))
        .border_l_1()
        .border_color(theme::edge_hard())
        .bg(theme::panel())
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(controls::section_label("Run details"))
        .child(controls::metric_row(
            "Tool uses",
            agent.tool_uses.to_string(),
        ))
        .child(controls::metric_row(
            "Concurrency",
            agent.max_concurrent.to_string(),
        ))
        .when_some(agent.queue_position, |panel, position| {
            panel.child(controls::metric_row("Queue", position.to_string()))
        })
        .when_some(agent.output_file.clone(), |panel, output| {
            panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(controls::section_label("Output"))
                    .child(
                        div()
                            .font_family(theme::mono())
                            .text_size(px(theme::T_TINY))
                            .text_color(theme::ash())
                            .child(short_path(&output)),
                    ),
            )
        })
        .when_some(agent.worktree.as_ref(), |panel, worktree| {
            panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(controls::section_label("Worktree"))
                    .child(
                        div()
                            .font_family(theme::mono())
                            .text_size(px(theme::T_TINY))
                            .text_color(theme::ash())
                            .child(format!(
                                "{}\n{}",
                                worktree.branch,
                                short_path(&worktree.work_path)
                            )),
                    )
                    .when_some(agent.worktree_result.as_ref(), |detail, result| {
                        detail.child(
                            div()
                                .font_family(theme::sans())
                                .text_size(px(theme::T_UI_SM))
                                .text_color(if result.has_changes {
                                    theme::live()
                                } else {
                                    theme::smoke()
                                })
                                .child(if result.has_changes {
                                    "Changes available"
                                } else {
                                    "No changes"
                                }),
                        )
                    }),
            )
        })
        .when_some(agent.memory.as_ref(), |panel, memory| {
            panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(controls::section_label("Memory"))
                    .child(
                        div()
                            .font_family(theme::mono())
                            .text_size(px(theme::T_TINY))
                            .text_color(theme::ash())
                            .child(match memory.path.as_deref() {
                                Some(path) => format!("{} · {}", memory.scope, short_path(path)),
                                None => memory.scope.clone(),
                            }),
                    ),
            )
        })
}

fn run_controls(
    conversation: &ConversationProjection,
    delivery_focus: DeliveryFocus,
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let locked = conversation.pending_operation.is_some();
    let can_run_controls = conversation.steering_mode.is_some();
    let settings_locked = locked || !can_run_controls;
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
    let run_status = run_status_label(
        conversation,
        abort_enabled,
        bash_running,
        abort_retry_enabled,
    );

    div()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .gap(px(8.0))
                .child(controls::section_label("Run"))
                .child(
                    div()
                        .min_w_0()
                        .font_family(theme::mono())
                        .text_size(px(theme::T_TINY))
                        .text_color(if conversation.pending_operation.is_some() {
                            theme::data()
                        } else {
                            theme::smoke()
                        })
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(
                            conversation
                                .pending_operation
                                .as_ref()
                                .map(runtime_operation_label)
                                .unwrap_or(run_status),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .w_full()
                .flex_row()
                .gap(px(6.0))
                .child(controls::tone_button(
                    "abort-run",
                    "Abort",
                    abort_enabled,
                    controls::ControlTone::Danger,
                    Box::new(cx.listener(|view, _, _, cx| {
                        view.controller.update(cx, |controller, cx| {
                            let _ = controller.abort(cx);
                        });
                    })),
                ))
                .child(controls::tone_button(
                    "abort-bash",
                    "Bash",
                    bash_running,
                    controls::ControlTone::Danger,
                    Box::new(cx.listener(|view, _, _, cx| {
                        view.controller.update(cx, |controller, cx| {
                            let _ = controller.abort_bash(cx);
                        });
                    })),
                ))
                .child(controls::tone_button(
                    "abort-retry",
                    "Retry",
                    abort_retry_enabled,
                    controls::ControlTone::Danger,
                    Box::new(cx.listener(|view, _, _, cx| view.abort_retry(cx))),
                ))
                .child(controls::tone_button(
                    "compact-now",
                    "Compact",
                    compact_enabled,
                    controls::ControlTone::Normal,
                    Box::new(cx.listener(|view, _, window, cx| {
                        view.open_compaction_modal(window, cx);
                    })),
                )),
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
                        .items_baseline()
                        .justify_between()
                        .gap(px(8.0))
                        .child(controls::section_label("Delivery"))
                        .child(
                            div()
                                .font_family(theme::mono())
                                .text_size(px(theme::T_TINY))
                                .text_color(theme::smoke())
                                .child("All"),
                        ),
                )
                .child(
                    controls::tab_track()
                        .child(controls::tab_button(
                            "delivery-steering",
                            "Steering",
                            delivery_focus == DeliveryFocus::Steering,
                            Box::new(cx.listener(|view, _, _, cx| {
                                view.delivery_focus = DeliveryFocus::Steering;
                                cx.notify();
                            })),
                        ))
                        .child(controls::tab_button(
                            "delivery-follow-up",
                            "Follow-up",
                            delivery_focus == DeliveryFocus::FollowUp,
                            Box::new(cx.listener(|view, _, _, cx| {
                                view.delivery_focus = DeliveryFocus::FollowUp;
                                cx.notify();
                            })),
                        )),
                )
                .child(
                    div()
                        .font_family(theme::sans())
                        .text_size(px(theme::T_TINY))
                        .line_height(gpui::relative(1.35))
                        .text_color(theme::smoke())
                        .child(match delivery_focus {
                            DeliveryFocus::Steering => {
                                "How queued steers land while the agent is running."
                            }
                            DeliveryFocus::FollowUp => {
                                "How follow-ups drain after the current turn finishes."
                            }
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(toggle_setting_row(
                    "auto-compaction",
                    "Auto compact",
                    conversation.auto_compaction_enabled,
                    settings_locked,
                    |view, enabled, cx| view.set_auto_compaction(enabled, cx),
                    cx,
                )),
        )
}

fn toggle_setting_row(
    prefix: &'static str,
    title: &'static str,
    current: Option<bool>,
    locked: bool,
    apply: fn(&mut RootView, bool, &mut Context<RootView>),
    cx: &mut Context<RootView>,
) -> impl IntoElement {
    let enabled = current.unwrap_or(false);
    let known = current.is_some();
    controls::setting_row(
        title,
        None::<&str>,
        div()
            .flex()
            .flex_row()
            .gap(px(4.0))
            .child(controls::chip_button(
                format!("{prefix}-on"),
                "On",
                known && enabled,
                known && !locked && !enabled,
                Box::new(cx.listener(move |view, _, _, cx| apply(view, true, cx))),
            ))
            .child(controls::chip_button(
                format!("{prefix}-off"),
                "Off",
                known && !enabled,
                known && !locked && enabled,
                Box::new(cx.listener(move |view, _, _, cx| apply(view, false, cx))),
            )),
    )
}

fn run_status_label(
    conversation: &ConversationProjection,
    abort_enabled: bool,
    bash_running: bool,
    abort_retry_enabled: bool,
) -> &'static str {
    if matches!(conversation.compaction, CompactionState::Running { .. }) {
        "Compacting"
    } else if abort_enabled && bash_running {
        "Agent + Bash"
    } else if abort_enabled {
        "Running"
    } else if bash_running {
        "Bash running"
    } else if abort_retry_enabled {
        "Retry waiting"
    } else {
        match conversation.lifecycle {
            RuntimeLifecycle::Ready => "Ready",
            RuntimeLifecycle::Settled => "Settled",
            RuntimeLifecycle::Running => "Running",
            _ => "Idle",
        }
    }
}

fn queue_panel(conversation: &ConversationProjection) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .child(controls::section_label("Queue"))
                .when(conversation.context_awaiting_fresh_usage, |row| {
                    row.child(
                        div()
                            .font_family(theme::mono())
                            .text_size(px(theme::T_TINY))
                            .text_color(theme::data())
                            .child("awaiting usage"),
                    )
                }),
        )
        .child(match conversation.queue.as_ref() {
            QueueContents::Unknown { pending_count } => controls::divider_list()
                .child(controls::empty_list_note(format!(
                    "Pi reports {pending_count} queued item{}",
                    plural(*pending_count)
                )))
                .into_any_element(),
            QueueContents::Known {
                steering,
                follow_up,
            } => controls::divider_list()
                .when(steering.is_empty() && follow_up.is_empty(), |list| {
                    list.child(controls::empty_list_note("Nothing queued."))
                })
                .children(steering.iter().enumerate().map(|(index, item)| {
                    div()
                        .when(index + 1 < steering.len() || !follow_up.is_empty(), |row| {
                            row.border_b_1().border_color(theme::edge_soft())
                        })
                        .child(controls::queue_row(
                            format!("STEER {:02}", index + 1),
                            item.clone(),
                        ))
                }))
                .children(follow_up.iter().enumerate().map(|(index, item)| {
                    div()
                        .when(index + 1 < follow_up.len(), |row| {
                            row.border_b_1().border_color(theme::edge_soft())
                        })
                        .child(controls::queue_row(
                            format!("FOLLOW {:02}", index + 1),
                            item.clone(),
                        ))
                }))
                .into_any_element(),
        })
}

pub(super) fn context_pct(label: &str) -> Option<f32> {
    let cleaned = label
        .split('·')
        .next()
        .unwrap_or(label)
        .trim()
        .replace(',', "");
    if let Some((used, total)) = cleaned.split_once('/') {
        let used = used.trim().parse::<f32>().ok()?;
        let total = total.trim().parse::<f32>().ok()?;
        if total > 0.0 {
            return Some((used / total).clamp(0.0, 1.0));
        }
    }
    cleaned
        .trim_end_matches('%')
        .parse::<f32>()
        .ok()
        .map(|value| (value / 100.0).clamp(0.0, 1.0))
}
