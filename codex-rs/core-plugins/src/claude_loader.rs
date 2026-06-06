use crate::claude::enabled_claude_plugin_roots;
use crate::config_layer_compat::base_user_home_dir;
use crate::loader::PluginLoadScope;
use crate::loader::load_plugin_from_root;
use crate::store::PluginStore;
use codex_config::ConfigLayerStack;
use codex_config::types::McpServerConfig;
use codex_config::types::PluginMcpServerConfig;
use codex_plugin::LoadedPlugin;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

pub(crate) async fn load_enabled_claude_plugins(
    config_layer_stack: &ConfigLayerStack,
    store: &PluginStore,
    configured_plugin_keys: &HashSet<String>,
    scope: &PluginLoadScope<'_>,
) -> Vec<LoadedPlugin<McpServerConfig>> {
    let home_dir = claude_plugins_home_dir(config_layer_stack);
    let mcp_server_policies: HashMap<String, PluginMcpServerConfig> = HashMap::new();
    let mut plugins = Vec::new();

    for claude_plugin in enabled_claude_plugin_roots(home_dir.as_deref()) {
        let config_name = claude_plugin.plugin_id.as_key();
        if configured_plugin_keys.contains(&config_name) {
            continue;
        }
        let plugin_data_root = store.plugin_data_root(&claude_plugin.plugin_id);
        plugins.push(
            load_plugin_from_root(
                config_name,
                claude_plugin.source_path,
                /*enabled*/ true,
                &claude_plugin.plugin_id,
                plugin_data_root,
                &mcp_server_policies,
                scope,
            )
            .await,
        );
    }

    plugins
}

fn claude_plugins_home_dir(config_layer_stack: &ConfigLayerStack) -> Option<PathBuf> {
    base_user_home_dir(config_layer_stack)
}
