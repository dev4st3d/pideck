//! Display and shell paths without Windows canonicalization prefixes.

use std::path::{Path, PathBuf};

pub(crate) fn without_windows_verbatim_prefix(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let value = path.to_string_lossy();
        if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = value.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_paths_are_unchanged() {
        let path = Path::new("synthetic/project");
        assert_eq!(without_windows_verbatim_prefix(path), path);
    }

    #[cfg(windows)]
    #[test]
    fn canonical_drive_and_unc_paths_keep_their_shell_form() {
        assert_eq!(
            without_windows_verbatim_prefix(Path::new(r"\\?\C:\synthetic\workspace")),
            PathBuf::from(r"C:\synthetic\workspace")
        );
        assert_eq!(
            without_windows_verbatim_prefix(Path::new(r"\\?\UNC\server\share\workspace")),
            PathBuf::from(r"\\server\share\workspace")
        );
    }
}
