//! GUI-session extension policy.
//!
//! Pideck keeps the user's installed Pi extensions on disk and in settings unchanged.
//! For native GUI RPC sessions only, TUI chrome packages that are replaced by GPUI are
//! omitted from the child process by turning off discovery and re-passing every other
//! enabled extension path with `--extension`.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

const AGENT_DIR_ENV: &str = "PI_CODING_AGENT_DIR";

/// Local entry basenames / directory names replaced by native GPUI chrome.
const DISABLED_LOCAL_ENTRIES: &[&str] = &[
    "activity-rail.ts",
    "activity-rail.js",
    "box-editor.ts",
    "box-editor.js",
    "quiet-topbar.ts",
    "quiet-topbar.js",
    "compact-resources",
];

/// Installed package identities (npm bare name or path segment) replaced by native UI.
const DISABLED_PACKAGE_IDENTITIES: &[&str] = &["pi-bar"];

/// Apply the native-shell extension denylist to a launch resource policy.
///
/// When extension discovery is enabled (normal GUI profiles), this:
/// 1. resolves the same user-scope extensions Pi would load under rejected project trust,
/// 2. drops the TUI-chrome replacements listed above,
/// 3. switches to `--no-extensions` plus explicit `--extension` paths so Pi never loads the
///    denylisted modules for this process.
///
/// Pre-existing explicit paths (for example the orchestration adapter) are preserved when they
/// are not themselves denylisted. User settings and installed files are never modified.
pub fn apply_gui_extension_policy(resources: &mut super::ResourcePolicy, agent_dir: &Path) {
    let existing = std::mem::take(&mut resources.extensions);
    let mut kept_explicit = Vec::with_capacity(existing.len());
    for path in existing {
        if !is_disabled_for_gui(&path) {
            kept_explicit.push(path);
        }
    }

    if !resources.discover_extensions {
        resources.extensions = kept_explicit;
        return;
    }

    let mut allowed = discover_user_extension_entries(agent_dir)
        .into_iter()
        .filter(|path| !is_disabled_for_gui(path))
        .collect::<Vec<_>>();

    for path in kept_explicit {
        if !allowed.iter().any(|existing| paths_equal(existing, &path)) {
            allowed.push(path);
        }
    }

    resources.discover_extensions = false;
    resources.extensions = allowed;
}

/// Resolve Pi's agent directory the same way session catalog does.
pub fn resolve_agent_dir(workspace: &Path) -> PathBuf {
    std::env::var_os(AGENT_DIR_ENV)
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".pi").join("agent")))
        .unwrap_or_else(|| workspace.join(".pi").join("agent"))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn discover_user_extension_entries(agent_dir: &Path) -> Vec<PathBuf> {
    let mut entries = Vec::new();
    let mut seen = Vec::new();

    let push = |entries: &mut Vec<PathBuf>, seen: &mut Vec<String>, path: PathBuf| {
        if !path.exists() {
            return;
        }
        let key = normalize_path_key(&path);
        if seen.iter().any(|existing| existing == &key) {
            return;
        }
        seen.push(key);
        entries.push(path);
    };

    let settings = read_settings_object(&agent_dir.join("settings.json"));
    let override_patterns = settings
        .as_ref()
        .and_then(|value| value.get("extensions"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // Auto-discovered local extensions under ~/.pi/agent/extensions.
    for path in collect_auto_extension_entries(&agent_dir.join("extensions")) {
        if is_enabled_by_overrides(&path, &override_patterns, agent_dir) {
            push(&mut entries, &mut seen, path);
        }
    }

    // Explicit local paths from settings.extensions (plain entries only).
    for pattern in &override_patterns {
        if is_override_pattern(pattern) {
            continue;
        }
        let resolved = resolve_from_base(pattern, agent_dir);
        for path in expand_local_extension_path(&resolved) {
            if is_enabled_by_overrides(&path, &override_patterns, agent_dir) {
                push(&mut entries, &mut seen, path);
            }
        }
    }

    // User-scoped packages from settings.packages (project packages stay off under --no-approve).
    if let Some(packages) = settings
        .as_ref()
        .and_then(|value| value.get("packages"))
        .and_then(Value::as_array)
    {
        for package in packages {
            for path in package_extension_entries(package, agent_dir) {
                push(&mut entries, &mut seen, path);
            }
        }
    }

    entries
}

fn collect_auto_extension_entries(dir: &Path) -> Vec<PathBuf> {
    let mut entries = Vec::new();
    if !dir.is_dir() {
        return entries;
    }

    // Directory itself may be a package root with a pi manifest / index.
    if let Some(root_entries) = resolve_extension_entries(dir) {
        return root_entries;
    }

    let Ok(read_dir) = fs::read_dir(dir) else {
        return entries;
    };
    for entry in read_dir.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let path = entry.path();
        let file_type = entry.file_type().ok();
        let is_file = file_type.as_ref().is_some_and(|kind| kind.is_file()) || path.is_file();
        let is_dir = file_type.as_ref().is_some_and(|kind| kind.is_dir()) || path.is_dir();
        if is_file && is_extension_file(&name) {
            entries.push(path);
            continue;
        }
        if is_dir {
            if let Some(resolved) = resolve_extension_entries(&path) {
                entries.extend(resolved);
            }
        }
    }
    entries
}

fn resolve_extension_entries(dir: &Path) -> Option<Vec<PathBuf>> {
    let package_json = dir.join("package.json");
    if package_json.is_file() {
        if let Some(manifest_paths) = read_pi_extension_manifest(&package_json, dir) {
            if !manifest_paths.is_empty() {
                return Some(manifest_paths);
            }
        }
    }
    for name in ["index.ts", "index.js"] {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(vec![candidate]);
        }
    }
    None
}

fn read_pi_extension_manifest(package_json: &Path, package_root: &Path) -> Option<Vec<PathBuf>> {
    let text = fs::read_to_string(package_json).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let extensions = value.get("pi")?.get("extensions")?.as_array()?;
    let mut paths = Vec::new();
    for entry in extensions {
        let Some(relative) = entry.as_str() else {
            continue;
        };
        // Manifest paths are plain relatives; skip glob-heavy patterns for the GUI denylist path.
        if relative.contains('*') || relative.contains('?') || relative.contains('[') {
            continue;
        }
        let resolved = package_root.join(relative);
        if resolved.exists() {
            paths.push(resolved);
        }
    }
    Some(paths)
}

fn expand_local_extension_path(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        return vec![path.to_path_buf()];
    }
    if path.is_dir() {
        return collect_auto_extension_entries(path);
    }
    Vec::new()
}

fn package_extension_entries(package: &Value, agent_dir: &Path) -> Vec<PathBuf> {
    let (source, extension_filter) = match package {
        Value::String(source) => (source.as_str(), None),
        Value::Object(map) => {
            let source = map
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let filter = map.get("extensions").cloned();
            (source, filter)
        }
        _ => return Vec::new(),
    };
    if source.is_empty() {
        return Vec::new();
    }
    if package_identity_disabled(source) {
        return Vec::new();
    }
    // Object form with `"extensions": []` means load none of that package's extensions.
    if let Some(Value::Array(patterns)) = &extension_filter {
        if patterns.is_empty() {
            return Vec::new();
        }
    }

    let Some(package_root) = resolve_package_root(source, agent_dir) else {
        return Vec::new();
    };

    let mut all_files = if let Some(from_manifest) =
        read_pi_extension_manifest(&package_root.join("package.json"), &package_root)
    {
        from_manifest
    } else {
        let extensions_dir = package_root.join("extensions");
        if extensions_dir.is_dir() {
            collect_auto_extension_entries(&extensions_dir)
        } else {
            resolve_extension_entries(&package_root).unwrap_or_default()
        }
    };

    if let Some(Value::Array(patterns)) = extension_filter {
        let pattern_strings = patterns
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        all_files.retain(|path| package_path_allowed(path, &pattern_strings, &package_root));
    }

    all_files
        .into_iter()
        .filter(|path| !is_disabled_for_gui(path))
        .collect()
}

fn resolve_package_root(source: &str, agent_dir: &Path) -> Option<PathBuf> {
    let source = source.trim();
    if let Some(spec) = source.strip_prefix("npm:") {
        let package_name = npm_package_name(spec);
        let managed = npm_package_dir(&agent_dir.join("npm").join("node_modules"), &package_name);
        return managed.is_dir().then_some(managed);
    }

    if source.starts_with("git:")
        || source.starts_with("https://")
        || source.starts_with("http://")
        || source.starts_with("ssh://")
    {
        // Git packages are uncommon for the denylisted set; skip complex resolution.
        return None;
    }

    // Local path package source.
    let path = resolve_from_base(source, agent_dir);
    path.is_dir().then_some(path)
}

fn npm_package_name(spec: &str) -> String {
    let spec = spec.trim();
    if let Some(rest) = spec.strip_prefix('@') {
        // @scope/name or @scope/name@version
        if let Some((scope, name_and_maybe_version)) = rest.split_once('/') {
            let name = name_and_maybe_version
                .split_once('@')
                .map(|(name, _)| name)
                .unwrap_or(name_and_maybe_version);
            return format!("@{scope}/{name}");
        }
        return spec.to_owned();
    }
    spec.split_once('@')
        .map(|(name, _)| name.to_owned())
        .unwrap_or_else(|| spec.to_owned())
}

fn npm_package_dir(node_modules: &Path, package_name: &str) -> PathBuf {
    if let Some(rest) = package_name.strip_prefix('@') {
        if let Some((scope, name)) = rest.split_once('/') {
            return node_modules.join(format!("@{scope}")).join(name);
        }
    }
    node_modules.join(package_name)
}

fn package_path_allowed(path: &Path, patterns: &[String], package_root: &Path) -> bool {
    if patterns.is_empty() {
        return true;
    }
    // Minimal filter: plain includes, `!` / `-` excludes, `+` force-includes. No full minimatch.
    let rel = path
        .strip_prefix(package_root)
        .map(normalize_path_key)
        .unwrap_or_else(|_| normalize_path_key(path));
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut has_plain_include = false;
    let mut included = false;
    for pattern in patterns {
        if let Some(exact) = pattern.strip_prefix('+') {
            if path_matches_simple(&rel, &name, exact) {
                return true;
            }
            continue;
        }
        if let Some(exact) = pattern.strip_prefix('-') {
            if path_matches_simple(&rel, &name, exact) {
                return false;
            }
            continue;
        }
        if let Some(globish) = pattern.strip_prefix('!') {
            if path_matches_simple(&rel, &name, globish) {
                return false;
            }
            continue;
        }
        has_plain_include = true;
        if path_matches_simple(&rel, &name, pattern) {
            included = true;
        }
    }
    if has_plain_include { included } else { true }
}

fn path_matches_simple(rel: &str, name: &str, pattern: &str) -> bool {
    let pattern = pattern
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_owned();
    let pattern = pattern.trim_start_matches('/');
    rel == pattern
        || name == pattern
        || rel.ends_with(&format!("/{pattern}"))
        || rel.ends_with(pattern)
}

pub(crate) fn is_disabled_for_gui(path: &Path) -> bool {
    let key = normalize_path_key(path);
    let file_name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    for entry in DISABLED_LOCAL_ENTRIES {
        let entry = entry.to_ascii_lowercase();
        if file_name == entry {
            return true;
        }
        if key.ends_with(&format!("/{entry}")) {
            return true;
        }
        // Directory package such as compact-resources/index.ts
        if key.contains(&format!("/{entry}/")) {
            return true;
        }
    }

    for identity in DISABLED_PACKAGE_IDENTITIES {
        let identity = identity.to_ascii_lowercase();
        if key.contains(&format!("/node_modules/{identity}/"))
            || key.contains(&format!("/node_modules/{identity}\\"))
            || key.contains(&format!("/{identity}/extensions/"))
            || key.ends_with(&format!("/{identity}"))
        {
            return true;
        }
    }
    false
}

fn package_identity_disabled(source: &str) -> bool {
    let source = source.to_ascii_lowercase();
    for identity in DISABLED_PACKAGE_IDENTITIES {
        let identity = identity.to_ascii_lowercase();
        if source == identity
            || source == format!("npm:{identity}")
            || source.starts_with(&format!("npm:{identity}@"))
            || source.ends_with(&format!("/{identity}"))
            || source.contains(&format!("/{identity}@"))
        {
            return true;
        }
    }
    false
}

fn is_enabled_by_overrides(path: &Path, patterns: &[String], base_dir: &Path) -> bool {
    let rel = path
        .strip_prefix(base_dir)
        .map(normalize_path_key)
        .unwrap_or_else(|_| normalize_path_key(path));
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut enabled = true;
    for pattern in patterns {
        if let Some(globish) = pattern.strip_prefix('!') {
            if path_matches_simple(&rel, &name, globish) {
                enabled = false;
            }
        } else if let Some(exact) = pattern.strip_prefix('+') {
            if path_matches_simple(&rel, &name, exact) {
                enabled = true;
            }
        } else if let Some(exact) = pattern.strip_prefix('-') {
            if path_matches_simple(&rel, &name, exact) {
                enabled = false;
            }
        }
    }
    enabled
}

fn is_override_pattern(pattern: &str) -> bool {
    pattern.starts_with('!') || pattern.starts_with('+') || pattern.starts_with('-')
}

fn is_extension_file(name: &str) -> bool {
    name.ends_with(".ts") || name.ends_with(".js")
}

fn read_settings_object(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn resolve_from_base(input: &str, base_dir: &Path) -> PathBuf {
    let trimmed = input.trim();
    if trimmed.starts_with("~/") || trimmed.starts_with("~\\") {
        if let Some(home) = home_dir() {
            return home.join(&trimmed[2..]);
        }
    }
    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}

fn normalize_path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    normalize_path_key(left) == normalize_path_key(right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "pi-gui-ext-{label}-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("temp directory");
        path
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent");
        }
        let mut file = fs::File::create(path).expect("create");
        file.write_all(contents.as_bytes()).expect("write");
    }

    #[test]
    fn denylist_matches_local_and_package_paths() {
        assert!(is_disabled_for_gui(Path::new(
            r"C:\Users\test\.pi\agent\extensions\activity-rail.ts"
        )));
        assert!(is_disabled_for_gui(Path::new(
            r"C:\Users\test\.pi\agent\extensions\box-editor.ts"
        )));
        assert!(is_disabled_for_gui(Path::new(
            r"C:\Users\test\.pi\agent\extensions\quiet-topbar.ts"
        )));
        assert!(is_disabled_for_gui(Path::new(
            r"C:\Users\test\.pi\agent\extensions\compact-resources\index.ts"
        )));
        assert!(is_disabled_for_gui(Path::new(
            r"C:\Users\test\.pi\agent\npm\node_modules\pi-bar\extensions\status-footer.ts"
        )));
        assert!(!is_disabled_for_gui(Path::new(
            r"C:\Users\test\.pi\agent\extensions\compact-tools.ts"
        )));
        assert!(!is_disabled_for_gui(Path::new(
            r"C:\Users\test\.pi\agent\npm\node_modules\@tintinweb\pi-tasks\src\index.ts"
        )));
    }

    #[test]
    fn apply_policy_disables_discovery_and_keeps_functional_extensions() {
        let root = temp_dir("policy");
        let agent_dir = root.join("agent");
        let extensions = agent_dir.join("extensions");
        write_file(&extensions.join("activity-rail.ts"), "export default {};");
        write_file(&extensions.join("box-editor.ts"), "export default {};");
        write_file(&extensions.join("quiet-topbar.ts"), "export default {};");
        write_file(
            &extensions.join("compact-resources").join("index.ts"),
            "export default {};",
        );
        write_file(&extensions.join("compact-tools.ts"), "export default {};");
        write_file(
            &extensions.join("binance").join("index.ts"),
            "export default {};",
        );
        write_file(
            &agent_dir
                .join("npm")
                .join("node_modules")
                .join("pi-bar")
                .join("package.json"),
            r#"{"name":"pi-bar","pi":{"extensions":["./extensions/status-footer.ts"]}}"#,
        );
        write_file(
            &agent_dir
                .join("npm")
                .join("node_modules")
                .join("pi-bar")
                .join("extensions")
                .join("status-footer.ts"),
            "export default {};",
        );
        write_file(
            &agent_dir
                .join("npm")
                .join("node_modules")
                .join("@tintinweb")
                .join("pi-tasks")
                .join("package.json"),
            r#"{"name":"@tintinweb/pi-tasks","pi":{"extensions":["./src/index.ts"]}}"#,
        );
        write_file(
            &agent_dir
                .join("npm")
                .join("node_modules")
                .join("@tintinweb")
                .join("pi-tasks")
                .join("src")
                .join("index.ts"),
            "export default {};",
        );
        write_file(
            &agent_dir.join("settings.json"),
            r#"{
              "packages": [
                "npm:pi-bar",
                "npm:@tintinweb/pi-tasks"
              ]
            }"#,
        );

        let adapter = root.join("orchestration-adapter.mjs");
        write_file(&adapter, "export {};");

        let mut resources = super::super::ResourcePolicy::command_sources();
        resources.extensions.push(adapter.clone());
        apply_gui_extension_policy(&mut resources, &agent_dir);

        assert!(!resources.discover_extensions);
        let keys = resources
            .extensions
            .iter()
            .map(|path| normalize_path_key(path))
            .collect::<Vec<_>>();
        assert!(
            keys.iter().any(|path| path.ends_with("/compact-tools.ts")),
            "keys={keys:?}"
        );
        assert!(
            keys.iter().any(|path| path.ends_with("/binance/index.ts")),
            "keys={keys:?}"
        );
        assert!(
            keys.iter()
                .any(|path| path.contains("pi-tasks") && path.ends_with("/index.ts")),
            "keys={keys:?}"
        );
        assert!(
            keys.iter()
                .any(|path| path.ends_with("/orchestration-adapter.mjs")),
            "keys={keys:?}"
        );
        assert!(!keys.iter().any(|path| path.contains("activity-rail")));
        assert!(!keys.iter().any(|path| path.contains("box-editor")));
        assert!(!keys.iter().any(|path| path.contains("quiet-topbar")));
        assert!(!keys.iter().any(|path| path.contains("compact-resources")));
        assert!(!keys.iter().any(|path| path.contains("/pi-bar/")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn package_filter_empty_extensions_skips_package() {
        let root = temp_dir("filter");
        let agent_dir = root.join("agent");
        write_file(
            &agent_dir
                .join("npm")
                .join("node_modules")
                .join("keep-pkg")
                .join("package.json"),
            r#"{"name":"keep-pkg","pi":{"extensions":["./ext.ts"]}}"#,
        );
        write_file(
            &agent_dir
                .join("npm")
                .join("node_modules")
                .join("keep-pkg")
                .join("ext.ts"),
            "export default {};",
        );
        write_file(
            &agent_dir
                .join("npm")
                .join("node_modules")
                .join("skip-pkg")
                .join("package.json"),
            r#"{"name":"skip-pkg","pi":{"extensions":["./ext.ts"]}}"#,
        );
        write_file(
            &agent_dir
                .join("npm")
                .join("node_modules")
                .join("skip-pkg")
                .join("ext.ts"),
            "export default {};",
        );
        write_file(
            &agent_dir.join("settings.json"),
            r#"{
              "packages": [
                "npm:keep-pkg",
                { "source": "npm:skip-pkg", "extensions": [] }
              ]
            }"#,
        );

        let mut resources = super::super::ResourcePolicy::command_sources();
        apply_gui_extension_policy(&mut resources, &agent_dir);
        let keys = resources
            .extensions
            .iter()
            .map(|path| normalize_path_key(path))
            .collect::<Vec<_>>();
        assert!(keys.iter().any(|path| path.contains("/keep-pkg/")));
        assert!(!keys.iter().any(|path| path.contains("/skip-pkg/")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn npm_package_name_strips_versions_and_keeps_scope() {
        assert_eq!(npm_package_name("pi-bar"), "pi-bar");
        assert_eq!(npm_package_name("pi-bar@0.3.39"), "pi-bar");
        assert_eq!(
            npm_package_name("@tintinweb/pi-tasks"),
            "@tintinweb/pi-tasks"
        );
        assert_eq!(
            npm_package_name("@tintinweb/pi-tasks@0.7.1"),
            "@tintinweb/pi-tasks"
        );
    }
}
