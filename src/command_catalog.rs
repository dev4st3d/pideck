//! Unified native and Pi-discovered command catalog.

use crate::state::runtime::{CommandProvenance, CommandSource, FacetStatus, RuntimeCommand};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CommandGroup {
    Native,
    Extension,
    Prompt,
    Skill,
}

impl CommandGroup {
    pub fn label(self) -> &'static str {
        match self {
            Self::Native => "Native",
            Self::Extension => "Extension",
            Self::Prompt => "Prompt",
            Self::Skill => "Skill",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeAction {
    Model,
    NewSession,
    Sessions,
    Tree,
    Fork,
    Clone,
    Compact,
    ExportHtml,
    ExportJsonl,
    CopyLastResponse,
    Abort,
    RenameSession,
    Settings,
    Hotkeys,
    RefreshCommands,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandTarget {
    Native(NativeAction),
    Dynamic(CommandSource),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub group: CommandGroup,
    pub argument_hint: Option<String>,
    pub provenance: Option<CommandProvenance>,
    pub target: CommandTarget,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
}

impl CommandEntry {
    pub fn invocation(&self, arguments: &str) -> String {
        if arguments.is_empty() {
            format!("/{}", self.name)
        } else {
            format!("/{}{}", self.name, arguments)
        }
    }

    pub fn provenance_label(&self) -> String {
        let Some(provenance) = &self.provenance else {
            return "Built into Pideck".to_owned();
        };
        let source = if provenance.source.is_empty() {
            provenance.path.as_str()
        } else {
            provenance.source.as_str()
        };
        if provenance.path.is_empty() || provenance.path == source {
            format!("{} · {} · {}", provenance.scope, provenance.origin, source)
        } else {
            format!(
                "{} · {} · {} · {}",
                provenance.scope, provenance.origin, source, provenance.path
            )
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogFreshness {
    Loading,
    Fresh,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCatalog {
    pub entries: Vec<CommandEntry>,
    pub freshness: CatalogFreshness,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedInvocation<'a> {
    pub name: &'a str,
    /// Includes the exact whitespace separating the name from its arguments.
    pub arguments: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedInvocation {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvocationResolution<'a> {
    Command {
        entry: &'a CommandEntry,
        invocation: OwnedInvocation,
    },
    UnsupportedBuiltin(String),
    NotACommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCompletion<'a> {
    pub matches: Vec<&'a CommandEntry>,
    pub intercept_enter: bool,
}

impl CommandCatalog {
    pub fn build(status: &FacetStatus, dynamic: &[RuntimeCommand]) -> Self {
        let freshness = match status {
            FacetStatus::Loading => CatalogFreshness::Loading,
            FacetStatus::Ready => CatalogFreshness::Fresh,
            FacetStatus::Failed(_) => CatalogFreshness::Stale,
        };
        let dynamic_enabled = freshness == CatalogFreshness::Fresh;
        let disabled_reason = (!dynamic_enabled)
            .then(|| "Command catalog is refreshing; try again shortly.".to_owned());

        let mut entries = native_commands()
            .iter()
            .map(native_entry)
            .collect::<Vec<_>>();
        entries.extend(
            dynamic
                .iter()
                .enumerate()
                .filter(|(_, command)| !replaced_by_native_gui(command))
                .map(|(index, command)| {
                    let group = match command.source {
                        CommandSource::Extension => CommandGroup::Extension,
                        CommandSource::Prompt => CommandGroup::Prompt,
                        CommandSource::Skill => CommandGroup::Skill,
                    };
                    CommandEntry {
                        id: format!(
                            "dynamic:{:?}:{}:{}:{}",
                            command.source, command.name, command.provenance.path, index
                        ),
                        name: command.name.clone(),
                        description: command
                            .description
                            .clone()
                            .unwrap_or_else(|| "Pi command".to_owned()),
                        group,
                        argument_hint: None,
                        provenance: Some(command.provenance.clone()),
                        target: CommandTarget::Dynamic(command.source),
                        enabled: dynamic_enabled,
                        disabled_reason: disabled_reason.clone(),
                    }
                }),
        );
        entries.sort_by(|left, right| {
            left.group
                .cmp(&right.group)
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.id.cmp(&right.id))
        });
        Self { entries, freshness }
    }

    pub fn filtered(&self, query: &str) -> Vec<&CommandEntry> {
        let query = query.trim().trim_start_matches('/').to_lowercase();
        let mut matches = self
            .entries
            .iter()
            .filter_map(|entry| {
                let score = command_score(entry, &query)?;
                Some((score, entry))
            })
            .collect::<Vec<_>>();
        matches.sort_by(|(left_score, left), (right_score, right)| {
            left.group
                .cmp(&right.group)
                .then_with(|| right_score.cmp(left_score))
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.id.cmp(&right.id))
        });
        matches.into_iter().map(|(_, entry)| entry).collect()
    }

    pub fn resolve(&self, text: &str) -> InvocationResolution<'_> {
        let Some(invocation) = parse_invocation(text) else {
            return InvocationResolution::NotACommand;
        };
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(invocation.name))
        {
            return InvocationResolution::Command {
                entry,
                invocation: OwnedInvocation {
                    name: invocation.name.to_owned(),
                    arguments: invocation.arguments.to_owned(),
                },
            };
        }
        if unsupported_tui_builtin(invocation.name) {
            InvocationResolution::UnsupportedBuiltin(invocation.name.to_owned())
        } else {
            InvocationResolution::NotACommand
        }
    }

    pub fn slash_completion(&self, text: &str) -> Option<SlashCompletion<'_>> {
        let invocation = parse_invocation(text)?;
        if text.contains('\n') {
            return None;
        }
        let has_arguments = !invocation.arguments.is_empty();
        let matches = if has_arguments {
            self.entries
                .iter()
                .filter(|entry| entry.name.eq_ignore_ascii_case(invocation.name))
                .collect()
        } else {
            self.filtered(invocation.name)
        };
        (!matches.is_empty()).then_some(SlashCompletion {
            matches,
            intercept_enter: !has_arguments,
        })
    }
}

pub fn parse_invocation(text: &str) -> Option<ParsedInvocation<'_>> {
    let body = text.strip_prefix('/')?;
    if body.is_empty() || body.starts_with(char::is_whitespace) {
        return Some(ParsedInvocation {
            name: "",
            arguments: "",
        });
    }
    let split = body.find(char::is_whitespace).unwrap_or(body.len());
    Some(ParsedInvocation {
        name: &body[..split],
        arguments: &body[split..],
    })
}

fn command_score(entry: &CommandEntry, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let name = entry.name.to_lowercase();
    if name == query {
        return Some(1_000);
    }
    if name.starts_with(query) {
        return Some(800 - name.len() as i32);
    }
    if name.contains(query) {
        return Some(600 - name.len() as i32);
    }
    let haystack = format!(
        "{} {} {}",
        name,
        entry.description.to_lowercase(),
        entry.provenance_label().to_lowercase()
    );
    fuzzy_subsequence_score(&haystack, query)
}

fn fuzzy_subsequence_score(haystack: &str, needle: &str) -> Option<i32> {
    let mut score = 0_i32;
    let mut last = None;
    let mut chars = haystack.char_indices();
    for wanted in needle.chars() {
        let (index, _) = chars.find(|(_, candidate)| *candidate == wanted)?;
        score += if last.is_some_and(|previous| index == previous + 1) {
            12
        } else {
            2
        };
        last = Some(index);
    }
    Some(score)
}

fn unsupported_tui_builtin(name: &str) -> bool {
    const BUILTINS: &[&str] = &[
        "box",
        "bar",
        "clear",
        "editor",
        "help",
        "keybindings",
        "login",
        "logout",
        "models",
        "providers",
        "scoped-models",
        "scoped-skills",
        "resume",
        "share",
        "theme",
        "thinking",
        "tools",
        "update",
        "usage",
        "changelog",
        "quit",
        "exit",
        "reload",
        "rail",
        "topbar",
    ];
    let name = name.to_ascii_lowercase();
    BUILTINS.iter().any(|candidate| {
        name == *candidate
            || name
                .strip_prefix(candidate)
                .is_some_and(|suffix| suffix.starts_with(':'))
    })
}

fn replaced_by_native_gui(command: &RuntimeCommand) -> bool {
    if command.source != CommandSource::Extension {
        return false;
    }
    let path = command.provenance.path.replace('\\', "/").to_lowercase();
    let source = command.provenance.source.replace('\\', "/").to_lowercase();
    let matches_source = |expected: &str| {
        path.ends_with(expected)
            || source.ends_with(expected)
            || path.contains(&format!("/{expected}"))
            || source.contains(&format!("/{expected}"))
    };
    let name = command.name.to_lowercase();
    let base_name = name.split(':').next().unwrap_or(name.as_str());
    match base_name {
        "box" => matches_source("box-editor.ts"),
        "rail" => matches_source("activity-rail.ts"),
        "topbar" => matches_source("quiet-topbar.ts"),
        "bar" => {
            matches_source("status-footer.ts")
                || path.contains("/pi-bar/")
                || source.contains("/pi-bar/")
                || source.contains("pi-bar")
        }
        _ => false,
    }
}

struct NativeCommandDefinition {
    name: &'static str,
    description: &'static str,
    argument_hint: Option<&'static str>,
    action: NativeAction,
}

const NATIVE_COMMANDS: &[NativeCommandDefinition] = &[
    NativeCommandDefinition {
        name: "model",
        description: "Choose the active model",
        argument_hint: None,
        action: NativeAction::Model,
    },
    NativeCommandDefinition {
        name: "new",
        description: "Start a new session",
        argument_hint: None,
        action: NativeAction::NewSession,
    },
    NativeCommandDefinition {
        name: "session",
        description: "Browse or rename the current session",
        argument_hint: None,
        action: NativeAction::Sessions,
    },
    NativeCommandDefinition {
        name: "tree",
        description: "Open native session history",
        argument_hint: None,
        action: NativeAction::Tree,
    },
    NativeCommandDefinition {
        name: "fork",
        description: "Choose a message to fork before",
        argument_hint: None,
        action: NativeAction::Fork,
    },
    NativeCommandDefinition {
        name: "clone",
        description: "Clone the current active path",
        argument_hint: None,
        action: NativeAction::Clone,
    },
    NativeCommandDefinition {
        name: "compact",
        description: "Compact context now",
        argument_hint: Some("[focus instructions]"),
        action: NativeAction::Compact,
    },
    NativeCommandDefinition {
        name: "export",
        description: "Export the active session as HTML",
        argument_hint: Some("[output path]"),
        action: NativeAction::ExportHtml,
    },
    NativeCommandDefinition {
        name: "export-jsonl",
        description: "Export the active branch as JSONL",
        argument_hint: Some("[output path]"),
        action: NativeAction::ExportJsonl,
    },
    NativeCommandDefinition {
        name: "copy",
        description: "Copy the last assistant response",
        argument_hint: None,
        action: NativeAction::CopyLastResponse,
    },
    NativeCommandDefinition {
        name: "abort",
        description: "Abort the active agent or Bash run",
        argument_hint: None,
        action: NativeAction::Abort,
    },
    NativeCommandDefinition {
        name: "name",
        description: "Rename the current session",
        argument_hint: Some("<name>"),
        action: NativeAction::RenameSession,
    },
    NativeCommandDefinition {
        name: "settings",
        description: "Open native settings",
        argument_hint: None,
        action: NativeAction::Settings,
    },
    NativeCommandDefinition {
        name: "hotkeys",
        description: "Show native keyboard shortcuts",
        argument_hint: None,
        action: NativeAction::Hotkeys,
    },
    NativeCommandDefinition {
        name: "reload-commands",
        description: "Refresh extension, prompt, and skill commands",
        argument_hint: None,
        action: NativeAction::RefreshCommands,
    },
];

fn native_commands() -> &'static [NativeCommandDefinition] {
    NATIVE_COMMANDS
}

fn native_entry(definition: &NativeCommandDefinition) -> CommandEntry {
    CommandEntry {
        id: format!("native:{}", definition.name),
        name: definition.name.to_owned(),
        description: definition.description.to_owned(),
        group: CommandGroup::Native,
        argument_hint: definition.argument_hint.map(ToOwned::to_owned),
        provenance: None,
        target: CommandTarget::Native(definition.action),
        enabled: true,
        disabled_reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::runtime::{CommandProvenance as RuntimeProvenance, ErrorKind, SafeError};

    fn dynamic(name: &str, source: CommandSource, path: &str) -> RuntimeCommand {
        RuntimeCommand {
            name: name.to_owned(),
            description: Some(format!("{name} description")),
            source,
            provenance: RuntimeProvenance {
                path: path.to_owned(),
                source: "fixture-package".to_owned(),
                scope: "project".to_owned(),
                origin: "package".to_owned(),
                base_dir: Some("C:/fixture".to_owned()),
            },
        }
    }

    #[test]
    fn merges_groups_filters_and_preserves_duplicate_identity() {
        let catalog = CommandCatalog::build(
            &FacetStatus::Ready,
            &[
                dynamic("deploy", CommandSource::Extension, "a.ts"),
                dynamic("deploy", CommandSource::Extension, "b.ts"),
                dynamic("deploy:2", CommandSource::Extension, "b.ts"),
                dynamic("fix-tests", CommandSource::Prompt, "fix.md"),
                dynamic("skill:search", CommandSource::Skill, "SKILL.md"),
            ],
        );
        let deploy = catalog.filtered("deploy");
        assert_eq!(deploy.len(), 3);
        assert_ne!(deploy[0].id, deploy[1].id);
        assert!(deploy.iter().any(|entry| entry.name == "deploy:2"));
        assert!(
            catalog
                .filtered("search")
                .iter()
                .any(|entry| entry.group == CommandGroup::Skill && entry.name == "skill:search")
        );
    }

    #[test]
    fn source_info_and_exact_arguments_survive_catalog_and_resolution() {
        let catalog = CommandCatalog::build(
            &FacetStatus::Ready,
            &[dynamic(
                "skill:search",
                CommandSource::Skill,
                "C:/fixture/SKILL.md",
            )],
        );
        let InvocationResolution::Command { entry, invocation } =
            catalog.resolve("/skill:search   exact  \"arguments\"")
        else {
            panic!("command should resolve");
        };
        assert_eq!(invocation.arguments, "   exact  \"arguments\"");
        assert_eq!(
            entry.invocation(&invocation.arguments),
            "/skill:search   exact  \"arguments\""
        );
        assert_eq!(
            entry.provenance.as_ref().unwrap().path,
            "C:/fixture/SKILL.md"
        );
        assert!(entry.provenance_label().contains("fixture-package"));
    }

    #[test]
    fn stale_dynamic_commands_are_visible_but_disabled() {
        let failed =
            FacetStatus::Failed(SafeError::new(ErrorKind::OptionalFacet, "refresh failed"));
        let catalog = CommandCatalog::build(
            &failed,
            &[dynamic("deploy", CommandSource::Extension, "a.ts")],
        );
        let deploy = catalog
            .entries
            .iter()
            .find(|entry| entry.name == "deploy")
            .unwrap();
        assert!(!deploy.enabled);
        assert_eq!(catalog.freshness, CatalogFreshness::Stale);
        assert!(
            catalog
                .entries
                .iter()
                .find(|entry| entry.name == "settings")
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn slash_completion_stops_intercepting_enter_after_arguments_begin() {
        let catalog = CommandCatalog::build(&FacetStatus::Ready, &[]);
        let completion = catalog.slash_completion("/comp").unwrap();
        assert!(completion.intercept_enter);
        assert_eq!(completion.matches[0].name, "compact");
        let completion = catalog.slash_completion("/compact keep tools").unwrap();
        assert!(!completion.intercept_enter);
        assert_eq!(
            completion.matches[0].argument_hint.as_deref(),
            Some("[focus instructions]")
        );
    }

    #[test]
    fn unsupported_tui_builtins_are_guarded_but_unknown_slash_text_is_not_claimed() {
        let catalog = CommandCatalog::build(&FacetStatus::Ready, &[]);
        assert_eq!(
            catalog.resolve("/login"),
            InvocationResolution::UnsupportedBuiltin("login".to_owned())
        );
        assert_eq!(
            catalog.resolve("/box"),
            InvocationResolution::UnsupportedBuiltin("box".to_owned())
        );
        assert_eq!(
            catalog.resolve("/topbar:2"),
            InvocationResolution::UnsupportedBuiltin("topbar:2".to_owned())
        );
        assert_eq!(
            catalog.resolve("/ordinary prose"),
            InvocationResolution::NotACommand
        );
    }

    #[test]
    fn native_gui_replacements_are_hidden_without_suppressing_functional_extensions() {
        let catalog = CommandCatalog::build(
            &FacetStatus::Ready,
            &[
                dynamic(
                    "box",
                    CommandSource::Extension,
                    "C:/Users/test/.pi/agent/extensions/box-editor.ts",
                ),
                dynamic(
                    "box:2",
                    CommandSource::Extension,
                    "C:/Users/test/.pi/agent/extensions/box-editor.ts",
                ),
                dynamic(
                    "rail",
                    CommandSource::Extension,
                    "C:/Users/test/.pi/agent/extensions/activity-rail.ts",
                ),
                dynamic(
                    "topbar",
                    CommandSource::Extension,
                    "C:/Users/test/.pi/agent/extensions/quiet-topbar.ts",
                ),
                dynamic(
                    "bar",
                    CommandSource::Extension,
                    "C:/Users/test/.pi/agent/npm/node_modules/pi-bar/extensions/status-footer.ts",
                ),
                dynamic(
                    "binance",
                    CommandSource::Extension,
                    "C:/Users/test/.pi/agent/extensions/binance/index.ts",
                ),
                dynamic(
                    "box",
                    CommandSource::Extension,
                    "C:/project/extensions/functional-box.ts",
                ),
            ],
        );

        assert!(!catalog.entries.iter().any(|entry| entry.name == "rail"));
        assert!(!catalog.entries.iter().any(|entry| entry.name == "topbar"));
        assert!(!catalog.entries.iter().any(|entry| entry.name == "bar"));
        assert_eq!(
            catalog
                .entries
                .iter()
                .filter(|entry| entry.name == "box")
                .count(),
            1
        );
        assert!(catalog.entries.iter().any(|entry| entry.name == "binance"));
        assert!(matches!(
            catalog.resolve("/box"),
            InvocationResolution::Command { .. }
        ));
        assert!(matches!(
            catalog.resolve("/binance enable"),
            InvocationResolution::Command { .. }
        ));
    }

    #[test]
    fn every_native_command_has_an_enabled_execution_target() {
        let catalog = CommandCatalog::build(&FacetStatus::Ready, &[]);
        let native_count = catalog
            .entries
            .iter()
            .filter(|entry| entry.group == CommandGroup::Native)
            .count();
        assert_eq!(native_count, NATIVE_COMMANDS.len());
        assert!(catalog.entries.iter().all(|entry| {
            entry.group != CommandGroup::Native
                || (entry.enabled && matches!(entry.target, CommandTarget::Native(_)))
        }));
    }
}
