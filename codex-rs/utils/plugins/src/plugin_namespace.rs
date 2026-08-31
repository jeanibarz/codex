//! Resolve plugin namespace from skill file paths by walking ancestors for `plugin.json`.

use crate::PluginIdentity;
use crate::PluginSkillRoot;
use crate::SkillDiscoveryMode;
use codex_exec_server::ExecutorFileSystem;
use codex_exec_server::GetMetadataOptions;
use codex_exec_server::ReadFileOptions;
use codex_exec_server_protocol::DISCOVERABLE_PLUGIN_MANIFEST_PATHS;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use std::path::Path;
use std::path::PathBuf;

pub const AGENT_PLUGIN_MANIFEST_RELATIVE_PATH: &str = "plugin.json";
/// Published Agent Plugins v1 manifest schema:
/// https://github.com/agentplugins/agent-plugins-spec/blob/main/schemas/1.0.0/plugin.schema.json
pub const AGENT_PLUGIN_SCHEMA_URI: &str =
    "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";
pub const SUPPORTED_AGENT_PLUGIN_SCHEMA_URIS: &[&str] = &[AGENT_PLUGIN_SCHEMA_URI];
pub const AGENT_PLUGIN_SCHEMA_PREFIX: &str = "https://agent-plugins.org/schemas/";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentPluginSchemaStatus {
    Supported,
    Unsupported,
    Unrelated,
}

pub fn agent_plugin_schema_status(contents: &str) -> AgentPluginSchemaStatus {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(contents) else {
        return AgentPluginSchemaStatus::Unrelated;
    };
    let Some(schema) = value.get("$schema").and_then(serde_json::Value::as_str) else {
        return AgentPluginSchemaStatus::Unrelated;
    };
    if SUPPORTED_AGENT_PLUGIN_SCHEMA_URIS.contains(&schema) {
        AgentPluginSchemaStatus::Supported
    } else if schema.starts_with(AGENT_PLUGIN_SCHEMA_PREFIX) {
        AgentPluginSchemaStatus::Unsupported
    } else {
        AgentPluginSchemaStatus::Unrelated
    }
}

pub fn find_plugin_manifest_path(plugin_root: &Path) -> Option<PathBuf> {
    let agent_manifest_path = plugin_root.join(AGENT_PLUGIN_MANIFEST_RELATIVE_PATH);
    match std::fs::symlink_metadata(&agent_manifest_path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_file() => {
            return None;
        }
        Ok(_) => {
            if std::fs::read_to_string(&agent_manifest_path)
                .ok()
                .is_some_and(|contents| {
                    agent_plugin_schema_status(&contents) != AgentPluginSchemaStatus::Unrelated
                })
            {
                return Some(agent_manifest_path);
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return None,
    }

    for relative_path in DISCOVERABLE_PLUGIN_MANIFEST_PATHS {
        let manifest_path = plugin_root.join(relative_path);
        let manifest_parent = manifest_path.parent()?;
        match std::fs::symlink_metadata(manifest_parent) {
            Ok(metadata) if !metadata.file_type().is_dir() => return None,
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return None,
        }
        match std::fs::symlink_metadata(&manifest_path) {
            Ok(metadata) if metadata.file_type().is_file() => return Some(manifest_path),
            Ok(_) => return None,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
    }
    None
}

/// Build a [`PluginSkillRoot`] from a directory passed via `--plugin-dir`.
///
/// Reads the plugin manifest at `<dir>/.codex-plugin/plugin.json` or
/// `<dir>/.claude-plugin/plugin.json` (in that order) for the plugin's
/// `name`. Returns `None` if the directory has no manifest, the manifest
/// is unreadable, or the `<dir>/skills/` subdirectory cannot be resolved.
///
/// Used by the CLI `--plugin-dir` flag to inject extra skill roots
/// without going through the marketplace install flow. Mirrors Claude
/// Code's `--plugin-dir` semantics for symmetric supervisor injection.
pub fn plugin_skill_root_from_cli_dir(dir: &Path) -> Option<PluginSkillRoot> {
    let manifest_path = find_plugin_manifest_path(dir)?;
    let contents = std::fs::read_to_string(&manifest_path).ok()?;
    let raw: RawPluginManifestName = serde_json::from_str(&contents).ok()?;
    let name = if raw.name.trim().is_empty() {
        dir.file_name()?.to_str()?.to_string()
    } else {
        raw.name
    };
    let canonical = std::fs::canonicalize(dir).ok()?;
    let abs_dir = AbsolutePathBuf::from_absolute_path_checked(canonical).ok()?;
    Some(PluginSkillRoot {
        path: abs_dir.join("skills"),
        plugin_identity: PluginIdentity {
            plugin_id: name.clone(),
            remote_plugin_id: None,
        },
        plugin_namespace: name,
        plugin_root: abs_dir,
        discovery_mode: SkillDiscoveryMode::Recursive,
    })
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPluginManifestName {
    #[serde(default)]
    name: String,
}

/// Returns the plugin manifest `name` defined directly below `plugin_root`.
pub async fn plugin_namespace_for_root_uri(
    fs: &dyn ExecutorFileSystem,
    plugin_root: &PathUri,
) -> Option<String> {
    let mut manifest_path = None;
    for relative_path in DISCOVERABLE_PLUGIN_MANIFEST_PATHS {
        let candidate = plugin_root.join(relative_path).ok()?;
        match fs
            .get_metadata(
                &candidate,
                GetMetadataOptions::default(),
                /*sandbox*/ None,
            )
            .await
        {
            Ok(metadata) if metadata.is_file => {
                manifest_path = Some(candidate);
                break;
            }
            Ok(_) | Err(_) => {}
        }
    }
    let contents = fs
        .read_file_text(
            &manifest_path?,
            ReadFileOptions::default(),
            /*sandbox*/ None,
        )
        .await
        .ok()?;
    let RawPluginManifestName { name: raw_name } = serde_json::from_str(&contents).ok()?;
    Some(
        plugin_root
            .basename()
            .filter(|_| raw_name.trim().is_empty())
            .unwrap_or(raw_name),
    )
}

#[cfg(test)]
mod tests {
    use super::AGENT_PLUGIN_MANIFEST_RELATIVE_PATH;
    use super::AGENT_PLUGIN_SCHEMA_URI;
    use super::find_plugin_manifest_path;
    use super::plugin_namespace_for_root_uri;
    use codex_exec_server::LOCAL_FS;
    use codex_utils_absolute_path::test_support::PathBufExt;
    use codex_utils_path_uri::PathUri;
    use std::fs;
    use tempfile::tempdir;

    const ALTERNATE_PLUGIN_CLA_MANIFEST_RELATIVE_PATH: &str = ".claude-plugin/plugin.json";
    const ALTERNATE_PLUGIN_CUR_MANIFEST_RELATIVE_PATH: &str = ".cursor-plugin/plugin.json";

    #[tokio::test]
    async fn uses_manifest_name() {
        let tmp = tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugins/sample");
        let skill_path = plugin_root.join("skills/search/SKILL.md");

        fs::create_dir_all(skill_path.parent().expect("parent")).expect("mkdir");
        fs::create_dir_all(plugin_root.join(".codex-plugin")).expect("mkdir manifest");
        fs::write(
            plugin_root.join(".codex-plugin/plugin.json"),
            r#"{"name":"sample"}"#,
        )
        .expect("write manifest");
        fs::write(&skill_path, "---\ndescription: search\n---\n").expect("write skill");

        assert_eq!(
            plugin_namespace_for_root_uri(
                LOCAL_FS.as_ref(),
                &PathUri::from_abs_path(&plugin_root.abs()),
            )
            .await,
            Some("sample".to_string())
        );
    }

    #[tokio::test]
    async fn uses_name_from_alternate_discoverable_manifest_path() {
        let tmp = tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugins/sample");
        let skill_path = plugin_root.join("skills/search/SKILL.md");
        let manifest_path = plugin_root.join(ALTERNATE_PLUGIN_CLA_MANIFEST_RELATIVE_PATH);

        fs::create_dir_all(skill_path.parent().expect("parent")).expect("mkdir");
        fs::create_dir_all(manifest_path.parent().expect("manifest parent"))
            .expect("mkdir manifest");
        fs::write(&manifest_path, r#"{"name":"sample"}"#).expect("write manifest");
        fs::write(&skill_path, "---\ndescription: search\n---\n").expect("write skill");

        assert_eq!(
            plugin_namespace_for_root_uri(
                LOCAL_FS.as_ref(),
                &PathUri::from_abs_path(&plugin_root.abs()),
            )
            .await,
            Some("sample".to_string())
        );
        assert_eq!(find_plugin_manifest_path(&plugin_root), Some(manifest_path));
    }

    #[tokio::test]
    async fn uses_name_from_cur_plugin_manifest_path() {
        let tmp = tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugins/sample");
        let skill_path = plugin_root.join("skills/search/SKILL.md");
        let manifest_path = plugin_root.join(ALTERNATE_PLUGIN_CUR_MANIFEST_RELATIVE_PATH);

        fs::create_dir_all(skill_path.parent().expect("parent")).expect("mkdir");
        fs::create_dir_all(manifest_path.parent().expect("manifest parent"))
            .expect("mkdir manifest");
        fs::write(&manifest_path, r#"{"name":"sample"}"#).expect("write manifest");
        fs::write(&skill_path, "---\ndescription: search\n---\n").expect("write skill");

        assert_eq!(
            plugin_namespace_for_root_uri(
                LOCAL_FS.as_ref(),
                &PathUri::from_abs_path(&plugin_root.abs()),
            )
            .await,
            Some("sample".to_string())
        );
        assert_eq!(find_plugin_manifest_path(&plugin_root), Some(manifest_path));
    }

    #[test]
    fn plugin_skill_root_from_cli_dir_resolves_codex_manifest() {
        use super::plugin_skill_root_from_cli_dir;
        let tmp = tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugins/codex-style");
        fs::create_dir_all(plugin_root.join(".codex-plugin")).expect("mkdir codex-plugin");
        fs::create_dir_all(plugin_root.join("skills")).expect("mkdir skills");
        fs::write(
            plugin_root.join(".codex-plugin/plugin.json"),
            r#"{"name":"my-toolkit"}"#,
        )
        .expect("write manifest");

        let root = plugin_skill_root_from_cli_dir(&plugin_root).expect("plugin root resolved");
        assert_eq!(root.plugin_identity.plugin_id, "my-toolkit");
        assert!(root.path.as_path().ends_with("skills"));
    }

    #[test]
    fn plugin_skill_root_from_cli_dir_resolves_claude_manifest() {
        use super::plugin_skill_root_from_cli_dir;
        let tmp = tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugins/claude-style");
        fs::create_dir_all(plugin_root.join(".claude-plugin")).expect("mkdir claude-plugin");
        fs::create_dir_all(plugin_root.join("skills")).expect("mkdir skills");
        fs::write(
            plugin_root.join(".claude-plugin/plugin.json"),
            r#"{"name":"kookr-toolkit"}"#,
        )
        .expect("write manifest");

        let root = plugin_skill_root_from_cli_dir(&plugin_root).expect("plugin root resolved");
        assert_eq!(root.plugin_identity.plugin_id, "kookr-toolkit");
    }

    #[test]
    fn plugin_skill_root_from_cli_dir_returns_none_when_manifest_missing() {
        use super::plugin_skill_root_from_cli_dir;
        let tmp = tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugins/no-manifest");
        fs::create_dir_all(&plugin_root).expect("mkdir");

        assert!(plugin_skill_root_from_cli_dir(&plugin_root).is_none());
    }

    #[test]
    fn plugin_skill_root_from_cli_dir_falls_back_to_dirname_for_empty_name() {
        use super::plugin_skill_root_from_cli_dir;
        let tmp = tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugins/fallback-name");
        fs::create_dir_all(plugin_root.join(".codex-plugin")).expect("mkdir codex-plugin");
        fs::create_dir_all(plugin_root.join("skills")).expect("mkdir skills");
        fs::write(
            plugin_root.join(".codex-plugin/plugin.json"),
            r#"{"name":"   "}"#,
        )
        .expect("write manifest");

        let root = plugin_skill_root_from_cli_dir(&plugin_root).expect("plugin root resolved");
        assert_eq!(root.plugin_identity.plugin_id, "fallback-name");
    }

    #[test]
    fn recognizes_schema_declared_root_plugin_manifest() {
        let tmp = tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugins/portable");
        let skill_path = plugin_root.join("skills/search/SKILL.md");
        fs::create_dir_all(skill_path.parent().expect("parent")).expect("mkdir");
        let manifest_path = plugin_root.join(AGENT_PLUGIN_MANIFEST_RELATIVE_PATH);
        fs::write(
            &manifest_path,
            format!(r#"{{"$schema":"{AGENT_PLUGIN_SCHEMA_URI}","name":"portable"}}"#),
        )
        .expect("write manifest");

        assert_eq!(find_plugin_manifest_path(&plugin_root), Some(manifest_path));
    }

    #[test]
    fn ignores_unrelated_root_plugin_manifest_before_legacy_fallback() {
        let tmp = tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugins/sample");
        let legacy_path = plugin_root.join(".codex-plugin/plugin.json");
        fs::create_dir_all(legacy_path.parent().expect("parent")).expect("mkdir");
        fs::write(plugin_root.join("plugin.json"), r#"{"name":"npm-package"}"#)
            .expect("write unrelated root");
        fs::write(&legacy_path, r#"{"name":"sample"}"#).expect("write legacy");

        assert_eq!(find_plugin_manifest_path(&plugin_root), Some(legacy_path));
    }

    #[test]
    fn rejects_nonregular_root_plugin_manifest() {
        let tmp = tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugins/sample");
        let legacy_path = plugin_root.join(".codex-plugin/plugin.json");
        fs::create_dir_all(plugin_root.join("plugin.json")).expect("root manifest directory");
        fs::create_dir_all(legacy_path.parent().expect("parent")).expect("legacy parent");
        fs::write(&legacy_path, r#"{"name":"sample"}"#).expect("legacy manifest");

        assert_eq!(find_plugin_manifest_path(&plugin_root), None);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_root_plugin_manifest() {
        let tmp = tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugins/sample");
        let manifest_target = tmp.path().join("manifest.json");
        let legacy_path = plugin_root.join(".codex-plugin/plugin.json");
        fs::create_dir_all(&plugin_root).expect("plugin root");
        fs::write(
            &manifest_target,
            format!(r#"{{"$schema":"{AGENT_PLUGIN_SCHEMA_URI}","name":"sample"}}"#),
        )
        .expect("manifest target");
        std::os::unix::fs::symlink(&manifest_target, plugin_root.join("plugin.json"))
            .expect("root manifest symlink");
        fs::create_dir_all(legacy_path.parent().expect("parent")).expect("legacy parent");
        fs::write(&legacy_path, r#"{"name":"sample"}"#).expect("legacy manifest");

        assert_eq!(find_plugin_manifest_path(&plugin_root), None);
    }

    #[test]
    fn rejects_nonregular_legacy_plugin_manifest_before_lower_precedence_manifest() {
        let tmp = tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugins/sample");
        let codex_path = plugin_root.join(".codex-plugin/plugin.json");
        let claude_path = plugin_root.join(ALTERNATE_PLUGIN_CLA_MANIFEST_RELATIVE_PATH);
        fs::create_dir_all(&codex_path).expect("nonregular Codex manifest");
        fs::create_dir_all(claude_path.parent().expect("Claude manifest parent"))
            .expect("Claude manifest parent");
        fs::write(&claude_path, r#"{"name":"sample"}"#).expect("Claude manifest");

        assert_eq!(find_plugin_manifest_path(&plugin_root), None);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_legacy_plugin_manifest_before_lower_precedence_manifest() {
        let tmp = tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugins/sample");
        let codex_path = plugin_root.join(".codex-plugin/plugin.json");
        let claude_path = plugin_root.join(ALTERNATE_PLUGIN_CLA_MANIFEST_RELATIVE_PATH);
        fs::create_dir_all(codex_path.parent().expect("Codex manifest parent"))
            .expect("Codex manifest parent");
        fs::create_dir_all(claude_path.parent().expect("Claude manifest parent"))
            .expect("Claude manifest parent");
        fs::write(plugin_root.join("benign.json"), r#"{"name":"sample"}"#)
            .expect("benign manifest");
        fs::write(&claude_path, r#"{"name":"sample"}"#).expect("Claude manifest");
        std::os::unix::fs::symlink("../benign.json", &codex_path).expect("Codex manifest symlink");

        assert_eq!(find_plugin_manifest_path(&plugin_root), None);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_legacy_plugin_manifest_directory_before_lower_precedence_manifest() {
        let tmp = tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugins/sample");
        let manifest_directory = tmp.path().join("manifest-directory");
        let claude_path = plugin_root.join(ALTERNATE_PLUGIN_CLA_MANIFEST_RELATIVE_PATH);
        fs::create_dir_all(&manifest_directory).expect("manifest target directory");
        fs::create_dir_all(claude_path.parent().expect("Claude manifest parent"))
            .expect("Claude manifest parent");
        fs::write(
            manifest_directory.join("plugin.json"),
            r#"{"name":"sample"}"#,
        )
        .expect("benign manifest");
        fs::write(&claude_path, r#"{"name":"sample"}"#).expect("Claude manifest");
        std::os::unix::fs::symlink(&manifest_directory, plugin_root.join(".codex-plugin"))
            .expect("Codex manifest directory symlink");

        assert_eq!(find_plugin_manifest_path(&plugin_root), None);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_claude_manifest_before_cursor_manifest() {
        let tmp = tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugins/sample");
        let claude_path = plugin_root.join(ALTERNATE_PLUGIN_CLA_MANIFEST_RELATIVE_PATH);
        let cursor_path = plugin_root.join(ALTERNATE_PLUGIN_CUR_MANIFEST_RELATIVE_PATH);
        fs::create_dir_all(claude_path.parent().expect("Claude manifest parent"))
            .expect("Claude manifest parent");
        fs::create_dir_all(cursor_path.parent().expect("Cursor manifest parent"))
            .expect("Cursor manifest parent");
        let manifest_target = plugin_root.join("benign.json");
        fs::write(&manifest_target, r#"{"name":"sample"}"#).expect("benign manifest");
        fs::write(&cursor_path, r#"{"name":"sample"}"#).expect("Cursor manifest");
        std::os::unix::fs::symlink(&manifest_target, &claude_path)
            .expect("Claude manifest symlink");

        assert_eq!(find_plugin_manifest_path(&plugin_root), None);
    }

    #[test]
    fn preserves_codex_claude_cursor_legacy_precedence() {
        let tmp = tempdir().expect("tempdir");
        let plugin_root = tmp.path().join("plugins/sample");
        let codex_path = plugin_root.join(".codex-plugin/plugin.json");
        let claude_path = plugin_root.join(".claude-plugin/plugin.json");
        let cursor_path = plugin_root.join(".cursor-plugin/plugin.json");
        for path in [&codex_path, &claude_path, &cursor_path] {
            fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            fs::write(path, r#"{"name":"sample"}"#).expect("write manifest");
        }

        assert_eq!(
            find_plugin_manifest_path(&plugin_root),
            Some(codex_path.clone())
        );
        fs::remove_file(codex_path).expect("remove Codex manifest");
        assert_eq!(find_plugin_manifest_path(&plugin_root), Some(claude_path));
    }
}
