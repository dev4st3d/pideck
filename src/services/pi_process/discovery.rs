use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::diagnostics::redact_diagnostic;
use super::platform::{ExitStatus, spawn_contained};

pub const SUPPORTED_PI_VERSION: &str = "0.80.10";
const MAX_PROBE_OUTPUT: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutableSource {
    Explicit,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiCapabilities {
    pub rpc_mode: bool,
    pub explicit_trust: bool,
    pub explicit_session: bool,
    pub resource_controls: bool,
}

impl PiCapabilities {
    fn from_help(help: &str) -> (Self, Vec<&'static str>) {
        let required = [
            ("--mode <mode>", "RPC mode"),
            ("--approve", "approved project trust"),
            ("--no-approve", "unapproved project trust"),
            ("--no-session", "ephemeral sessions"),
            ("--session <path|id>", "existing sessions"),
            ("--session-id <id>", "session IDs"),
            ("--session-dir <dir>", "session directories"),
            ("--no-extensions", "extension discovery controls"),
            ("--extension", "explicit extensions"),
            ("--no-skills", "skill discovery controls"),
            ("--skill", "explicit skills"),
            (
                "--no-prompt-templates",
                "prompt-template discovery controls",
            ),
            ("--prompt-template", "explicit prompt templates"),
            ("--no-themes", "theme discovery controls"),
            ("--theme", "explicit themes"),
            ("--no-context-files", "context-file controls"),
        ];
        let mut missing = required
            .into_iter()
            .filter_map(|(needle, name)| (!help.contains(needle)).then_some(name))
            .collect::<Vec<_>>();
        if help.contains("--mode <mode>") && !help.contains("rpc") {
            missing.push("RPC mode");
        }

        (
            Self {
                rpc_mode: help.contains("--mode <mode>") && help.contains("rpc"),
                explicit_trust: help.contains("--approve") && help.contains("--no-approve"),
                explicit_session: help.contains("--no-session")
                    && help.contains("--session <path|id>"),
                resource_controls: help.contains("--no-extensions")
                    && help.contains("--extension")
                    && help.contains("--no-skills")
                    && help.contains("--skill")
                    && help.contains("--no-prompt-templates")
                    && help.contains("--prompt-template")
                    && help.contains("--no-themes")
                    && help.contains("--theme")
                    && help.contains("--no-context-files"),
            },
            missing,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiInstallation {
    pub executable: PathBuf,
    pub launcher_arguments: Vec<OsString>,
    pub source: ExecutableSource,
    pub version: String,
    pub capabilities: PiCapabilities,
}

#[derive(Debug)]
pub enum DiscoveryError {
    MissingExplicit(PathBuf),
    NotAFile(PathBuf),
    NotExecutable(PathBuf),
    MissingFromPath,
    UnsafeScript(PathBuf),
    InvalidNpmInstall {
        path: PathBuf,
        message: String,
    },
    Io {
        operation: &'static str,
        source: io::Error,
    },
    ProbeTimedOut {
        operation: &'static str,
        timeout: Duration,
    },
    MissingProbePipe(&'static str),
    ProbeFailed {
        operation: &'static str,
        status: ExitStatus,
        detail: String,
    },
    IncompatibleVersion {
        found: String,
        required: &'static str,
    },
    MissingCapabilities(Vec<&'static str>),
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingExplicit(path) => {
                write!(formatter, "the configured Pi executable does not exist: {}", path.display())
            }
            Self::NotAFile(path) => {
                write!(formatter, "the configured Pi executable is not a file: {}", path.display())
            }
            Self::NotExecutable(path) => {
                write!(formatter, "the configured Pi executable is not executable: {}", path.display())
            }
            Self::MissingFromPath => formatter.write_str(
                "Pi was not found on PATH; install @earendil-works/pi-coding-agent or configure an explicit executable",
            ),
            Self::UnsafeScript(path) => write!(
                formatter,
                "refusing to launch {} through a command shell; configure a native executable or a standard npm Pi shim",
                path.display()
            ),
            Self::InvalidNpmInstall { path, message } => {
                write!(formatter, "the npm Pi install at {} is invalid: {message}", path.display())
            }
            Self::Io { operation, source } => write!(formatter, "could not {operation}: {source}"),
            Self::ProbeTimedOut { operation, timeout } => write!(
                formatter,
                "Pi {operation} did not finish within {} ms",
                timeout.as_millis()
            ),
            Self::MissingProbePipe(pipe) => {
                write!(formatter, "the Pi compatibility probe started without a {pipe} pipe")
            }
            Self::ProbeFailed {
                operation,
                status,
                detail,
            } => write!(formatter, "Pi {operation} failed ({status}): {detail}"),
            Self::IncompatibleVersion { found, required } => write!(
                formatter,
                "Pi {found} is incompatible; this build requires exactly {required}"
            ),
            Self::MissingCapabilities(capabilities) => write!(
                formatter,
                "Pi is missing required command-line capabilities: {}",
                capabilities.join(", ")
            ),
        }
    }
}

impl std::error::Error for DiscoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn discover_and_probe(
    executable_override: Option<&Path>,
    timeout: Duration,
) -> Result<PiInstallation, DiscoveryError> {
    let (candidate, source) = match executable_override {
        Some(path) => (validate_explicit(path)?, ExecutableSource::Explicit),
        None => (find_on_path()?, ExecutableSource::Path),
    };
    let (executable, launcher_arguments) = resolve_launcher(&candidate)?;
    let working_directory = executable
        .parent()
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| Path::new("."));

    let version_output = run_probe(
        &executable,
        &launcher_arguments,
        OsStr::new("--version"),
        working_directory,
        timeout,
        "version probe",
    )?;
    let version = first_nonempty_line(&version_output).unwrap_or_default();
    if version != SUPPORTED_PI_VERSION {
        return Err(DiscoveryError::IncompatibleVersion {
            found: if version.is_empty() {
                "an unknown version".to_owned()
            } else {
                version
            },
            required: SUPPORTED_PI_VERSION,
        });
    }

    let help = run_probe(
        &executable,
        &launcher_arguments,
        OsStr::new("--help"),
        working_directory,
        timeout,
        "capability probe",
    )?;
    let (capabilities, missing) = PiCapabilities::from_help(&help);
    if !missing.is_empty()
        || !capabilities.rpc_mode
        || !capabilities.explicit_trust
        || !capabilities.explicit_session
        || !capabilities.resource_controls
    {
        return Err(DiscoveryError::MissingCapabilities(missing));
    }

    Ok(PiInstallation {
        executable,
        launcher_arguments,
        source,
        version,
        capabilities,
    })
}

fn validate_explicit(path: &Path) -> Result<PathBuf, DiscoveryError> {
    if !path.exists() {
        return Err(DiscoveryError::MissingExplicit(path.to_path_buf()));
    }
    if !path.is_file() {
        return Err(DiscoveryError::NotAFile(path.to_path_buf()));
    }
    ensure_executable(path)?;
    fs::canonicalize(path).map_err(|source| DiscoveryError::Io {
        operation: "canonicalize the configured Pi executable",
        source,
    })
}

fn find_on_path() -> Result<PathBuf, DiscoveryError> {
    let Some(path_value) = env::var_os("PATH") else {
        return Err(DiscoveryError::MissingFromPath);
    };
    find_in_directories(env::split_paths(&path_value)).ok_or(DiscoveryError::MissingFromPath)
}

fn find_in_directories(directories: impl Iterator<Item = PathBuf>) -> Option<PathBuf> {
    for directory in directories.filter(|path| !path.as_os_str().is_empty()) {
        for name in executable_names() {
            let candidate = directory.join(name);
            if candidate.is_file() && ensure_executable(&candidate).is_ok() {
                if let Ok(path) = fs::canonicalize(candidate) {
                    return Some(path);
                }
            }
        }
    }
    None
}

#[cfg(windows)]
fn executable_names() -> &'static [&'static str] {
    &["pi.exe", "pi.com", "pi.cmd"]
}

#[cfg(not(windows))]
fn executable_names() -> &'static [&'static str] {
    &["pi"]
}

#[cfg(unix)]
fn ensure_executable(path: &Path) -> Result<(), DiscoveryError> {
    use std::os::unix::fs::PermissionsExt;

    let mode = path
        .metadata()
        .map_err(|source| DiscoveryError::Io {
            operation: "read Pi executable metadata",
            source,
        })?
        .permissions()
        .mode();
    if mode & 0o111 == 0 {
        return Err(DiscoveryError::NotExecutable(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_executable(_path: &Path) -> Result<(), DiscoveryError> {
    Ok(())
}

fn resolve_launcher(candidate: &Path) -> Result<(PathBuf, Vec<OsString>), DiscoveryError> {
    let extension = candidate
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();

    if extension == "cmd" {
        return resolve_npm_shim(candidate);
    }
    if matches!(extension.as_str(), "bat" | "ps1") {
        return Err(DiscoveryError::UnsafeScript(candidate.to_path_buf()));
    }

    Ok((candidate.to_path_buf(), Vec::new()))
}

#[derive(Deserialize)]
struct PackageManifest {
    name: String,
    version: String,
    bin: PackageBin,
}

#[derive(Deserialize)]
struct PackageBin {
    pi: String,
}

fn resolve_npm_shim(shim: &Path) -> Result<(PathBuf, Vec<OsString>), DiscoveryError> {
    let Some(path_value) = env::var_os("PATH") else {
        return Err(DiscoveryError::InvalidNpmInstall {
            path: shim.to_path_buf(),
            message: "node.exe was not found on PATH".to_owned(),
        });
    };
    resolve_npm_shim_with_paths(shim, env::split_paths(&path_value))
}

fn resolve_npm_shim_with_paths(
    shim: &Path,
    node_search_paths: impl Iterator<Item = PathBuf>,
) -> Result<(PathBuf, Vec<OsString>), DiscoveryError> {
    let Some(bin_directory) = shim.parent() else {
        return Err(DiscoveryError::UnsafeScript(shim.to_path_buf()));
    };
    let package_directory = bin_directory
        .join("node_modules")
        .join("@earendil-works")
        .join("pi-coding-agent");
    let manifest_path = package_directory.join("package.json");
    let bytes = fs::read(&manifest_path).map_err(|source| DiscoveryError::InvalidNpmInstall {
        path: shim.to_path_buf(),
        message: format!("could not read {}: {source}", manifest_path.display()),
    })?;
    let manifest: PackageManifest =
        serde_json::from_slice(&bytes).map_err(|source| DiscoveryError::InvalidNpmInstall {
            path: shim.to_path_buf(),
            message: format!("could not parse {}: {source}", manifest_path.display()),
        })?;
    if manifest.name != "@earendil-works/pi-coding-agent"
        || manifest.version != SUPPORTED_PI_VERSION
        || manifest.bin.pi != "dist/cli.js"
    {
        return Err(DiscoveryError::InvalidNpmInstall {
            path: shim.to_path_buf(),
            message:
                "package identity, version, or bin entry does not match the supported Pi contract"
                    .to_owned(),
        });
    }

    let package_directory = fs::canonicalize(&package_directory).map_err(|source| {
        DiscoveryError::InvalidNpmInstall {
            path: shim.to_path_buf(),
            message: format!("could not canonicalize the package directory: {source}"),
        }
    })?;
    let cli = fs::canonicalize(package_directory.join(&manifest.bin.pi)).map_err(|source| {
        DiscoveryError::InvalidNpmInstall {
            path: shim.to_path_buf(),
            message: format!("could not resolve the package CLI: {source}"),
        }
    })?;
    if !cli.starts_with(&package_directory) || !cli.is_file() {
        return Err(DiscoveryError::InvalidNpmInstall {
            path: shim.to_path_buf(),
            message: "the package CLI resolves outside the package directory".to_owned(),
        });
    }

    let node = find_program_in_directories(node_search_paths, node_executable_names()).ok_or_else(
        || DiscoveryError::InvalidNpmInstall {
            path: shim.to_path_buf(),
            message: "node.exe was not found on PATH".to_owned(),
        },
    )?;

    Ok((
        child_compatible_path(node),
        vec![child_compatible_path(cli).into_os_string()],
    ))
}

#[cfg(windows)]
fn node_executable_names() -> &'static [&'static str] {
    &["node.exe"]
}

#[cfg(not(windows))]
fn node_executable_names() -> &'static [&'static str] {
    &["node"]
}

fn find_program_in_directories(
    directories: impl Iterator<Item = PathBuf>,
    names: &[&str],
) -> Option<PathBuf> {
    for directory in directories.filter(|path| !path.as_os_str().is_empty()) {
        for name in names {
            let candidate = directory.join(name);
            if candidate.is_file() && ensure_executable(&candidate).is_ok() {
                if let Ok(path) = fs::canonicalize(candidate) {
                    return Some(path);
                }
            }
        }
    }
    None
}

#[cfg(windows)]
fn child_compatible_path(path: PathBuf) -> PathBuf {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    let encoded = path.as_os_str().encode_wide().collect::<Vec<_>>();
    let verbatim = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    if !encoded.starts_with(&verbatim) {
        return path;
    }
    let unc = [b'U' as u16, b'N' as u16, b'C' as u16, b'\\' as u16];
    let normalized = if encoded[verbatim.len()..].starts_with(&unc) {
        [b'\\' as u16, b'\\' as u16]
            .into_iter()
            .chain(encoded[verbatim.len() + unc.len()..].iter().copied())
            .collect()
    } else {
        encoded[verbatim.len()..].to_vec()
    };
    PathBuf::from(OsString::from_wide(&normalized))
}

#[cfg(not(windows))]
fn child_compatible_path(path: PathBuf) -> PathBuf {
    path
}

fn run_probe(
    executable: &Path,
    launcher_arguments: &[OsString],
    probe_argument: &OsStr,
    working_directory: &Path,
    timeout: Duration,
    operation: &'static str,
) -> Result<String, DiscoveryError> {
    let mut arguments = launcher_arguments.to_vec();
    arguments.push(probe_argument.to_os_string());
    let mut process =
        spawn_contained(executable, &arguments, working_directory, &[]).map_err(|source| {
            DiscoveryError::Io {
                operation: "start the Pi compatibility probe",
                source,
            }
        })?;
    drop(process.stdin.take());

    let Some(stdout) = process.stdout.take() else {
        let _ = process.handle.terminate();
        return Err(DiscoveryError::MissingProbePipe("stdout"));
    };
    let Some(stderr) = process.stderr.take() else {
        let _ = process.handle.terminate();
        return Err(DiscoveryError::MissingProbePipe("stderr"));
    };
    let stdout_reader = spawn_probe_reader(stdout);
    let stderr_reader = spawn_probe_reader(stderr);

    let started = Instant::now();
    let status = loop {
        match process
            .handle
            .try_wait()
            .map_err(|source| DiscoveryError::Io {
                operation: "wait for the Pi compatibility probe",
                source,
            })? {
            Some(status) => break status,
            None if started.elapsed() >= timeout => {
                let _ = process.handle.terminate();
                let _ = process.handle.wait_for(Duration::from_secs(2));
                return Err(DiscoveryError::ProbeTimedOut { operation, timeout });
            }
            None => thread::sleep(Duration::from_millis(5)),
        }
    };

    // The probe's root may have spawned descendants that inherited its pipes. End the contained
    // tree before joining drainers so a malformed installation cannot hang discovery.
    process
        .handle
        .terminate()
        .map_err(|source| DiscoveryError::Io {
            operation: "clean up the Pi compatibility probe process tree",
            source,
        })?;
    let stdout = stdout_reader
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| DiscoveryError::ProbeTimedOut {
            operation: "stdout drain",
            timeout: Duration::from_secs(2),
        })?;
    let stderr = stderr_reader
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| DiscoveryError::ProbeTimedOut {
            operation: "stderr drain",
            timeout: Duration::from_secs(2),
        })?;
    let combined = if stdout.is_empty() {
        stderr.clone()
    } else {
        stdout
    };
    if !status.success() {
        return Err(DiscoveryError::ProbeFailed {
            operation,
            status,
            detail: concise_probe_detail(&stderr),
        });
    }
    Ok(String::from_utf8_lossy(&combined).into_owned())
}

fn spawn_probe_reader(stream: Box<dyn Read + Send>) -> mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = sender.send(read_probe_stream(stream));
    });
    receiver
}

fn read_probe_stream(mut stream: Box<dyn Read + Send>) -> Vec<u8> {
    let mut retained = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                let remaining = MAX_PROBE_OUTPUT.saturating_sub(retained.len());
                retained.extend_from_slice(&buffer[..count.min(remaining)]);
            }
        }
    }
    retained
}

fn first_nonempty_line(bytes: &str) -> Option<String> {
    bytes
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn concise_probe_detail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let first = text.lines().map(str::trim).find(|line| !line.is_empty());
    redact_diagnostic(first.unwrap_or("no diagnostic output"))
        .chars()
        .take(240)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_probe_requires_every_launch_control() {
        let help = "--mode <mode> rpc --approve --no-approve --no-session --session <path|id> \
                    --session-id <id> --session-dir <dir> --no-extensions --extension <path> \
                    --no-skills --skill <path> --no-prompt-templates --prompt-template <path> \
                    --no-themes --theme <path> --no-context-files";
        let (capabilities, missing) = PiCapabilities::from_help(help);
        assert!(missing.is_empty());
        assert!(capabilities.rpc_mode);
        assert!(capabilities.explicit_trust);
        assert!(capabilities.explicit_session);
        assert!(capabilities.resource_controls);
    }

    #[test]
    fn capability_probe_reports_missing_controls() {
        let (_, missing) = PiCapabilities::from_help("--mode <mode> rpc");
        assert!(missing.contains(&"approved project trust"));
        assert!(missing.contains(&"context-file controls"));
    }

    #[test]
    fn path_search_uses_explicit_entries_without_a_shell() {
        let root = std::env::temp_dir().join(format!("pi-gui-path-search-{}", std::process::id()));
        let first = root.join("empty");
        let second = root.join("bin with spaces 日本語");
        fs::create_dir_all(&first).expect("create empty PATH entry");
        fs::create_dir_all(&second).expect("create populated PATH entry");
        let candidate = second.join(executable_names()[0]);
        fs::write(&candidate, b"fake").expect("write candidate");
        make_test_executable(&candidate);

        let found = find_in_directories([first, second].into_iter()).expect("find Pi on PATH");
        assert_eq!(
            found,
            fs::canonicalize(candidate).expect("canonical candidate")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn validated_npm_shim_resolves_to_node_without_cmd() {
        let root = std::env::temp_dir().join(format!("pi-gui-npm-shim-{}", std::process::id()));
        let package = root
            .join("node_modules")
            .join("@earendil-works")
            .join("pi-coding-agent");
        fs::create_dir_all(package.join("dist")).expect("create fake npm package");
        let shim = root.join("pi.cmd");
        let node = root.join("node.exe");
        fs::write(&shim, "untrusted shell content").expect("write shim");
        fs::write(&node, "fake node").expect("write node");
        fs::write(package.join("dist/cli.js"), "fake cli").expect("write cli");
        fs::write(
            package.join("package.json"),
            r#"{"name":"@earendil-works/pi-coding-agent","version":"0.80.10","bin":{"pi":"dist/cli.js"}}"#,
        )
        .expect("write manifest");

        let (launcher, prefix) = resolve_npm_shim_with_paths(&shim, [root.clone()].into_iter())
            .expect("resolve standard npm shim");
        assert_eq!(
            launcher,
            child_compatible_path(fs::canonicalize(node).expect("canonical node"))
        );
        assert_eq!(prefix.len(), 1);
        assert!(Path::new(&prefix[0]).ends_with("dist/cli.js"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn shell_scripts_other_than_validated_npm_shims_are_rejected() {
        let error = resolve_launcher(Path::new("unsafe user launcher.ps1"))
            .expect_err("PowerShell launchers must be rejected");
        assert!(matches!(error, DiscoveryError::UnsafeScript(_)));
    }

    #[cfg(unix)]
    fn make_test_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = path.metadata().expect("candidate metadata").permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions).expect("make candidate executable");
    }

    #[cfg(not(unix))]
    fn make_test_executable(_path: &Path) {}
}
