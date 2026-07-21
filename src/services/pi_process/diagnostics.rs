use std::collections::VecDeque;
use std::io::Read;
use std::sync::{Arc, Mutex};

const MAX_DIAGNOSTIC_SEGMENT: usize = 16 * 1024;

#[derive(Debug)]
pub(crate) struct StderrRing {
    capacity: usize,
    retained_bytes: usize,
    entries: VecDeque<String>,
}

impl StderrRing {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            retained_bytes: 0,
            entries: VecDeque::new(),
        }
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) {
        if self.capacity == 0 || bytes.is_empty() {
            return;
        }

        let text = String::from_utf8_lossy(bytes);
        let redacted = redact_diagnostic(text.trim_end_matches(['\r', '\n']));
        if redacted.is_empty() {
            return;
        }
        let entry = tail_at_char_boundary(redacted, self.capacity);
        self.retained_bytes = self.retained_bytes.saturating_add(entry.len());
        self.entries.push_back(entry);
        while self.retained_bytes > self.capacity {
            let Some(removed) = self.entries.pop_front() else {
                break;
            };
            self.retained_bytes = self.retained_bytes.saturating_sub(removed.len());
        }
    }

    pub(crate) fn snapshot(&self) -> Vec<String> {
        self.entries.iter().cloned().collect()
    }

    #[cfg(test)]
    fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

pub(crate) fn drain_stderr(mut stream: Box<dyn Read + Send>, ring: Arc<Mutex<StderrRing>>) {
    let mut read_buffer = [0_u8; 8192];
    let mut segment = Vec::new();
    loop {
        match stream.read(&mut read_buffer) {
            Ok(0) | Err(_) => {
                flush_segment(&ring, &mut segment);
                return;
            }
            Ok(count) => {
                for byte in &read_buffer[..count] {
                    segment.push(*byte);
                    if *byte == b'\n' || segment.len() >= MAX_DIAGNOSTIC_SEGMENT {
                        flush_segment(&ring, &mut segment);
                    }
                }
            }
        }
    }
}

fn flush_segment(ring: &Mutex<StderrRing>, segment: &mut Vec<u8>) {
    if segment.is_empty() {
        return;
    }
    ring.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(segment);
    segment.clear();
}

pub(crate) fn redact_diagnostic(value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        format!("[redacted Pi diagnostic: {} bytes]", value.len())
    }
}

fn tail_at_char_boundary(mut value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }

    let mut start = value.len() - max_bytes;
    while !value.is_char_boundary(start) {
        start += 1;
    }
    value.drain(..start);
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_is_byte_bounded_and_keeps_the_tail() {
        let mut ring = StderrRing::new(70);
        ring.push(b"first\n");
        ring.push(b"second\n");
        ring.push(b"third\n");

        assert!(ring.retained_bytes() <= 70);
        assert_eq!(ring.snapshot().len(), 2);
    }

    #[test]
    fn sensitive_lines_are_replaced_not_partially_masked() {
        let mut ring = StderrRing::new(1024);
        ring.push(br#"Authorization: Bearer private-value"#);
        ring.push(br#"{"api_key":"private-value"}"#);

        let snapshot = ring.snapshot();
        assert!(
            snapshot
                .iter()
                .all(|line| line.starts_with("[redacted Pi diagnostic:"))
        );
        assert!(!snapshot.join("\n").contains("private-value"));
    }

    #[test]
    fn truncation_preserves_utf8_boundaries() {
        let truncated = tail_at_char_boundary("ééé".to_owned(), 5);
        assert_eq!(truncated, "éé");
    }
}
