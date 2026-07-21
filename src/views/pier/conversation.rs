//! Turn grouping and conversation stream presentation.

use gpui::{
    AnyElement, FontWeight, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder, px,
    relative,
};

use crate::{state::StreamEntry, theme};

// ── Conversation stream: turn-grouped ───────────────────────────────────────
//
// Grouped turns, not flat chat rows:
//   preamble → turn { prompt | activity spine | reply }
// Hierarchy is tonal: panel prompt → floor work → canvas answer.
// Prompt and answer labels stay quiet; borders stay off the activity chrome.

#[derive(Debug)]
enum StreamSegment<'a> {
    Preamble(&'a StreamEntry),
    Turn {
        index: usize,
        user: &'a StreamEntry,
        activity: Vec<&'a StreamEntry>,
        reply: Option<&'a StreamEntry>,
    },
}

fn segment_stream(entries: &[StreamEntry]) -> Vec<StreamSegment<'_>> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut turn_no = 0usize;

    while i < entries.len() {
        if matches!(entries[i], StreamEntry::User { .. }) {
            turn_no += 1;
            let user = &entries[i];
            i += 1;
            let mut activity = Vec::new();
            let mut reply = None;

            while i < entries.len() {
                match &entries[i] {
                    StreamEntry::User { .. } => break,
                    StreamEntry::Assistant { .. } => {
                        reply = Some(&entries[i]);
                        i += 1;
                        break;
                    }
                    StreamEntry::System { .. } | StreamEntry::Compaction { .. }
                        if activity.is_empty() =>
                    {
                        break;
                    }
                    _ => {
                        activity.push(&entries[i]);
                        i += 1;
                    }
                }
            }

            out.push(StreamSegment::Turn {
                index: turn_no,
                user,
                activity,
                reply,
            });
        } else {
            out.push(StreamSegment::Preamble(&entries[i]));
            i += 1;
        }
    }
    out
}

/// Full conversation body for the turn-grouped stream.
/// Always fills the host column width (no centered max-width block).
pub(in crate::views) fn stream(entries: &[StreamEntry]) -> impl IntoElement {
    let segments = segment_stream(entries);
    let turn_count = segments
        .iter()
        .filter(|s| matches!(s, StreamSegment::Turn { .. }))
        .count();

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(16.0))
        .child(
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
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::ash())
                        .child("Conversation"),
                )
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(format!(
                            "{turn_count} turn{}",
                            if turn_count == 1 { "" } else { "s" }
                        )),
                ),
        )
        .children(segments.into_iter().map(|segment| match segment {
            StreamSegment::Preamble(entry) => preamble_row(entry),
            StreamSegment::Turn {
                index,
                user,
                activity,
                reply,
            } => turn_card(index, user, &activity, reply).into_any_element(),
        }))
}

fn preamble_row(entry: &StreamEntry) -> AnyElement {
    match entry {
        StreamEntry::Compaction { body } => div()
            .w_full()
            .px(px(14.0))
            .py(px(11.0))
            .rounded(px(theme::RADIUS_SM))
            .bg(theme::data_wash())
            .child(
                div()
                    .font_family(theme::SANS)
                    .text_size(px(theme::T_UI_SM))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme::bone_dim())
                    .child(format!("Compacted · {body}")),
            )
            .into_any_element(),
        StreamEntry::System { title, body } => div()
            .w_full()
            .px(px(2.0))
            .py(px(4.0))
            .child(
                div()
                    .font_family(theme::SANS)
                    .text_size(px(theme::T_UI_SM))
                    .text_color(theme::smoke())
                    .child(format!("{title} · {body}")),
            )
            .into_any_element(),
        StreamEntry::Thinking { body, level } => {
            thinking_step(body, level, true).into_any_element()
        }
        StreamEntry::Tool {
            name,
            body,
            summary,
        } => tool_step(name, body, summary, true).into_any_element(),
        StreamEntry::Assistant { .. } => turn_reply(entry).into_any_element(),
        StreamEntry::User { .. } => div().into_any_element(),
    }
}

fn turn_card(
    index: usize,
    user: &StreamEntry,
    activity: &[&StreamEntry],
    reply: Option<&StreamEntry>,
) -> impl IntoElement {
    // Full-width turn: prompt band → work band → answer band.
    div()
        .w_full()
        .flex()
        .flex_col()
        .rounded(px(theme::RADIUS))
        .overflow_hidden()
        .bg(theme::floor())
        .border_1()
        .border_color(theme::edge_soft())
        .child(turn_prompt(index, user))
        .when(!activity.is_empty(), |el| {
            el.child(
                div()
                    .w_full()
                    .px(px(18.0))
                    .pt(px(12.0))
                    .pb(if reply.is_some() { px(4.0) } else { px(14.0) })
                    .bg(theme::floor())
                    .flex()
                    .flex_col()
                    .children(
                        activity
                            .iter()
                            .enumerate()
                            .map(|(i, entry)| activity_step(entry, i + 1 == activity.len())),
                    ),
            )
        })
        .when_some(reply, |el, entry| el.child(turn_reply(entry)))
}

fn turn_prompt(index: usize, user: &StreamEntry) -> impl IntoElement {
    let StreamEntry::User { body, timestamp } = user else {
        return div();
    };

    div()
        .w_full()
        .px(px(18.0))
        .pt(px(14.0))
        .pb(px(14.0))
        .bg(theme::panel())
        .border_b_1()
        .border_color(theme::edge_soft())
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .w_full()
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .gap(px(12.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_baseline()
                        .gap(px(8.0))
                        .child(
                            div()
                                .font_family(theme::MONO)
                                .text_size(px(theme::T_TINY))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme::signal())
                                .child(format!("{index:02}")),
                        )
                        .child(
                            div()
                                .font_family(theme::SANS)
                                .text_size(px(theme::T_UI_SM))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::ash())
                                .child("You"),
                        ),
                )
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(*timestamp),
                ),
        )
        .child(
            div()
                .w_full()
                .font_family(theme::SANS)
                .text_size(px(theme::T_BODY))
                .font_weight(FontWeight::MEDIUM)
                .line_height(relative(1.55))
                .text_color(theme::bone())
                .child(*body),
        )
}

fn turn_reply(entry: &StreamEntry) -> impl IntoElement {
    let StreamEntry::Assistant { body, timestamp } = entry else {
        return div();
    };

    div()
        .w_full()
        .px(px(18.0))
        .pt(px(14.0))
        .pb(px(16.0))
        .bg(theme::canvas())
        .border_t_1()
        .border_color(theme::edge_soft())
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .child(
                    div()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_UI_SM))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::ash())
                        .child("Pi"),
                )
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(theme::T_TINY))
                        .text_color(theme::smoke())
                        .child(*timestamp),
                ),
        )
        .children(body.split("\n\n").map(|para| {
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .children(para.lines().map(|line| {
                    div()
                        .w_full()
                        .font_family(theme::SANS)
                        .text_size(px(theme::T_BODY_SM))
                        .font_weight(FontWeight::NORMAL)
                        .line_height(relative(1.6))
                        .text_color(theme::bone())
                        .child(line.to_string())
                }))
                .into_any_element()
        }))
}

fn activity_step(entry: &StreamEntry, is_last: bool) -> AnyElement {
    match entry {
        StreamEntry::Thinking { body, level } => {
            thinking_step(body, level, is_last).into_any_element()
        }
        StreamEntry::Tool {
            name,
            body,
            summary,
        } => tool_step(name, body, summary, is_last).into_any_element(),
        StreamEntry::Compaction { body } => compaction_step(body, is_last).into_any_element(),
        StreamEntry::System { body, .. } => system_step(body, is_last).into_any_element(),
        StreamEntry::Assistant { .. } | StreamEntry::User { .. } => div().into_any_element(),
    }
}

fn step_shell(is_last: bool, marker: gpui::Rgba, body: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .gap(px(11.0))
        .child(
            div()
                .w(px(10.0))
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .mt(px(6.0))
                        .w(px(5.0))
                        .h(px(5.0))
                        .rounded_full()
                        .bg(marker)
                        .flex_shrink_0(),
                )
                .when(!is_last, |el| {
                    el.child(
                        div()
                            .flex_1()
                            .w(px(1.0))
                            .min_h(px(8.0))
                            .mt(px(3.0))
                            .bg(theme::edge()),
                    )
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pb(if is_last { px(6.0) } else { px(12.0) })
                .child(body),
        )
}

fn thinking_step(body: &'static str, level: &'static str, is_last: bool) -> impl IntoElement {
    // Quiet internal monologue: no chrome, readable dim prose.
    step_shell(
        is_last,
        theme::smoke(),
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_baseline()
                    .gap(px(8.0))
                    .child(
                        div()
                            .font_family(theme::MONO)
                            .text_size(px(theme::T_TINY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme::smoke())
                            .child("thinking"),
                    )
                    .when(!level.is_empty(), |el| {
                        el.child(
                            div()
                                .font_family(theme::MONO)
                                .text_size(px(theme::T_TINY))
                                .text_color(theme::smoke())
                                .child(level),
                        )
                    }),
            )
            .child(
                div()
                    .font_family(theme::SANS)
                    .text_size(px(theme::T_UI))
                    .line_height(relative(1.58))
                    .text_color(theme::bone_dim())
                    .child(body),
            ),
    )
}

fn tool_step(
    name: &'static str,
    body: &'static str,
    summary: &'static str,
    is_last: bool,
) -> impl IntoElement {
    let running = summary.eq_ignore_ascii_case("running") || summary.eq_ignore_ascii_case("agent");
    let marker = if running {
        theme::data()
    } else {
        theme::live()
    };
    let summary = if summary.is_empty() { "done" } else { summary };
    let lines: Vec<&str> = body
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .collect();

    // Compact instrument row + bare mono body. No nested bordered cards.
    step_shell(
        is_last,
        marker,
        div()
            .flex()
            .flex_col()
            .gap(px(5.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_baseline()
                    .justify_between()
                    .gap(px(12.0))
                    .child(
                        div()
                            .font_family(theme::MONO)
                            .text_size(px(theme::T_MONO))
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::bone())
                            .child(name),
                    )
                    .child(
                        div()
                            .font_family(theme::MONO)
                            .text_size(px(theme::T_TINY))
                            .text_color(if running {
                                theme::data()
                            } else {
                                theme::smoke()
                            })
                            .child(summary),
                    ),
            )
            .when(!lines.is_empty(), |el| {
                el.child(
                    div()
                        .mt(px(2.0))
                        .px(px(10.0))
                        .py(px(8.0))
                        .rounded(px(theme::RADIUS_SM))
                        .bg(theme::canvas())
                        .border_1()
                        .border_color(theme::edge_soft())
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .children(lines.into_iter().map(tool_body_line)),
                )
            }),
    )
}

fn tool_body_line(line: &str) -> AnyElement {
    let (color, weight) = tool_line_style(line);
    div()
        .font_family(theme::MONO)
        .text_size(px(theme::T_MONO_SM))
        .font_weight(weight)
        .line_height(relative(1.48))
        .text_color(color)
        .child(line.to_string())
        .into_any_element()
}

fn tool_line_style(line: &str) -> (gpui::Rgba, FontWeight) {
    let trimmed = line.trim_start();
    if trimmed.starts_with('+') && !trimmed.starts_with("+++") {
        (theme::live(), FontWeight::MEDIUM)
    } else if trimmed.starts_with('-') && !trimmed.starts_with("---") {
        (theme::signal(), FontWeight::MEDIUM)
    } else if trimmed.starts_with("@@") {
        (theme::data(), FontWeight::MEDIUM)
    } else {
        (theme::ash(), FontWeight::NORMAL)
    }
}

fn compaction_step(body: &'static str, is_last: bool) -> impl IntoElement {
    step_shell(
        is_last,
        theme::data(),
        div()
            .font_family(theme::SANS)
            .text_size(px(theme::T_UI_SM))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::bone_dim())
            .child(format!("Compacted · {body}")),
    )
}

fn system_step(body: &'static str, is_last: bool) -> impl IntoElement {
    step_shell(
        is_last,
        theme::smoke(),
        div()
            .font_family(theme::SANS)
            .text_size(px(theme::T_UI_SM))
            .text_color(theme::smoke())
            .child(body),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_segments_preamble_and_turns() {
        let entries = [
            StreamEntry::System {
                title: "Ready",
                body: "Context loaded",
            },
            StreamEntry::User {
                body: "First prompt",
                timestamp: "12:00",
            },
            StreamEntry::Thinking {
                body: "Inspect the state",
                level: "low",
            },
            StreamEntry::Assistant {
                body: "First reply",
                timestamp: "12:01",
            },
            StreamEntry::User {
                body: "Second prompt",
                timestamp: "12:02",
            },
        ];

        let segments = segment_stream(&entries);
        assert_eq!(segments.len(), 3);
        assert!(matches!(segments[0], StreamSegment::Preamble(_)));
        assert!(matches!(
            &segments[1],
            StreamSegment::Turn {
                index: 1,
                activity,
                reply: Some(_),
                ..
            } if activity.len() == 1
        ));
        assert!(matches!(
            &segments[2],
            StreamSegment::Turn {
                index: 2,
                activity,
                reply: None,
                ..
            } if activity.is_empty()
        ));
    }
}
