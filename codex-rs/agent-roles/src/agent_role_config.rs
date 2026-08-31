use codex_config::config_toml::ConfigToml;
use codex_utils_absolute_path::AbsolutePathBufGuard;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use toml::Value as TomlValue;

const GPT_5_6_LUNA_MODEL: &str = "gpt-5.6-luna";
const GPT_5_6_SOL_MODEL: &str = "gpt-5.6-sol";

#[derive(Clone, Copy)]
struct ClaudeAgentModelMapping {
    model: &'static str,
    reasoning_effort: &'static str,
}

#[derive(Debug, Default, Deserialize)]
struct ClaudeAgentFrontmatter {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default, rename = "nickname-candidates", alias = "nickname_candidates")]
    nickname_candidates: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentRoleConfig {
    /// Human-facing role documentation used in spawn tool guidance.
    /// Required for loaded user-defined roles after deprecated/new metadata precedence resolves.
    pub description: Option<String>,
    /// Path to a role-specific config layer.
    pub config_file: Option<PathBuf>,
    /// Candidate nicknames for agents spawned with this role.
    pub nickname_candidates: Option<Vec<String>>,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(deny_unknown_fields)]
struct RawAgentRoleFileToml {
    name: Option<String>,
    description: Option<String>,
    nickname_candidates: Option<Vec<String>>,
    #[serde(flatten)]
    config: ConfigToml,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedAgentRoleFile {
    pub role_name: String,
    pub description: Option<String>,
    pub nickname_candidates: Option<Vec<String>>,
    pub config: TomlValue,
}

pub fn parse_agent_role_file_contents(
    contents: &str,
    role_file_label: &Path,
    config_base_dir: &Path,
    role_name_hint: Option<&str>,
) -> std::io::Result<ResolvedAgentRoleFile> {
    if looks_like_claude_agent_markdown(contents) {
        return parse_claude_agent_role_file_contents(contents, role_file_label, role_name_hint);
    }

    parse_toml_agent_role_file_contents(contents, role_file_label, config_base_dir, role_name_hint)
}

fn parse_toml_agent_role_file_contents(
    contents: &str,
    role_file_label: &Path,
    config_base_dir: &Path,
    role_name_hint: Option<&str>,
) -> std::io::Result<ResolvedAgentRoleFile> {
    let role_file_toml: TomlValue = toml::from_str(contents).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "failed to parse agent role file at {}: {err}",
                role_file_label.display()
            ),
        )
    })?;
    let _guard = AbsolutePathBufGuard::new(config_base_dir);
    let parsed: RawAgentRoleFileToml = role_file_toml.clone().try_into().map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "failed to deserialize agent role file at {}: {err}",
                role_file_label.display()
            ),
        )
    })?;
    let description = normalize_agent_role_description(
        &format!("agent role file {}.description", role_file_label.display()),
        parsed.description.as_deref(),
    )?;
    validate_agent_role_file_developer_instructions(
        role_file_label,
        parsed.config.developer_instructions.as_deref(),
        role_name_hint.is_none(),
    )?;

    let role_name = parsed
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| role_name_hint.map(ToOwned::to_owned))
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "agent role file at {} must define a non-empty `name`",
                    role_file_label.display()
                ),
            )
        })?;

    let nickname_candidates = normalize_agent_role_nickname_candidates(
        &format!(
            "agent role file {}.nickname_candidates",
            role_file_label.display()
        ),
        parsed.nickname_candidates.as_deref(),
    )?;

    let mut config = role_file_toml;
    let Some(config_table) = config.as_table_mut() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "agent role file at {} must contain a TOML table",
                role_file_label.display()
            ),
        ));
    };
    config_table.remove("name");
    config_table.remove("description");
    config_table.remove("nickname_candidates");

    Ok(ResolvedAgentRoleFile {
        role_name,
        description,
        nickname_candidates,
        config,
    })
}

fn parse_claude_agent_role_file_contents(
    contents: &str,
    role_file_label: &Path,
    role_name_hint: Option<&str>,
) -> std::io::Result<ResolvedAgentRoleFile> {
    let (frontmatter, body) = extract_claude_agent_frontmatter(contents, role_file_label)?;
    let parsed: ClaudeAgentFrontmatter = serde_yaml::from_str(&frontmatter).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "failed to parse Claude agent frontmatter at {}: {err}",
                role_file_label.display()
            ),
        )
    })?;
    let description = normalize_agent_role_description(
        &format!("agent role file {}.description", role_file_label.display()),
        parsed.description.as_deref(),
    )?;

    let developer_instructions = body.trim();
    validate_agent_role_file_developer_instructions(
        role_file_label,
        Some(developer_instructions),
        true,
    )?;

    let role_name = parsed
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .or_else(|| role_name_hint.map(str::to_owned))
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "agent role file at {} must define a non-empty `name`",
                    role_file_label.display()
                ),
            )
        })?;

    let nickname_candidates = normalize_agent_role_nickname_candidates(
        &format!(
            "agent role file {}.nickname_candidates",
            role_file_label.display()
        ),
        parsed.nickname_candidates.as_deref(),
    )?;

    let mut config = toml::map::Map::new();
    config.insert(
        "developer_instructions".to_string(),
        TomlValue::String(developer_instructions.to_string()),
    );
    if let Some(raw_model) = parsed
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        let model = normalize_claude_agent_model_name(raw_model);
        config.insert("model".to_string(), TomlValue::String(model));
        if let Some(reasoning_effort) = claude_agent_model_reasoning_effort(raw_model) {
            config.insert(
                "model_reasoning_effort".to_string(),
                TomlValue::String(reasoning_effort.to_string()),
            );
        }
    }

    Ok(ResolvedAgentRoleFile {
        role_name,
        description,
        nickname_candidates,
        config: TomlValue::Table(config),
    })
}

fn looks_like_claude_agent_markdown(contents: &str) -> bool {
    contents
        .lines()
        .next()
        .is_some_and(|line| line.trim() == "---")
}

fn extract_claude_agent_frontmatter(
    contents: &str,
    role_file_label: &Path,
) -> std::io::Result<(String, String)> {
    let mut lines = contents.split_inclusive('\n');
    let Some(first_line) = lines.next() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Claude agent file at {} is missing YAML frontmatter",
                role_file_label.display()
            ),
        ));
    };
    if first_line.trim() != "---" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Claude agent file at {} is missing YAML frontmatter",
                role_file_label.display()
            ),
        ));
    }

    let mut frontmatter = String::new();
    let mut consumed = first_line.len();
    for chunk in lines {
        consumed += chunk.len();
        if chunk.trim() == "---" {
            return Ok((frontmatter, contents[consumed..].to_string()));
        }
        frontmatter.push_str(chunk);
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!(
            "Claude agent file at {} is missing closing YAML frontmatter delimiter",
            role_file_label.display()
        ),
    ))
}

fn normalize_claude_agent_model_name(model: &str) -> String {
    let trimmed = model.trim();
    claude_agent_model_mapping(trimmed)
        .map(|mapping| mapping.model.to_string())
        .unwrap_or_else(|| trimmed.to_string())
}

fn claude_agent_model_reasoning_effort(model: &str) -> Option<&'static str> {
    claude_agent_model_mapping(model).map(|mapping| mapping.reasoning_effort)
}

fn claude_agent_model_mapping(model: &str) -> Option<ClaudeAgentModelMapping> {
    let lower = model.to_ascii_lowercase();
    let family = match lower.as_str() {
        "opus" | "sonnet" | "haiku" => lower.as_str(),
        _ => lower
            .strip_prefix("claude-")?
            .split('-')
            .find(|segment| matches!(*segment, "opus" | "sonnet" | "haiku"))?,
    };
    if family == "opus" {
        Some(ClaudeAgentModelMapping {
            model: GPT_5_6_SOL_MODEL,
            reasoning_effort: "high",
        })
    } else if family == "sonnet" {
        Some(ClaudeAgentModelMapping {
            model: GPT_5_6_LUNA_MODEL,
            reasoning_effort: "high",
        })
    } else if family == "haiku" {
        Some(ClaudeAgentModelMapping {
            model: GPT_5_6_LUNA_MODEL,
            reasoning_effort: "medium",
        })
    } else {
        None
    }
}

pub(crate) fn normalize_agent_role_description(
    field_label: &str,
    description: Option<&str>,
) -> std::io::Result<Option<String>> {
    match description.map(str::trim) {
        Some("") => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{field_label} cannot be blank"),
        )),
        Some(description) => Ok(Some(description.to_string())),
        None => Ok(None),
    }
}

fn validate_agent_role_file_developer_instructions(
    role_file_label: &Path,
    developer_instructions: Option<&str>,
    require_present: bool,
) -> std::io::Result<()> {
    match developer_instructions.map(str::trim) {
        Some("") => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "agent role file at {}.developer_instructions cannot be blank",
                role_file_label.display()
            ),
        )),
        Some(_) => Ok(()),
        None if require_present => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "agent role file at {} must define `developer_instructions`",
                role_file_label.display()
            ),
        )),
        None => Ok(()),
    }
}

pub(crate) fn normalize_agent_role_nickname_candidates(
    field_label: &str,
    nickname_candidates: Option<&[String]>,
) -> std::io::Result<Option<Vec<String>>> {
    let Some(nickname_candidates) = nickname_candidates else {
        return Ok(None);
    };

    if nickname_candidates.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{field_label} must contain at least one name"),
        ));
    }

    let mut normalized_candidates = Vec::with_capacity(nickname_candidates.len());
    let mut seen_candidates = BTreeSet::new();

    for nickname in nickname_candidates {
        let normalized_nickname = nickname.trim();
        if normalized_nickname.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{field_label} cannot contain blank names"),
            ));
        }

        if !seen_candidates.insert(normalized_nickname.to_owned()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{field_label} cannot contain duplicates"),
            ));
        }

        if !normalized_nickname
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "{field_label} may only contain ASCII letters, digits, spaces, hyphens, and underscores"
                ),
            ));
        }

        normalized_candidates.push(normalized_nickname.to_owned());
    }

    Ok(Some(normalized_candidates))
}

#[cfg(test)]
#[path = "agent_role_config_tests.rs"]
mod tests;
