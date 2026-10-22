//! Terminal font preference: family, size, and line height. Other text keeps its design faces.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::LazyLock;

use gpui::{App, Global, SharedString};
use serde::{Deserialize, Serialize};

pub(crate) const DEFAULT_FAMILY: &str = "IBM Plex Mono";
pub(crate) const DEFAULT_SIZE: f32 = crate::theme::T_MONO;
pub(crate) const MIN_SIZE: f32 = 9.0;
pub(crate) const MAX_SIZE: f32 = 24.0;
/// Multiple of the font size; 13 px at 1.4 gives the original 18 px rows.
pub(crate) const DEFAULT_LINE_HEIGHT: f32 = 1.4;
pub(crate) const MIN_LINE_HEIGHT: f32 = 1.0;
pub(crate) const MAX_LINE_HEIGHT: f32 = 1.8;
const LINE_HEIGHT_STEP: f32 = 0.1;
const MAX_FAMILY_LEN: usize = 128;

static DEFAULT_FONT: LazyLock<TerminalFont> = LazyLock::new(TerminalFont::default);

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TerminalFont {
    pub(crate) family: SharedString,
    pub(crate) size: f32,
    pub(crate) line_height: f32,
}

impl Default for TerminalFont {
    fn default() -> Self {
        Self {
            family: DEFAULT_FAMILY.into(),
            size: DEFAULT_SIZE,
            line_height: DEFAULT_LINE_HEIGHT,
        }
    }
}

impl Global for TerminalFont {}

impl TerminalFont {
    /// The active font, or the defaults before preferences have loaded.
    pub(crate) fn global(cx: &App) -> &Self {
        cx.try_global::<Self>().unwrap_or(&DEFAULT_FONT)
    }

    /// Whole-pixel row height so cell rows stay aligned at every size.
    pub(crate) fn line_height_px(&self) -> f32 {
        (self.size * self.line_height).round().max(1.0)
    }

    pub(crate) fn with_family(&self, family: SharedString) -> Self {
        Self {
            family,
            ..self.clone()
        }
    }

    /// One step larger (`1`) or smaller (`-1`); `None` at the limit.
    pub(crate) fn stepped_size(&self, direction: i8) -> Option<Self> {
        let size = (self.size + f32::from(direction)).clamp(MIN_SIZE, MAX_SIZE);
        (size != self.size).then(|| Self {
            size,
            ..self.clone()
        })
    }

    pub(crate) fn stepped_line_height(&self, direction: i8) -> Option<Self> {
        let next = self.line_height + f32::from(direction) * LINE_HEIGHT_STEP;
        // Round to tenths so repeated steps do not accumulate float error.
        let line_height = ((next * 10.0).round() / 10.0).clamp(MIN_LINE_HEIGHT, MAX_LINE_HEIGHT);
        (line_height != self.line_height).then(|| Self {
            line_height,
            ..self.clone()
        })
    }

    pub(crate) fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Deserialize, Serialize)]
struct StoredFont {
    version: u32,
    family: String,
    size: f32,
    line_height: f32,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(crate) fn load(path: &Path) -> io::Result<TerminalFont> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(TerminalFont::default());
        }
        Err(error) => return Err(error),
    };
    let stored: StoredFont = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if stored.version != 1 {
        return Err(invalid("Unsupported terminal font version"));
    }
    let family = stored.family.trim();
    if family.is_empty()
        || family.chars().count() > MAX_FAMILY_LEN
        || family.chars().any(char::is_control)
    {
        return Err(invalid("Invalid terminal font family"));
    }
    if !(MIN_SIZE..=MAX_SIZE).contains(&stored.size)
        || !(MIN_LINE_HEIGHT..=MAX_LINE_HEIGHT).contains(&stored.line_height)
    {
        return Err(invalid("Terminal font size is out of range"));
    }
    Ok(TerminalFont {
        family: family.to_owned().into(),
        size: stored.size,
        line_height: stored.line_height,
    })
}

pub(crate) fn save(path: &Path, font: &TerminalFont) -> io::Result<()> {
    let stored = StoredFont {
        version: 1,
        family: font.family.to_string(),
        size: font.size,
        line_height: font.line_height,
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
                "zideck-terminal-font-{}-{}",
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
    fn default_keeps_the_original_eighteen_pixel_rows() {
        let font = TerminalFont::default();
        assert_eq!(font.family, DEFAULT_FAMILY);
        assert_eq!(font.line_height_px(), 18.0);
        assert!(font.is_default());
    }

    #[test]
    fn steps_stop_at_their_limits_without_float_drift() {
        let mut font = TerminalFont::default();
        for _ in 0..30 {
            if let Some(next) = font.stepped_size(1) {
                font = next;
            }
        }
        assert_eq!(font.size, MAX_SIZE);
        assert!(font.stepped_size(1).is_none());
        for _ in 0..30 {
            if let Some(next) = font.stepped_size(-1) {
                font = next;
            }
        }
        assert_eq!(font.size, MIN_SIZE);
        assert!(font.stepped_size(-1).is_none());

        let mut font = TerminalFont::default();
        for _ in 0..4 {
            font = font.stepped_line_height(1).unwrap();
        }
        assert_eq!(font.line_height, MAX_LINE_HEIGHT);
        assert!(font.stepped_line_height(1).is_none());
        for _ in 0..8 {
            font = font.stepped_line_height(-1).unwrap();
        }
        assert_eq!(font.line_height, MIN_LINE_HEIGHT);
        assert!(font.stepped_line_height(-1).is_none());
    }

    #[test]
    fn row_height_is_whole_pixels_and_never_collapses() {
        let larger = TerminalFont {
            size: 14.0,
            ..TerminalFont::default()
        };
        assert_eq!(larger.line_height_px(), 20.0);
        let tight = TerminalFont {
            size: MIN_SIZE,
            line_height: MIN_LINE_HEIGHT,
            ..TerminalFont::default()
        };
        assert_eq!(tight.line_height_px(), 9.0);
    }

    #[test]
    fn preferences_roundtrip_and_missing_file_means_defaults() {
        let root = TestDirectory::new();
        let path = root.0.join("terminal-font.json");
        assert_eq!(load(&path).unwrap(), TerminalFont::default());
        let font = TerminalFont {
            family: "Cascadia Mono".into(),
            size: 15.0,
            line_height: 1.2,
        };
        save(&path, &font).unwrap();
        assert_eq!(load(&path).unwrap(), font);
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
    }

    #[test]
    fn corrupt_out_of_range_and_future_preferences_are_preserved() {
        let root = TestDirectory::new();
        let path = root.0.join("terminal-font.json");
        for bytes in [
            b"invalid json".as_slice(),
            br#"{"version":2,"family":"Consolas","size":13,"line_height":1.4}"#.as_slice(),
            br#"{"version":1,"family":"","size":13,"line_height":1.4}"#.as_slice(),
            br#"{"version":1,"family":"Consolas","size":4,"line_height":1.4}"#.as_slice(),
            br#"{"version":1,"family":"Consolas","size":13,"line_height":9}"#.as_slice(),
            br#"{"version":1,"family":"Con\u0007solas","size":13,"line_height":1.4}"#.as_slice(),
        ] {
            fs::write(&path, bytes).unwrap();
            assert_eq!(load(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}
