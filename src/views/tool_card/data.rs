use std::collections::HashSet;

use serde_json::Value;

use super::{CardImage, CardStatus, ToolPayload};
use crate::controller::ConversationProjection;
use crate::services::rpc::{RequestId, ToolCallId};
use crate::state::runtime::{
    BashExecution, BashStatus, MessageBlock, ToolImage, ToolStatus, sanitize_untrusted_text,
};

pub(super) fn payload_from_value(value: &Value) -> ToolPayload {
    let mut payload = ToolPayload::default();
    collect_payload(value, &mut payload);
    if payload.text.is_empty()
        && payload.diff.is_none()
        && payload.images.is_empty()
        && payload.details.is_none()
        && !value.is_null()
    {
        payload.details = Some(value.clone());
    }
    payload
}

fn collect_payload(value: &Value, payload: &mut ToolPayload) {
    match value {
        Value::Null => {}
        Value::String(text) => push_text(&mut payload.text, text),
        Value::Array(values) => {
            for value in values {
                collect_payload(value, payload);
            }
        }
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("image") {
                if let (Some(data), Some(mime_type)) = (
                    object.get("data").and_then(Value::as_str),
                    object.get("mimeType").and_then(Value::as_str),
                ) {
                    payload.images.push(CardImage {
                        data: data.to_owned(),
                        mime_type: sanitize_untrusted_text(mime_type),
                    });
                    return;
                }
            }
            if object.get("type").and_then(Value::as_str) == Some("text")
                && let Some(text) = object.get("text").and_then(Value::as_str)
            {
                push_text(&mut payload.text, text);
                return;
            }
            if let Some(content) = object.get("content") {
                collect_payload(content, payload);
            }
            for key in ["text", "output", "stdout", "message"] {
                if let Some(text) = object.get(key).and_then(Value::as_str) {
                    push_text(&mut payload.text, text);
                }
            }
            for key in ["diff", "patch"] {
                if payload.diff.is_none()
                    && let Some(diff) = object.get(key).and_then(Value::as_str)
                {
                    payload.diff = Some(sanitize_untrusted_text(diff));
                }
            }
            if let Some(details) = object.get("details")
                && !details.is_null()
            {
                if payload.diff.is_none() {
                    payload.diff =
                        find_string_field(details, &["diff", "patch"]).map(sanitize_untrusted_text);
                }
                payload.full_output_path = payload.full_output_path.take().or_else(|| {
                    find_string_field(details, &["fullOutputPath"]).map(ToOwned::to_owned)
                });
                payload.truncated |= find_bool_field(details, "truncated").unwrap_or(false);
                payload.details = Some(details.clone());
            }
            payload.truncated |= object
                .get("truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            payload.full_output_path = object
                .get("fullOutputPath")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| payload.full_output_path.take());
            let original = object
                .get("originalBytes")
                .or_else(|| object.get("totalBytes"))
                .or_else(|| object.get("originalLength"))
                .and_then(Value::as_u64);
            let shown = object
                .get("shownBytes")
                .or_else(|| object.get("outputBytes"))
                .and_then(Value::as_u64);
            if let Some(original) = original {
                payload.truncation_note = Some(shown.map_or_else(
                    || format!("Pi truncated an original {original}-byte result."),
                    |shown| format!("Pi returned {shown} of {original} bytes."),
                ));
            }
        }
        Value::Bool(_) | Value::Number(_) => {
            payload.details = Some(value.clone());
        }
    }
}

fn find_string_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    match value {
        Value::Object(object) => object
            .iter()
            .find_map(|(key, value)| {
                keys.contains(&key.as_str())
                    .then(|| value.as_str())
                    .flatten()
            })
            .or_else(|| {
                object
                    .values()
                    .find_map(|value| find_string_field(value, keys))
            }),
        Value::Array(values) => values
            .iter()
            .find_map(|value| find_string_field(value, keys)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

fn find_bool_field(value: &Value, key: &str) -> Option<bool> {
    match value {
        Value::Object(object) => object.get(key).and_then(Value::as_bool).or_else(|| {
            object
                .values()
                .find_map(|value| find_bool_field(value, key))
        }),
        Value::Array(values) => values.iter().find_map(|value| find_bool_field(value, key)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

fn push_text(target: &mut String, text: &str) {
    if !target.is_empty() {
        target.push('\n');
    }
    target.push_str(text);
}

fn payload_from_persisted(
    content: &str,
    images: &[ToolImage],
    details: Option<&Value>,
) -> ToolPayload {
    let mut payload = details.map(payload_from_value).unwrap_or_default();
    let content = content.to_owned();
    if !content.is_empty() {
        if payload.text.is_empty() {
            payload.text = content;
        } else {
            payload.text = format!("{content}\n{}", payload.text);
        }
    }
    payload.images.extend(images.iter().map(|image| CardImage {
        data: image.data.clone(),
        mime_type: sanitize_untrusted_text(&image.mime_type),
    }));
    if let Some(details) = details {
        payload.details = Some(details.clone());
    }
    payload
}

fn bash_body(command: &str, output: &str) -> String {
    if output.is_empty() {
        format!("$ {command}")
    } else {
        format!("$ {command}\n{output}")
    }
}

pub(crate) fn tool_key(id: &ToolCallId) -> String {
    format!("tool:{}", id.as_str())
}

/// One path/command row under a compact tool header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolTreeRow {
    pub label: String,
    pub detail: Option<String>,
}

/// Compact, non-interactive tool presentation for the conversation spine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPresentation {
    pub name: String,
    pub status: CardStatus,
    pub rows: Vec<ToolTreeRow>,
    pub call_id: Option<String>,
    pub arguments: Option<Value>,
    pub payload: ToolPayload,
    pub elapsed_ms: Option<u128>,
    pub context_excluded: bool,
}

impl ToolPresentation {
    pub fn groupable(&self) -> bool {
        matches!(
            self.name.as_str(),
            "read" | "edit" | "write" | "grep" | "find" | "ls"
        )
    }

    pub fn title(&self, group_count: usize) -> String {
        let name = sanitize_untrusted_text(&self.name);
        if group_count <= 1 {
            return name;
        }
        match name.as_str() {
            "read" => format!("read({group_count} files)"),
            "edit" => format!("edit({group_count} files)"),
            "write" => format!("write({group_count} files)"),
            "grep" => format!("grep({group_count})"),
            "find" => format!("find({group_count})"),
            "ls" => format!("ls({group_count})"),
            other => format!("{other}({group_count})"),
        }
    }
}

pub(crate) fn presentation_for_tool_call(
    projection: &ConversationProjection,
    id: &ToolCallId,
    name: &str,
    arguments: &Value,
    persisted: Option<(&str, &[ToolImage], Option<&Value>, bool)>,
) -> ToolPresentation {
    let live = projection.tools.get(id);
    let persisted = persisted.map(|(content, images, details, is_error)| {
        (payload_from_persisted(content, images, details), is_error)
    });
    let status = live.map_or_else(
        || {
            persisted
                .as_ref()
                .map_or(CardStatus::Pending, |(_, is_error)| {
                    if *is_error {
                        CardStatus::Error
                    } else {
                        CardStatus::Success
                    }
                })
        },
        |tool| match tool.status {
            ToolStatus::Pending => CardStatus::Pending,
            ToolStatus::Running => CardStatus::Running,
            ToolStatus::Succeeded => CardStatus::Success,
            ToolStatus::Failed => CardStatus::Error,
            ToolStatus::Cancelled => CardStatus::Cancelled,
            ToolStatus::Uncertain => CardStatus::Uncertain,
        },
    );
    let payload = live
        .and_then(|tool| tool.result.as_ref())
        .map(payload_from_value)
        .or_else(|| persisted.map(|(payload, _)| payload))
        .unwrap_or_default();
    let mut presentation = presentation(name, Some(arguments), &payload, status);
    presentation.call_id = Some(id.as_str().to_owned());
    presentation.elapsed_ms = live.map(|tool| tool.elapsed_ms());
    presentation
}

pub(crate) fn presentation_for_standalone_result(
    id: &ToolCallId,
    name: &str,
    content: &str,
    images: &[ToolImage],
    details: Option<&Value>,
    is_error: bool,
) -> ToolPresentation {
    let payload = payload_from_persisted(content, images, details);
    let mut presentation = presentation(
        name,
        None,
        &payload,
        if is_error {
            CardStatus::Error
        } else {
            CardStatus::Success
        },
    );
    presentation.call_id = Some(id.as_str().to_owned());
    presentation
}

pub(crate) fn presentation_for_bash_block(
    command: &str,
    output: &str,
    cancelled: bool,
    exit_code: Option<i32>,
    context_excluded: bool,
) -> ToolPresentation {
    let status = if cancelled {
        CardStatus::Cancelled
    } else if exit_code.is_some_and(|code| code != 0) {
        CardStatus::Error
    } else {
        CardStatus::Success
    };
    let payload = ToolPayload {
        text: bash_body(command, output),
        ..ToolPayload::default()
    };
    let mut presentation = presentation(
        "bash",
        Some(&serde_json::json!({ "command": command })),
        &payload,
        status,
    );
    presentation.context_excluded = context_excluded;
    presentation
}

pub(crate) fn presentation_for_local_bash(execution: &BashExecution) -> ToolPresentation {
    let status = match execution.status {
        BashStatus::Running => CardStatus::Running,
        BashStatus::Cancelling => CardStatus::Cancelling,
        BashStatus::Succeeded => CardStatus::Success,
        BashStatus::Failed => CardStatus::Error,
        BashStatus::Cancelled => CardStatus::Cancelled,
        BashStatus::Uncertain => CardStatus::Uncertain,
    };
    let payload = ToolPayload {
        text: bash_body(&execution.command, &execution.output),
        truncated: execution.truncated,
        full_output_path: execution.full_output_path.clone(),
        ..ToolPayload::default()
    };
    let mut presentation = presentation(
        "bash",
        Some(&serde_json::json!({ "command": execution.command })),
        &payload,
        status,
    );
    presentation.call_id = Some(execution.request.as_str().to_owned());
    presentation.elapsed_ms = Some(execution.elapsed_ms());
    presentation.context_excluded = execution.exclude_from_context;
    presentation
}

fn presentation(
    name: &str,
    arguments: Option<&Value>,
    payload: &ToolPayload,
    status: CardStatus,
) -> ToolPresentation {
    let name = name.trim();
    let rows = match name {
        "bash" => bash_rows(arguments, payload),
        "read" | "edit" | "write" => path_rows(arguments, payload, name),
        "grep" => grep_rows(arguments),
        "find" | "ls" => path_or_pattern_rows(arguments),
        _ => generic_rows(arguments, payload),
    };
    ToolPresentation {
        name: name.to_owned(),
        status,
        rows,
        call_id: None,
        arguments: arguments.cloned(),
        payload: payload.clone(),
        elapsed_ms: None,
        context_excluded: false,
    }
}

fn bash_rows(arguments: Option<&Value>, payload: &ToolPayload) -> Vec<ToolTreeRow> {
    if let Some(command) = string_arg(arguments, &["command"]) {
        return vec![ToolTreeRow {
            label: sanitize_untrusted_text(&format!("$ {command}")),
            detail: None,
        }];
    }
    payload
        .text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| {
            vec![ToolTreeRow {
                label: sanitize_untrusted_text(line),
                detail: None,
            }]
        })
        .unwrap_or_default()
}

fn path_rows(arguments: Option<&Value>, payload: &ToolPayload, name: &str) -> Vec<ToolTreeRow> {
    let path = string_arg(arguments, &["path", "file", "filePath", "file_path"])
        .map(|path| display_path(&path))
        .unwrap_or_else(|| name.to_owned());
    let detail = match name {
        "read" => read_line_detail(payload, arguments),
        "edit" | "write" => payload
            .diff
            .as_deref()
            .and_then(diff_stat)
            .or_else(|| read_line_detail(payload, arguments)),
        _ => None,
    };
    vec![ToolTreeRow {
        label: path,
        detail,
    }]
}

fn grep_rows(arguments: Option<&Value>) -> Vec<ToolTreeRow> {
    let pattern = string_arg(arguments, &["pattern", "query", "regex"]).unwrap_or_default();
    let path = string_arg(arguments, &["path", "glob", "include", "cwd"]);
    let label = match (pattern.is_empty(), path.as_deref()) {
        (false, Some(path)) => format!("{pattern} · {}", display_path(path)),
        (false, None) => pattern,
        (true, Some(path)) => display_path(path),
        (true, None) => "grep".to_owned(),
    };
    vec![ToolTreeRow {
        label: sanitize_untrusted_text(&label),
        detail: None,
    }]
}

fn path_or_pattern_rows(arguments: Option<&Value>) -> Vec<ToolTreeRow> {
    let label = string_arg(arguments, &["path", "pattern", "glob", "query", "cwd"])
        .map(|value| display_path(&value))
        .unwrap_or_else(|| "…".to_owned());
    vec![ToolTreeRow {
        label: sanitize_untrusted_text(&label),
        detail: None,
    }]
}

fn generic_rows(arguments: Option<&Value>, payload: &ToolPayload) -> Vec<ToolTreeRow> {
    if let Some(path) = string_arg(arguments, &["path", "file", "filePath", "file_path"]) {
        return vec![ToolTreeRow {
            label: display_path(&path),
            detail: read_line_detail(payload, arguments),
        }];
    }
    if let Some(command) = string_arg(arguments, &["command"]) {
        return vec![ToolTreeRow {
            label: sanitize_untrusted_text(&format!("$ {command}")),
            detail: None,
        }];
    }
    payload
        .text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| {
            let mut label = sanitize_untrusted_text(line);
            if label.len() > 120 {
                let mut end = 120;
                while !label.is_char_boundary(end) {
                    end = end.saturating_sub(1);
                }
                label.truncate(end);
                label.push('…');
            }
            vec![ToolTreeRow {
                label,
                detail: None,
            }]
        })
        .unwrap_or_default()
}

fn read_line_detail(payload: &ToolPayload, arguments: Option<&Value>) -> Option<String> {
    let lines = payload.text.lines().filter(|line| !line.is_empty()).count();
    if lines > 0 {
        return Some(lines.to_string());
    }
    let offset = arguments
        .and_then(|value| value.get("offset"))
        .and_then(Value::as_u64);
    let limit = arguments
        .and_then(|value| value.get("limit"))
        .and_then(Value::as_u64);
    match (offset, limit) {
        (Some(offset), Some(limit)) => Some(format!("{offset}-{}", offset.saturating_add(limit))),
        (Some(offset), None) => Some(format!("{offset}+")),
        (None, Some(limit)) => Some(format!("{limit} lines")),
        (None, None) => None,
    }
}

fn diff_stat(diff: &str) -> Option<String> {
    let mut plus = 0u32;
    let mut minus = 0u32;
    for line in diff.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('+') && !trimmed.starts_with("+++") {
            plus = plus.saturating_add(1);
        } else if trimmed.starts_with('-') && !trimmed.starts_with("---") {
            minus = minus.saturating_add(1);
        }
    }
    if plus == 0 && minus == 0 {
        None
    } else {
        Some(format!("+{plus} -{minus}"))
    }
}

fn string_arg(arguments: Option<&Value>, keys: &[&str]) -> Option<String> {
    let object = arguments?.as_object()?;
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn display_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let trimmed = normalized.trim();
    let parts = trimmed
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let label = if parts.len() <= 3 {
        parts.join("/")
    } else {
        format!("…/{}/{}", parts[parts.len() - 2], parts[parts.len() - 1])
    };
    sanitize_untrusted_text(&label)
}

fn local_bash_key(request: &RequestId) -> String {
    format!("bash-local:{}", request.as_str())
}

pub(crate) fn tail_card_keys(projection: &ConversationProjection) -> Vec<String> {
    let calls = projection
        .messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            MessageBlock::ToolCall { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let mut tools = projection
        .tools
        .values()
        .filter(|tool| !calls.contains(&tool.id))
        .collect::<Vec<_>>();
    tools.sort_by_key(|tool| tool.sequence);
    let mut keys = tools
        .into_iter()
        .map(|tool| tool_key(&tool.id))
        .collect::<Vec<_>>();
    keys.extend(
        projection
            .bash_executions
            .iter()
            .filter(|execution| !execution.reconciled)
            .map(|execution| local_bash_key(&execution.request)),
    );
    keys
}

pub(crate) fn tail_presentations(projection: &ConversationProjection) -> Vec<ToolPresentation> {
    let keys = tail_card_keys(projection);
    let mut out = Vec::new();
    for tool in projection.tools.values() {
        let key = tool_key(&tool.id);
        if keys.iter().any(|candidate| candidate == &key) {
            out.push(presentation_for_tool_call(
                projection,
                &tool.id,
                &tool.name,
                &tool.arguments,
                None,
            ));
        }
    }
    out.extend(
        projection
            .bash_executions
            .iter()
            .filter(|execution| !execution.reconciled)
            .map(presentation_for_local_bash),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_payload_keeps_text_images_diff_details_and_truncation() {
        let payload = payload_from_value(&serde_json::json!({
            "content": [
                {"type": "text", "text": "done"},
                {"type": "image", "data": "AA==", "mimeType": "image/png"}
            ],
            "diff": "@@ -1 +1 @@\n-old\n+new",
            "details": {"opaque": [1, 2]},
            "truncated": true,
            "totalBytes": 100,
            "shownBytes": 40,
            "fullOutputPath": "C:/tmp/full.txt"
        }));
        assert_eq!(payload.text, "done");
        assert_eq!(payload.images.len(), 1);
        assert!(payload.diff.as_deref().unwrap().contains("+new"));
        assert_eq!(
            payload
                .details
                .as_ref()
                .and_then(|details| details.get("opaque")),
            Some(&serde_json::json!([1, 2]))
        );
        assert!(payload.truncated);
        assert_eq!(payload.full_output_path.as_deref(), Some("C:/tmp/full.txt"));
        assert_eq!(
            payload.truncation_note.as_deref(),
            Some("Pi returned 40 of 100 bytes.")
        );
    }

    #[test]
    fn read_presentation_builds_tree_rows_and_group_titles() {
        let payload = ToolPayload {
            text: "line1\nline2\nline3\n".to_owned(),
            ..ToolPayload::default()
        };
        let one = presentation(
            "read",
            Some(&serde_json::json!({"path": "src/views/tool_card.rs"})),
            &payload,
            CardStatus::Success,
        );
        assert_eq!(one.title(1), "read");
        assert_eq!(one.title(2), "read(2 files)");
        assert_eq!(one.rows.len(), 1);
        assert_eq!(one.rows[0].label, "src/views/tool_card.rs");
        assert_eq!(one.rows[0].detail.as_deref(), Some("3"));

        let bash = presentation(
            "bash",
            Some(&serde_json::json!({"command": "cargo test"})),
            &ToolPayload::default(),
            CardStatus::Running,
        );
        assert_eq!(bash.rows[0].label, "$ cargo test");
        assert!(!bash.groupable());
        assert!(one.groupable());
    }
}
