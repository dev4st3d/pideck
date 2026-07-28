//! Local file attachment loading and prompt expansion.
//!
//! Pi 0.82 RPC accepts images natively. Readable files therefore remain GUI-owned
//! draft data and are expanded into bounded, named text blocks only at the RPC edge.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

pub const MAX_ATTACHMENTS: usize = 8;
pub const MAX_IMAGE_ATTACHMENTS: usize = 4;
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_TEXT_SNAPSHOT_BYTES: usize = 256 * 1024;
pub const MAX_TOTAL_TEXT_SNAPSHOT_BYTES: usize = 1024 * 1024;

const TEXT_SNIFF_BYTES: usize = 16 * 1024;
const ATTACHMENT_MARKER: &str = "<!-- pi-gui-attachment:";
const ATTACHMENT_MARKER_END: &str = " -->\n";
const FILE_CLOSE: &str = "\n</file>\n";
const REFERENCE_NOTICE: &str = "Content was not snapshotted because this readable file is large. Read it from this path when needed: ";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileDelivery {
    Snapshot,
    PathReference,
}

impl FileDelivery {
    pub fn label(self) -> &'static str {
        match self {
            Self::Snapshot => "Snapshot",
            Self::PathReference => "Path reference",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptFileMetadata {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub delivery: FileDelivery,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptFile {
    pub metadata: PromptFileMetadata,
    pub content: Option<Arc<str>>,
}

impl PromptFile {
    pub fn snapshot_bytes(&self) -> usize {
        self.content.as_ref().map_or(0, |content| content.len())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadedAttachment {
    Image {
        data: String,
        mime_type: String,
        file_name: String,
        source_path: String,
        bytes: usize,
    },
    File(PromptFile),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentIssue {
    pub name: String,
    pub message: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LoadedAttachmentBatch {
    pub attachments: Vec<LoadedAttachment>,
    pub issues: Vec<AttachmentIssue>,
}

#[derive(Debug, Clone)]
pub struct AttachmentLoadLimits {
    pub remaining_attachments: usize,
    pub remaining_images: usize,
    pub remaining_image_bytes: usize,
    pub remaining_snapshot_bytes: usize,
    pub existing_sources: HashSet<String>,
}

impl AttachmentLoadLimits {
    fn consume(&mut self, attachment: &LoadedAttachment) {
        self.remaining_attachments = self.remaining_attachments.saturating_sub(1);
        match attachment {
            LoadedAttachment::Image {
                bytes, source_path, ..
            } => {
                self.remaining_images = self.remaining_images.saturating_sub(1);
                self.remaining_image_bytes = self.remaining_image_bytes.saturating_sub(*bytes);
                self.existing_sources.insert(source_path.clone());
            }
            LoadedAttachment::File(file) => {
                self.remaining_snapshot_bytes = self
                    .remaining_snapshot_bytes
                    .saturating_sub(file.snapshot_bytes());
                self.existing_sources.insert(file.metadata.path.clone());
            }
        }
    }
}

/// Loads selected files synchronously. Callers must run this on a background executor.
pub fn load_attachments(
    paths: Vec<PathBuf>,
    mut limits: AttachmentLoadLimits,
) -> LoadedAttachmentBatch {
    let mut batch = LoadedAttachmentBatch::default();
    for path in paths {
        if limits.remaining_attachments == 0 {
            batch.issues.push(AttachmentIssue {
                name: display_name(&path),
                message: format!("You can attach up to {MAX_ATTACHMENTS} files."),
            });
            continue;
        }

        match load_attachment(&path, &limits) {
            Ok(attachment) => {
                limits.consume(&attachment);
                batch.attachments.push(attachment);
            }
            Err(issue) => batch.issues.push(issue),
        }
    }
    batch
}

fn load_attachment(
    path: &Path,
    limits: &AttachmentLoadLimits,
) -> Result<LoadedAttachment, AttachmentIssue> {
    let name = display_name(path);
    let canonical = path.canonicalize().map_err(|_| AttachmentIssue {
        name: name.clone(),
        message: "The file is no longer available.".to_owned(),
    })?;
    let source_path = canonical.to_string_lossy().into_owned();
    if limits.existing_sources.contains(&source_path) {
        return Err(AttachmentIssue {
            name,
            message: "This file is already attached.".to_owned(),
        });
    }

    let metadata = fs::metadata(&canonical).map_err(|_| AttachmentIssue {
        name: name.clone(),
        message: "The file metadata could not be read.".to_owned(),
    })?;
    if !metadata.is_file() {
        return Err(AttachmentIssue {
            name,
            message: "Folders cannot be attached. Choose individual files.".to_owned(),
        });
    }
    if metadata.len() == 0 {
        return Err(AttachmentIssue {
            name,
            message: "Empty files are not attached.".to_owned(),
        });
    }

    let sniff = read_prefix(&canonical, TEXT_SNIFF_BYTES).map_err(|_| AttachmentIssue {
        name: name.clone(),
        message: "The file could not be read.".to_owned(),
    })?;
    if let Some(mime_type) = supported_image_mime(&canonical, &sniff) {
        if limits.remaining_images == 0 {
            return Err(AttachmentIssue {
                name,
                message: format!("You can attach up to {MAX_IMAGE_ATTACHMENTS} images."),
            });
        }
        let byte_len = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
        if byte_len > limits.remaining_image_bytes {
            return Err(AttachmentIssue {
                name,
                message: "Attached images must total 5 MB or less.".to_owned(),
            });
        }
        let bytes = read_bounded(&canonical, limits.remaining_image_bytes)
            .map_err(|_| AttachmentIssue {
                name: name.clone(),
                message: "The image could not be read.".to_owned(),
            })?
            .ok_or_else(|| AttachmentIssue {
                name: name.clone(),
                message: "Attached images must total 5 MB or less.".to_owned(),
            })?;
        if mime_type != "image/svg+xml" && image::load_from_memory(&bytes).is_err() {
            return Err(AttachmentIssue {
                name,
                message: "The image data could not be decoded.".to_owned(),
            });
        }
        return Ok(LoadedAttachment::Image {
            data: STANDARD.encode(&bytes),
            mime_type: mime_type.to_owned(),
            file_name: name,
            source_path,
            bytes: bytes.len(),
        });
    }

    if !is_readable_text(&sniff, metadata.len() <= sniff.len() as u64) {
        return Err(AttachmentIssue {
            name,
            message: "This release supports images and readable text/code files.".to_owned(),
        });
    }

    let mut size = metadata.len();
    let can_snapshot =
        size <= MAX_TEXT_SNAPSHOT_BYTES as u64 && size <= limits.remaining_snapshot_bytes as u64;
    let content = if can_snapshot {
        let snapshot_limit = MAX_TEXT_SNAPSHOT_BYTES.min(limits.remaining_snapshot_bytes);
        let bytes = read_bounded(&canonical, snapshot_limit).map_err(|_| AttachmentIssue {
            name: name.clone(),
            message: "The file could not be read.".to_owned(),
        })?;
        if let Some(bytes) = bytes {
            if bytes.is_empty() {
                return Err(AttachmentIssue {
                    name,
                    message: "Empty files are not attached.".to_owned(),
                });
            }
            if !is_readable_text(&bytes, true) {
                return Err(AttachmentIssue {
                    name,
                    message: "This release supports images and readable text/code files."
                        .to_owned(),
                });
            }
            size = bytes.len() as u64;
            let text = String::from_utf8(bytes).map_err(|_| AttachmentIssue {
                name: name.clone(),
                message: "The text file is not valid UTF-8.".to_owned(),
            })?;
            Some(Arc::<str>::from(text))
        } else {
            None
        }
    } else {
        None
    };
    let delivery = if content.is_some() {
        FileDelivery::Snapshot
    } else {
        size = fs::metadata(&canonical).map_or(size, |metadata| metadata.len());
        FileDelivery::PathReference
    };

    Ok(LoadedAttachment::File(PromptFile {
        metadata: PromptFileMetadata {
            name,
            path: source_path,
            size,
            delivery,
        },
        content,
    }))
}

fn read_prefix(path: &Path, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::with_capacity(limit);
    file.by_ref().take(limit as u64).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn read_bounded(path: &Path, limit: usize) -> std::io::Result<Option<Vec<u8>>> {
    let read_limit = limit.saturating_add(1);
    let mut file = File::open(path)?;
    let mut bytes = Vec::with_capacity(read_limit);
    file.by_ref()
        .take(read_limit as u64)
        .read_to_end(&mut bytes)?;
    Ok((bytes.len() <= limit).then_some(bytes))
}

fn supported_image_mime(path: &Path, sniff: &[u8]) -> Option<&'static str> {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
        && std::str::from_utf8(sniff)
            .ok()
            .is_some_and(|text| text.to_ascii_lowercase().contains("<svg"))
    {
        return Some("image/svg+xml");
    }
    match image::guess_format(sniff).ok()? {
        image::ImageFormat::Png => Some("image/png"),
        image::ImageFormat::Jpeg => Some("image/jpeg"),
        image::ImageFormat::WebP => Some("image/webp"),
        image::ImageFormat::Gif => Some("image/gif"),
        image::ImageFormat::Bmp => Some("image/bmp"),
        image::ImageFormat::Tiff => Some("image/tiff"),
        _ => None,
    }
}

fn is_readable_text(bytes: &[u8], complete: bool) -> bool {
    if bytes.contains(&0) {
        return false;
    }
    let valid = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) if !complete && error.error_len().is_none() => {
            let Ok(text) = std::str::from_utf8(&bytes[..error.valid_up_to()]) else {
                return false;
            };
            text
        }
        Err(_) => return false,
    };
    valid.chars().all(|character| {
        !character.is_control() || matches!(character, '\n' | '\r' | '\t' | '\u{feff}')
    })
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

#[derive(Debug, Serialize, Deserialize)]
struct PromptFileWireMetadata {
    #[serde(flatten)]
    file: PromptFileMetadata,
    content_bytes: usize,
}

/// Adds GUI-owned file attachments to the text sent through Pi's image-only RPC prompt contract.
pub fn expand_prompt(text: &str, files: &[PromptFile]) -> String {
    if files.is_empty() {
        return text.to_owned();
    }

    let mut expanded = String::new();
    for file in files {
        let reference_body;
        let body = if let Some(content) = file.content.as_deref() {
            content
        } else {
            reference_body = format!("{REFERENCE_NOTICE}{}", file.metadata.path);
            &reference_body
        };
        let wire = PromptFileWireMetadata {
            file: file.metadata.clone(),
            content_bytes: body.len(),
        };
        let encoded = STANDARD.encode(
            serde_json::to_vec(&wire).expect("plain prompt file metadata must serialize as JSON"),
        );
        expanded.push_str("<file name=\"");
        expanded.push_str(&escape_xml_attribute(&file.metadata.path));
        expanded.push_str("\">\n");
        expanded.push_str(ATTACHMENT_MARKER);
        expanded.push_str(&encoded);
        expanded.push_str(ATTACHMENT_MARKER_END);
        expanded.push_str(body);
        expanded.push_str(FILE_CLOSE);
    }
    if !text.is_empty() {
        expanded.push('\n');
        expanded.push_str(text);
    }
    expanded
}

/// Removes prompt-expanded attachment bodies from an authoritative user message for native display.
pub fn parse_expanded_prompt(text: &str) -> Option<(String, Vec<PromptFileMetadata>)> {
    let mut rest = text;
    let mut files = Vec::new();
    loop {
        if !rest.starts_with("<file name=\"") {
            break;
        }
        let opening_end = rest.find("\">\n")? + 3;
        rest = &rest[opening_end..];
        let marker = rest.strip_prefix(ATTACHMENT_MARKER)?;
        let marker_end = marker.find(ATTACHMENT_MARKER_END)?;
        let encoded = &marker[..marker_end];
        let wire: PromptFileWireMetadata =
            serde_json::from_slice(&STANDARD.decode(encoded).ok()?).ok()?;
        rest = &marker[marker_end + ATTACHMENT_MARKER_END.len()..];
        let body_end = wire.content_bytes;
        if body_end > rest.len() || !rest.is_char_boundary(body_end) {
            return None;
        }
        rest = &rest[body_end..];
        rest = rest.strip_prefix(FILE_CLOSE)?;
        files.push(wire.file);
    }
    if files.is_empty() {
        return None;
    }
    let visible_text = rest.strip_prefix('\n').unwrap_or(rest).to_owned();
    Some((visible_text, files))
}

fn escape_xml_attribute(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f >= MIB {
        format!("{:.1} MB", bytes_f / MIB)
    } else if bytes_f >= KIB {
        format!("{:.0} KB", bytes_f / KIB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("pi-gui-attachments-{label}-{nonce}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn limits() -> AttachmentLoadLimits {
        AttachmentLoadLimits {
            remaining_attachments: MAX_ATTACHMENTS,
            remaining_images: MAX_IMAGE_ATTACHMENTS,
            remaining_image_bytes: MAX_IMAGE_BYTES,
            remaining_snapshot_bytes: MAX_TOTAL_TEXT_SNAPSHOT_BYTES,
            existing_sources: HashSet::new(),
        }
    }

    #[test]
    fn snapshots_small_utf8_and_references_large_text() {
        let root = temp_dir("text");
        let small = root.join("small.rs");
        let large = root.join("large.log");
        fs::write(&small, "fn main() {}\n").unwrap();
        fs::write(&large, "x".repeat(MAX_TEXT_SNAPSHOT_BYTES + 1)).unwrap();

        let batch = load_attachments(vec![small, large], limits());
        assert!(batch.issues.is_empty(), "{:?}", batch.issues);
        let LoadedAttachment::File(small) = &batch.attachments[0] else {
            panic!("expected small text file");
        };
        assert_eq!(small.metadata.delivery, FileDelivery::Snapshot);
        assert_eq!(small.content.as_deref(), Some("fn main() {}\n"));
        let LoadedAttachment::File(large) = &batch.attachments[1] else {
            panic!("expected large text file");
        };
        assert_eq!(large.metadata.delivery, FileDelivery::PathReference);
        assert!(large.content.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_binary_and_duplicate_files() {
        let root = temp_dir("binary");
        let binary = root.join("data.bin");
        fs::write(&binary, [0, 1, 2, 3]).unwrap();
        let mut duplicate_limits = limits();
        duplicate_limits.existing_sources.insert(
            binary
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );

        let binary_batch = load_attachments(vec![binary.clone()], limits());
        assert_eq!(binary_batch.issues.len(), 1);
        assert!(binary_batch.attachments.is_empty());
        let duplicate_batch = load_attachments(vec![binary], duplicate_limits);
        assert_eq!(
            duplicate_batch.issues[0].message,
            "This file is already attached."
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn expanded_prompt_round_trips_metadata_without_displaying_contents() {
        let files = vec![
            PromptFile {
                metadata: PromptFileMetadata {
                    name: "main.rs".to_owned(),
                    path: r#"C:\work\a&b\main.rs"#.to_owned(),
                    size: 24,
                    delivery: FileDelivery::Snapshot,
                },
                content: Some(Arc::from("fn main() {\n</file>\n}")),
            },
            PromptFile {
                metadata: PromptFileMetadata {
                    name: "large.log".to_owned(),
                    path: r#"C:\work\large.log"#.to_owned(),
                    size: 900_000,
                    delivery: FileDelivery::PathReference,
                },
                content: None,
            },
        ];
        let expanded = expand_prompt("review these", &files);
        assert!(expanded.contains("fn main()"));
        assert!(expanded.contains("&amp;"));

        let (visible, parsed) = parse_expanded_prompt(&expanded).expect("expanded prompt");
        assert_eq!(visible, "review these");
        assert_eq!(
            parsed,
            files
                .iter()
                .map(|file| file.metadata.clone())
                .collect::<Vec<_>>()
        );
    }
}
