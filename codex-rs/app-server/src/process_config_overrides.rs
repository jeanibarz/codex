use codex_core::config::Config;
use codex_core::config::ConfigOverrides;
use std::path::PathBuf;

pub(crate) fn apply_process_settings_file(
    overrides: &mut ConfigOverrides,
    process_settings_file: &Option<PathBuf>,
) {
    if overrides.settings_file.is_none() {
        overrides.settings_file = process_settings_file.clone();
    }
}

pub(crate) fn apply_thread_runtime_overrides(overrides: &mut ConfigOverrides, config: &Config) {
    if overrides.settings_file.is_none() {
        overrides.settings_file = config.settings_file.clone();
    }
    if overrides.bypass_hook_trust.is_none() {
        overrides.bypass_hook_trust = Some(config.bypass_hook_trust);
    }
    if overrides.cli_plugin_dirs.is_empty() {
        overrides.cli_plugin_dirs = config.cli_plugin_dirs.clone();
    }
}
