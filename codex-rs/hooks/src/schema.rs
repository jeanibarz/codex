use schemars::r#gen::SchemaGenerator;
use schemars::r#gen::SchemaSettings;
use schemars::schema::InstanceType;
use schemars::schema::RootSchema;
use schemars::schema::Schema;
use schemars::schema::SchemaObject;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;
use std::path::Path;
use std::path::PathBuf;

const GENERATED_DIR: &str = "generated";
const POST_TOOL_USE_INPUT_FIXTURE: &str = "post-tool-use.command.input.schema.json";
const POST_TOOL_USE_OUTPUT_FIXTURE: &str = "post-tool-use.command.output.schema.json";
const POST_TOOL_USE_FAILURE_INPUT_FIXTURE: &str = "post-tool-use-failure.command.input.schema.json";
const POST_TOOL_USE_FAILURE_OUTPUT_FIXTURE: &str = "post-tool-use-failure.command.output.schema.json";
const NOTIFICATION_INPUT_FIXTURE: &str = "notification.command.input.schema.json";
const NOTIFICATION_OUTPUT_FIXTURE: &str = "notification.command.output.schema.json";
const PERMISSION_REQUEST_INPUT_FIXTURE: &str = "permission-request.command.input.schema.json";
const PERMISSION_REQUEST_OUTPUT_FIXTURE: &str = "permission-request.command.output.schema.json";
const PRE_TOOL_USE_INPUT_FIXTURE: &str = "pre-tool-use.command.input.schema.json";
const PRE_TOOL_USE_OUTPUT_FIXTURE: &str = "pre-tool-use.command.output.schema.json";
const SESSION_START_INPUT_FIXTURE: &str = "session-start.command.input.schema.json";
const SESSION_START_OUTPUT_FIXTURE: &str = "session-start.command.output.schema.json";
const USER_PROMPT_SUBMIT_INPUT_FIXTURE: &str = "user-prompt-submit.command.input.schema.json";
const USER_PROMPT_SUBMIT_OUTPUT_FIXTURE: &str = "user-prompt-submit.command.output.schema.json";
const STOP_INPUT_FIXTURE: &str = "stop.command.input.schema.json";
const STOP_OUTPUT_FIXTURE: &str = "stop.command.output.schema.json";

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub(crate) struct NullableString(Option<String>);

impl NullableString {
    pub(crate) fn from_path(path: Option<PathBuf>) -> Self {
        Self(path.map(|path| path.display().to_string()))
    }

    pub(crate) fn from_string(value: Option<String>) -> Self {
        Self(value)
    }
}

impl JsonSchema for NullableString {
    fn schema_name() -> String {
        "NullableString".to_string()
    }

    fn json_schema(_gen: &mut SchemaGenerator) -> Schema {
        Schema::Object(SchemaObject {
            instance_type: Some(vec![InstanceType::String, InstanceType::Null].into()),
            ..Default::default()
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub(crate) struct HookUniversalOutputWire {
    #[serde(default = "default_continue")]
    pub r#continue: bool,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub suppress_output: bool,
    #[serde(default)]
    pub system_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub(crate) enum HookEventNameWire {
    #[serde(rename = "PreToolUse")]
    PreToolUse,
    #[serde(rename = "PostToolUse")]
    PostToolUse,
    #[serde(rename = "PostToolUseFailure")]
    PostToolUseFailure,
    #[serde(rename = "Notification")]
    Notification,
    #[serde(rename = "SessionStart")]
    SessionStart,
    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit,
    #[serde(rename = "Stop")]
    Stop,
    #[serde(rename = "PermissionRequest")]
    PermissionRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(rename = "pre-tool-use.command.output")]
pub(crate) struct PreToolUseCommandOutputWire {
    #[serde(flatten)]
    pub universal: HookUniversalOutputWire,
    #[serde(default)]
    pub decision: Option<PreToolUseDecisionWire>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub hook_specific_output: Option<PreToolUseHookSpecificOutputWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(rename = "post-tool-use.command.output")]
pub(crate) struct PostToolUseCommandOutputWire {
    #[serde(flatten)]
    pub universal: HookUniversalOutputWire,
    #[serde(default)]
    pub decision: Option<BlockDecisionWire>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub hook_specific_output: Option<PostToolUseHookSpecificOutputWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(rename = "post-tool-use-failure.command.output")]
pub(crate) struct PostToolUseFailureCommandOutputWire {
    #[serde(flatten)]
    pub universal: HookUniversalOutputWire,
    #[serde(default)]
    pub decision: Option<BlockDecisionWire>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub hook_specific_output: Option<PostToolUseFailureHookSpecificOutputWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub(crate) struct PostToolUseHookSpecificOutputWire {
    pub hook_event_name: HookEventNameWire,
    #[serde(default)]
    pub additional_context: Option<String>,
    #[serde(default)]
    #[serde(rename = "updatedMCPToolOutput")]
    pub updated_mcp_tool_output: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub(crate) struct PostToolUseFailureHookSpecificOutputWire {
    pub hook_event_name: HookEventNameWire,
    #[serde(default)]
    pub additional_context: Option<String>,
    #[serde(default)]
    #[serde(rename = "updatedMCPToolOutput")]
    pub updated_mcp_tool_output: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub(crate) struct PreToolUseHookSpecificOutputWire {
    pub hook_event_name: HookEventNameWire,
    #[serde(default)]
    pub permission_decision: Option<PreToolUsePermissionDecisionWire>,
    #[serde(default)]
    pub permission_decision_reason: Option<String>,
    #[serde(default)]
    pub updated_input: Option<Value>,
    #[serde(default)]
    pub additional_context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub(crate) enum PreToolUsePermissionDecisionWire {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "deny")]
    Deny,
    #[serde(rename = "ask")]
    Ask,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub(crate) enum PreToolUseDecisionWire {
    #[serde(rename = "approve")]
    Approve,
    #[serde(rename = "block")]
    Block,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub(crate) struct PreToolUseToolInput {
    pub command: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "pre-tool-use.command.input")]
pub(crate) struct PreToolUseCommandInput {
    pub session_id: String,
    /// Codex extension: expose the active turn id to internal turn-scoped hooks.
    pub turn_id: String,
    pub transcript_path: NullableString,
    pub cwd: String,
    #[schemars(schema_with = "pre_tool_use_hook_event_name_schema")]
    pub hook_event_name: String,
    pub model: String,
    #[schemars(schema_with = "permission_mode_schema")]
    pub permission_mode: String,
    #[schemars(schema_with = "pre_tool_use_tool_name_schema")]
    pub tool_name: String,
    pub tool_input: PreToolUseToolInput,
    pub tool_use_id: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub(crate) struct PostToolUseToolInput {
    pub command: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "post-tool-use.command.input")]
pub(crate) struct PostToolUseCommandInput {
    pub session_id: String,
    /// Codex extension: expose the active turn id to internal turn-scoped hooks.
    pub turn_id: String,
    pub transcript_path: NullableString,
    pub cwd: String,
    #[schemars(schema_with = "post_tool_use_hook_event_name_schema")]
    pub hook_event_name: String,
    pub model: String,
    #[schemars(schema_with = "permission_mode_schema")]
    pub permission_mode: String,
    #[schemars(schema_with = "post_tool_use_tool_name_schema")]
    pub tool_name: String,
    pub tool_input: PostToolUseToolInput,
    pub tool_response: Value,
    pub tool_use_id: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "post-tool-use-failure.command.input")]
pub(crate) struct PostToolUseFailureCommandInput {
    pub session_id: String,
    /// Codex extension: expose the active turn id to internal turn-scoped hooks.
    pub turn_id: String,
    pub transcript_path: NullableString,
    pub cwd: String,
    #[schemars(schema_with = "post_tool_use_failure_hook_event_name_schema")]
    pub hook_event_name: String,
    pub model: String,
    #[schemars(schema_with = "permission_mode_schema")]
    pub permission_mode: String,
    #[schemars(schema_with = "post_tool_use_failure_tool_name_schema")]
    pub tool_name: String,
    pub tool_input: Value,
    pub tool_use_id: String,
    pub error: String,
    pub is_interrupt: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(rename = "session-start.command.output")]
pub(crate) struct SessionStartCommandOutputWire {
    #[serde(flatten)]
    pub universal: HookUniversalOutputWire,
    #[serde(default)]
    pub hook_specific_output: Option<SessionStartHookSpecificOutputWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(rename = "notification.command.output")]
pub(crate) struct NotificationCommandOutputWire {
    #[serde(flatten)]
    pub universal: HookUniversalOutputWire,
    #[serde(default)]
    pub hook_specific_output: Option<NotificationHookSpecificOutputWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub(crate) struct NotificationHookSpecificOutputWire {
    pub hook_event_name: HookEventNameWire,
    #[serde(default)]
    pub additional_context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionStartHookSpecificOutputWire {
    pub hook_event_name: HookEventNameWire,
    #[serde(default)]
    pub additional_context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(rename = "permission-request.command.output")]
pub(crate) struct PermissionRequestCommandOutputWire {
    #[serde(flatten)]
    pub universal: HookUniversalOutputWire,
    #[serde(default)]
    pub hook_specific_output: Option<PermissionRequestHookSpecificOutputWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub(crate) struct PermissionRequestHookSpecificOutputWire {
    pub hook_event_name: HookEventNameWire,
    #[serde(default)]
    pub additional_context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(rename = "user-prompt-submit.command.output")]
pub(crate) struct UserPromptSubmitCommandOutputWire {
    #[serde(flatten)]
    pub universal: HookUniversalOutputWire,
    #[serde(default)]
    pub decision: Option<BlockDecisionWire>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub hook_specific_output: Option<UserPromptSubmitHookSpecificOutputWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub(crate) struct UserPromptSubmitHookSpecificOutputWire {
    pub hook_event_name: HookEventNameWire,
    #[serde(default)]
    pub additional_context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(rename = "stop.command.output")]
pub(crate) struct StopCommandOutputWire {
    #[serde(flatten)]
    pub universal: HookUniversalOutputWire,
    #[serde(default)]
    pub decision: Option<BlockDecisionWire>,
    /// Claude requires `reason` when `decision` is `block`; we enforce that
    /// semantic rule during output parsing rather than in the JSON schema.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub(crate) enum BlockDecisionWire {
    #[serde(rename = "block")]
    Block,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "session-start.command.input")]
pub(crate) struct SessionStartCommandInput {
    pub session_id: String,
    pub transcript_path: NullableString,
    pub cwd: String,
    #[schemars(schema_with = "session_start_hook_event_name_schema")]
    pub hook_event_name: String,
    pub model: String,
    #[schemars(schema_with = "permission_mode_schema")]
    pub permission_mode: String,
    #[schemars(schema_with = "session_start_source_schema")]
    pub source: String,
    pub codex_hook_capabilities: CodexHookCapabilitiesWire,
}

impl SessionStartCommandInput {
    pub(crate) fn new(
        session_id: impl Into<String>,
        transcript_path: Option<PathBuf>,
        cwd: impl Into<String>,
        model: impl Into<String>,
        permission_mode: impl Into<String>,
        source: impl Into<String>,
        codex_hook_capabilities: CodexHookCapabilitiesWire,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            transcript_path: NullableString::from_path(transcript_path),
            cwd: cwd.into(),
            hook_event_name: "SessionStart".to_string(),
            model: model.into(),
            permission_mode: permission_mode.into(),
            source: source.into(),
            codex_hook_capabilities,
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodexHookCapabilitiesWire {
    pub surface_version: u32,
    pub supported_events: Vec<HookEventNameWire>,
    pub handler_features: CodexHookHandlerFeaturesWire,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodexHookHandlerFeaturesWire {
    pub command_if: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "user-prompt-submit.command.input")]
pub(crate) struct UserPromptSubmitCommandInput {
    pub session_id: String,
    /// Codex extension: expose the active turn id to internal turn-scoped hooks.
    pub turn_id: String,
    pub transcript_path: NullableString,
    pub cwd: String,
    #[schemars(schema_with = "user_prompt_submit_hook_event_name_schema")]
    pub hook_event_name: String,
    pub model: String,
    #[schemars(schema_with = "permission_mode_schema")]
    pub permission_mode: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "stop.command.input")]
pub(crate) struct StopCommandInput {
    pub session_id: String,
    /// Codex extension: expose the active turn id to internal turn-scoped hooks.
    pub turn_id: String,
    pub transcript_path: NullableString,
    pub cwd: String,
    #[schemars(schema_with = "stop_hook_event_name_schema")]
    pub hook_event_name: String,
    pub model: String,
    #[schemars(schema_with = "permission_mode_schema")]
    pub permission_mode: String,
    pub stop_hook_active: bool,
    pub last_assistant_message: NullableString,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "permission-request.command.input")]
pub(crate) struct PermissionRequestCommandInput {
    pub session_id: String,
    /// Codex extension: expose the active turn id to internal turn-scoped hooks.
    pub turn_id: String,
    pub transcript_path: NullableString,
    pub cwd: String,
    #[schemars(schema_with = "permission_request_hook_event_name_schema")]
    pub hook_event_name: String,
    pub model: String,
    #[schemars(schema_with = "permission_mode_schema")]
    pub permission_mode: String,
    pub tool_name: String,
    pub tool_input: Map<String, Value>,
    pub permission_suggestions: Vec<Map<String, Value>>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "notification.command.input")]
pub(crate) struct NotificationCommandInput {
    pub session_id: String,
    /// Codex extension: expose the active turn id to internal turn-scoped hooks.
    pub turn_id: String,
    pub transcript_path: NullableString,
    pub cwd: String,
    #[schemars(schema_with = "notification_hook_event_name_schema")]
    pub hook_event_name: String,
    pub model: String,
    pub notification_type: String,
    pub message: String,
}

pub fn write_schema_fixtures(schema_root: &Path) -> anyhow::Result<()> {
    let generated_dir = schema_root.join(GENERATED_DIR);
    ensure_empty_dir(&generated_dir)?;

    write_schema(
        &generated_dir.join(POST_TOOL_USE_INPUT_FIXTURE),
        schema_json::<PostToolUseCommandInput>()?,
    )?;
    write_schema(
        &generated_dir.join(POST_TOOL_USE_OUTPUT_FIXTURE),
        schema_json::<PostToolUseCommandOutputWire>()?,
    )?;
    write_schema(
        &generated_dir.join(POST_TOOL_USE_FAILURE_INPUT_FIXTURE),
        schema_json::<PostToolUseFailureCommandInput>()?,
    )?;
    write_schema(
        &generated_dir.join(POST_TOOL_USE_FAILURE_OUTPUT_FIXTURE),
        schema_json::<PostToolUseFailureCommandOutputWire>()?,
    )?;
    write_schema(
        &generated_dir.join(NOTIFICATION_INPUT_FIXTURE),
        schema_json::<NotificationCommandInput>()?,
    )?;
    write_schema(
        &generated_dir.join(NOTIFICATION_OUTPUT_FIXTURE),
        schema_json::<NotificationCommandOutputWire>()?,
    )?;
    write_schema(
        &generated_dir.join(PERMISSION_REQUEST_INPUT_FIXTURE),
        schema_json::<PermissionRequestCommandInput>()?,
    )?;
    write_schema(
        &generated_dir.join(PERMISSION_REQUEST_OUTPUT_FIXTURE),
        schema_json::<PermissionRequestCommandOutputWire>()?,
    )?;
    write_schema(
        &generated_dir.join(PRE_TOOL_USE_INPUT_FIXTURE),
        schema_json::<PreToolUseCommandInput>()?,
    )?;
    write_schema(
        &generated_dir.join(PRE_TOOL_USE_OUTPUT_FIXTURE),
        schema_json::<PreToolUseCommandOutputWire>()?,
    )?;
    write_schema(
        &generated_dir.join(SESSION_START_INPUT_FIXTURE),
        schema_json::<SessionStartCommandInput>()?,
    )?;
    write_schema(
        &generated_dir.join(SESSION_START_OUTPUT_FIXTURE),
        schema_json::<SessionStartCommandOutputWire>()?,
    )?;
    write_schema(
        &generated_dir.join(USER_PROMPT_SUBMIT_INPUT_FIXTURE),
        schema_json::<UserPromptSubmitCommandInput>()?,
    )?;
    write_schema(
        &generated_dir.join(USER_PROMPT_SUBMIT_OUTPUT_FIXTURE),
        schema_json::<UserPromptSubmitCommandOutputWire>()?,
    )?;
    write_schema(
        &generated_dir.join(STOP_INPUT_FIXTURE),
        schema_json::<StopCommandInput>()?,
    )?;
    write_schema(
        &generated_dir.join(STOP_OUTPUT_FIXTURE),
        schema_json::<StopCommandOutputWire>()?,
    )?;

    Ok(())
}

fn write_schema(path: &Path, json: Vec<u8>) -> anyhow::Result<()> {
    std::fs::write(path, json)?;
    Ok(())
}

fn ensure_empty_dir(dir: &Path) -> anyhow::Result<()> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::create_dir_all(dir)?;
    Ok(())
}

fn schema_json<T>() -> anyhow::Result<Vec<u8>>
where
    T: JsonSchema,
{
    let schema = schema_for_type::<T>();
    let value = serde_json::to_value(schema)?;
    let value = canonicalize_json(&value);
    Ok(serde_json::to_vec_pretty(&value)?)
}

fn schema_for_type<T>() -> RootSchema
where
    T: JsonSchema,
{
    SchemaSettings::draft07()
        .with(|settings| {
            settings.option_add_null_type = false;
        })
        .into_generator()
        .into_root_schema_for::<T>()
}

fn canonicalize_json(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonicalize_json).collect()),
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            let mut sorted = Map::with_capacity(map.len());
            for (key, child) in entries {
                sorted.insert(key.clone(), canonicalize_json(child));
            }
            Value::Object(sorted)
        }
        _ => value.clone(),
    }
}

fn session_start_hook_event_name_schema(_gen: &mut SchemaGenerator) -> Schema {
    string_const_schema("SessionStart")
}

fn post_tool_use_hook_event_name_schema(_gen: &mut SchemaGenerator) -> Schema {
    string_const_schema("PostToolUse")
}

fn post_tool_use_tool_name_schema(_gen: &mut SchemaGenerator) -> Schema {
    string_const_schema("Bash")
}

fn post_tool_use_failure_hook_event_name_schema(_gen: &mut SchemaGenerator) -> Schema {
    string_const_schema("PostToolUseFailure")
}

fn post_tool_use_failure_tool_name_schema(_gen: &mut SchemaGenerator) -> Schema {
    string_const_schema("Bash")
}

fn pre_tool_use_hook_event_name_schema(_gen: &mut SchemaGenerator) -> Schema {
    string_const_schema("PreToolUse")
}

fn pre_tool_use_tool_name_schema(_gen: &mut SchemaGenerator) -> Schema {
    string_const_schema("Bash")
}

fn user_prompt_submit_hook_event_name_schema(_gen: &mut SchemaGenerator) -> Schema {
    string_const_schema("UserPromptSubmit")
}

fn stop_hook_event_name_schema(_gen: &mut SchemaGenerator) -> Schema {
    string_const_schema("Stop")
}

fn permission_request_hook_event_name_schema(_gen: &mut SchemaGenerator) -> Schema {
    string_const_schema("PermissionRequest")
}

fn notification_hook_event_name_schema(_gen: &mut SchemaGenerator) -> Schema {
    string_const_schema("Notification")
}

fn permission_mode_schema(_gen: &mut SchemaGenerator) -> Schema {
    string_enum_schema(&[
        "default",
        "acceptEdits",
        "plan",
        "dontAsk",
        "bypassPermissions",
    ])
}

fn session_start_source_schema(_gen: &mut SchemaGenerator) -> Schema {
    string_enum_schema(&["startup", "resume", "clear"])
}

fn string_const_schema(value: &str) -> Schema {
    let mut schema = SchemaObject {
        instance_type: Some(InstanceType::String.into()),
        ..Default::default()
    };
    schema.const_value = Some(Value::String(value.to_string()));
    Schema::Object(schema)
}

fn string_enum_schema(values: &[&str]) -> Schema {
    let mut schema = SchemaObject {
        instance_type: Some(InstanceType::String.into()),
        ..Default::default()
    };
    schema.enum_values = Some(
        values
            .iter()
            .map(|value| Value::String((*value).to_string()))
            .collect(),
    );
    Schema::Object(schema)
}

fn default_continue() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::schema_json;
    use super::write_schema_fixtures;
    use super::NotificationCommandInput;
    use super::NOTIFICATION_INPUT_FIXTURE;
    use super::NOTIFICATION_OUTPUT_FIXTURE;
    use super::PermissionRequestCommandInput;
    use super::PostToolUseCommandInput;
    use super::PostToolUseFailureCommandInput;
    use super::PreToolUseCommandInput;
    use super::StopCommandInput;
    use super::UserPromptSubmitCommandInput;
    use super::PERMISSION_REQUEST_INPUT_FIXTURE;
    use super::PERMISSION_REQUEST_OUTPUT_FIXTURE;
    use super::POST_TOOL_USE_INPUT_FIXTURE;
    use super::POST_TOOL_USE_OUTPUT_FIXTURE;
    use super::POST_TOOL_USE_FAILURE_INPUT_FIXTURE;
    use super::POST_TOOL_USE_FAILURE_OUTPUT_FIXTURE;
    use super::PRE_TOOL_USE_INPUT_FIXTURE;
    use super::PRE_TOOL_USE_OUTPUT_FIXTURE;
    use super::SESSION_START_INPUT_FIXTURE;
    use super::SESSION_START_OUTPUT_FIXTURE;
    use super::STOP_INPUT_FIXTURE;
    use super::STOP_OUTPUT_FIXTURE;
    use super::USER_PROMPT_SUBMIT_INPUT_FIXTURE;
    use super::USER_PROMPT_SUBMIT_OUTPUT_FIXTURE;
    use pretty_assertions::assert_eq;
    use serde_json::Value;
    use tempfile::TempDir;

    fn expected_fixture(name: &str) -> &'static str {
        match name {
            POST_TOOL_USE_INPUT_FIXTURE => {
                include_str!("../schema/generated/post-tool-use.command.input.schema.json")
            }
            POST_TOOL_USE_OUTPUT_FIXTURE => {
                include_str!("../schema/generated/post-tool-use.command.output.schema.json")
            }
            POST_TOOL_USE_FAILURE_INPUT_FIXTURE => {
                include_str!("../schema/generated/post-tool-use-failure.command.input.schema.json")
            }
            POST_TOOL_USE_FAILURE_OUTPUT_FIXTURE => {
                include_str!("../schema/generated/post-tool-use-failure.command.output.schema.json")
            }
            NOTIFICATION_INPUT_FIXTURE => {
                include_str!("../schema/generated/notification.command.input.schema.json")
            }
            NOTIFICATION_OUTPUT_FIXTURE => {
                include_str!("../schema/generated/notification.command.output.schema.json")
            }
            PERMISSION_REQUEST_INPUT_FIXTURE => {
                include_str!("../schema/generated/permission-request.command.input.schema.json")
            }
            PERMISSION_REQUEST_OUTPUT_FIXTURE => {
                include_str!("../schema/generated/permission-request.command.output.schema.json")
            }
            PRE_TOOL_USE_INPUT_FIXTURE => {
                include_str!("../schema/generated/pre-tool-use.command.input.schema.json")
            }
            PRE_TOOL_USE_OUTPUT_FIXTURE => {
                include_str!("../schema/generated/pre-tool-use.command.output.schema.json")
            }
            SESSION_START_INPUT_FIXTURE => {
                include_str!("../schema/generated/session-start.command.input.schema.json")
            }
            SESSION_START_OUTPUT_FIXTURE => {
                include_str!("../schema/generated/session-start.command.output.schema.json")
            }
            USER_PROMPT_SUBMIT_INPUT_FIXTURE => {
                include_str!("../schema/generated/user-prompt-submit.command.input.schema.json")
            }
            USER_PROMPT_SUBMIT_OUTPUT_FIXTURE => {
                include_str!("../schema/generated/user-prompt-submit.command.output.schema.json")
            }
            STOP_INPUT_FIXTURE => {
                include_str!("../schema/generated/stop.command.input.schema.json")
            }
            STOP_OUTPUT_FIXTURE => {
                include_str!("../schema/generated/stop.command.output.schema.json")
            }
            _ => panic!("unexpected fixture name: {name}"),
        }
    }

    fn normalize_newlines(value: &str) -> String {
        value.replace("\r\n", "\n")
    }

    #[test]
    fn generated_hook_schemas_match_fixtures() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let schema_root = temp_dir.path().join("schema");
        write_schema_fixtures(&schema_root).expect("write generated hook schemas");

        for fixture in [
            POST_TOOL_USE_INPUT_FIXTURE,
            POST_TOOL_USE_OUTPUT_FIXTURE,
            POST_TOOL_USE_FAILURE_INPUT_FIXTURE,
            POST_TOOL_USE_FAILURE_OUTPUT_FIXTURE,
            NOTIFICATION_INPUT_FIXTURE,
            NOTIFICATION_OUTPUT_FIXTURE,
            PERMISSION_REQUEST_INPUT_FIXTURE,
            PERMISSION_REQUEST_OUTPUT_FIXTURE,
            PRE_TOOL_USE_INPUT_FIXTURE,
            PRE_TOOL_USE_OUTPUT_FIXTURE,
            SESSION_START_INPUT_FIXTURE,
            SESSION_START_OUTPUT_FIXTURE,
            USER_PROMPT_SUBMIT_INPUT_FIXTURE,
            USER_PROMPT_SUBMIT_OUTPUT_FIXTURE,
            STOP_INPUT_FIXTURE,
            STOP_OUTPUT_FIXTURE,
        ] {
            let expected = normalize_newlines(expected_fixture(fixture));
            let actual = std::fs::read_to_string(schema_root.join("generated").join(fixture))
                .unwrap_or_else(|err| panic!("read generated schema {fixture}: {err}"));
            let actual = normalize_newlines(&actual);
            assert_eq!(expected, actual, "fixture should match generated schema");
        }
    }

    #[test]
    fn turn_scoped_hook_inputs_include_codex_turn_id_extension() {
        // Codex intentionally diverges from Claude's public hook docs here so
        // internal hook consumers can key off the active turn.
        let pre_tool_use: Value = serde_json::from_slice(
            &schema_json::<PreToolUseCommandInput>().expect("serialize pre tool use input schema"),
        )
        .expect("parse pre tool use input schema");
        let post_tool_use: Value = serde_json::from_slice(
            &schema_json::<PostToolUseCommandInput>()
                .expect("serialize post tool use input schema"),
        )
        .expect("parse post tool use input schema");
        let post_tool_use_failure: Value = serde_json::from_slice(
            &schema_json::<PostToolUseFailureCommandInput>()
                .expect("serialize post tool use failure input schema"),
        )
        .expect("parse post tool use failure input schema");
        let user_prompt_submit: Value = serde_json::from_slice(
            &schema_json::<UserPromptSubmitCommandInput>()
                .expect("serialize user prompt submit input schema"),
        )
        .expect("parse user prompt submit input schema");
        let stop: Value = serde_json::from_slice(
            &schema_json::<StopCommandInput>().expect("serialize stop input schema"),
        )
        .expect("parse stop input schema");
        let permission_request: Value = serde_json::from_slice(
            &schema_json::<PermissionRequestCommandInput>()
                .expect("serialize permission request input schema"),
        )
        .expect("parse permission request input schema");
        let notification: Value = serde_json::from_slice(
            &schema_json::<NotificationCommandInput>()
                .expect("serialize notification input schema"),
        )
        .expect("parse notification input schema");

        for schema in [
            &pre_tool_use,
            &post_tool_use,
            &post_tool_use_failure,
            &notification,
            &user_prompt_submit,
            &stop,
            &permission_request,
        ] {
            assert_eq!(schema["properties"]["turn_id"]["type"], "string");
            assert!(schema["required"]
                .as_array()
                .expect("schema required fields")
                .contains(&Value::String("turn_id".to_string())));
        }
    }

    #[test]
    fn permission_request_schema_matches_claude_style_payload_shape() {
        let permission_request: Value = serde_json::from_slice(
            &schema_json::<PermissionRequestCommandInput>()
                .expect("serialize permission request input schema"),
        )
        .expect("parse permission request input schema");

        assert_eq!(
            permission_request["properties"]["hook_event_name"]["const"],
            "PermissionRequest"
        );
        assert_eq!(
            permission_request["properties"]["permission_suggestions"]["type"],
            "array"
        );
        assert_eq!(
            permission_request["properties"]["tool_name"]["type"],
            "string"
        );
        assert_eq!(
            permission_request["properties"]["tool_input"]["type"],
            "object"
        );
    }

    #[test]
    fn session_start_schema_advertises_codex_hook_capabilities() {
        let session_start: Value = serde_json::from_slice(
            &schema_json::<super::SessionStartCommandInput>()
                .expect("serialize session start input schema"),
        )
        .expect("parse session start input schema");

        assert_eq!(
            session_start["properties"]["codex_hook_capabilities"]["$ref"],
            "#/definitions/CodexHookCapabilitiesWire"
        );
        assert_eq!(
            session_start["definitions"]["CodexHookCapabilitiesWire"]["type"],
            "object"
        );
        assert_eq!(
            session_start["definitions"]["CodexHookCapabilitiesWire"]["properties"]["surface_version"]["type"],
            "integer"
        );
        assert_eq!(
            session_start["definitions"]["CodexHookHandlerFeaturesWire"]["properties"]["command_if"]["type"],
            "boolean"
        );
    }
}
