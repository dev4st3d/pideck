use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathAction {
    Reveal,
    OpenFolder,
}

pub fn activate_untrusted_output_path(path: &str, action: PathAction) -> Result<(), String> {
    if path.is_empty() || path.chars().any(char::is_control) {
        return Err("The output path is not safe to pass to the platform shell.".to_owned());
    }
    let path = Path::new(path);
    let target = match action {
        PathAction::Reveal => path,
        PathAction::OpenFolder => path.parent().unwrap_or(path),
    };

    #[cfg(windows)]
    {
        let mut command = Command::new("explorer.exe");
        if action == PathAction::Reveal {
            command.arg("/select,");
        }
        command
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|_| "Windows Explorer could not be opened.".to_owned())
    }

    #[cfg(not(windows))]
    {
        let folder = if action == PathAction::Reveal {
            target.parent().unwrap_or(target)
        } else {
            target
        };
        Command::new("xdg-open")
            .arg(folder)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|_| "The output folder could not be opened.".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_characters_are_rejected_before_platform_dispatch() {
        assert!(activate_untrusted_output_path("bad\u{1b}path", PathAction::Reveal).is_err());
    }
}
