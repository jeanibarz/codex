use super::*;
use codex_exec_server::LOCAL_FS;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use std::fs;
use tempfile::TempDir;

fn cwd_for(tmp: &TempDir) -> AbsolutePathBuf {
    AbsolutePathBuf::from_absolute_path(tmp.path()).expect("tempdir is absolute")
}

#[tokio::test]
async fn returns_empty_when_no_rules_dir() {
    let tmp = TempDir::new().unwrap();
    let cwd = cwd_for(&tmp);
    let rules = discover_rules(&cwd, LOCAL_FS.as_ref(), 4096).await.unwrap();
    assert!(rules.is_empty());
    assert_eq!(render_rules(&rules), None);
}

#[tokio::test]
async fn returns_empty_when_max_bytes_zero() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".codex/rules")).unwrap();
    fs::write(tmp.path().join(".codex/rules/a.md"), "rule body").unwrap();

    let cwd = cwd_for(&tmp);
    let rules = discover_rules(&cwd, LOCAL_FS.as_ref(), 0).await.unwrap();
    assert!(rules.is_empty());
}

#[tokio::test]
async fn loads_rule_without_frontmatter() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".codex/rules")).unwrap();
    fs::write(
        tmp.path().join(".codex/rules/a.md"),
        "Always commit using conventional commits.\n",
    )
    .unwrap();

    let cwd = cwd_for(&tmp);
    let rules = discover_rules(&cwd, LOCAL_FS.as_ref(), 4096).await.unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].display_path, ".codex/rules/a.md");
    assert!(rules[0].paths_globs.is_empty());
    assert_eq!(rules[0].description, None);
    assert_eq!(rules[0].body, "Always commit using conventional commits.\n");
}

#[tokio::test]
async fn loads_rule_with_paths_frontmatter() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".codex/rules")).unwrap();
    fs::write(
        tmp.path().join(".codex/rules/server.md"),
        "---\npaths:\n  - \"src/server/**/*.ts\"\n  - \"src/api/**/*.ts\"\ndescription: Server-side conventions\n---\n\nUse Hono for HTTP handlers. Never Express.\n",
    )
    .unwrap();

    let cwd = cwd_for(&tmp);
    let rules = discover_rules(&cwd, LOCAL_FS.as_ref(), 4096).await.unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(
        rules[0].paths_globs,
        vec!["src/server/**/*.ts".to_string(), "src/api/**/*.ts".to_string()]
    );
    assert_eq!(
        rules[0].description.as_deref(),
        Some("Server-side conventions"),
    );
    assert!(rules[0].body.contains("Use Hono for HTTP handlers."));

    let rendered = render_rules(&rules).expect("rendered");
    assert!(rendered.contains("--- conditional-rules ---"));
    assert!(rendered.contains(".codex/rules/server.md"));
    assert!(rendered.contains("Server-side conventions"));
    assert!(rendered.contains("paths: src/server/**/*.ts, src/api/**/*.ts"));
    assert!(rendered.contains("Use Hono for HTTP handlers."));
}

#[tokio::test]
async fn merges_codex_and_claude_rules() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".codex/rules")).unwrap();
    fs::create_dir_all(tmp.path().join(".claude/rules")).unwrap();
    fs::write(tmp.path().join(".codex/rules/codex_only.md"), "codex rule").unwrap();
    fs::write(tmp.path().join(".claude/rules/claude_only.md"), "claude rule").unwrap();

    let cwd = cwd_for(&tmp);
    let rules = discover_rules(&cwd, LOCAL_FS.as_ref(), 4096).await.unwrap();
    let display_paths: Vec<&str> = rules.iter().map(|r| r.display_path.as_str()).collect();
    assert_eq!(
        display_paths,
        vec![".codex/rules/codex_only.md", ".claude/rules/claude_only.md"]
    );
}

#[tokio::test]
async fn skips_non_markdown_files() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".codex/rules")).unwrap();
    fs::write(tmp.path().join(".codex/rules/keep.md"), "kept").unwrap();
    fs::write(tmp.path().join(".codex/rules/skip.txt"), "skipped").unwrap();
    fs::write(tmp.path().join(".codex/rules/.gitignore"), "ignored").unwrap();

    let cwd = cwd_for(&tmp);
    let rules = discover_rules(&cwd, LOCAL_FS.as_ref(), 4096).await.unwrap();
    let names: Vec<&str> = rules.iter().map(|r| r.display_path.as_str()).collect();
    assert_eq!(names, vec![".codex/rules/keep.md"]);
}

#[tokio::test]
async fn skips_rule_with_invalid_frontmatter() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".codex/rules")).unwrap();
    // Open frontmatter that is never closed.
    fs::write(
        tmp.path().join(".codex/rules/broken.md"),
        "---\npaths:\n  - bad\n",
    )
    .unwrap();
    fs::write(tmp.path().join(".codex/rules/good.md"), "ok").unwrap();

    let cwd = cwd_for(&tmp);
    let rules = discover_rules(&cwd, LOCAL_FS.as_ref(), 4096).await.unwrap();
    let names: Vec<&str> = rules.iter().map(|r| r.display_path.as_str()).collect();
    assert_eq!(names, vec![".codex/rules/good.md"]);
}

#[tokio::test]
async fn skips_rule_with_empty_body() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".codex/rules")).unwrap();
    fs::write(
        tmp.path().join(".codex/rules/meta_only.md"),
        "---\npaths:\n  - \"foo\"\n---\n\n   \n",
    )
    .unwrap();
    fs::write(tmp.path().join(".codex/rules/has_body.md"), "real rule").unwrap();

    let cwd = cwd_for(&tmp);
    let rules = discover_rules(&cwd, LOCAL_FS.as_ref(), 4096).await.unwrap();
    let names: Vec<&str> = rules.iter().map(|r| r.display_path.as_str()).collect();
    assert_eq!(names, vec![".codex/rules/has_body.md"]);
}

#[tokio::test]
async fn truncates_when_budget_exhausted() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".codex/rules")).unwrap();
    fs::write(tmp.path().join(".codex/rules/a.md"), "a".repeat(20)).unwrap();
    fs::write(tmp.path().join(".codex/rules/b.md"), "b".repeat(20)).unwrap();

    let cwd = cwd_for(&tmp);
    // Budget only fits the first file.
    let rules = discover_rules(&cwd, LOCAL_FS.as_ref(), 20).await.unwrap();
    let names: Vec<&str> = rules.iter().map(|r| r.display_path.as_str()).collect();
    assert_eq!(names, vec![".codex/rules/a.md"]);
}
