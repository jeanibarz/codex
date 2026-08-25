use super::GPT_5_6_LUNA_MODEL;
use super::GPT_5_6_SOL_MODEL;
use super::claude_agent_model_reasoning_effort;
use super::normalize_claude_agent_model_name;
use super::parse_agent_role_file_contents;
use pretty_assertions::assert_eq;
use std::path::Path;
use toml::Value as TomlValue;

#[test]
fn claude_agent_model_aliases_map_to_capability_tiers() {
    assert_eq!(normalize_claude_agent_model_name("opus"), GPT_5_6_SOL_MODEL);
    assert_eq!(claude_agent_model_reasoning_effort("opus"), Some("high"));
    assert_eq!(
        normalize_claude_agent_model_name("claude-opus-5"),
        GPT_5_6_SOL_MODEL
    );
    assert_eq!(
        normalize_claude_agent_model_name("claude-3-opus-20240229"),
        GPT_5_6_SOL_MODEL
    );
    assert_eq!(
        normalize_claude_agent_model_name("claude-sonnet-4-20250514"),
        GPT_5_6_LUNA_MODEL
    );
    assert_eq!(
        normalize_claude_agent_model_name("claude-3-5-sonnet-20241022"),
        GPT_5_6_LUNA_MODEL
    );
    assert_eq!(
        claude_agent_model_reasoning_effort("claude-sonnet-4-20250514"),
        Some("high")
    );
    assert_eq!(
        normalize_claude_agent_model_name("claude-haiku-4-5"),
        GPT_5_6_LUNA_MODEL
    );
    assert_eq!(
        claude_agent_model_reasoning_effort("claude-haiku-4-5"),
        Some("medium")
    );
    assert_eq!(
        normalize_claude_agent_model_name("custom-opus-reviewer"),
        "custom-opus-reviewer"
    );
    assert_eq!(
        claude_agent_model_reasoning_effort("custom-opus-reviewer"),
        None
    );
    assert_eq!(
        normalize_claude_agent_model_name("opus-local"),
        "opus-local"
    );
    assert_eq!(claude_agent_model_reasoning_effort("opus-local"), None);
}

#[test]
fn claude_agent_markdown_import_normalizes_model_alias() {
    let parsed = parse_agent_role_file_contents(
        "---\nname: reviewer\ndescription: Review code\nmodel: sonnet\n---\nReview carefully.\n",
        Path::new("reviewer.md"),
        Path::new("."),
        None,
    )
    .expect("parse Claude agent role");

    let expected: TomlValue = toml::from_str(
        r#"
developer_instructions = "Review carefully."
model = "gpt-5.6-luna"
model_reasoning_effort = "high"
"#,
    )
    .expect("parse expected config");

    assert_eq!(parsed.role_name, "reviewer");
    assert_eq!(parsed.description.as_deref(), Some("Review code"));
    assert_eq!(parsed.config, expected);
}

#[test]
fn claude_agent_markdown_import_maps_opus_to_sol_high() {
    let parsed = parse_agent_role_file_contents(
        "---\nname: reviewer\ndescription: Review code\nmodel: opus\n---\nReview carefully.\n",
        Path::new("reviewer.md"),
        Path::new("."),
        None,
    )
    .expect("parse Claude agent role");

    let expected: TomlValue = toml::from_str(
        r#"
developer_instructions = "Review carefully."
model = "gpt-5.6-sol"
model_reasoning_effort = "high"
"#,
    )
    .expect("parse expected config");

    assert_eq!(parsed.config, expected);
}
