//! Replace one app-owned file without ever deleting the last valid destination.
//!
//! A unique sibling avoids temp-name collisions and keeps rename on one volume.
//! Serialization happens before calling this function. Call it on an I/O worker.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

struct TemporaryFile(PathBuf);

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        // Never remove the destination, even when a Windows reader denies rename.
        let _ = fs::remove_file(&self.0);
    }
}

pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "A file destination is required")
    })?;

    let (temporary, mut file) = loop {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let mut temporary_name = name.to_os_string();
        temporary_name.push(format!(".{}.{}.tmp", std::process::id(), sequence));
        let candidate = parent.join(temporary_name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(file) => break (TemporaryFile(candidate), file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };

    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    // std::fs::rename replaces an existing file on Windows and Unix. On failure
    // leave it untouched, rather than unlinking it and attempting a second move.
    fs::rename(&temporary.0, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "pideck-atomic-{label}-{}-{}",
            std::process::id(), NEXT_TEMP.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn replacement_is_complete_and_leaves_no_temporary_file() {
        let root = directory("replace");
        let path = root.join("settings.json");
        write(&path, b"old").unwrap();
        write(&path, b"new and longer").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new and longer");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_replacement_does_not_destroy_existing_destination() {
        let root = directory("failure");
        let path = root.join("occupied");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("valuable"), b"keep").unwrap();
        assert!(write(&path, b"replacement").is_err());
        assert_eq!(fs::read(path.join("valuable")).unwrap(), b"keep");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_writes_never_share_a_temporary_name() {
        let root = directory("concurrent");
        let path = root.join("projects.json");
        std::thread::scope(|scope| {
            for value in 0..8_u8 {
                let path = &path;
                scope.spawn(move || {
                    // A Windows sharing violation may reject a concurrent move;
                    // any successful destination must still be one whole write.
                    let _ = write(path, &vec![value; 16_384]);
                });
            }
        });
        let bytes = fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 16_384);
        assert!(bytes.iter().all(|byte| *byte == bytes[0]));
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
