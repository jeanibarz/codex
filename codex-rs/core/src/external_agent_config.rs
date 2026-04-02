use serde_json::Value as JsonValue;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use toml::Value as TomlValue;

const EXTERNAL_AGENT_CONFIG_DETECT_METRIC: &str = "codex.external_agent_config.detect";
const EXTERNAL_AGENT_CONFIG_IMPORT_METRIC: &str = "codex.external_agent_config.import";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalAgentConfigDetectOptions {
    pub include_home: bool,
    pub cwds: Option<Vec<PathBuf>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalAgentConfigMigrationItemType {
    Config,
    McpServerConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalAgentConfigMigrationItem {
    pub item_type: ExternalAgentConfigMigrationItemType,
    pub description: String,
    pub cwd: Option<PathBuf>,
}

#[derive(Clone)]
pub struct ExternalAgentConfigService {
    codex_home: PathBuf,
    claude_home: PathBuf,
}

impl ExternalAgentConfigService {
    pub fn new(codex_home: PathBuf) -> Self {
        let claude_home = default_claude_home();
        Self {
            codex_home,
            claude_home,
        }
    }

    #[cfg(test)]
    fn new_for_test(codex_home: PathBuf, claude_home: PathBuf) -> Self {
        Self {
            codex_home,
            claude_home,
        }
    }

    pub fn detect(
        &self,
        params: ExternalAgentConfigDetectOptions,
    ) -> io::Result<Vec<ExternalAgentConfigMigrationItem>> {
        let mut items = Vec::new();
        if params.include_home {
            self.detect_migrations(/*repo_root*/ None, &mut items)?;
        }

        for cwd in params.cwds.as_deref().unwrap_or(&[]) {
            let Some(repo_root) = find_repo_root(Some(cwd))? else {
                continue;
            };
            self.detect_migrations(Some(&repo_root), &mut items)?;
        }

        Ok(items)
    }

    pub fn import(&self, migration_items: Vec<ExternalAgentConfigMigrationItem>) -> io::Result<()> {
        for migration_item in migration_items {
            match migration_item.item_type {
                ExternalAgentConfigMigrationItemType::Config => {
                    self.import_config(migration_item.cwd.as_deref())?;
                    emit_migration_metric(
                        EXTERNAL_AGENT_CONFIG_IMPORT_METRIC,
                        ExternalAgentConfigMigrationItemType::Config,
                        /*skills_count*/ None,
                    );
                }
                ExternalAgentConfigMigrationItemType::McpServerConfig => {}
            }
        }

        Ok(())
    }

    fn detect_migrations(
        &self,
        repo_root: Option<&Path>,
        items: &mut Vec<ExternalAgentConfigMigrationItem>,
    ) -> io::Result<()> {
        let cwd = repo_root.map(Path::to_path_buf);
        let source_settings = repo_root.map_or_else(
            || self.claude_home.join("settings.json"),
            |repo_root| repo_root.join(".claude").join("settings.json"),
        );
        let target_config = repo_root.map_or_else(
            || self.codex_home.join("config.toml"),
            |repo_root| repo_root.join(".codex").join("config.toml"),
        );
        if source_settings.is_file() {
            let raw_settings = fs::read_to_string(&source_settings)?;
            let settings: JsonValue = serde_json::from_str(&raw_settings)
                .map_err(|err| invalid_data_error(err.to_string()))?;
            let migrated = build_config_from_external(&settings)?;
            if !is_empty_toml_table(&migrated) {
                let mut should_include = true;
                if target_config.exists() {
                    let existing_raw = fs::read_to_string(&target_config)?;
                    let mut existing = if existing_raw.trim().is_empty() {
                        TomlValue::Table(Default::default())
                    } else {
                        toml::from_str::<TomlValue>(&existing_raw).map_err(|err| {
                            invalid_data_error(format!("invalid existing config.toml: {err}"))
                        })?
                    };
                    should_include = merge_missing_toml_values(&mut existing, &migrated)?;
                }

                if should_include {
                    items.push(ExternalAgentConfigMigrationItem {
                        item_type: ExternalAgentConfigMigrationItemType::Config,
                        description: format!(
                            "Migrate {} into {}",
                            source_settings.display(),
                            target_config.display()
                        ),
                        cwd: cwd.clone(),
                    });
                    emit_migration_metric(
                        EXTERNAL_AGENT_CONFIG_DETECT_METRIC,
                        ExternalAgentConfigMigrationItemType::Config,
                        /*skills_count*/ None,
                    );
                }
            }
        }

        Ok(())
    }

    fn import_config(&self, cwd: Option<&Path>) -> io::Result<()> {
        let (source_settings, target_config) = if let Some(repo_root) = find_repo_root(cwd)? {
            (
                repo_root.join(".claude").join("settings.json"),
                repo_root.join(".codex").join("config.toml"),
            )
        } else if cwd.is_some_and(|cwd| !cwd.as_os_str().is_empty()) {
            return Ok(());
        } else {
            (
                self.claude_home.join("settings.json"),
                self.codex_home.join("config.toml"),
            )
        };
        if !source_settings.is_file() {
            return Ok(());
        }

        let raw_settings = fs::read_to_string(&source_settings)?;
        let settings: JsonValue = serde_json::from_str(&raw_settings)
            .map_err(|err| invalid_data_error(err.to_string()))?;
        let migrated = build_config_from_external(&settings)?;
        if is_empty_toml_table(&migrated) {
            return Ok(());
        }

        let Some(target_parent) = target_config.parent() else {
            return Err(invalid_data_error("config target path has no parent"));
        };
        fs::create_dir_all(target_parent)?;
        if !target_config.exists() {
            write_toml_file(&target_config, &migrated)?;
            return Ok(());
        }

        let existing_raw = fs::read_to_string(&target_config)?;
        let mut existing = if existing_raw.trim().is_empty() {
            TomlValue::Table(Default::default())
        } else {
            toml::from_str::<TomlValue>(&existing_raw)
                .map_err(|err| invalid_data_error(format!("invalid existing config.toml: {err}")))?
        };

        let changed = merge_missing_toml_values(&mut existing, &migrated)?;
        if !changed {
            return Ok(());
        }

        write_toml_file(&target_config, &existing)?;
        Ok(())
    }
}

fn default_claude_home() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        return PathBuf::from(home).join(".claude");
    }

    PathBuf::from(".claude")
}

fn find_repo_root(cwd: Option<&Path>) -> io::Result<Option<PathBuf>> {
    let Some(cwd) = cwd.filter(|cwd| !cwd.as_os_str().is_empty()) else {
        return Ok(None);
    };

    let mut current = if cwd.is_absolute() {
        cwd.to_path_buf()
    } else {
        std::env::current_dir()?.join(cwd)
    };

    if !current.exists() {
        return Ok(None);
    }

    if current.is_file() {
        let Some(parent) = current.parent() else {
            return Ok(None);
        };
        current = parent.to_path_buf();
    }

    let fallback = current.clone();
    loop {
        let git_path = current.join(".git");
        if git_path.is_dir() || git_path.is_file() {
            return Ok(Some(current));
        }
        if !current.pop() {
            break;
        }
    }

    Ok(Some(fallback))
}

fn build_config_from_external(settings: &JsonValue) -> io::Result<TomlValue> {
    let Some(settings_obj) = settings.as_object() else {
        return Err(invalid_data_error(
            "external agent settings root must be an object",
        ));
    };

    let mut root = toml::map::Map::new();

    if let Some(env) = settings_obj.get("env").and_then(JsonValue::as_object)
        && !env.is_empty()
    {
        let mut shell_policy = toml::map::Map::new();
        shell_policy.insert("inherit".to_string(), TomlValue::String("core".to_string()));
        shell_policy.insert(
            "set".to_string(),
            TomlValue::Table(json_object_to_env_toml_table(env)),
        );
        root.insert(
            "shell_environment_policy".to_string(),
            TomlValue::Table(shell_policy),
        );
    }

    if let Some(sandbox_enabled) = settings_obj
        .get("sandbox")
        .and_then(JsonValue::as_object)
        .and_then(|sandbox| sandbox.get("enabled"))
        .and_then(JsonValue::as_bool)
        && sandbox_enabled
    {
        root.insert(
            "sandbox_mode".to_string(),
            TomlValue::String("workspace-write".to_string()),
        );
    }

    Ok(TomlValue::Table(root))
}

fn json_object_to_env_toml_table(
    object: &serde_json::Map<String, JsonValue>,
) -> toml::map::Map<String, TomlValue> {
    let mut table = toml::map::Map::new();
    for (key, value) in object {
        if let Some(value) = json_env_value_to_string(value) {
            table.insert(key.clone(), TomlValue::String(value));
        }
    }
    table
}

fn json_env_value_to_string(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::String(value) => Some(value.clone()),
        JsonValue::Null => None,
        JsonValue::Bool(value) => Some(value.to_string()),
        JsonValue::Number(value) => Some(value.to_string()),
        JsonValue::Array(_) | JsonValue::Object(_) => None,
    }
}

fn merge_missing_toml_values(existing: &mut TomlValue, incoming: &TomlValue) -> io::Result<bool> {
    match (existing, incoming) {
        (TomlValue::Table(existing_table), TomlValue::Table(incoming_table)) => {
            let mut changed = false;
            for (key, incoming_value) in incoming_table {
                match existing_table.get_mut(key) {
                    Some(existing_value) => {
                        if matches!(
                            (&*existing_value, incoming_value),
                            (TomlValue::Table(_), TomlValue::Table(_))
                        ) && merge_missing_toml_values(existing_value, incoming_value)?
                        {
                            changed = true;
                        }
                    }
                    None => {
                        existing_table.insert(key.clone(), incoming_value.clone());
                        changed = true;
                    }
                }
            }
            Ok(changed)
        }
        _ => Err(invalid_data_error(
            "expected TOML table while merging migrated config values",
        )),
    }
}

fn write_toml_file(path: &Path, value: &TomlValue) -> io::Result<()> {
    let serialized = toml::to_string_pretty(value)
        .map_err(|err| invalid_data_error(format!("failed to serialize config.toml: {err}")))?;
    fs::write(path, format!("{}\n", serialized.trim_end()))
}

fn is_empty_toml_table(value: &TomlValue) -> bool {
    match value {
        TomlValue::Table(table) => table.is_empty(),
        TomlValue::String(_)
        | TomlValue::Integer(_)
        | TomlValue::Float(_)
        | TomlValue::Boolean(_)
        | TomlValue::Datetime(_)
        | TomlValue::Array(_) => false,
    }
}

fn invalid_data_error(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn migration_metric_tags(
    item_type: ExternalAgentConfigMigrationItemType,
    _skills_count: Option<usize>,
) -> Vec<(&'static str, String)> {
    let migration_type = match item_type {
        ExternalAgentConfigMigrationItemType::Config => "config",
        ExternalAgentConfigMigrationItemType::McpServerConfig => "mcp_server_config",
    };
    vec![("migration_type", migration_type.to_string())]
}

fn emit_migration_metric(
    metric_name: &str,
    item_type: ExternalAgentConfigMigrationItemType,
    skills_count: Option<usize>,
) {
    let Some(metrics) = codex_otel::metrics::global() else {
        return;
    };
    let tags = migration_metric_tags(item_type, skills_count);
    let tag_refs = tags
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect::<Vec<_>>();
    let _ = metrics.counter(metric_name, /*inc*/ 1, &tag_refs);
}

#[cfg(test)]
#[path = "external_agent_config_tests.rs"]
mod tests;
