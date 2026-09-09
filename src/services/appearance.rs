//! Independent appearance preference; legacy settings and workspace data stay intact.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::theme::Appearance;

#[derive(Deserialize, Serialize)]
struct StoredAppearance {
    version: u32,
    theme: String,
}

pub(crate) fn load(path: &Path) -> io::Result<Appearance> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Appearance::default()),
        Err(error) => return Err(error),
    };
    let stored: StoredAppearance = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if stored.version != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Unsupported appearance version",
        ));
    }
    Appearance::from_id(&stored.theme)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Unknown appearance"))
}

pub(crate) fn save(path: &Path, appearance: Appearance) -> io::Result<()> {
    let stored = StoredAppearance {
        version: 1,
        theme: appearance.id().to_owned(),
    };
    let mut bytes = serde_json::to_vec_pretty(&stored)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    bytes.push(b'\n');
    super::atomic_file::write(path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST: AtomicU64 = AtomicU64::new(1);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "pideck-appearance-{}-{}",
                std::process::id(),
                NEXT_TEST.fetch_add(1, Ordering::Relaxed),
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
    fn preferences_roundtrip_without_touching_legacy_settings() {
        let root = TestDirectory::new();
        let path = root.0.join("appearance.json");
        let legacy = root.0.join("settings.json");
        fs::write(&legacy, b"legacy settings preserved").unwrap();
        assert_eq!(load(&path).unwrap(), Appearance::Paper);
        for appearance in Appearance::ALL {
            save(&path, appearance).unwrap();
            assert_eq!(load(&path).unwrap(), appearance);
        }
        assert_eq!(fs::read(&legacy).unwrap(), b"legacy settings preserved");
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 2);
    }

    #[test]
    fn corrupt_unknown_and_future_preferences_are_preserved() {
        let root = TestDirectory::new();
        let path = root.0.join("appearance.json");
        for bytes in [
            b"invalid json".as_slice(),
            br#"{"version":1,"theme":"unknown"}"#.as_slice(),
            br#"{"version":2,"theme":"paper"}"#.as_slice(),
        ] {
            fs::write(&path, bytes).unwrap();
            assert_eq!(load(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}
