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

pub fn open_provider_auth_url(url: &str) -> Result<(), String> {
    if url.chars().any(char::is_control)
        || !(url.starts_with("https://") || url.starts_with("http://localhost"))
    {
        return Err("The provider returned an unsupported authentication URL.".to_owned());
    }

    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let url = OsStr::new(url)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        // ShellExecuteW follows the user's registered HTTPS handler. The URL is
        // validated above and passed as one UTF-16 value, never through a shell.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                std::ptr::null(),
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if result as isize > 32 {
            Ok(())
        } else {
            Err("The authentication page could not be opened in your default browser.".to_owned())
        }
    }

    #[cfg(not(windows))]
    {
        Command::new("xdg-open")
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|_| "The authentication page could not be opened.".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_characters_are_rejected_before_platform_dispatch() {
        assert!(activate_untrusted_output_path("bad\u{1b}path", PathAction::Reveal).is_err());
        assert!(open_provider_auth_url("https://example.test/\u{1b}").is_err());
    }

    #[test]
    fn provider_auth_rejects_unsafe_browser_schemes() {
        assert!(open_provider_auth_url("file:///private/token").is_err());
        assert!(open_provider_auth_url("http://provider.example/login").is_err());
    }
}
