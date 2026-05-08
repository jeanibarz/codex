//! InstructionsLoaded hook execution (Claude-compat).
//!
//! Fires once per session after Codex has resolved AGENTS.md / CLAUDE.md
//! (and any configured fallback) and assembled the final user-instructions
//! string. Fire-and-forget: handlers cannot block session bootstrap.
//!
//! Equivalent to Claude Code's `InstructionsLoaded` event, exposing the
//! discovered source paths so observability tooling can verify which
//! instruction layers reached the model.

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
use crate::schema::InstructionsLoadedCommandInput;
use crate::schema::NullableString;

#[derive(Debug, Clone)]
pub struct InstructionsLoadedRequest {
    pub session_id: ThreadId,
    pub cwd: PathBuf,
    pub transcript_path: Option<PathBuf>,
    pub model: String,
    pub permission_mode: String,
    pub instruction_paths: Vec<String>,
    pub instructions_byte_len: u64,
}

#[derive(Debug)]
pub struct InstructionsLoadedOutcome {
    pub hook_events: Vec<HookCompletedEvent>,
}

pub(crate) fn preview(
    handlers: &[ConfiguredHandler],
    _request: &InstructionsLoadedRequest,
) -> Vec<HookRunSummary> {
    dispatcher::select_handlers(handlers, HookEventName::InstructionsLoaded, None)
        .into_iter()
        .map(|handler| dispatcher::running_summary(&handler))
        .collect()
}

pub(crate) async fn run(
    handlers: &[ConfiguredHandler],
    shell: &CommandShell,
    request: InstructionsLoadedRequest,
) -> InstructionsLoadedOutcome {
    let matched = dispatcher::select_handlers(handlers, HookEventName::InstructionsLoaded, None);
    if matched.is_empty() {
        return InstructionsLoadedOutcome {
            hook_events: Vec::new(),
        };
    }

    let input_json = match serde_json::to_string(&InstructionsLoadedCommandInput {
        session_id: request.session_id.to_string(),
        transcript_path: NullableString::from_path(request.transcript_path.clone()),
        cwd: request.cwd.display().to_string(),
        hook_event_name: "InstructionsLoaded".to_string(),
        model: request.model.clone(),
        permission_mode: request.permission_mode.clone(),
        instruction_paths: request.instruction_paths.clone(),
        instructions_byte_len: request.instructions_byte_len,
    }) {
        Ok(input_json) => input_json,
        Err(error) => {
            return InstructionsLoadedOutcome {
                hook_events: common::serialization_failure_hook_events(
                    matched,
                    None,
                    format!("failed to serialize instructions loaded hook input: {error}"),
                ),
            };
        }
    };

    let results = dispatcher::execute_handlers(
        shell,
        matched,
        input_json,
        request.cwd.as_path(),
        None,
        parse_completed,
    )
    .await;

    InstructionsLoadedOutcome {
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
