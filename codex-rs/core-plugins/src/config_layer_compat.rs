use codex_config::ConfigLayerEntry;
use codex_config::ConfigLayerSource;
use codex_config::ConfigLayerStack;
use codex_config::ConfigLayerStackOrdering;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::path::Path;
use std::path::PathBuf;
use toml::Value;

/// Returns the home directory paired with the base user config layer.
///
/// Claude compatibility uses this for `$HOME/.claude` discovery. It must stay
/// anchored to the base user layer rather than the active user layer, because
/// profile-v2 overlays may be stored under a different config file while still
/// sharing the same Claude home.
pub(crate) fn base_user_home_dir(config_layer_stack: &ConfigLayerStack) -> Option<PathBuf> {
    let user_config_folders =
        base_user_layers(config_layer_stack).filter_map(ConfigLayerEntry::config_folder);

    home_dir_from_base_user_config_folders(user_config_folders)
}

/// Returns the merged config from enabled user layers only.
///
/// This is a local adapter for upstream profile-v2 APIs so plugin config reads
/// keep using base user config on older branches and profile overlays after the
/// daily rebase.
pub(crate) fn effective_user_config(config_layer_stack: &ConfigLayerStack) -> Option<Value> {
    let user_configs = base_user_layers(config_layer_stack).map(|layer| &layer.config);
    merge_user_config_values(user_configs)
}

fn base_user_layers(
    config_layer_stack: &ConfigLayerStack,
) -> impl Iterator<Item = &ConfigLayerEntry> {
    config_layer_stack
        .get_layers(
            ConfigLayerStackOrdering::LowestPrecedenceFirst,
            /*include_disabled*/ false,
        )
        .into_iter()
        .filter(|layer| matches!(&layer.name, ConfigLayerSource::User { .. }))
}

fn merge_user_config_values<'a>(
    user_configs: impl IntoIterator<Item = &'a Value>,
) -> Option<Value> {
    let mut user_configs = user_configs.into_iter();
    let mut merged = user_configs.next()?.clone();
    for user_config in user_configs {
        merge_toml_values(&mut merged, user_config);
    }
    Some(merged)
}

fn merge_toml_values(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Table(base_table), Value::Table(overlay_table)) => {
            for (key, value) in overlay_table {
                if let Some(base_value) = base_table.get_mut(key) {
                    merge_toml_values(base_value, value);
                } else {
                    base_table.insert(key.clone(), value.clone());
                }
            }
        }
        (base_value, overlay_value) => *base_value = overlay_value.clone(),
    }
}

fn home_dir_from_base_user_config_folders(
    user_config_folders: impl IntoIterator<Item = AbsolutePathBuf>,
) -> Option<PathBuf> {
    user_config_folders
        .into_iter()
        .next()
        .and_then(|config_folder| config_folder.as_path().parent().map(Path::to_path_buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_config::ConfigLayerStack;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;
    use toml::Value;

    #[test]
    fn base_user_home_dir_uses_parent_of_user_config_folder() {
        let home = tempdir().expect("tempdir");
        let config_toml = AbsolutePathBuf::try_from(home.path().join(".codex/config.toml"))
            .expect("absolute path");
        let stack = ConfigLayerStack::default()
            .with_user_config(&config_toml, Value::Table(toml::map::Map::new()))
            .expect("valid user config");

        assert_eq!(base_user_home_dir(&stack), Some(home.path().to_path_buf()));
    }

    #[test]
    fn home_dir_from_base_user_config_folders_prefers_base_over_profile_overlay() {
        let home = tempdir().expect("tempdir");
        let profile_home = tempdir().expect("tempdir");
        let base_config_folder =
            AbsolutePathBuf::try_from(home.path().join(".codex")).expect("absolute path");
        let profile_config_folder =
            AbsolutePathBuf::try_from(profile_home.path().join(".codex")).expect("absolute path");

        assert_eq!(
            home_dir_from_base_user_config_folders([base_config_folder, profile_config_folder]),
            Some(home.path().to_path_buf())
        );
    }

    #[test]
    fn merge_user_config_values_overlays_profile_config() {
        let base = r#"
[features]
plugins = true

[plugins."sample@test"]
enabled = false
"#;
        let overlay = r#"
[plugins."sample@test"]
enabled = true
"#;
        let expected = r#"
[features]
plugins = true

[plugins."sample@test"]
enabled = true
"#;

        let merged = merge_user_config_values([
            &toml::from_str::<Value>(base).expect("base toml"),
            &toml::from_str::<Value>(overlay).expect("overlay toml"),
        ]);

        assert_eq!(
            merged,
            Some(toml::from_str::<Value>(expected).expect("expected toml"))
        );
    }
}
