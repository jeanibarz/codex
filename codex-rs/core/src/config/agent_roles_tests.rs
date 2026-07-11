use super::GPT_5_6_LUNA_MODEL;
use super::GPT_5_6_SOL_MODEL;
use super::GPT_5_6_TERRA_MODEL;
use super::normalize_claude_agent_model_name;
use super::parse_agent_role_file_contents;
use pretty_assertions::assert_eq;
use std::path::Path;
use toml::Value as TomlValue;

#[test]
fn claude_agent_model_aliases_map_to_gpt_5_6_tiers() {
    assert_eq!(normalize_claude_agent_model_name("opus"), GPT_5_6_SOL_MODEL);
    assert_eq!(
        normalize_claude_agent_model_name("claude-sonnet-4-20250514"),
        GPT_5_6_TERRA_MODEL
    );
    assert_eq!(
        normalize_claude_agent_model_name("claude-haiku-4-5"),
        GPT_5_6_LUNA_MODEL
    );
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
model = "gpt-5.6-terra"
"#,
    )
    .expect("parse expected config");

    assert_eq!(parsed.role_name, "reviewer");
    assert_eq!(parsed.description.as_deref(), Some("Review code"));
    assert_eq!(parsed.config, expected);
}
