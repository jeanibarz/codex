//! PostToolUseFailure hook execution (Claude-compat).
//!
//! Fires when a tool call fails (non-zero exit, error, or interrupt).
//! Fire-and-forget — supervisors use this to attribute tool-level failures.

use std::path::PathBuf;

use codex_protocol::ThreadId;
use codex_protocol::protocol::HookCompletedEvent;
use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::HookOutputEntry;
use codex_protocol::protocol::HookOutputEntryKind;
use codex_protocol::protocol::HookRunStatus;
use codex_protocol::protocol::HookRunSummary;
use serde_json::Value;

use super::common;
use crate::engine::CommandShell;
use crate::engine::ConfiguredHandler;
use crate::engine::command_runner::CommandRunResult;
use crate::engine::dispatcher;
use crate::schema::NullableString;
use crate::schema::PostToolUseFailureCommandInput;

#[derive(Debug, Clone)]
pub struct PostToolUseFailureRequest {
    pub session_id: ThreadId,
    pub turn_id: String,
    pub cwd: PathBuf,
    pub transcript_path: Option<PathBuf>,
    pub model: String,
    pub permission_mode: String,
    pub tool_name: String,
    pub tool_use_id: String,
    pub tool_input: Value,
    pub error: String,
    pub is_interrupt: bool,
}

#[derive(Debug)]
pub struct PostToolUseFailureOutcome {
    pub hook_events: Vec<HookCompletedEvent>,
}

#[allow(dead_code)]
pub(crate) fn preview(
    handlers: &[ConfiguredHandler],
    request: &PostToolUseFailureRequest,
) -> Vec<HookRunSummary> {
    dispatcher::select_handlers(
        handlers,
        HookEventName::PostToolUseFailure,
        Some(request.tool_name.as_str()),
    )
    .into_iter()
    .map(|handler| dispatcher::running_summary(&handler))
    .collect()
}

pub(crate) async fn run(
    handlers: &[ConfiguredHandler],
    shell: &CommandShell,
    request: PostToolUseFailureRequest,
) -> PostToolUseFailureOutcome {
    let matched = dispatcher::select_handlers(
        handlers,
        HookEventName::PostToolUseFailure,
        Some(request.tool_name.as_str()),
    );
    if matched.is_empty() {
        return PostToolUseFailureOutcome {
            hook_events: Vec::new(),
        };
    }

    let input_json = match serde_json::to_string(&PostToolUseFailureCommandInput {
        session_id: request.session_id.to_string(),
        turn_id: request.turn_id.clone(),
        transcript_path: NullableString::from_path(request.transcript_path.clone()),
        cwd: request.cwd.display().to_string(),
        hook_event_name: "PostToolUseFailure".to_string(),
        model: request.model.clone(),
        permission_mode: request.permission_mode.clone(),
        tool_name: request.tool_name,
        tool_input: request.tool_input,
        tool_use_id: request.tool_use_id,
        error: request.error,
        is_interrupt: request.is_interrupt,
    }) {
        Ok(input_json) => input_json,
        Err(error) => {
            return PostToolUseFailureOutcome {
                hook_events: common::serialization_failure_hook_events(
                    matched,
                    Some(request.turn_id),
                    format!("failed to serialize post tool use failure hook input: {error}"),
                ),
            };
        }
    };

    let results = dispatcher::execute_handlers(
        shell,
        matched,
        input_json,
        request.cwd.as_path(),
        Some(request.turn_id),
        parse_completed,
    )
    .await;

    PostToolUseFailureOutcome {
        hook_events: results.into_iter().map(|result| result.completed).collect(),
    }
}

fn parse_completed(
    handler: &ConfiguredHandler,
    run_result: CommandRunResult,
    turn_id: Option<String>,
) -> dispatcher::ParsedHandler<()> {
    let mut entries = Vec::new();
    let mut status = HookRunStatus::Completed;
    if let Some(error) = run_result.error.as_deref() {
        status = HookRunStatus::Failed;
        entries.push(HookOutputEntry {
            kind: HookOutputEntryKind::Error,
            text: error.to_string(),
        });
    } else if matches!(run_result.exit_code, Some(code) if code != 0) {
        status = HookRunStatus::Failed;
        if !run_result.stderr.trim().is_empty() {
            entries.push(HookOutputEntry {
                kind: HookOutputEntryKind::Error,
                text: run_result.stderr.clone(),
            });
        }
    }
    let completed = HookCompletedEvent {
        turn_id,
        run: dispatcher::completed_summary(handler, &run_result, status, entries),
    };
    dispatcher::ParsedHandler {
        completed,
        data: (),
    }
}
