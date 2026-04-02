use super::*;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

fn fixture_paths() -> (TempDir, PathBuf, PathBuf) {
    let root = TempDir::new().expect("create tempdir");
    let claude_home = root.path().join(".claude");
    let codex_home = root.path().join(".codex");
    fs::create_dir_all(&claude_home).expect("create claude home");
    fs::create_dir_all(&codex_home).expect("create codex home");
    (root, claude_home, codex_home)
}

fn service_for_paths(claude_home: PathBuf, codex_home: PathBuf) -> ExternalAgentConfigService {
    ExternalAgentConfigService::new_for_test(codex_home, claude_home)
}

#[test]
fn detect_home_lists_only_config_migration() {
    let (_root, claude_home, codex_home) = fixture_paths();
    fs::write(claude_home.join("CLAUDE.md"), "claude rules").expect("write claude md");
    fs::write(
        claude_home.join("settings.json"),
        r#"{"model":"claude","env":{"FOO":"bar"}}"#,
    )
    .expect("write settings");

    let items = service_for_paths(claude_home.clone(), codex_home.clone())
        .detect(ExternalAgentConfigDetectOptions {
            include_home: true,
            cwds: None,
        })
        .expect("detect");

    let expected = vec![
        ExternalAgentConfigMigrationItem {
            item_type: ExternalAgentConfigMigrationItemType::Config,
            description: format!(
                "Migrate {} into {}",
                claude_home.join("settings.json").display(),
                codex_home.join("config.toml").display()
            ),
            cwd: None,
        },
    ];

    assert_eq!(items, expected);
}

#[test]
fn detect_repo_does_not_list_project_doc_migrations() {
    let root = TempDir::new().expect("create tempdir");
    let repo_root = root.path().join("repo");
    let nested = repo_root.join("nested").join("child");
    fs::create_dir_all(repo_root.join(".git")).expect("create git dir");
    fs::create_dir_all(&nested).expect("create nested");
    fs::write(repo_root.join("CLAUDE.md"), "Claude code guidance").expect("write source");

    let items = service_for_paths(root.path().join(".claude"), root.path().join(".codex"))
        .detect(ExternalAgentConfigDetectOptions {
            include_home: false,
            cwds: Some(vec![nested, repo_root.clone()]),
        })
        .expect("detect");

    assert_eq!(items, Vec::<ExternalAgentConfigMigrationItem>::new());
}

#[test]
fn import_home_migrates_supported_config_fields_only() {
    let (_root, claude_home, codex_home) = fixture_paths();
    fs::write(
            claude_home.join("settings.json"),
            r#"{"model":"claude","permissions":{"ask":["git push"]},"env":{"FOO":"bar","CI":false,"MAX_RETRIES":3,"MY_TEAM":"codex","IGNORED":null,"LIST":["a","b"],"MAP":{"x":1}},"sandbox":{"enabled":true,"network":{"allowLocalBinding":true}}}"#,
        )
        .expect("write settings");
    fs::write(claude_home.join("CLAUDE.md"), "Claude code guidance").expect("write agents");

    service_for_paths(claude_home, codex_home.clone())
        .import(vec![
            ExternalAgentConfigMigrationItem {
                item_type: ExternalAgentConfigMigrationItemType::Config,
                description: String::new(),
                cwd: None,
            },
        ])
        .expect("import");

    assert_eq!(
        fs::read_to_string(codex_home.join("config.toml")).expect("read config"),
        "sandbox_mode = \"workspace-write\"\n\n[shell_environment_policy]\ninherit = \"core\"\n\n[shell_environment_policy.set]\nCI = \"false\"\nFOO = \"bar\"\nMAX_RETRIES = \"3\"\nMY_TEAM = \"codex\"\n"
    );
    assert!(!codex_home.join("CLAUDE.md").exists());
}

#[test]
fn import_home_skips_empty_config_migration() {
    let (_root, claude_home, codex_home) = fixture_paths();
    fs::create_dir_all(&claude_home).expect("create claude home");
    fs::write(
        claude_home.join("settings.json"),
        r#"{"model":"claude","sandbox":{"enabled":false}}"#,
    )
    .expect("write settings");

    service_for_paths(claude_home, codex_home.clone())
        .import(vec![ExternalAgentConfigMigrationItem {
            item_type: ExternalAgentConfigMigrationItemType::Config,
            description: String::new(),
            cwd: None,
        }])
        .expect("import");

    assert!(!codex_home.join("config.toml").exists());
}

#[test]
fn detect_home_skips_config_when_target_already_has_supported_fields() {
    let (_root, claude_home, codex_home) = fixture_paths();
    fs::create_dir_all(&claude_home).expect("create claude home");
    fs::create_dir_all(&codex_home).expect("create codex home");
    fs::write(
        claude_home.join("settings.json"),
        r#"{"env":{"FOO":"bar"},"sandbox":{"enabled":true}}"#,
    )
    .expect("write settings");
    fs::write(
        codex_home.join("config.toml"),
        r#"
            sandbox_mode = "workspace-write"

            [shell_environment_policy]
            inherit = "core"

            [shell_environment_policy.set]
            FOO = "bar"
            "#,
    )
    .expect("write config");

    let items = service_for_paths(claude_home, codex_home)
        .detect(ExternalAgentConfigDetectOptions {
            include_home: true,
            cwds: None,
        })
        .expect("detect");

    assert_eq!(items, Vec::<ExternalAgentConfigMigrationItem>::new());
}

#[test]
fn migration_metric_tags_for_config_omit_skills_count() {
    assert_eq!(
        migration_metric_tags(ExternalAgentConfigMigrationItemType::Config, Some(3)),
        vec![("migration_type", "config".to_string())]
    );
}
