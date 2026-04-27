use crate::marketplace::find_marketplace_manifest_path;
use crate::marketplace::find_marketplace_plugin;
use crate::marketplace::MarketplacePluginSource;
use codex_plugin::PluginId;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use tracing::warn;

const CLAUDE_SETTINGS_RELATIVE_PATH: &str = ".claude/settings.json";
const CLAUDE_MARKETPLACES_RELATIVE_PATH: &str = ".claude/plugins/marketplaces";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeEnabledPlugin {
    pub plugin_id: PluginId,
    pub source_path: AbsolutePathBuf,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeSettings {
    #[serde(default)]
    enabled_plugins: BTreeMap<String, bool>,
}

pub fn enabled_claude_plugin_roots(home_dir: Option<&Path>) -> Vec<ClaudeEnabledPlugin> {
    let Some(home_dir) = home_dir else {
        return Vec::new();
    };
    let settings_path = home_dir.join(CLAUDE_SETTINGS_RELATIVE_PATH);
    let contents = match fs::read_to_string(&settings_path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(err) => {
            warn!(
                path = %settings_path.display(),
                "failed to read Claude settings while loading Claude plugins: {err}"
            );
            return Vec::new();
        }
    };
    let settings = match serde_json::from_str::<ClaudeSettings>(&contents) {
        Ok(settings) => settings,
        Err(err) => {
            warn!(
                path = %settings_path.display(),
                "failed to parse Claude settings while loading Claude plugins: {err}"
            );
            return Vec::new();
        }
    };

    let mut plugins = settings
        .enabled_plugins
        .into_iter()
        .filter(|(_, enabled)| *enabled)
        .filter_map(|(plugin_key, _)| enabled_claude_plugin_root(home_dir, plugin_key))
        .collect::<Vec<_>>();
    plugins.sort_unstable_by_key(|plugin| plugin.plugin_id.as_key());
    plugins.dedup_by(|left, right| left.plugin_id == right.plugin_id);
    plugins
}

fn enabled_claude_plugin_root(home_dir: &Path, plugin_key: String) -> Option<ClaudeEnabledPlugin> {
    let plugin_id = match PluginId::parse(&plugin_key) {
        Ok(plugin_id) => plugin_id,
        Err(err) => {
            warn!(
                plugin = plugin_key,
                "ignoring invalid Claude enabled plugin key: {err}"
            );
            return None;
        }
    };

    let marketplace_root = home_dir
        .join(CLAUDE_MARKETPLACES_RELATIVE_PATH)
        .join(&plugin_id.marketplace_name);
    let Some(marketplace_path) = find_marketplace_manifest_path(&marketplace_root) else {
        warn!(
            plugin = plugin_key,
            marketplace = %marketplace_root.display(),
            "ignoring Claude enabled plugin because marketplace manifest is missing"
        );
        return None;
    };
    let resolved = match find_marketplace_plugin(&marketplace_path, &plugin_id.plugin_name) {
        Ok(resolved) => resolved,
        Err(err) => {
            warn!(
                plugin = plugin_key,
                path = %marketplace_path.display(),
                "ignoring Claude enabled plugin because marketplace resolution failed: {err}"
            );
            return None;
        }
    };

    match resolved.source {
        MarketplacePluginSource::Local { path } => {
            if path.as_path().is_dir() {
                Some(ClaudeEnabledPlugin {
                    plugin_id,
                    source_path: path,
                })
            } else {
                warn!(
                    plugin = plugin_key,
                    path = %path.display(),
                    "ignoring Claude enabled plugin because source path is missing"
                );
                None
            }
        }
        MarketplacePluginSource::Git { .. } => {
            warn!(
                plugin = plugin_key,
                "ignoring Claude enabled plugin because Codex cannot load a Git source directly from Claude settings"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::enabled_claude_plugin_roots;
    use codex_plugin::PluginId;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn write_file(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, contents).expect("write file");
    }

    #[test]
    fn resolves_enabled_plugins_from_claude_marketplaces() {
        let home = tempdir().expect("tempdir");
        let plugin_root = home
            .path()
            .join(".claude/plugins/marketplaces/looper/plugin");

        write_file(
            &home.path().join(".claude/settings.json"),
            r#"{
  "enabledPlugins": {
    "looper-toolkit@looper": true
  }
}"#,
        );
        write_file(
            &home
                .path()
                .join(".claude/plugins/marketplaces/looper/.claude-plugin/marketplace.json"),
            r#"{
  "name": "looper",
  "plugins": [
    {
      "name": "looper-toolkit",
      "source": "./plugin"
    }
  ]
}"#,
        );
        write_file(
            &plugin_root.join(".claude-plugin/plugin.json"),
            r#"{"name":"looper-toolkit"}"#,
        );

        let plugins = enabled_claude_plugin_roots(Some(home.path()));

        assert_eq!(plugins.len(), 1);
        assert_eq!(
            plugins[0].plugin_id,
            PluginId::parse("looper-toolkit@looper").expect("plugin id")
        );
        assert_eq!(
            plugins[0].source_path,
            AbsolutePathBuf::try_from(plugin_root).expect("absolute path")
        );
    }

    #[test]
    fn ignores_disabled_claude_plugins() {
        let home = tempdir().expect("tempdir");
        write_file(
            &home.path().join(".claude/settings.json"),
            r#"{
  "enabledPlugins": {
    "looper-toolkit@looper": false
  }
}"#,
        );

        assert_eq!(enabled_claude_plugin_roots(Some(home.path())), Vec::new());
    }
}
