//! Project reminders as a preorder outline; no GPUI or I/O dependencies.

use serde::{Deserialize, Serialize};

const MAX_DEPTH: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Task {
    pub label: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub depth: usize,
    #[serde(default)]
    pub collapsed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Section {
    pub label: String,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub tasks: Vec<Task>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Checklist {
    pub sections: Vec<Section>,
}

impl Default for Checklist {
    fn default() -> Self {
        Self {
            sections: vec![Section::new("Workspace".into())],
        }
    }
}

impl Section {
    pub fn new(label: String) -> Self {
        Self {
            label,
            collapsed: false,
            tasks: Vec::new(),
        }
    }

    pub fn subtree_end(&self, index: usize) -> usize {
        let Some(task) = self.tasks.get(index) else {
            return self.tasks.len();
        };
        (index + 1..self.tasks.len())
            .find(|&next| self.tasks[next].depth <= task.depth)
            .unwrap_or(self.tasks.len())
    }

    pub fn visible(&self) -> Vec<usize> {
        if self.collapsed {
            return Vec::new();
        }
        let mut rows = Vec::new();
        let mut index = 0;
        while index < self.tasks.len() {
            rows.push(index);
            index = if self.tasks[index].collapsed {
                self.subtree_end(index)
            } else {
                index + 1
            };
        }
        rows
    }

    pub fn progress(&self, index: usize) -> (usize, usize) {
        let end = self.subtree_end(index);
        let leaves: Vec<_> = (index + 1..end)
            .filter(|&i| i + 1 == end || self.tasks[i + 1].depth <= self.tasks[i].depth)
            .collect();
        (
            leaves.iter().filter(|&&i| self.tasks[i].done).count(),
            leaves.len(),
        )
    }

    pub fn toggle(&mut self, index: usize) {
        let Some(task) = self.tasks.get(index) else {
            return;
        };
        let done = !task.done;
        let end = self.subtree_end(index);
        for task in &mut self.tasks[index..end] {
            task.done = done;
        }
        self.refresh_parents();
    }

    pub fn add(&mut self, label: String, parent: Option<usize>) -> usize {
        let (index, depth) = parent
            .filter(|&i| self.tasks.get(i).is_some_and(|t| t.depth < MAX_DEPTH))
            .map_or((self.tasks.len(), 0), |i| {
                self.tasks[i].collapsed = false;
                (self.subtree_end(i), self.tasks[i].depth + 1)
            });
        self.collapsed = false;
        self.tasks.insert(
            index,
            Task {
                label,
                done: false,
                depth,
                collapsed: false,
            },
        );
        self.refresh_parents();
        index
    }

    pub fn remove(&mut self, index: usize) {
        if index >= self.tasks.len() {
            return;
        }
        let end = self.subtree_end(index);
        self.tasks.drain(index..end);
        self.refresh_parents();
    }

    /// Reparent the entire subtree, keeping later siblings under their original parent.
    pub fn indent(&mut self, index: usize, outdent: bool) -> Option<usize> {
        let depth = self.tasks.get(index)?.depth;
        let end = self.subtree_end(index);
        if outdent {
            if depth == 0 {
                return None;
            }
            let parent = (0..index).rev().find(|&i| self.tasks[i].depth < depth)?;
            let destination = self.subtree_end(parent);
            let mut moved: Vec<_> = self.tasks.drain(index..end).collect();
            for task in &mut moved {
                task.depth -= 1;
            }
            let index = destination - moved.len();
            self.tasks.splice(index..index, moved);
            self.refresh_parents();
            Some(index)
        } else {
            let previous = (0..index).rev().find(|&i| self.tasks[i].depth <= depth)?;
            if self.tasks[previous].depth != depth
                || self.tasks[index..end].iter().any(|t| t.depth >= MAX_DEPTH)
            {
                return None;
            }
            self.tasks[previous].collapsed = false;
            for task in &mut self.tasks[index..end] {
                task.depth += 1;
            }
            self.refresh_parents();
            Some(index)
        }
    }

    pub fn normalize(&mut self) {
        let mut previous = 0;
        for (index, task) in self.tasks.iter_mut().enumerate() {
            task.depth = if index == 0 {
                0
            } else {
                task.depth.min(previous + 1).min(MAX_DEPTH)
            };
            previous = task.depth;
        }
        self.refresh_parents();
    }

    fn refresh_parents(&mut self) {
        for index in (0..self.tasks.len()).rev() {
            let end = self.subtree_end(index);
            if end > index + 1 {
                self.tasks[index].done = self.tasks[index + 1..end].iter().all(|t| t.done);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outline() -> Section {
        let mut s = Section::new("Synthetic".into());
        s.add("Parent".into(), None);
        s.add("First".into(), Some(0));
        s.add("Second".into(), Some(0));
        s.add("Sibling".into(), None);
        s
    }

    #[test]
    fn completion_and_collapse_follow_the_tree() {
        let mut s = outline();
        s.toggle(1);
        assert_eq!(s.progress(0), (1, 2));
        assert!(!s.tasks[0].done);
        s.toggle(2);
        assert!(s.tasks[0].done);
        s.toggle(0);
        assert!(s.tasks.iter().all(|t| !t.done));
        s.tasks[0].collapsed = true;
        assert_eq!(s.visible(), [0, 3]);
        s.add("Third".into(), Some(0));
        assert_eq!(s.visible(), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn outdent_preserves_later_siblings_and_moves_the_whole_subtree() {
        let mut s = outline();
        s.add("Grandchild".into(), Some(1));
        assert_eq!(s.indent(1, true), Some(2));
        assert_eq!(
            s.tasks
                .iter()
                .map(|t| (t.label.as_str(), t.depth))
                .collect::<Vec<_>>(),
            [
                ("Parent", 0),
                ("Second", 1),
                ("First", 0),
                ("Grandchild", 1),
                ("Sibling", 0)
            ]
        );
        assert_eq!(s.indent(2, false), Some(2));
        assert_eq!(s.tasks[3].depth, 2);
        s.remove(2);
        assert_eq!(
            s.tasks.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(),
            ["Parent", "Second", "Sibling"]
        );
    }

    #[test]
    fn invalid_depths_are_repaired_and_indent_is_bounded() {
        let mut s = outline();
        s.tasks[0].depth = 99;
        s.tasks[1].depth = 99;
        s.normalize();
        assert_eq!(s.tasks[0].depth, 0);
        assert_eq!(s.tasks[1].depth, 1);
        assert_eq!(s.indent(0, false), None);
        assert_eq!(s.indent(0, true), None);
        for i in 0..MAX_DEPTH {
            s.add(format!("Level {i}"), Some(i));
        }
        s.normalize();
        assert!(s.tasks.iter().all(|t| t.depth <= MAX_DEPTH));
    }
}
