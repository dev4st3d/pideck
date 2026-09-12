//! Explicit explorer operations. Existing destinations are never overwritten.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
pub(crate) fn copy_to_system_clipboard(paths: &[PathBuf]) -> Result<(), String> {
    let paths: Vec<_> = paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    use clipboard_win::Setter;
    let _clipboard = clipboard_win::Clipboard::new()
        .map_err(|_| "Clipboard is busy. Try copying again.".to_owned())?;
    clipboard_win::formats::FileList
        .write_clipboard(paths.as_slice())
        .map_err(|_| "Clipboard is busy. Try copying again.".into())
}

#[cfg(windows)]
pub(crate) fn system_clipboard_files() -> Result<Vec<PathBuf>, String> {
    let paths: Vec<String> = clipboard_win::get_clipboard(clipboard_win::formats::FileList)
        .map_err(|_| "Copy files first, then paste into a project folder.".to_owned())?;
    Ok(paths.into_iter().map(PathBuf::from).collect())
}

#[derive(Clone, Debug)]
pub(crate) enum Operation {
    Create {
        path: PathBuf,
        directory: bool,
    },
    Rename {
        from: PathBuf,
        to: PathBuf,
    },
    Transfer {
        paths: Vec<PathBuf>,
        destination: PathBuf,
        cut: bool,
    },
    Trash(Vec<PathBuf>),
}

#[derive(Default)]
pub(crate) struct Outcome {
    pub(crate) moved: Vec<(PathBuf, PathBuf)>,
    pub(crate) created: Vec<PathBuf>,
    pub(crate) removed: Vec<PathBuf>,
    pub(crate) error: Option<String>,
}

pub(crate) fn unique_roots(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort();
    paths.dedup();
    let mut roots: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !roots.iter().any(|parent| path.starts_with(parent)) {
            roots.push(path);
        }
    }
    roots
}

pub(crate) fn named_path(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if name.is_empty()
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        || Path::new(name)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        return Err("Enter a valid file or folder name without path separators.".into());
    }
    Ok(parent.join(name))
}

fn confined(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let path = fs::canonicalize(path).map_err(|_| "This path is no longer available.")?;
    if !path.starts_with(root) || path == root {
        return Err("Choose an item inside this project.".into());
    }
    Ok(path)
}

fn destination(root: &Path, path: &Path) -> Result<(), String> {
    let parent = path.parent().ok_or("Choose a destination folder.")?;
    let parent = fs::canonicalize(parent).map_err(|_| "The destination folder is unavailable.")?;
    if !parent.starts_with(root) {
        return Err("Choose a destination inside this project.".into());
    }
    if fs::symlink_metadata(path).is_ok() {
        return Err(
            "An item with that name already exists. Rename it or choose another folder.".into(),
        );
    }
    Ok(())
}

fn copy(source: &Path, target: &Path, cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        return Err("Copy cancelled. Completed files remain in the destination.".into());
    }
    let metadata = fs::symlink_metadata(source).map_err(|_| "Could not read the source item.")?;
    if metadata.file_type().is_symlink() {
        return Err("Links cannot be copied recursively. Copy their target explicitly.".into());
    }
    if metadata.is_dir() {
        fs::create_dir(target).map_err(|_| "Could not create the destination folder.")?;
        for entry in fs::read_dir(source).map_err(|_| "Could not read this folder.")? {
            let entry = entry.map_err(|_| "Could not read a folder entry.")?;
            copy(&entry.path(), &target.join(entry.file_name()), cancel)?;
        }
    } else if metadata.is_file() {
        let mut input = fs::File::open(source).map_err(|_| "Could not open the source file.")?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(
                |_| "Could not create the destination file. Check its name and permissions.",
            )?;
        let mut buffer = [0; 64 * 1024];
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err(
                    "Copy cancelled. The source is intact; the destination file may be incomplete."
                        .into(),
                );
            }
            let count = input
                .read(&mut buffer)
                .map_err(|_| "Could not finish reading the source file.")?;
            if count == 0 {
                break;
            }
            output
                .write_all(&buffer[..count])
                .map_err(|_| "Copy could not finish. Check free space and permissions.")?;
        }
    } else {
        return Err("Only regular files and folders can be copied.".into());
    }
    Ok(())
}

fn recycle(path: &Path) -> Result<(), String> {
    const ERROR: &str =
        "Could not move this item to the Recycle Bin. Check permissions and open applications.";
    // GPUI uses Windows Runtime MTA workers, but trash initializes STA COM.
    // A fresh thread avoids RPC_E_CHANGED_MODE; joining contains library panics.
    #[cfg(windows)]
    {
        let path = path.to_path_buf();
        std::thread::Builder::new()
            .name("pideck-recycle".into())
            .spawn(move || trash::delete(path))
            .map_err(|_| ERROR.to_owned())?
            .join()
            .map_err(|_| ERROR.to_owned())?
            .map_err(|_| ERROR.to_owned())
    }
    #[cfg(not(windows))]
    trash::delete(path).map_err(|_| ERROR.to_owned())
}

pub(crate) fn run(root: &Path, operation: Operation, cancel: &AtomicBool) -> Outcome {
    let mut outcome = Outcome::default();
    let result = (|| -> Result<(), String> {
        if cancel.load(Ordering::Acquire) {
            return Err("Operation cancelled.".into());
        }
        let root = fs::canonicalize(root).map_err(|_| "The project folder is unavailable.")?;
        match operation {
            Operation::Create { path, directory } => {
                destination(&root, &path)?;
                if directory {
                    fs::create_dir(&path).map_err(|_| "Could not create this folder.")?;
                } else {
                    OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)
                        .map_err(|_| "Could not create this file.")?;
                }
                outcome.created.push(path);
            }
            Operation::Rename { from, to } => {
                confined(&root, &from)?;
                destination(&root, &to)?;
                fs::rename(&from, &to).map_err(
                    |_| "Could not rename this item. Check permissions and open applications.",
                )?;
                outcome.moved.push((from, to));
            }
            Operation::Transfer {
                paths,
                destination: target,
                cut,
            } => {
                let canonical_target = fs::canonicalize(&target)
                    .map_err(|_| "Choose an available destination folder.")?;
                if !canonical_target.starts_with(&root) {
                    return Err("Choose a folder inside this project.".into());
                }
                let paths = unique_roots(paths);
                let mut transfers = Vec::new();
                for source in paths {
                    let canonical_source = fs::canonicalize(&source)
                        .map_err(|_| "A clipboard item is no longer available.")?;
                    if cut {
                        confined(&root, &source)?;
                    }
                    if canonical_target.starts_with(&canonical_source) {
                        return Err("A folder cannot be placed inside itself.".into());
                    }
                    let name = source
                        .file_name()
                        .ok_or("Choose a file or folder to copy.")?;
                    let mut to = target.join(name);
                    if !cut && fs::canonicalize(&to).ok().as_ref() == Some(&canonical_source) {
                        let stem = if source.is_file() {
                            source.file_stem().unwrap_or(name)
                        } else {
                            name
                        }
                        .to_string_lossy();
                        let extension = if source.is_file() {
                            source
                                .extension()
                                .map(|e| format!(".{}", e.to_string_lossy()))
                                .unwrap_or_default()
                        } else {
                            String::new()
                        };
                        for number in 1..=10_000 {
                            to = target.join(format!(
                                "{stem} copy{suffix}{extension}",
                                suffix = if number == 1 {
                                    String::new()
                                } else {
                                    format!(" {number}")
                                }
                            ));
                            if fs::symlink_metadata(&to).is_err() {
                                break;
                            }
                        }
                    }
                    destination(&root, &to)?;
                    if transfers
                        .iter()
                        .any(|(_, existing): &(PathBuf, PathBuf)| existing == &to)
                    {
                        return Err(
                            "Selected items have the same name. Copy them separately.".into()
                        );
                    }
                    transfers.push((source, to));
                }
                for (source, to) in transfers {
                    if cancel.load(Ordering::Acquire) {
                        return Err("Operation cancelled. Completed items are kept.".into());
                    }
                    if cut {
                        fs::rename(&source, &to).map_err(
                            |_| "Could not move this item. For another drive, copy it first.",
                        )?;
                        outcome.moved.push((source, to));
                    } else {
                        copy(&source, &to, cancel).map_err(|error| {
                            format!("{error} Some destination files may already have been copied.")
                        })?;
                        outcome.created.push(to);
                    }
                }
            }
            Operation::Trash(paths) => {
                let paths = unique_roots(paths);
                for path in &paths {
                    confined(&root, path)?;
                }
                for path in paths {
                    if cancel.load(Ordering::Acquire) {
                        return Err("Operation cancelled. Completed items are kept.".into());
                    }
                    recycle(&path)?;
                    outcome.removed.push(path);
                }
            }
        }
        Ok(())
    })();
    outcome.error = result.err();
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    #[ignore = "moves disposable files to the Windows Recycle Bin"]
    fn recycle_from_mta_worker() {
        std::thread::spawn(|| {
            use windows_sys::Win32::System::Com::{
                COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize,
            };
            // This test thread owns the successful COM initialization and
            // balances it before exiting, reproducing GPUI's worker apartment.
            let initialized =
                unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
            assert!(initialized >= 0);
            let root = std::env::temp_dir().join(format!(
                "pideck-recycle-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&root).unwrap();
            let path = root.join("disposable.txt");
            fs::write(&path, "PiDeck recycle regression fixture").unwrap();
            let cancelled = run(
                &root,
                Operation::Trash(vec![path.clone()]),
                &AtomicBool::new(true),
            );
            assert!(cancelled.error.is_some());
            assert!(path.exists());
            let outcome = run(
                &root,
                Operation::Trash(vec![path.clone()]),
                &AtomicBool::new(false),
            );
            assert!(outcome.error.is_none(), "{:?}", outcome.error);
            assert_eq!(outcome.removed, vec![path.clone()]);
            assert!(!path.exists());
            let missing = run(&root, Operation::Trash(vec![path]), &AtomicBool::new(false));
            assert!(missing.error.is_some());
            assert!(missing.removed.is_empty());
            fs::remove_dir(&root).unwrap();
            unsafe { CoUninitialize() };
        })
        .join()
        .unwrap();
    }

    #[test]
    fn selection_removes_descendants_and_rejects_invalid_names() {
        assert_eq!(
            unique_roots(vec![
                "src/a.rs".into(),
                "src".into(),
                "src".into(),
                "test.rs".into()
            ]),
            vec![PathBuf::from("src"), PathBuf::from("test.rs")]
        );
        for name in ["", "../a", "a/b", "a\\b", "CON.txt", "a:", ".", "file."] {
            assert!(named_path(Path::new("root"), name).is_err(), "{name}");
        }
        assert!(named_path(Path::new("root"), "hello world.rs").is_ok());
    }
    #[test]
    fn copy_collision_move_and_self_nesting() {
        let root = std::env::temp_dir().join(format!("pideck-operations-{}", std::process::id()));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.txt"), "original").unwrap();
        let cancel = AtomicBool::new(false);
        let transfer = |paths, destination, cut| {
            run(
                &root,
                Operation::Transfer {
                    paths,
                    destination,
                    cut,
                },
                &cancel,
            )
        };
        assert!(
            transfer(vec![root.join("src")], root.join("src"), false)
                .error
                .is_some()
        );
        assert!(
            transfer(vec![root.join("src/a.txt")], root.clone(), false)
                .error
                .is_none()
        );
        fs::write(root.join("a.txt"), "keep").unwrap();
        assert!(
            transfer(vec![root.join("src/a.txt")], root.clone(), false)
                .error
                .is_some()
        );
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "keep");
        assert!(
            transfer(vec![root.join("src/a.txt")], root.join("src"), false)
                .error
                .is_none()
        );
        assert!(root.join("src/a copy.txt").exists());
        assert!(
            run(
                &root,
                Operation::Rename {
                    from: root.join("src/a.txt"),
                    to: root.join("src/b.txt")
                },
                &cancel
            )
            .error
            .is_none()
        );
        assert!(!root.join("src/a.txt").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
