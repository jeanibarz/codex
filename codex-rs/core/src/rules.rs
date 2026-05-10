//! Conditional rules discovery (Claude-compat).
//!
//! Loads supplemental instruction files from `.codex/rules/*.md` and
//! `.claude/rules/*.md` directories beneath the current working directory.
//! Each rule file may declare YAML frontmatter with a `paths:` field listing
//! globs of files for which the rule is relevant; the globs are forwarded to
//! the model as metadata so it can decide when to apply the rule.
//!
//! The runtime side (matching the agent's tool calls against the globs to
//! suppress irrelevant rules) is intentionally out of scope for this first
//! pass — a future change can add that on top of the discovery+formatting
//! infrastructure built here.

use codex_exec_server::ExecutorFileSystem;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::io;
use tracing::warn;

/// Subdirectory (relative to cwd) where Codex looks for conditional rules.
pub const CODEX_RULES_DIRNAME: &str = ".codex/rules";

/// Cross-tool compatibility: also load Claude Code's rules dir if present.
pub const CLAUDE_RULES_DIRNAME: &str = ".claude/rules";

const RULE_FILE_EXTENSION: &str = "md";

/// A loaded rule file with its parsed frontmatter and body.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct LoadedRule {
    /// Absolute path to the rule file.
    pub(crate) path: AbsolutePathBuf,
    /// Path display label relative to `cwd` when possible, else absolute.
    pub(crate) display_path: String,
    /// `paths:` globs from frontmatter (empty when not specified).
    pub(crate) paths_globs: Vec<String>,
    /// `description:` from frontmatter, if any.
    pub(crate) description: Option<String>,
    /// Rule body (markdown content after the frontmatter block).
    pub(crate) body: String,
}

/// Discover all candidate rule files under `<cwd>/.codex/rules/*.md` and
/// `<cwd>/.claude/rules/*.md` without reading their contents. Returned paths
/// are sorted by filename within each directory; the `.codex` directory is
/// listed before `.claude`.
pub(crate) async fn discover_rule_paths(
    cwd: &AbsolutePathBuf,
    fs: &dyn ExecutorFileSystem,
) -> io::Result<Vec<AbsolutePathBuf>> {
    let mut rule_paths: Vec<AbsolutePathBuf> = Vec::new();
    for dirname in [CODEX_RULES_DIRNAME, CLAUDE_RULES_DIRNAME] {
        let dir = cwd.join(dirname);
        match fs.get_metadata(&dir, /*sandbox*/ None).await {
            Ok(md) if md.is_directory => {}
            Ok(_) => continue,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        }

        let mut entries = match fs.read_directory(&dir, /*sandbox*/ None).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        entries.sort_by(|a, b| a.file_name.cmp(&b.file_name));

        for entry in entries {
            if !entry.is_file {
                continue;
            }
            if !has_markdown_extension(&entry.file_name) {
                continue;
            }
            rule_paths.push(dir.join(&entry.file_name));
        }
    }
    Ok(rule_paths)
}

/// Discover and read rule files from `<cwd>/.codex/rules/*.md` and
/// `<cwd>/.claude/rules/*.md`.
///
/// `max_bytes` is a global ceiling on the total bytes consumed across all
/// rule bodies; once the budget is exhausted later rules are dropped. Per-file
/// parsing errors (malformed frontmatter, non-utf8 content) are logged and
/// the offending file is skipped.
pub(crate) async fn discover_rules(
    cwd: &AbsolutePathBuf,
    fs: &dyn ExecutorFileSystem,
    max_bytes: usize,
) -> io::Result<Vec<LoadedRule>> {
    if max_bytes == 0 {
        return Ok(Vec::new());
    }

    let rule_paths = discover_rule_paths(cwd, fs).await?;

    let mut loaded = Vec::with_capacity(rule_paths.len());
    let mut remaining: u64 = max_bytes as u64;
    for path in rule_paths {
        if remaining == 0 {
            break;
        }
        let mut data = match fs.read_file(&path, /*sandbox*/ None).await {
            Ok(data) => data,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        let original_len = data.len() as u64;
        if original_len > remaining {
            data.truncate(remaining as usize);
            warn!(
                "Rule file `{}` exceeds remaining budget ({} bytes) — truncating.",
                path.display(),
                remaining,
            );
        }
        let raw = String::from_utf8_lossy(&data).to_string();
        let display_path = relative_label(&path, cwd);
        let parsed = match parse_rule_document(&raw) {
            Ok(parsed) => parsed,
            Err(err) => {
                warn!(
                    "ignoring rule `{}` due to invalid frontmatter: {err}",
                    path.display(),
                );
                continue;
            }
        };
        if parsed.body.trim().is_empty() {
            // Skip rules that are pure frontmatter / empty bodies.
            continue;
        }
        remaining = remaining.saturating_sub(data.len() as u64);
        loaded.push(LoadedRule {
            path,
            display_path,
            paths_globs: parsed.paths_globs,
            description: parsed.description,
            body: parsed.body,
        });
    }
    Ok(loaded)
}

/// Format a list of loaded rules as a single string suitable for appending to
/// the user instructions block. Returns `None` when the input is empty.
pub(crate) fn render_rules(rules: &[LoadedRule]) -> Option<String> {
    if rules.is_empty() {
        return None;
    }
    let mut out = String::new();
    out.push_str("--- conditional-rules ---\n");
    out.push_str(
        "Each block below is a project-defined rule. The `paths:` line, when present, lists glob patterns describing the files the rule is most relevant for; consult the rule when the work touches matching paths.\n",
    );
    for rule in rules {
        out.push_str("\n[from ");
        out.push_str(&rule.display_path);
        if let Some(description) = &rule.description {
            out.push_str(" — ");
            out.push_str(description.trim());
        }
        out.push_str("]\n");
        if !rule.paths_globs.is_empty() {
            out.push_str("paths: ");
            for (idx, pattern) in rule.paths_globs.iter().enumerate() {
                if idx > 0 {
                    out.push_str(", ");
                }
                out.push_str(pattern);
            }
            out.push('\n');
        }
        out.push_str(rule.body.trim_end_matches('\n'));
        out.push('\n');
    }
    Some(out)
}

#[derive(Debug, Default)]
struct ParsedRule {
    paths_globs: Vec<String>,
    description: Option<String>,
    body: String,
}

fn has_markdown_extension(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case(RULE_FILE_EXTENSION))
}

fn parse_rule_document(raw: &str) -> Result<ParsedRule, String> {
    let Some(rest) = raw.strip_prefix("---") else {
        return Ok(ParsedRule {
            body: raw.to_string(),
            ..Default::default()
        });
    };
    let rest = rest.strip_prefix('\r').unwrap_or(rest);
    let Some(rest) = rest.strip_prefix('\n') else {
        return Ok(ParsedRule {
            body: raw.to_string(),
            ..Default::default()
        });
    };
    let Some(end_idx) = find_closing_delimiter(rest) else {
        return Err("frontmatter block is not closed by `---`".to_string());
    };
    let frontmatter_text = &rest[..end_idx];
    let body_start = end_idx + closing_delimiter_len(&rest[end_idx..]);
    let body = rest[body_start..].trim_start_matches('\n').to_string();

    let yaml: serde_yaml::Value = if frontmatter_text.trim().is_empty() {
        serde_yaml::Value::Null
    } else {
        serde_yaml::from_str(frontmatter_text)
            .map_err(|err| format!("frontmatter is not valid YAML: {err}"))?
    };

    let paths_globs = extract_string_list(&yaml, "paths");
    let description = extract_string(&yaml, "description");

    Ok(ParsedRule {
        paths_globs,
        description,
        body,
    })
}

fn find_closing_delimiter(text: &str) -> Option<usize> {
    // Locate a line that is exactly `---` (followed by newline or EOF).
    let mut search_from = 0;
    while let Some(rel) = text[search_from..].find("\n---") {
        let abs = search_from + rel + 1;
        let after = &text[abs + 3..];
        if after.is_empty() || after.starts_with('\n') || after.starts_with("\r\n") {
            return Some(abs);
        }
        search_from = abs + 3;
    }
    None
}

fn closing_delimiter_len(text: &str) -> usize {
    if text.starts_with("---\r\n") {
        5
    } else if text.starts_with("---\n") {
        4
    } else if text.starts_with("---") {
        3
    } else {
        0
    }
}

fn extract_string_list(yaml: &serde_yaml::Value, key: &str) -> Vec<String> {
    let Some(value) = yaml.get(key) else {
        return Vec::new();
    };
    match value {
        serde_yaml::Value::Sequence(seq) => seq
            .iter()
            .filter_map(|item| match item {
                serde_yaml::Value::String(s) => Some(s.trim().to_string()),
                serde_yaml::Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .filter(|s| !s.is_empty())
            .collect(),
        serde_yaml::Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Vec::new()
            } else {
                vec![trimmed.to_string()]
            }
        }
        _ => Vec::new(),
    }
}

fn extract_string(yaml: &serde_yaml::Value, key: &str) -> Option<String> {
    match yaml.get(key)? {
        serde_yaml::Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        _ => None,
    }
}

fn relative_label(path: &AbsolutePathBuf, cwd: &AbsolutePathBuf) -> String {
    path.as_path()
        .strip_prefix(cwd.as_path())
        .map(|rel| rel.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
