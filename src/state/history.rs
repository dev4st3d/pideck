//! UI-independent session tree browsing and keyboard selection.

use std::collections::HashSet;

use crate::services::rpc::EntryId;

use super::runtime::{EntryKind, MessageBlock, MessageRole, RuntimeEntry, RuntimeTreeNode};

/// One visible row of the flattened session tree; carries only what keyboard
/// navigation and the selection need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
    pub id: EntryId,
    pub active_leaf: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntryDetails {
    pub kind: String,
    pub title: String,
    pub body: String,
    pub label: Option<String>,
    pub child_count: usize,
    pub active_leaf: bool,
}

#[derive(Debug, Clone, Default)]
pub struct HistoryBrowser {
    selected: Option<EntryId>,
    collapsed: HashSet<EntryId>,
}

impl HistoryBrowser {
    pub fn selected(&self) -> Option<&EntryId> {
        self.selected.as_ref()
    }

    pub fn synchronize(&mut self, tree: &[RuntimeTreeNode], leaf: Option<&EntryId>) {
        if self
            .selected
            .as_ref()
            .is_some_and(|selected| find_node(tree, selected).is_some())
        {
            return;
        }
        self.selected = leaf
            .filter(|leaf| find_node(tree, leaf).is_some())
            .cloned()
            .or_else(|| tree.first().map(|node| node.entry.id.clone()));
    }

    pub fn rows(&self, tree: &[RuntimeTreeNode], leaf: Option<&EntryId>) -> Vec<HistoryRow> {
        let mut flattened = Vec::new();
        flatten(tree, &mut Vec::new(), &mut flattened);
        flattened
            .into_iter()
            .filter(|node| {
                !node
                    .ancestors
                    .iter()
                    .any(|ancestor| self.collapsed.contains(ancestor))
            })
            .map(|node| HistoryRow {
                id: node.node.entry.id.clone(),
                active_leaf: leaf == Some(&node.node.entry.id),
            })
            .collect()
    }

    pub fn details(
        &self,
        tree: &[RuntimeTreeNode],
        leaf: Option<&EntryId>,
    ) -> Option<HistoryEntryDetails> {
        let selected = self.selected.as_ref()?;
        let node = find_node(tree, selected)?;
        let (title, _, body) = entry_copy(&node.entry);
        Some(HistoryEntryDetails {
            kind: entry_kind_name(&node.entry.kind).to_owned(),
            title,
            body,
            label: node.label.clone(),
            child_count: node.children.len(),
            active_leaf: leaf == Some(&node.entry.id),
        })
    }

    pub fn move_next(&mut self, rows: &[HistoryRow]) -> bool {
        self.move_by(rows, 1)
    }

    pub fn move_previous(&mut self, rows: &[HistoryRow]) -> bool {
        self.move_by(rows, -1)
    }

    pub fn move_first(&mut self, rows: &[HistoryRow]) -> bool {
        self.select_row(rows.first())
    }

    pub fn move_last(&mut self, rows: &[HistoryRow]) -> bool {
        self.select_row(rows.last())
    }

    pub fn fold_or_parent(&mut self, tree: &[RuntimeTreeNode]) -> bool {
        let Some(selected) = self.selected.clone() else {
            return false;
        };
        let Some(node) = find_node(tree, &selected) else {
            return false;
        };
        if !node.children.is_empty() && self.collapsed.insert(selected.clone()) {
            return true;
        }
        let Some(parent) = node.entry.parent_id.clone() else {
            return false;
        };
        self.selected = Some(parent);
        true
    }

    pub fn unfold_or_child(&mut self, tree: &[RuntimeTreeNode]) -> bool {
        let Some(selected) = self.selected.clone() else {
            return false;
        };
        let Some(node) = find_node(tree, &selected) else {
            return false;
        };
        if self.collapsed.remove(&selected) {
            return true;
        }
        let Some(child) = node.children.first() else {
            return false;
        };
        self.selected = Some(child.entry.id.clone());
        true
    }

    fn move_by(&mut self, rows: &[HistoryRow], delta: isize) -> bool {
        if rows.is_empty() {
            return false;
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|selected| rows.iter().position(|row| &row.id == selected))
            .unwrap_or(0);
        let next = current
            .saturating_add_signed(delta)
            .min(rows.len().saturating_sub(1));
        if next == current && self.selected.is_some() {
            return false;
        }
        self.selected = Some(rows[next].id.clone());
        true
    }

    fn select_row(&mut self, row: Option<&HistoryRow>) -> bool {
        let Some(row) = row else {
            return false;
        };
        if self.selected.as_ref() == Some(&row.id) {
            return false;
        }
        self.selected = Some(row.id.clone());
        true
    }
}

struct FlatNode<'a> {
    node: &'a RuntimeTreeNode,
    ancestors: Vec<EntryId>,
}

fn flatten<'a>(
    nodes: &'a [RuntimeTreeNode],
    ancestors: &mut Vec<EntryId>,
    output: &mut Vec<FlatNode<'a>>,
) {
    for node in nodes {
        output.push(FlatNode {
            node,
            ancestors: ancestors.clone(),
        });
        ancestors.push(node.entry.id.clone());
        flatten(&node.children, ancestors, output);
        ancestors.pop();
    }
}

fn find_node<'a>(nodes: &'a [RuntimeTreeNode], id: &EntryId) -> Option<&'a RuntimeTreeNode> {
    for node in nodes {
        if &node.entry.id == id {
            return Some(node);
        }
        if let Some(found) = find_node(&node.children, id) {
            return Some(found);
        }
    }
    None
}

fn entry_copy(entry: &RuntimeEntry) -> (String, String, String) {
    match &entry.kind {
        EntryKind::Message(message) => {
            let role = match message.role {
                MessageRole::User => "User",
                MessageRole::Assistant => "Assistant",
                MessageRole::ToolResult => "Tool result",
                MessageRole::BashExecution => "Bash",
                MessageRole::Custom => "Custom message",
                MessageRole::BranchSummary => "Branch summary",
                MessageRole::CompactionSummary => "Compaction summary",
                MessageRole::Unknown => "Message",
            };
            let body = message
                .content
                .iter()
                .filter_map(|block| match block {
                    MessageBlock::Text { text, .. }
                    | MessageBlock::Thinking { text, .. }
                    | MessageBlock::Summary { text, .. }
                    | MessageBlock::Custom { text, .. } => Some(text.as_str()),
                    MessageBlock::Bash {
                        command, output, ..
                    } => Some(if output.is_empty() { command } else { output }),
                    MessageBlock::File { metadata, .. } => Some(metadata.name.as_str()),
                    MessageBlock::Image { .. }
                    | MessageBlock::ToolCall { .. }
                    | MessageBlock::ToolResult { .. }
                    | MessageBlock::Unsupported { .. } => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            (role.to_owned(), excerpt(&body), body)
        }
        EntryKind::ThinkingLevel(level) => (
            "Thinking level".to_owned(),
            level.clone(),
            format!("Thinking changed to {level}"),
        ),
        EntryKind::Model { provider, model_id } => (
            "Model change".to_owned(),
            format!("{provider}/{model_id}"),
            format!("Provider: {provider}\nModel: {model_id}"),
        ),
        EntryKind::Compaction { summary } => {
            ("Compaction".to_owned(), excerpt(summary), summary.clone())
        }
        EntryKind::BranchSummary { summary } => (
            "Branch summary".to_owned(),
            excerpt(summary),
            summary.clone(),
        ),
        EntryKind::Custom { kind } => ("Custom entry".to_owned(), kind.clone(), kind.clone()),
        EntryKind::CustomMessage { kind, content, .. } => {
            let body = content
                .iter()
                .filter_map(|block| match block {
                    MessageBlock::Text { text, .. } | MessageBlock::Custom { text, .. } => {
                        Some(text.as_str())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            ("Custom message".to_owned(), kind.clone(), body)
        }
        EntryKind::Label { target, label } => (
            "Label change".to_owned(),
            label.clone().unwrap_or_else(|| "Cleared".to_owned()),
            format!("Target: {target}"),
        ),
        EntryKind::SessionInfo { name } => (
            "Session info".to_owned(),
            name.clone().unwrap_or_else(|| "Unnamed".to_owned()),
            name.clone().unwrap_or_default(),
        ),
        EntryKind::Unknown { entry_type } => (
            "Unknown entry".to_owned(),
            entry_type.clone(),
            entry_type.clone(),
        ),
    }
}

fn entry_kind_name(kind: &EntryKind) -> &'static str {
    match kind {
        EntryKind::Message(_) => "Message",
        EntryKind::ThinkingLevel(_) => "Thinking level",
        EntryKind::Model { .. } => "Model change",
        EntryKind::Compaction { .. } => "Compaction",
        EntryKind::BranchSummary { .. } => "Branch summary",
        EntryKind::Custom { .. } => "Custom entry",
        EntryKind::CustomMessage { .. } => "Custom message",
        EntryKind::Label { .. } => "Label change",
        EntryKind::SessionInfo { .. } => "Session info",
        EntryKind::Unknown { .. } => "Unknown",
    }
}

fn excerpt(value: &str) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = normalized.chars();
    let prefix = chars.by_ref().take(96).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else if prefix.is_empty() {
        "No text".to_owned()
    } else {
        prefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::runtime::{BlockKey, MessageKey, RuntimeMessage};

    fn entry(id: &str, parent: Option<&str>, kind: EntryKind) -> RuntimeEntry {
        RuntimeEntry {
            id: EntryId::from(id),
            parent_id: parent.map(EntryId::from),
            timestamp: "2026-07-22T00:00:00Z".to_owned(),
            kind,
        }
    }

    fn message(id: &str, parent: Option<&str>, role: MessageRole, text: &str) -> RuntimeEntry {
        entry(
            id,
            parent,
            EntryKind::Message(Box::new(RuntimeMessage {
                key: MessageKey(id.to_owned()),
                role,
                timestamp: 1,
                content: vec![MessageBlock::Text {
                    key: BlockKey(format!("{id}:0")),
                    text: text.to_owned(),
                }],
                visible: true,
                terminal: true,
                stop_reason: None,
                error: None,
                assistant: None,
            })),
        )
    }

    fn synthetic_tree() -> Vec<RuntimeTreeNode> {
        vec![
            RuntimeTreeNode {
                entry: message("u1", None, MessageRole::User, "root prompt").into(),
                label: Some("start".to_owned()),
                children: vec![
                    RuntimeTreeNode {
                        entry: message("a1", Some("u1"), MessageRole::Assistant, "first answer")
                            .into(),
                        label: None,
                        children: vec![RuntimeTreeNode {
                            entry: entry(
                                "compact",
                                Some("a1"),
                                EntryKind::Compaction {
                                    summary: "kept context".to_owned(),
                                },
                            )
                            .into(),
                            label: None,
                            children: Vec::new(),
                        }],
                    },
                    RuntimeTreeNode {
                        entry: message("orphan", Some("missing"), MessageRole::User, "other path")
                            .into(),
                        label: None,
                        children: Vec::new(),
                    },
                ],
            },
            RuntimeTreeNode {
                entry: entry(
                    "summary-root",
                    Some("gone"),
                    EntryKind::BranchSummary {
                        summary: "orphaned branch context".to_owned(),
                    },
                )
                .into(),
                label: Some("review".to_owned()),
                children: Vec::new(),
            },
        ]
    }

    #[test]
    fn tree_shapes_orphans_folding_and_keyboard_are_stable() {
        let tree = synthetic_tree();
        let leaf = EntryId::from("compact");
        let mut browser = HistoryBrowser::default();
        browser.synchronize(&tree, Some(&leaf));
        assert_eq!(browser.selected(), Some(&leaf));
        let rows = browser.rows(&tree, Some(&leaf));
        assert_eq!(rows.len(), 5);
        assert!(rows.iter().find(|row| row.id == leaf).unwrap().active_leaf);
        assert!(browser.move_previous(&rows));
        assert_eq!(browser.selected(), Some(&EntryId::from("a1")));
        assert!(browser.fold_or_parent(&tree));
        let rows = browser.rows(&tree, Some(&leaf));
        assert!(rows.iter().all(|row| row.id != leaf));
        assert!(browser.unfold_or_child(&tree));
        assert_eq!(browser.rows(&tree, Some(&leaf)).len(), 5);
        assert!(browser.move_last(&rows));
        assert_eq!(browser.selected(), Some(&EntryId::from("summary-root")));
    }
}
