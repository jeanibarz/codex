mod config_rules;
mod engine;
pub(crate) mod events;
mod legacy_notify;
mod registry;
mod schema;
mod types;

pub use engine::HookListEntry;
/// Hook event names as they appear in hooks JSON and config files.
pub const HOOK_EVENT_NAMES: [&str; 11] = [
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "PostToolUseFailure",
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "Stop",
    "StopFailure",
    "Notification",
    "FileChanged",
];

/// Hook event names whose matcher fields are meaningful during dispatch.
///
/// Other events can appear in hooks JSON, but Codex ignores their matcher
/// fields because those events do not dispatch against a tool or session-start
/// source.
pub const HOOK_EVENT_NAMES_WITH_MATCHERS: [&str; 6] = [
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "PostToolUseFailure",
    "SessionStart",
    "FileChanged",
];

pub use events::notification::NotificationOutcome;
pub use events::notification::NotificationRequest;
pub use events::permission_request::PermissionRequestDecision;
pub use events::permission_request::PermissionRequestOutcome;
pub use events::permission_request::PermissionRequestRequest;
pub use events::post_tool_use::PostToolUseOutcome;
pub use events::post_tool_use::PostToolUseRequest;
pub use events::post_tool_use_failure::PostToolUseFailureOutcome;
pub use events::post_tool_use_failure::PostToolUseFailureRequest;
pub use events::pre_tool_use::PreToolUseOutcome;
pub use events::pre_tool_use::PreToolUseRequest;
pub use events::session_end::SessionEndOutcome;
pub use events::session_end::SessionEndReason;
pub use events::session_end::SessionEndRequest;
pub use events::session_start::SessionStartOutcome;
pub use events::session_start::SessionStartRequest;
pub use events::session_start::SessionStartSource;
pub use events::stop::StopOutcome;
pub use events::stop::StopRequest;
pub use events::stop_failure::StopFailureOutcome;
pub use events::stop_failure::StopFailureRequest;
pub use events::user_prompt_submit::UserPromptSubmitOutcome;
pub use events::user_prompt_submit::UserPromptSubmitRequest;
pub use legacy_notify::legacy_notify_json;
pub use legacy_notify::notify_hook;
pub use registry::HookListOutcome;
pub use registry::Hooks;
pub use registry::HooksConfig;
pub use registry::command_from_argv;
pub use registry::list_hooks;
pub use schema::write_schema_fixtures;
pub use types::Hook;
pub use types::HookEvent;
pub use types::HookEventAfterAgent;
pub use types::HookEventAfterToolUse;
pub use types::HookPayload;
pub use types::HookResponse;
pub use types::HookResult;
pub use types::HookToolInput;
pub use types::HookToolInputLocalShell;
pub use types::HookToolKind;
