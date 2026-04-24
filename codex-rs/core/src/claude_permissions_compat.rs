use std::fs;
use std::path::Path;
use std::path::PathBuf;

use codex_config::ConfigLayerStack;
use codex_config::ConfigLayerStackOrdering;
use codex_execpolicy::Decision;
use serde_json::Value as JsonValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaudePermissionDecision {
    Allow,
    Prompt,
    Forbidden,
}

impl ClaudePermissionDecision {
    pub(crate) fn into_exec_policy_decision(self) -> Decision {
        match self {
            Self::Allow => Decision::Allow,
            Self::Prompt => Decision::Prompt,
            Self::Forbidden => Decision::Forbidden,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaudePermissionMatch {
    pub(crate) decision: ClaudePermissionDecision,
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ClaudePermissionRules {
    rules: Vec<ClaudePermissionRule>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClaudePermissionRule {
    decision: ClaudePermissionDecision,
    tool_pattern: String,
    command_pattern: Option<String>,
    raw_pattern: String,
    source_path: PathBuf,
    load_order: usize,
}

impl ClaudePermissionRule {
    fn matches(&self, tool_name: &str, command: &str) -> bool {
        if !wildcard_pattern_matches(&self.tool_pattern, tool_name) {
            return false;
        }

        match self.command_pattern.as_deref() {
            Some(pattern) => wildcard_pattern_matches(pattern, command),
            None => true,
        }
    }

    fn reason(&self, command: &str) -> Option<String> {
        match self.decision {
            ClaudePermissionDecision::Allow => None,
            ClaudePermissionDecision::Prompt => Some(format!(
                "`{command}` requires approval by Claude permission rule `{}` from {}",
                self.raw_pattern,
                self.source_path.display()
            )),
            ClaudePermissionDecision::Forbidden => Some(format!(
                "`{command}` rejected by Claude permission rule `{}` from {}",
                self.raw_pattern,
                self.source_path.display()
            )),
        }
    }
}

impl ClaudePermissionRules {
    pub(crate) fn load(config_layer_stack: &ConfigLayerStack) -> Self {
        let mut rules = Vec::new();
        let mut warnings = Vec::new();
        let mut load_order = 0_usize;

        for layer in config_layer_stack.get_layers(
            ConfigLayerStackOrdering::LowestPrecedenceFirst,
            /*include_disabled*/ false,
        ) {
            let Some(config_folder) = layer.config_folder() else {
                continue;
            };

            for settings_path in claude_settings_fallback_paths(config_folder.as_path()) {
                load_rules_from_settings_file(
                    settings_path.as_path(),
                    &mut rules,
                    &mut warnings,
                    &mut load_order,
                );
            }
        }

        Self { rules, warnings }
    }

    pub(crate) fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub(crate) fn evaluate(
        &self,
        tool_name: &str,
        command: &str,
    ) -> Option<ClaudePermissionMatch> {
        let mut selected: Option<&ClaudePermissionRule> = None;

        for rule in &self.rules {
            if !rule.matches(tool_name, command) {
                continue;
            }

            match selected {
                None => selected = Some(rule),
                Some(current) => {
                    let current_decision = current.decision.into_exec_policy_decision();
                    let next_decision = rule.decision.into_exec_policy_decision();
                    if next_decision > current_decision
                        || (next_decision == current_decision
                            && rule.load_order >= current.load_order)
                    {
                        selected = Some(rule);
                    }
                }
            }
        }

        selected.map(|rule| ClaudePermissionMatch {
            decision: rule.decision,
            reason: rule.reason(command),
        })
    }
}

fn load_rules_from_settings_file(
    settings_path: &Path,
    rules: &mut Vec<ClaudePermissionRule>,
    warnings: &mut Vec<String>,
    load_order: &mut usize,
) {
    if !settings_path.is_file() {
        return;
    }

    let contents = match fs::read_to_string(settings_path) {
        Ok(contents) => contents,
        Err(err) => {
            warnings.push(format!(
                "failed to read Claude settings file {}: {err}",
                settings_path.display()
            ));
            return;
        }
    };

    let settings: JsonValue = match serde_json::from_str(&contents) {
        Ok(settings) => settings,
        Err(err) => {
            warnings.push(format!(
                "failed to parse Claude settings file {}: {err}",
                settings_path.display()
            ));
            return;
        }
    };

    let Some(permissions) = settings.get("permissions").and_then(JsonValue::as_object) else {
        return;
    };

    if let Some(default_mode) = permissions.get("defaultMode").and_then(JsonValue::as_str)
        && matches!(default_mode, "dontAsk" | "plan" | "bypassPermissions")
    {
        warnings.push(format!(
            "Claude permissions defaultMode `{default_mode}` in {} is only partially compatible with Codex shell approvals",
            settings_path.display()
        ));
    }

    for (field_name, decision) in [
        ("deny", ClaudePermissionDecision::Forbidden),
        ("ask", ClaudePermissionDecision::Prompt),
        ("allow", ClaudePermissionDecision::Allow),
    ] {
        let Some(entries) = permissions.get(field_name) else {
            continue;
        };
        let Some(patterns) = entries.as_array() else {
            warnings.push(format!(
                "Claude permissions `{field_name}` in {} must be an array",
                settings_path.display()
            ));
            continue;
        };

        for pattern in patterns {
            let Some(pattern) = pattern.as_str() else {
                warnings.push(format!(
                    "Claude permissions `{field_name}` entry in {} must be a string",
                    settings_path.display()
                ));
                continue;
            };

            match parse_permission_rule(pattern, decision, settings_path, *load_order) {
                Ok(rule) => {
                    rules.push(rule);
                    *load_order += 1;
                }
                Err(err) => warnings.push(format!(
                    "skipping Claude permission rule {pattern:?} in {}: {err}",
                    settings_path.display()
                )),
            }
        }
    }
}

fn parse_permission_rule(
    raw_pattern: &str,
    decision: ClaudePermissionDecision,
    source_path: &Path,
    load_order: usize,
) -> Result<ClaudePermissionRule, String> {
    let trimmed = raw_pattern.trim();
    if trimmed.is_empty() {
        return Err("permission pattern cannot be empty".to_string());
    }

    let (tool_pattern, command_pattern) = match trimmed.split_once('(') {
        Some((tool_pattern, rest)) => {
            if !rest.ends_with(')') {
                return Err("permission pattern must end with ')'".to_string());
            }
            if tool_pattern.trim().is_empty() {
                return Err("permission pattern is missing a tool name".to_string());
            }
            if rest[..rest.len() - 1].contains('(') {
                return Err("nested '(' is not supported in permission patterns".to_string());
            }
            let command_pattern = &rest[..rest.len() - 1];
            (
                tool_pattern.trim().to_string(),
                (!command_pattern.is_empty()).then(|| command_pattern.to_string()),
            )
        }
        None => (trimmed.to_string(), None),
    };

    Ok(ClaudePermissionRule {
        decision,
        tool_pattern,
        command_pattern,
        raw_pattern: trimmed.to_string(),
        source_path: source_path.to_path_buf(),
        load_order,
    })
}

fn claude_settings_fallback_paths(config_folder: &Path) -> Vec<PathBuf> {
    let root = if config_folder.file_name().is_some_and(|name| name == ".codex") {
        config_folder.parent().unwrap_or(config_folder)
    } else {
        config_folder
    };

    let claude_dir = root.join(".claude");
    vec![
        claude_dir.join("settings.json"),
        claude_dir.join("settings.local.json"),
    ]
}

fn wildcard_pattern_matches(pattern: &str, value: &str) -> bool {
    if pattern.is_empty() || pattern == "*" {
        return true;
    }

    let parts = pattern.split('*').collect::<Vec<_>>();
    let anchored_start = !pattern.starts_with('*');
    let anchored_end = !pattern.ends_with('*');
    let mut cursor = 0_usize;

    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        if index == 0 && anchored_start {
            if !value[cursor..].starts_with(part) {
                return false;
            }
            cursor += part.len();
            continue;
        }

        match value[cursor..].find(part) {
            Some(offset) => cursor += offset + part.len(),
            None => return false,
        }
    }

    if anchored_end
        && let Some(last_non_empty) = parts.iter().rev().find(|part| !part.is_empty())
    {
        return value.ends_with(last_non_empty);
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluate_prefers_higher_severity_then_higher_precedence() {
        let rules = ClaudePermissionRules {
            rules: vec![
                ClaudePermissionRule {
                    decision: ClaudePermissionDecision::Allow,
                    tool_pattern: "Bash".to_string(),
                    command_pattern: None,
                    raw_pattern: "Bash".to_string(),
                    source_path: PathBuf::from("/tmp/a.json"),
                    load_order: 0,
                },
                ClaudePermissionRule {
                    decision: ClaudePermissionDecision::Prompt,
                    tool_pattern: "Bash".to_string(),
                    command_pattern: Some("git push*".to_string()),
                    raw_pattern: "Bash(git push*)".to_string(),
                    source_path: PathBuf::from("/tmp/b.json"),
                    load_order: 1,
                },
                ClaudePermissionRule {
                    decision: ClaudePermissionDecision::Forbidden,
                    tool_pattern: "Bash".to_string(),
                    command_pattern: Some("git push --force*".to_string()),
                    raw_pattern: "Bash(git push --force*)".to_string(),
                    source_path: PathBuf::from("/tmp/c.json"),
                    load_order: 2,
                },
            ],
            warnings: Vec::new(),
        };

        assert_eq!(
            rules
                .evaluate("Bash", "git push origin main")
                .expect("ask match")
                .decision,
            ClaudePermissionDecision::Prompt
        );
        assert_eq!(
            rules
                .evaluate("Bash", "git push --force origin main")
                .expect("deny match")
                .decision,
            ClaudePermissionDecision::Forbidden
        );
        assert_eq!(
            rules.evaluate("Bash", "ls -la").expect("allow match").decision,
            ClaudePermissionDecision::Allow
        );
    }
}
