use super::AgentRoleConfig;
use super::AgentRoleToml;
use super::AgentsToml;
use super::ConfigToml;
use crate::config_loader::ConfigLayerStack;
use crate::config_loader::ConfigLayerStackOrdering;
use codex_git_utils::get_git_repo_root;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_absolute_path::AbsolutePathBufGuard;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;
use toml::Value as TomlValue;

pub(crate) fn load_agent_roles(
    cfg: &ConfigToml,
    config_layer_stack: &ConfigLayerStack,
    codex_home: &Path,
    resolved_cwd: &Path,
    trust_repo_agent_roles: bool,
    startup_warnings: &mut Vec<String>,
) -> std::io::Result<BTreeMap<String, AgentRoleConfig>> {
    let layers = config_layer_stack.get_layers(
        ConfigLayerStackOrdering::LowestPrecedenceFirst,
        /*include_disabled*/ false,
    );
    if layers.is_empty() {
        return load_agent_roles_without_layers(
            cfg,
            codex_home,
            resolved_cwd,
            trust_repo_agent_roles,
            startup_warnings,
        );
    }

    let mut roles: BTreeMap<String, DiscoveredAgentRole> =
        discover_home_claude_agent_roles(codex_home, startup_warnings)?;
    for layer in layers {
        let mut layer_roles: BTreeMap<String, AgentRoleConfig> = BTreeMap::new();
        let mut declared_role_files = BTreeSet::new();
        let agents_toml = match agents_toml_from_layer(&layer.config) {
            Ok(agents_toml) => agents_toml,
            Err(err) => {
                push_agent_role_warning(startup_warnings, err);
                None
            }
        };
        if let Some(agents_toml) = agents_toml {
            for (declared_role_name, role_toml) in &agents_toml.roles {
                let (role_name, role) = match read_declared_role(declared_role_name, role_toml) {
                    Ok(role) => role,
                    Err(err) => {
                        push_agent_role_warning(startup_warnings, err);
                        continue;
                    }
                };
                if let Some(config_file) = role.config_file.clone() {
                    declared_role_files.insert(config_file);
                }
                if layer_roles.contains_key(&role_name) {
                    push_agent_role_warning(
                        startup_warnings,
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            format!(
                                "duplicate agent role name `{role_name}` declared in the same config layer"
                            ),
                        ),
                    );
                    continue;
                }
                layer_roles.insert(role_name, role);
            }
        }

        if let Some(config_folder) = layer.config_folder() {
            for (role_name, role) in discover_agent_roles_in_dir(
                config_folder.as_path().join("agents").as_path(),
                &declared_role_files,
                startup_warnings,
            )? {
                if layer_roles.contains_key(&role_name) {
                    push_agent_role_warning(
                        startup_warnings,
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            format!(
                                "duplicate agent role name `{role_name}` declared in the same config layer"
                            ),
                        ),
                    );
                    continue;
                }
                layer_roles.insert(role_name, role);
            }
        }

        for (role_name, role) in layer_roles {
            let mut merged_role = role;
            if let Some(existing_role) = roles.get(&role_name) {
                merge_missing_role_fields(&mut merged_role, &existing_role.config);
            }
            if let Err(err) = validate_required_agent_role_description(
                &role_name,
                merged_role.description.as_deref(),
            ) {
                push_agent_role_warning(startup_warnings, err);
                continue;
            }
            roles.insert(role_name, DiscoveredAgentRole::layer(merged_role));
        }
    }

    merge_repo_claude_agent_roles(
        &mut roles,
        resolved_cwd,
        trust_repo_agent_roles,
        startup_warnings,
    )?;

    Ok(roles
        .into_iter()
        .map(|(role_name, role)| (role_name, role.config))
        .collect())
}

fn push_agent_role_warning(startup_warnings: &mut Vec<String>, err: std::io::Error) {
    let message = format!("Ignoring malformed agent role definition: {err}");
    tracing::warn!("{message}");
    startup_warnings.push(message);
}

fn load_agent_roles_without_layers(
    cfg: &ConfigToml,
    codex_home: &Path,
    resolved_cwd: &Path,
    trust_repo_agent_roles: bool,
    startup_warnings: &mut Vec<String>,
) -> std::io::Result<BTreeMap<String, AgentRoleConfig>> {
    let mut roles: BTreeMap<String, DiscoveredAgentRole> =
        discover_home_claude_agent_roles(codex_home, startup_warnings)?;
    if let Some(agents_toml) = cfg.agents.as_ref() {
        for (declared_role_name, role_toml) in &agents_toml.roles {
            let (role_name, role) = read_declared_role(declared_role_name, role_toml)?;
            validate_required_agent_role_description(&role_name, role.description.as_deref())?;

            if let Some(existing_role) = roles.get(&role_name) {
                if existing_role.origin == AgentRoleOrigin::Layer {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("duplicate agent role name `{role_name}` declared in config"),
                    ));
                }
            }
            let mut merged_role = role;
            if let Some(existing_role) = roles.get(&role_name) {
                merge_missing_role_fields(&mut merged_role, &existing_role.config);
            }
            roles.insert(role_name.clone(), DiscoveredAgentRole::layer(merged_role));
        }
    }

    merge_repo_claude_agent_roles(
        &mut roles,
        resolved_cwd,
        trust_repo_agent_roles,
        startup_warnings,
    )?;

    Ok(roles
        .into_iter()
        .map(|(role_name, role)| (role_name, role.config))
        .collect())
}

fn read_declared_role(
    declared_role_name: &str,
    role_toml: &AgentRoleToml,
) -> std::io::Result<(String, AgentRoleConfig)> {
    let mut role = agent_role_config_from_toml(declared_role_name, role_toml)?;
    let mut role_name = declared_role_name.to_string();
    if let Some(config_file) = role.config_file.as_deref() {
        let parsed_file = read_resolved_agent_role_file(config_file, Some(declared_role_name))?;
        role_name = parsed_file.role_name;
        role.description = parsed_file.description.or(role.description);
        role.nickname_candidates = parsed_file.nickname_candidates.or(role.nickname_candidates);
    }

    Ok((role_name, role))
}

fn merge_missing_role_fields(role: &mut AgentRoleConfig, fallback: &AgentRoleConfig) {
    role.description = role.description.clone().or(fallback.description.clone());
    role.config_file = role.config_file.clone().or(fallback.config_file.clone());
    role.nickname_candidates = role
        .nickname_candidates
        .clone()
        .or(fallback.nickname_candidates.clone());
}

fn agents_toml_from_layer(layer_toml: &TomlValue) -> std::io::Result<Option<AgentsToml>> {
    let Some(agents_toml) = layer_toml.get("agents") else {
        return Ok(None);
    };

    agents_toml
        .clone()
        .try_into()
        .map(Some)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))
}

fn agent_role_config_from_toml(
    role_name: &str,
    role: &AgentRoleToml,
) -> std::io::Result<AgentRoleConfig> {
    let config_file = role.config_file.as_ref().map(AbsolutePathBuf::to_path_buf);
    validate_agent_role_config_file(role_name, config_file.as_deref())?;
    let description = normalize_agent_role_description(
        &format!("agents.{role_name}.description"),
        role.description.as_deref(),
    )?;
    let nickname_candidates = normalize_agent_role_nickname_candidates(
        &format!("agents.{role_name}.nickname_candidates"),
        role.nickname_candidates.as_deref(),
    )?;

    Ok(AgentRoleConfig {
        description,
        config_file,
        nickname_candidates,
    })
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

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
struct RawClaudeAgentFrontmatter {
    name: Option<String>,
    description: Option<String>,
    skills: Option<ClaudeAgentSkills>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
enum ClaudeAgentSkills {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedAgentRoleFile {
    pub(crate) role_name: String,
    pub(crate) description: Option<String>,
    pub(crate) nickname_candidates: Option<Vec<String>>,
    pub(crate) config: TomlValue,
}

pub(crate) fn parse_agent_role_file_contents(
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

fn read_resolved_agent_role_file(
    path: &Path,
    role_name_hint: Option<&str>,
) -> std::io::Result<ResolvedAgentRoleFile> {
    let contents = std::fs::read_to_string(path)?;
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("md") => parse_claude_markdown_agent_role_file_contents(
            &contents,
            path,
            role_name_hint.or_else(|| path.file_stem().and_then(|name| name.to_str())),
        ),
        _ => parse_agent_role_file_contents(
            &contents,
            path,
            path.parent().unwrap_or(path),
            role_name_hint,
        ),
    }
}

fn parse_claude_markdown_agent_role_file_contents(
    contents: &str,
    role_file_label: &Path,
    role_name_hint: Option<&str>,
) -> std::io::Result<ResolvedAgentRoleFile> {
    let (frontmatter, body) = split_markdown_frontmatter(contents);
    let parsed_frontmatter = match frontmatter {
        Some(frontmatter) => serde_yaml::from_str::<RawClaudeAgentFrontmatter>(frontmatter)
            .map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "failed to parse Claude agent frontmatter at {}: {err}",
                        role_file_label.display()
                    ),
                )
            })?,
        None => RawClaudeAgentFrontmatter::default(),
    };
    let description = normalize_agent_role_description(
        &format!("agent role file {}.description", role_file_label.display()),
        parsed_frontmatter.description.as_deref(),
    )?;
    let role_name = parsed_frontmatter
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

    let developer_instructions =
        render_claude_markdown_developer_instructions(body, parsed_frontmatter.skills.as_ref());
    validate_agent_role_file_developer_instructions(
        role_file_label,
        Some(&developer_instructions),
        role_name_hint.is_none(),
    )?;

    let mut config = TomlValue::try_from(ConfigToml {
        developer_instructions: Some(developer_instructions),
        ..Default::default()
    })
    .map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "failed to build Codex config for Claude agent role at {}: {err}",
                role_file_label.display()
            ),
        )
    })?;
    let Some(config_table) = config.as_table_mut() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "generated config for Claude agent role at {} must contain a TOML table",
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
        nickname_candidates: None,
        config,
    })
}

fn split_markdown_frontmatter(content: &str) -> (Option<&str>, &str) {
    let mut segments = content.split_inclusive('\n');
    let Some(first_segment) = segments.next() else {
        return (None, "");
    };
    let first_line = first_segment.trim_end_matches(['\r', '\n']);
    if first_line.trim() != "---" {
        return (None, content);
    }

    let mut frontmatter_closed = false;
    let frontmatter_start = first_segment.len();
    let mut frontmatter_end = frontmatter_start;
    let mut consumed = first_segment.len();

    for segment in segments {
        let line = segment.trim_end_matches(['\r', '\n']);
        let trimmed = line.trim();
        if trimmed == "---" {
            frontmatter_closed = true;
            frontmatter_end = consumed;
            consumed += segment.len();
            break;
        }
        consumed += segment.len();
    }

    if !frontmatter_closed {
        return (None, content);
    }

    (
        Some(&content[frontmatter_start..frontmatter_end]),
        if consumed >= content.len() {
            ""
        } else {
            &content[consumed..]
        },
    )
}

fn render_claude_markdown_developer_instructions(
    body: &str,
    skills: Option<&ClaudeAgentSkills>,
) -> String {
    let body = body.trim();
    let Some(skills_note) = render_claude_skills_note(skills) else {
        return body.to_string();
    };
    if body.is_empty() {
        skills_note
    } else {
        format!("{skills_note}\n\n{body}")
    }
}

fn render_claude_skills_note(skills: Option<&ClaudeAgentSkills>) -> Option<String> {
    let skills = match skills? {
        ClaudeAgentSkills::One(skill) => vec![skill.trim().to_string()],
        ClaudeAgentSkills::Many(skills) => skills
            .iter()
            .map(|skill| skill.trim().to_string())
            .collect(),
    };
    let skills = skills
        .into_iter()
        .filter(|skill| !skill.is_empty())
        .collect::<Vec<_>>();
    if skills.is_empty() {
        None
    } else {
        Some(format!(
            "Preferred skills from the source Claude agent definition: {}.",
            skills.join(", ")
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentRoleOrigin {
    Compatibility,
    Layer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiscoveredAgentRole {
    config: AgentRoleConfig,
    origin: AgentRoleOrigin,
}

impl DiscoveredAgentRole {
    fn compatibility(config: AgentRoleConfig) -> Self {
        Self {
            config,
            origin: AgentRoleOrigin::Compatibility,
        }
    }

    fn layer(config: AgentRoleConfig) -> Self {
        Self {
            config,
            origin: AgentRoleOrigin::Layer,
        }
    }
}

fn discover_home_claude_agent_roles(
    codex_home: &Path,
    startup_warnings: &mut Vec<String>,
) -> std::io::Result<BTreeMap<String, DiscoveredAgentRole>> {
    let mut roles = BTreeMap::new();
    let Some(home_dir) = codex_home.parent() else {
        return Ok(roles);
    };
    for (role_name, role) in discover_agent_roles_in_dir(
        &home_dir.join(".claude").join("agents"),
        &BTreeSet::new(),
        startup_warnings,
    )? {
        roles.insert(role_name, DiscoveredAgentRole::compatibility(role));
    }
    Ok(roles)
}

fn merge_repo_claude_agent_roles(
    roles: &mut BTreeMap<String, DiscoveredAgentRole>,
    resolved_cwd: &Path,
    trust_repo_agent_roles: bool,
    startup_warnings: &mut Vec<String>,
) -> std::io::Result<()> {
    if !trust_repo_agent_roles {
        return Ok(());
    }
    let Some(repo_root) = get_git_repo_root(resolved_cwd) else {
        return Ok(());
    };
    for (role_name, role) in discover_agent_roles_in_dir(
        &repo_root.join(".claude").join("agents"),
        &BTreeSet::new(),
        startup_warnings,
    )? {
        match roles.get(&role_name) {
            Some(existing_role) if existing_role.origin == AgentRoleOrigin::Layer => continue,
            Some(existing_role) => {
                let mut merged_role = role;
                merge_missing_role_fields(&mut merged_role, &existing_role.config);
                roles.insert(role_name, DiscoveredAgentRole::compatibility(merged_role));
            }
            None => {
                roles.insert(role_name, DiscoveredAgentRole::compatibility(role));
            }
        }
    }
    Ok(())
}

fn normalize_agent_role_description(
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

fn validate_required_agent_role_description(
    role_name: &str,
    description: Option<&str>,
) -> std::io::Result<()> {
    if description.is_some() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("agent role `{role_name}` must define a description"),
        ))
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

fn validate_agent_role_config_file(
    role_name: &str,
    config_file: Option<&Path>,
) -> std::io::Result<()> {
    let Some(config_file) = config_file else {
        return Ok(());
    };

    let metadata = std::fs::metadata(config_file).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "agents.{role_name}.config_file must point to an existing file at {}: {e}",
                config_file.display()
            ),
        )
    })?;
    if metadata.is_file() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "agents.{role_name}.config_file must point to a file: {}",
                config_file.display()
            ),
        ))
    }
}

fn normalize_agent_role_nickname_candidates(
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

fn discover_agent_roles_in_dir(
    agents_dir: &Path,
    declared_role_files: &BTreeSet<PathBuf>,
    startup_warnings: &mut Vec<String>,
) -> std::io::Result<BTreeMap<String, AgentRoleConfig>> {
    let mut roles = BTreeMap::new();

    for agent_file in collect_agent_role_files(agents_dir)? {
        if declared_role_files.contains(&agent_file) {
            continue;
        }
        let parsed_file =
            match read_resolved_agent_role_file(&agent_file, /*role_name_hint*/ None) {
                Ok(parsed_file) => parsed_file,
                Err(err) => {
                    push_agent_role_warning(startup_warnings, err);
                    continue;
                }
            };
        let role_name = parsed_file.role_name;
        if roles.contains_key(&role_name) {
            push_agent_role_warning(
                startup_warnings,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "duplicate agent role name `{role_name}` discovered in {}",
                        agents_dir.display()
                    ),
                ),
            );
            continue;
        }
        roles.insert(
            role_name,
            AgentRoleConfig {
                description: parsed_file.description,
                config_file: Some(agent_file),
                nickname_candidates: parsed_file.nickname_candidates,
            },
        );
    }

    Ok(roles)
}

fn collect_agent_role_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_agent_role_files_recursive(dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_agent_role_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };

    for entry in read_dir {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_agent_role_files_recursive(&path, files)?;
            continue;
        }
        if file_type.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "toml" || extension == "md")
        {
            files.push(path);
        }
    }

    Ok(())
}
