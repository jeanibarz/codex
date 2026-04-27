//! FileChanged hook execution (Claude-compat).
//!
//! Fires after Codex successfully changes files through an agent tool call.

use std::collections::HashMap;
use std::path::PathBuf;

use codex_protocol::protocol::FileChange;
use codex_protocol::protocol::HookCompletedEvent;
use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::HookOutputEntry;
use codex_protocol::protocol::HookOutputEntryKind;
use codex_protocol::protocol::HookRunStatus;
use codex_protocol::protocol::HookRunSummary;
use codex_protocol::ThreadId;
use codex_utils_absolute_path::AbsolutePathBuf;

use super::common;
use crate::engine::command_runner::CommandRunResult;
use crate::engine::dispatcher;
use crate::engine::CommandShell;
use crate::engine::ConfiguredHandler;
use crate::schema::FileChangedCommandInput;
use crate::schema::NullableString;

#[derive(Debug, Clone)]
pub struct FileChangedRequest {
    pub session_id: ThreadId,
    pub turn_id: String,
    pub cwd: AbsolutePathBuf,
    pub transcript_path: Option<PathBuf>,
    pub model: String,
    pub permission_mode: String,
    pub tool_name: String,
    pub tool_use_id: String,
    pub changes: HashMap<PathBuf, FileChange>,
}

#[derive(Debug)]
pub struct FileChangedOutcome {
    pub hook_events: Vec<HookCompletedEvent>,
}

pub(crate) fn preview(
    handlers: &[ConfiguredHandler],
    request: &FileChangedRequest,
) -> Vec<HookRunSummary> {
    let file_paths = file_paths(&request.changes);
    let matcher_inputs = file_paths.iter().map(String::as_str).collect::<Vec<_>>();
    dispatcher::select_handlers_for_matcher_inputs(
        handlers,
        HookEventName::FileChanged,
        &matcher_inputs,
    )
    .into_iter()
    .map(|handler| {
        common::hook_run_for_tool_use(dispatcher::running_summary(&handler), &request.tool_use_id)
    })
    .collect()
}

pub(crate) async fn run(
    handlers: &[ConfiguredHandler],
    shell: &CommandShell,
    request: FileChangedRequest,
) -> FileChangedOutcome {
    let file_paths = file_paths(&request.changes);
    let matcher_inputs = file_paths.iter().map(String::as_str).collect::<Vec<_>>();
    let matched = dispatcher::select_handlers_for_matcher_inputs(
        handlers,
        HookEventName::FileChanged,
        &matcher_inputs,
    );
    if matched.is_empty() {
        return FileChangedOutcome {
            hook_events: Vec::new(),
        };
    }

    let input_json = match serde_json::to_string(&FileChangedCommandInput {
        session_id: request.session_id.to_string(),
        turn_id: request.turn_id.clone(),
        transcript_path: NullableString::from_path(request.transcript_path.clone()),
        cwd: request.cwd.display().to_string(),
        hook_event_name: "FileChanged".to_string(),
        model: request.model.clone(),
        permission_mode: request.permission_mode.clone(),
        tool_name: request.tool_name,
        tool_use_id: request.tool_use_id.clone(),
        file_paths,
        changes: request.changes,
    }) {
        Ok(input_json) => input_json,
        Err(error) => {
            return FileChangedOutcome {
                hook_events: common::serialization_failure_hook_events_for_tool_use(
                    matched,
                    Some(request.turn_id),
                    format!("failed to serialize file changed hook input: {error}"),
                    &request.tool_use_id,
                ),
            };
        }
    };

    let results = dispatcher::execute_handlers(
        shell,
        matched,
        input_json,
        request.cwd.as_path(),
        Some(request.turn_id.clone()),
        parse_completed,
    )
    .await;

    FileChangedOutcome {
        hook_events: results
            .into_iter()
            .map(|result| {
                common::hook_completed_for_tool_use(result.completed, &request.tool_use_id)
            })
            .collect(),
    }
}

fn file_paths(changes: &HashMap<PathBuf, FileChange>) -> Vec<String> {
    let mut file_paths = changes
        .keys()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    file_paths.sort();
    file_paths
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
