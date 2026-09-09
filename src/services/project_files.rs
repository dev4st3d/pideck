//! Worker-side project browsing and explicit, conflict-checked text saves.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FileError {
    Unavailable,
    OutsideProject,
    NotFile,
    NotDirectory,
    NotText,
    TooLarge,
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
            Self::NotFile => "Choose a regular text file to edit.",
            Self::NotDirectory => "This folder cannot be expanded.",
            Self::NotText => "This file is binary or is not UTF-8 text.",
            Self::TooLarge => "This file exceeds the 2 MiB editor limit. Open it in another editor.",
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

fn read_bounded(path: &Path) -> Result<Vec<u8>, FileError> {
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
    if metadata.len() > MAX_TEXT_BYTES as u64 {
        return Err(FileError::TooLarge);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_TEXT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| FileError::Unavailable)?;
    if bytes.len() > MAX_TEXT_BYTES {
        return Err(FileError::TooLarge);
    }
    Ok(bytes)
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
