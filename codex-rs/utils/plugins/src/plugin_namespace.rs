//! Resolve plugin namespace from skill file paths by walking ancestors for `plugin.json`.

use crate::PluginSkillRoot;
use codex_exec_server::ExecutorFileSystem;
use codex_exec_server_protocol::DISCOVERABLE_PLUGIN_MANIFEST_PATHS;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use std::path::Path;
use std::path::PathBuf;

pub fn find_plugin_manifest_path(plugin_root: &Path) -> Option<PathBuf> {
    DISCOVERABLE_PLUGIN_MANIFEST_PATHS
        .iter()
        .map(|relative_path| plugin_root.join(relative_path))
        .find(|manifest_path| manifest_path.is_file())
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
        plugin_id: name.clone(),
        plugin_namespace: name,
        plugin_root: abs_dir,
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
        match fs.get_metadata(&candidate, /*sandbox*/ None).await {
            Ok(metadata) if metadata.is_file => {
                manifest_path = Some(candidate);
                break;
            }
            Ok(_) | Err(_) => {}
        }
    }
    let contents = fs
        .read_file_text(&manifest_path?, /*sandbox*/ None)
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

/// Returns the plugin manifest `name` for the nearest ancestor of `path` that contains a valid
/// plugin manifest (same `name` rules as full manifest loading in codex-core).
pub async fn plugin_namespace_for_skill_path(
    fs: &dyn ExecutorFileSystem,
    path: &AbsolutePathBuf,
) -> Option<String> {
    plugin_namespace_for_skill_uri(fs, &PathUri::from_abs_path(path)).await
}

/// Returns the plugin manifest `name` for the nearest URI ancestor of `path`.
pub async fn plugin_namespace_for_skill_uri(
    fs: &dyn ExecutorFileSystem,
    path: &PathUri,
) -> Option<String> {
    let mut ancestor = Some(path.clone());
    while let Some(path) = ancestor {
        if let Some(name) = plugin_namespace_for_root_uri(fs, &path).await {
            return Some(name);
        }
        ancestor = path.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::find_plugin_manifest_path;
    use super::plugin_namespace_for_skill_path;
    use codex_exec_server::LOCAL_FS;
    use codex_utils_absolute_path::test_support::PathBufExt;
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
            plugin_namespace_for_skill_path(LOCAL_FS.as_ref(), &skill_path.abs()).await,
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
            plugin_namespace_for_skill_path(LOCAL_FS.as_ref(), &skill_path.abs()).await,
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
            plugin_namespace_for_skill_path(LOCAL_FS.as_ref(), &skill_path.abs()).await,
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
        assert_eq!(root.plugin_id, "my-toolkit");
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
        assert_eq!(root.plugin_id, "kookr-toolkit");
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
        assert_eq!(root.plugin_id, "fallback-name");
    }
}
