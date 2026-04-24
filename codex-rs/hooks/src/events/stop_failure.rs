//! StopFailure hook execution (Claude-compat).
//!
//! Fires when a turn ends with an API error rather than a clean stop.
//! Fire-and-forget — used by supervisors to detect stuck/failed agents.

use std::path::PathBuf;

use codex_protocol::ThreadId;
use codex_protocol::protocol::HookCompletedEvent;
use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::HookOutputEntry;
use codex_protocol::protocol::HookOutputEntryKind;
use codex_protocol::protocol::HookRunStatus;
use codex_protocol::protocol::HookRunSummary;

use super::common;
use crate::engine::CommandShell;
use crate::engine::ConfiguredHandler;
use crate::engine::command_runner::CommandRunResult;
use crate::engine::dispatcher;
use crate::schema::NullableString;
use crate::schema::StopFailureCommandInput;

#[derive(Debug, Clone)]
pub struct StopFailureRequest {
    pub session_id: ThreadId,
    pub turn_id: String,
    pub cwd: PathBuf,
    pub transcript_path: Option<PathBuf>,
    pub model: String,
    pub permission_mode: String,
    pub error: String,
    pub last_assistant_message: Option<String>,
}

#[derive(Debug)]
pub struct StopFailureOutcome {
    pub hook_events: Vec<HookCompletedEvent>,
}

#[allow(dead_code)]
pub(crate) fn preview(
    handlers: &[ConfiguredHandler],
    _request: &StopFailureRequest,
) -> Vec<HookRunSummary> {
    dispatcher::select_handlers(handlers, HookEventName::StopFailure, None)
        .into_iter()
        .map(|handler| dispatcher::running_summary(&handler))
        .collect()
}

pub(crate) async fn run(
    handlers: &[ConfiguredHandler],
    shell: &CommandShell,
    request: StopFailureRequest,
) -> StopFailureOutcome {
    let matched = dispatcher::select_handlers(handlers, HookEventName::StopFailure, None);
    if matched.is_empty() {
        return StopFailureOutcome {
            hook_events: Vec::new(),
        };
    }

    let input_json = match serde_json::to_string(&StopFailureCommandInput {
        session_id: request.session_id.to_string(),
        turn_id: request.turn_id.clone(),
        transcript_path: NullableString::from_path(request.transcript_path.clone()),
        cwd: request.cwd.display().to_string(),
        hook_event_name: "StopFailure".to_string(),
        model: request.model.clone(),
        permission_mode: request.permission_mode.clone(),
        error: request.error,
        last_assistant_message: NullableString::from_string(request.last_assistant_message.clone()),
    }) {
        Ok(input_json) => input_json,
        Err(error) => {
            return StopFailureOutcome {
                hook_events: common::serialization_failure_hook_events(
                    matched,
                    Some(request.turn_id),
                    format!("failed to serialize stop failure hook input: {error}"),
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

    StopFailureOutcome {
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
