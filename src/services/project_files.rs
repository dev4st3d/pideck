//! Worker-side project browsing, image snapshots, and explicit, conflict-checked text saves.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const MAX_IMAGE_PIXELS: u64 = 50_000_000;
const MAX_DIRECTORY_ENTRIES: usize = 2_000;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirectoryEntry {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) is_dir: bool,
    pub(crate) is_symlink: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct DirectoryListing {
    pub(crate) entries: Vec<DirectoryEntry>,
    pub(crate) truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LineEnding {
    Lf,
    CrLf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileSnapshot {
    pub(crate) path: PathBuf,
    pub(crate) text: String,
    pub(crate) baseline: Vec<u8>,
    pub(crate) line_ending: LineEnding,
    pub(crate) bom: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageKind {
    Png,
    Jpeg,
    Gif,
    Webp,
    Bmp,
    Tiff,
}

impl ImageKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Gif => "GIF",
            Self::Webp => "WebP",
            Self::Bmp => "BMP",
            Self::Tiff => "TIFF",
        }
    }

    fn from_extension(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "gif" => Some(Self::Gif),
            "webp" => Some(Self::Webp),
            "bmp" => Some(Self::Bmp),
            "tif" | "tiff" => Some(Self::Tiff),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImageSnapshot {
    pub(crate) path: PathBuf,
    pub(crate) bytes: Vec<u8>,
    pub(crate) format: ImageKind,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
}

pub(crate) fn is_image_path(path: &Path) -> bool {
    ImageKind::from_extension(path).is_some()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FileError {
    Unavailable,
    OutsideProject,
    NotFile,
    NotDirectory,
    NotText,
    NotImage,
    TooLarge,
    ImageTooLarge,
    ReadOnly,
    Conflict,
    SaveFailed,
    RecoveryBackup(PathBuf),
}

impl fmt::Display for FileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable => "Could not open this path. Check that it is available and readable.",
            Self::OutsideProject => "This link points outside the project. Open its folder as a project first.",
            Self::NotFile => "This path is not a regular file.",
            Self::NotDirectory => "This folder cannot be expanded.",
            Self::NotText => "This file is binary or is not UTF-8 text.",
            Self::NotImage => {
                "This file is not a supported PNG, JPEG, GIF, WebP, BMP, or TIFF image."
            }
            Self::TooLarge => "This file exceeds the 2 MiB editor limit. Open it in another editor.",
            Self::ImageTooLarge => {
                "This image exceeds the viewer limit (32 MiB, or 50 megapixels)."
            }
            Self::ReadOnly => "This file is read-only. Change its permissions before saving.",
            Self::Conflict => "This file changed on disk. Reopen it before saving your changes.",
            Self::SaveFailed => "Could not save this file. Your edits are still open; check folder permissions and retry.",
            Self::RecoveryBackup(path) => return write!(formatter, "The save could not finish. Keep your edits open; the original file's recovery copy is at {}.", path.display()),
        })
    }
}

impl std::error::Error for FileError {}

fn confined_path(root: &Path, path: &Path) -> Result<PathBuf, FileError> {
    let root = fs::canonicalize(root).map_err(|_| FileError::Unavailable)?;
    let path = fs::canonicalize(path).map_err(|_| FileError::Unavailable)?;
    if !path.starts_with(&root) {
        return Err(FileError::OutsideProject);
    }
    Ok(path)
}

pub(crate) fn list_directory(root: &Path, path: &Path) -> Result<DirectoryListing, FileError> {
    // Tree traversal never follows directory links, including links whose target
    // currently happens to be inside the project.
    let metadata = fs::symlink_metadata(path).map_err(|_| FileError::Unavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(FileError::NotDirectory);
    }
    let path = confined_path(root, path)?;
    let directory = fs::read_dir(&path).map_err(|_| FileError::Unavailable)?;
    let mut entries = Vec::new();
    let mut truncated = false;
    for entry in directory {
        if entries.len() == MAX_DIRECTORY_ENTRIES {
            truncated = true;
            break;
        }
        let entry = entry.map_err(|_| FileError::Unavailable)?;
        let kind = entry.file_type().map_err(|_| FileError::Unavailable)?;
        entries.push(DirectoryEntry {
            path: super::paths::without_windows_verbatim_prefix(&entry.path()),
            name: entry.file_name().to_string_lossy().into_owned(),
            is_dir: kind.is_dir() && !kind.is_symlink(),
            is_symlink: kind.is_symlink(),
        });
    }
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(DirectoryListing { entries, truncated })
}

/// Identify the project from local manifests on a worker.
pub(crate) fn project_kind(root: &Path) -> Option<&'static str> {
    if root.join("Cargo.toml").is_file() {
        return Some("Rust");
    }
    if root.join("artisan").is_file() {
        return Some("Laravel");
    }
    if let Ok(bytes) = read_bounded(&root.join("package.json"))
        && let Ok(package) = serde_json::from_slice::<serde_json::Value>(&bytes)
    {
        if package
            .get("dependencies")
            .and_then(|deps| deps.get("expo"))
            .is_some()
        {
            return Some("Expo");
        }
        return Some("JavaScript");
    }
    if root.join("pyproject.toml").is_file() {
        return Some("Python");
    }
    if root.join("go.mod").is_file() {
        return Some("Go");
    }
    None
}

/// Search filenames off the event loop. Keep ancestors and never follow links.
/// The visit cap prevents generated/vendor trees from blocking an interactive filter.
pub(crate) fn filter_files(
    root: &Path,
    query: &str,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<DirectoryListing, FileError> {
    use std::collections::{HashMap, HashSet};
    let query = query.trim().to_lowercase().replace('\\', "/");
    let mut pending = vec![root.to_path_buf()];
    let mut directories = HashMap::new();
    let mut retained = HashSet::new();
    let mut visited = 0;
    let mut matches = 0;
    let mut truncated = false;
    while let Some(path) = pending.pop() {
        if cancel.load(Ordering::Acquire) {
            return Ok(DirectoryListing {
                entries: Vec::new(),
                truncated: false,
            });
        }
        let listing = match list_directory(root, &path) {
            Ok(listing) => listing,
            Err(error) if path == root => return Err(error),
            Err(_) => {
                truncated = true;
                continue;
            }
        };
        truncated |= listing.truncated;
        for entry in &listing.entries {
            visited += 1;
            if visited > 20_000 || matches >= MAX_DIRECTORY_ENTRIES {
                truncated = true;
                break;
            }
            let relative = entry.path.strip_prefix(root).unwrap_or(&entry.path);
            if relative
                .to_string_lossy()
                .replace('\\', "/")
                .to_lowercase()
                .contains(&query)
            {
                matches += 1;
                let mut ancestor = Some(entry.path.as_path());
                while let Some(path) = ancestor {
                    if path == root {
                        break;
                    }
                    retained.insert(path.to_path_buf());
                    ancestor = path.parent();
                }
            }
            if entry.is_dir && !entry.is_symlink {
                pending.push(entry.path.clone());
            }
        }
        directories.insert(path, listing.entries);
        if visited > 20_000 || matches >= MAX_DIRECTORY_ENTRIES {
            break;
        }
    }
    fn append(
        path: &Path,
        directories: &HashMap<PathBuf, Vec<DirectoryEntry>>,
        retained: &HashSet<PathBuf>,
        output: &mut Vec<DirectoryEntry>,
    ) {
        if let Some(entries) = directories.get(path) {
            for entry in entries {
                if retained.contains(&entry.path) {
                    output.push(entry.clone());
                    if entry.is_dir {
                        append(&entry.path, directories, retained, output);
                    }
                }
            }
        }
    }
    let mut entries = Vec::new();
    append(root, &directories, &retained, &mut entries);
    Ok(DirectoryListing { entries, truncated })
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, FileError> {
    read_limited(path, MAX_TEXT_BYTES, FileError::TooLarge)
}

fn read_limited(path: &Path, max_bytes: usize, too_large: FileError) -> Result<Vec<u8>, FileError> {
    if !fs::metadata(path)
        .map_err(|_| FileError::Unavailable)?
        .is_file()
    {
        return Err(FileError::NotFile);
    }
    let file = fs::File::open(path).map_err(|_| FileError::Unavailable)?;
    let metadata = file.metadata().map_err(|_| FileError::Unavailable)?;
    if !metadata.is_file() {
        return Err(FileError::NotFile);
    }
    if metadata.len() > max_bytes as u64 {
        return Err(too_large.clone());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| FileError::Unavailable)?;
    if bytes.len() > max_bytes {
        return Err(too_large);
    }
    Ok(bytes)
}

pub(crate) fn load_image_file(root: &Path, path: &Path) -> Result<ImageSnapshot, FileError> {
    let path = confined_path(root, path)?;
    let bytes = read_limited(&path, MAX_IMAGE_BYTES, FileError::ImageTooLarge)?;
    if bytes.is_empty() {
        return Err(FileError::NotImage);
    }
    let Some(format) = detect_image(&bytes) else {
        return Err(FileError::NotImage);
    };
    let (width, height) = match image_dimensions(&bytes, format) {
        Some((width, height)) => {
            let pixels = u64::from(width).saturating_mul(u64::from(height));
            if width == 0 || height == 0 || pixels > MAX_IMAGE_PIXELS {
                return Err(FileError::ImageTooLarge);
            }
            (Some(width), Some(height))
        }
        None => (None, None),
    };
    Ok(ImageSnapshot {
        path: super::paths::without_windows_verbatim_prefix(&path),
        bytes,
        format,
        width,
        height,
    })
}

fn detect_image(bytes: &[u8]) -> Option<ImageKind> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(ImageKind::Png);
    }
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Some(ImageKind::Jpeg);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(ImageKind::Gif);
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some(ImageKind::Webp);
    }
    if bytes.starts_with(b"BM") {
        return Some(ImageKind::Bmp);
    }
    if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        return Some(ImageKind::Tiff);
    }
    None
}

fn image_dimensions(bytes: &[u8], format: ImageKind) -> Option<(u32, u32)> {
    match format {
        ImageKind::Png => png_dimensions(bytes),
        ImageKind::Jpeg => jpeg_dimensions(bytes),
        ImageKind::Gif => gif_dimensions(bytes),
        ImageKind::Webp => webp_dimensions(bytes),
        ImageKind::Bmp => bmp_dimensions(bytes),
        ImageKind::Tiff => None,
    }
}

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
        return None;
    }
    Some((
        u32::from_be_bytes(bytes[16..20].try_into().ok()?),
        u32::from_be_bytes(bytes[20..24].try_into().ok()?),
    ))
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut index = 2;
    while index + 1 < bytes.len() {
        if bytes[index] != 0xFF {
            return None;
        }
        while index < bytes.len() && bytes[index] == 0xFF {
            index += 1;
        }
        if index >= bytes.len() {
            return None;
        }
        let marker = bytes[index];
        index += 1;
        if marker == 0xD8 || marker == 0xD9 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if index + 1 >= bytes.len() {
            return None;
        }
        let length = u16::from_be_bytes([bytes[index], bytes[index + 1]]) as usize;
        if length < 2 || index + length > bytes.len() {
            return None;
        }
        if matches!(
            marker,
            0xC0 | 0xC1
                | 0xC2
                | 0xC3
                | 0xC5
                | 0xC6
                | 0xC7
                | 0xC9
                | 0xCA
                | 0xCB
                | 0xCD
                | 0xCE
                | 0xCF
        ) && length >= 7
        {
            let height = u16::from_be_bytes([bytes[index + 3], bytes[index + 4]]) as u32;
            let width = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]) as u32;
            return Some((width, height));
        }
        index += length;
    }
    None
}

fn gif_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 10 {
        return None;
    }
    Some((
        u16::from_le_bytes(bytes[6..8].try_into().ok()?) as u32,
        u16::from_le_bytes(bytes[8..10].try_into().ok()?) as u32,
    ))
}

fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 30 || !bytes.starts_with(b"RIFF") || &bytes[8..12] != b"WEBP" {
        return None;
    }
    match &bytes[12..16] {
        b"VP8X" => Some((
            1 + u32::from_le_bytes([bytes[24], bytes[25], bytes[26], 0]),
            1 + u32::from_le_bytes([bytes[27], bytes[28], bytes[29], 0]),
        )),
        b"VP8 " if bytes[23..26] == [0x9D, 0x01, 0x2A] => Some((
            u16::from_le_bytes([bytes[26], bytes[27]]) as u32 & 0x3FFF,
            u16::from_le_bytes([bytes[28], bytes[29]]) as u32 & 0x3FFF,
        )),
        b"VP8L" if bytes[20] == 0x2F => {
            let bits = u32::from_le_bytes(bytes[21..25].try_into().ok()?);
            Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
        }
        _ => None,
    }
}

fn bmp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 26 {
        return None;
    }
    let width = i32::from_le_bytes(bytes[18..22].try_into().ok()?);
    let height = i32::from_le_bytes(bytes[22..26].try_into().ok()?);
    if width <= 0 {
        return None;
    }
    Some((width as u32, height.unsigned_abs()))
}

fn snapshot(path: PathBuf, bytes: Vec<u8>) -> Result<FileSnapshot, FileError> {
    let bom = bytes.starts_with(b"\xef\xbb\xbf");
    let content = if bom { &bytes[3..] } else { &bytes[..] };
    let text = std::str::from_utf8(content).map_err(|_| FileError::NotText)?;
    if text.chars().any(|character| {
        character.is_control() && !matches!(character, '\n' | '\r' | '\t' | '\u{c}')
    }) {
        return Err(FileError::NotText);
    }
    let line_ending = text.find('\n').map_or(LineEnding::Lf, |index| {
        if index > 0 && text.as_bytes()[index - 1] == b'\r' {
            LineEnding::CrLf
        } else {
            LineEnding::Lf
        }
    });
    Ok(FileSnapshot {
        path: super::paths::without_windows_verbatim_prefix(&path),
        text: text.replace("\r\n", "\n"),
        baseline: bytes,
        line_ending,
        bom,
    })
}

pub(crate) fn load_text_file(root: &Path, path: &Path) -> Result<FileSnapshot, FileError> {
    let path = confined_path(root, path)?;
    snapshot(path.clone(), read_bounded(&path)?)
}

fn encode_text(previous: &FileSnapshot, text: &str) -> Result<Vec<u8>, FileError> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(FileError::TooLarge);
    }
    let normalized = text.replace("\r\n", "\n");
    if normalized == previous.text {
        return Ok(previous.baseline.clone());
    }
    let mut bytes = Vec::with_capacity(normalized.len().min(MAX_TEXT_BYTES));
    if previous.bom {
        bytes.extend_from_slice(b"\xef\xbb\xbf");
    }
    // Keep each existing line separator, including mixed-ending files. Newly
    // inserted trailing lines use the file's original default separator.
    let mut separators = previous
        .baseline
        .iter()
        .enumerate()
        .filter(|&(_, byte)| *byte == b'\n')
        .map(|(index, _)| {
            if index > 0 && previous.baseline[index - 1] == b'\r' {
                LineEnding::CrLf
            } else {
                LineEnding::Lf
            }
        });
    for byte in normalized.bytes() {
        if byte == b'\n' && separators.next().unwrap_or(previous.line_ending) == LineEnding::CrLf {
            bytes.push(b'\r');
        }
        bytes.push(byte);
        if bytes.len() > MAX_TEXT_BYTES {
            return Err(FileError::TooLarge);
        }
    }
    Ok(bytes)
}

struct TemporaryFile {
    path: PathBuf,
    remove_on_drop: bool,
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if self.remove_on_drop {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(windows)]
fn replace_preserving_permissions(
    replacement: &Path,
    destination: &Path,
    baseline: &[u8],
) -> Result<(), FileError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    let wide_path = |path: &Path| -> Result<Vec<u16>, FileError> {
        let mut encoded: Vec<u16> = path.as_os_str().encode_wide().collect();
        if encoded.contains(&0) {
            return Err(FileError::SaveFailed);
        }
        encoded.push(0);
        Ok(encoded)
    };
    let replacement_wide = wide_path(replacement)?;
    let destination_wide = wide_path(destination)?;
    let parent = destination.parent().ok_or(FileError::SaveFailed)?;
    let filename = destination.file_name().ok_or(FileError::SaveFailed)?;
    let mut backup = loop {
        let mut name = filename.to_os_string();
        name.push(format!(
            ".pideck-{}-{}.backup",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let path = parent.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                drop(file);
                break TemporaryFile {
                    path,
                    remove_on_drop: true,
                };
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(FileError::SaveFailed),
        }
    };
    let backup_wide = wide_path(&backup.path)?;
    if read_bounded(destination)? != baseline {
        return Err(FileError::Conflict);
    }
    // SAFETY: Every path buffer is NUL-terminated, has no interior NUL, and
    // stays alive for this synchronous call. Optional/reserved pointers are
    // null. Flags zero requires Windows to preserve the original ACL and attributes.
    let replaced = unsafe {
        ReplaceFileW(
            destination_wide.as_ptr(),
            replacement_wide.as_ptr(),
            backup_wide.as_ptr(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if replaced != 0 {
        return Ok(());
    }
    if read_bounded(destination).is_ok_and(|bytes| bytes == baseline) {
        return Err(FileError::SaveFailed);
    }
    // ReplaceFile can move the original to its backup before a later failure.
    // A hard link restores that exact original without overwriting any new file
    // that another process may already have created at the destination.
    if read_bounded(&backup.path).is_ok_and(|bytes| bytes == baseline)
        && fs::hard_link(&backup.path, destination).is_ok()
    {
        return Err(FileError::SaveFailed);
    }
    backup.remove_on_drop = false;
    Err(FileError::RecoveryBackup(
        super::paths::without_windows_verbatim_prefix(&backup.path),
    ))
}

pub(crate) fn save_text_file(
    root: &Path,
    previous: &FileSnapshot,
    text: &str,
) -> Result<FileSnapshot, FileError> {
    let path = confined_path(root, &previous.path)?;
    if read_bounded(&path)? != previous.baseline {
        return Err(FileError::Conflict);
    }
    let bytes = encode_text(previous, text)?;
    let updated = snapshot(path.clone(), bytes.clone())?;
    if bytes == previous.baseline {
        return Ok(updated);
    }
    let metadata = fs::metadata(&path).map_err(|_| FileError::Unavailable)?;
    if metadata.permissions().readonly() {
        return Err(FileError::ReadOnly);
    }
    let parent = path.parent().ok_or(FileError::SaveFailed)?;
    let name = path.file_name().ok_or(FileError::SaveFailed)?;
    let (temporary, mut file) = loop {
        let mut name = name.to_os_string();
        name.push(format!(
            ".pideck-{}-{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let candidate = parent.join(name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(file) => {
                break (
                    TemporaryFile {
                        path: candidate,
                        remove_on_drop: true,
                    },
                    file,
                );
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(FileError::SaveFailed),
        }
    };
    file.write_all(&bytes).map_err(|_| FileError::SaveFailed)?;
    file.sync_all().map_err(|_| FileError::SaveFailed)?;
    file.set_permissions(metadata.permissions())
        .map_err(|_| FileError::SaveFailed)?;
    drop(file);
    // Recheck after preparing the replacement, keeping the last valid file on
    // conflict or rename failure instead of removing it first.
    if read_bounded(&path)? != previous.baseline {
        return Err(FileError::Conflict);
    }
    #[cfg(windows)]
    replace_preserving_permissions(&temporary.path, &path, &previous.baseline)?;
    #[cfg(not(windows))]
    fs::rename(&temporary.path, &path).map_err(|_| FileError::SaveFailed)?;
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "pideck-files-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn filename_filter_keeps_nested_ancestors_and_handles_cancellation() {
        let directory = TestDirectory::new();
        let root = super::super::paths::without_windows_verbatim_prefix(&directory.0);
        fs::create_dir_all(root.join("src/views")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("src/views/terminal.rs"), "fixture").unwrap();
        fs::write(root.join("src/views/project.rs"), "fixture").unwrap();
        fs::write(root.join("docs/guide.md"), "fixture").unwrap();
        let cancellation = std::sync::atomic::AtomicBool::new(false);
        let listing = filter_files(&root, "terminal.rs", &cancellation).unwrap();
        let names: Vec<_> = listing
            .entries
            .iter()
            .map(|entry| {
                entry
                    .path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(names, ["src", "src/views", "src/views/terminal.rs"]);
        assert!(!listing.truncated);
        assert!(
            filter_files(&root, "no-match", &cancellation)
                .unwrap()
                .entries
                .is_empty()
        );
        cancellation.store(true, Ordering::Release);
        assert!(
            filter_files(&root, "terminal", &cancellation)
                .unwrap()
                .entries
                .is_empty()
        );
    }

    #[test]
    fn directory_listing_sorts_folders_first_and_caps_large_directories() {
        let root = TestDirectory::new();
        fs::write(root.0.join("a.txt"), b"a").unwrap();
        fs::create_dir(root.0.join("z-folder")).unwrap();
        fs::create_dir(root.0.join("B-folder")).unwrap();
        let listing = list_directory(&root.0, &root.0).unwrap();
        assert_eq!(
            listing
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["B-folder", "z-folder", "a.txt"]
        );
        assert!(!listing.truncated);
        assert!(listing.entries.iter().all(|entry| entry.path.is_absolute()));
        for index in 0..MAX_DIRECTORY_ENTRIES {
            fs::write(root.0.join(format!("extra-{index}")), b"").unwrap();
        }
        let listing = list_directory(&root.0, &root.0).unwrap();
        assert_eq!(listing.entries.len(), MAX_DIRECTORY_ENTRIES);
        assert!(listing.truncated);
    }

    #[test]
    fn utf8_bom_crlf_and_permissions_survive_save() {
        let root = TestDirectory::new();
        let path = root.0.join("text.txt");
        fs::write(&path, b"\xef\xbb\xbfone\r\ntwo\r\n").unwrap();
        let original = load_text_file(&root.0, &path).unwrap();
        assert_eq!(original.text, "one\ntwo\n");
        assert!(original.bom);
        assert_eq!(original.line_ending, LineEnding::CrLf);
        let saved = save_text_file(&root.0, &original, "one\nchanged\nthree\n").unwrap();
        assert_eq!(
            fs::read(&path).unwrap(),
            b"\xef\xbb\xbfone\r\nchanged\r\nthree\r\n"
        );
        assert_eq!(saved.text, "one\nchanged\nthree\n");
        assert_eq!(saved.baseline, fs::read(&path).unwrap());
        assert!(!fs::metadata(&path).unwrap().permissions().readonly());
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
    }

    #[test]
    fn disk_changes_prevent_save_and_keep_both_versions_available() {
        let root = TestDirectory::new();
        let path = root.0.join("text.txt");
        fs::write(&path, b"original\n").unwrap();
        let original = load_text_file(&root.0, &path).unwrap();
        fs::write(&path, b"external\n").unwrap();
        assert_eq!(
            save_text_file(&root.0, &original, "my edit\n"),
            Err(FileError::Conflict)
        );
        assert_eq!(fs::read(path).unwrap(), b"external\n");
        assert_eq!(original.text, "original\n");
    }

    #[test]
    fn binary_invalid_utf8_and_large_files_are_rejected() {
        let root = TestDirectory::new();
        let path = root.0.join("file");
        for content in [b"zero\0byte".as_slice(), b"\xff\xfe"] {
            fs::write(&path, content).unwrap();
            assert_eq!(load_text_file(&root.0, &path), Err(FileError::NotText));
        }
        let file = fs::File::create(&path).unwrap();
        file.set_len(MAX_TEXT_BYTES as u64 + 1).unwrap();
        assert_eq!(load_text_file(&root.0, &path), Err(FileError::TooLarge));
    }

    #[test]
    fn image_paths_cover_supported_raster_extensions() {
        assert!(is_image_path(Path::new("photo.PNG")));
        assert!(is_image_path(Path::new("a.JPEG")));
        assert!(is_image_path(Path::new(r"C:\assets\icon.webp")));
        assert!(is_image_path(Path::new("tile.tif")));
        assert!(!is_image_path(Path::new("icon.svg")));
        assert!(!is_image_path(Path::new("notes.txt")));
        assert!(!is_image_path(Path::new("archive.tar.gz")));
    }

    #[test]
    fn image_loader_reads_headers_and_rejects_unsupported_or_oversized_files() {
        let root = TestDirectory::new();
        let png = root.0.join("pixel.png");
        fs::write(&png, TINY_PNG).unwrap();
        let loaded = load_image_file(&root.0, &png).unwrap();
        assert_eq!(loaded.format, ImageKind::Png);
        assert_eq!(loaded.width, Some(1));
        assert_eq!(loaded.height, Some(1));
        assert_eq!(loaded.bytes, TINY_PNG);

        let jpeg = root.0.join("photo.jpg");
        fs::write(&jpeg, jpeg_sof0(1920, 1080)).unwrap();
        let loaded = load_image_file(&root.0, &jpeg).unwrap();
        assert_eq!(loaded.format, ImageKind::Jpeg);
        assert_eq!(loaded.width, Some(1920));
        assert_eq!(loaded.height, Some(1080));

        let gif = root.0.join("loop.gif");
        let mut gif_bytes = b"GIF89a".to_vec();
        gif_bytes.extend_from_slice(&320u16.to_le_bytes());
        gif_bytes.extend_from_slice(&240u16.to_le_bytes());
        gif_bytes.extend_from_slice(&[0, 0, 0]);
        fs::write(&gif, &gif_bytes).unwrap();
        let loaded = load_image_file(&root.0, &gif).unwrap();
        assert_eq!(loaded.format, ImageKind::Gif);
        assert_eq!((loaded.width, loaded.height), (Some(320), Some(240)));

        fs::write(root.0.join("notes.png"), b"not an image").unwrap();
        assert_eq!(
            load_image_file(&root.0, &root.0.join("notes.png")),
            Err(FileError::NotImage)
        );

        let huge = root.0.join("huge.jpg");
        fs::write(&huge, jpeg_sof0(10_000, 10_000)).unwrap();
        assert_eq!(
            load_image_file(&root.0, &huge),
            Err(FileError::ImageTooLarge)
        );

        let oversized = root.0.join("oversized.png");
        let file = fs::File::create(&oversized).unwrap();
        file.set_len(MAX_IMAGE_BYTES as u64 + 1).unwrap();
        assert_eq!(
            load_image_file(&root.0, &oversized),
            Err(FileError::ImageTooLarge)
        );

        let outside = TestDirectory::new();
        let foreign = outside.0.join("pixel.png");
        fs::write(&foreign, TINY_PNG).unwrap();
        assert_eq!(
            load_image_file(&root.0, &foreign),
            Err(FileError::OutsideProject)
        );
    }

    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x18, 0xDD, 0x8D, 0xB4, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn jpeg_sof0(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x0B, 0x08];
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&[0x01, 0x01, 0x11, 0x00, 0xFF, 0xD9]);
        bytes
    }

    #[test]
    fn mixed_endings_and_missing_final_newline_are_retained() {
        let root = TestDirectory::new();
        let path = root.0.join("text");
        fs::write(&path, b"one\r\ntwo\nthree").unwrap();
        let original = load_text_file(&root.0, &path).unwrap();
        save_text_file(&root.0, &original, "changed\ntwo\nthree").unwrap();
        assert_eq!(fs::read(path).unwrap(), b"changed\r\ntwo\nthree");
    }

    #[test]
    fn paths_outside_the_project_are_rejected() {
        let root = TestDirectory::new();
        let outside = TestDirectory::new();
        let path = outside.0.join("text");
        fs::write(&path, b"outside").unwrap();
        assert_eq!(
            load_text_file(&root.0, &path),
            Err(FileError::OutsideProject)
        );
        assert!(matches!(
            list_directory(&root.0, &outside.0),
            Err(FileError::OutsideProject)
        ));
    }

    #[cfg(windows)]
    #[test]
    fn failed_windows_replacement_keeps_original_and_cleans_reserved_backup() {
        let root = TestDirectory::new();
        let path = root.0.join("original.txt");
        fs::write(&path, b"original").unwrap();
        let result =
            replace_preserving_permissions(&root.0.join("missing-replacement"), &path, b"original");
        assert_eq!(result, Err(FileError::SaveFailed));
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn save_retains_executable_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let root = TestDirectory::new();
        let path = root.0.join("script");
        fs::write(&path, b"first\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o750)).unwrap();
        let original = load_text_file(&root.0, &path).unwrap();
        save_text_file(&root.0, &original, "second\n").unwrap();
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o750
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_directory_traversal_and_external_file_links_are_rejected() {
        use std::os::unix::fs::symlink;
        let root = TestDirectory::new();
        let outside = TestDirectory::new();
        fs::write(outside.0.join("text"), b"outside").unwrap();
        symlink(&outside.0, root.0.join("folder-link")).unwrap();
        symlink(outside.0.join("text"), root.0.join("file-link")).unwrap();
        assert!(matches!(
            list_directory(&root.0, &root.0.join("folder-link")),
            Err(FileError::NotDirectory)
        ));
        assert_eq!(
            load_text_file(&root.0, &root.0.join("file-link")),
            Err(FileError::OutsideProject)
        );
        assert!(
            list_directory(&root.0, &root.0)
                .unwrap()
                .entries
                .iter()
                .all(|entry| entry.is_symlink && !entry.is_dir)
        );
    }
}
