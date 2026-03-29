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
use crate::schema::PermissionRequestCommandInput;
use crate::schema::PermissionRequestToolInput;

#[derive(Debug, Clone)]
pub struct PermissionRequestRequest {
    pub session_id: ThreadId,
    pub turn_id: String,
    pub cwd: PathBuf,
    pub transcript_path: Option<PathBuf>,
    pub model: String,
    pub permission_mode: String,
    pub tool_name: String,
    pub tool_input: String,
}

#[derive(Debug)]
pub struct PermissionRequestOutcome {
    pub hook_events: Vec<HookCompletedEvent>,
}

#[derive(Debug, Default)]
struct PermissionRequestHandlerData;

pub(crate) fn preview(
    handlers: &[ConfiguredHandler],
    request: &PermissionRequestRequest,
) -> Vec<HookRunSummary> {
    dispatcher::select_handlers(
        handlers,
        HookEventName::PermissionRequest,
        Some(&request.tool_name),
    )
    .into_iter()
    .map(|handler| dispatcher::running_summary(&handler))
    .collect()
}

pub(crate) async fn run(
    handlers: &[ConfiguredHandler],
    shell: &CommandShell,
    request: PermissionRequestRequest,
) -> PermissionRequestOutcome {
    let matched = dispatcher::select_handlers(
        handlers,
        HookEventName::PermissionRequest,
        Some(&request.tool_name),
    );
    if matched.is_empty() {
        return PermissionRequestOutcome {
            hook_events: Vec::new(),
        };
    }

    let input_json = match serde_json::to_string(&PermissionRequestCommandInput {
        session_id: request.session_id.to_string(),
        turn_id: request.turn_id.clone(),
        transcript_path: crate::schema::NullableString::from_path(request.transcript_path.clone()),
        cwd: request.cwd.display().to_string(),
        hook_event_name: "PermissionRequest".to_string(),
        model: request.model.clone(),
        permission_mode: request.permission_mode.clone(),
        tool_name: request.tool_name.clone(),
        tool_input: PermissionRequestToolInput {
            command: request.tool_input.clone(),
        },
    }) {
        Ok(input_json) => input_json,
        Err(error) => {
            return PermissionRequestOutcome {
                hook_events: common::serialization_failure_hook_events(
                    matched,
                    Some(request.turn_id),
                    format!("failed to serialize permission request hook input: {error}"),
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

    PermissionRequestOutcome {
        hook_events: results.into_iter().map(|result| result.completed).collect(),
    }
}

fn parse_completed(
    handler: &ConfiguredHandler,
    run_result: CommandRunResult,
    turn_id: Option<String>,
) -> dispatcher::ParsedHandler<PermissionRequestHandlerData> {
    let mut entries = Vec::new();
    let mut status = HookRunStatus::Completed;

    match run_result.error.as_deref() {
        Some(error) => {
            status = HookRunStatus::Failed;
            entries.push(HookOutputEntry {
                kind: HookOutputEntryKind::Error,
                text: error.to_string(),
            });
        }
        None => match run_result.exit_code {
            Some(0) => {
                // PermissionRequest is a notification-only hook.
                // We do not parse stdout — any output is silently ignored.
            }
            Some(exit_code) => {
                status = HookRunStatus::Failed;
                entries.push(HookOutputEntry {
                    kind: HookOutputEntryKind::Error,
                    text: format!("hook exited with code {exit_code}"),
                });
            }
            None => {
                status = HookRunStatus::Failed;
                entries.push(HookOutputEntry {
                    kind: HookOutputEntryKind::Error,
                    text: "hook exited without a status code".to_string(),
                });
            }
        },
    }

    dispatcher::ParsedHandler {
        completed: HookCompletedEvent {
            turn_id,
            run: dispatcher::completed_summary(handler, &run_result, status, entries),
        },
        data: PermissionRequestHandlerData,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use codex_protocol::protocol::HookEventName;
    use codex_protocol::protocol::HookRunStatus;
    use pretty_assertions::assert_eq;

    use super::parse_completed;
    use crate::engine::ConfiguredHandler;
    use crate::engine::command_runner::CommandRunResult;

    #[test]
    fn successful_hook_completes() {
        let parsed = parse_completed(
            &handler(),
            run_result(Some(0), "", ""),
            Some("turn-1".to_string()),
        );

        assert_eq!(parsed.completed.run.status, HookRunStatus::Completed);
        assert_eq!(parsed.completed.run.entries, vec![]);
    }

    #[test]
    fn nonzero_exit_code_fails() {
        let parsed = parse_completed(
            &handler(),
            run_result(Some(1), "", "some error"),
            Some("turn-1".to_string()),
        );

        assert_eq!(parsed.completed.run.status, HookRunStatus::Failed);
        assert_eq!(parsed.completed.run.entries.len(), 1);
    }

    fn handler() -> ConfiguredHandler {
        ConfiguredHandler {
            event_name: HookEventName::PermissionRequest,
            matcher: None,
            command: "echo hook".to_string(),
            timeout_sec: 5,
            status_message: None,
            source_path: PathBuf::from("/tmp/hooks.json"),
            display_order: 0,
        }
    }

    fn run_result(exit_code: Option<i32>, stdout: &str, stderr: &str) -> CommandRunResult {
        CommandRunResult {
            started_at: 1,
            completed_at: 2,
            duration_ms: 1,
            exit_code,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            error: None,
        }
    }
}
