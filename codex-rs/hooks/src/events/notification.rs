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
use crate::engine::output_parser;
use crate::schema::NotificationCommandInput;
use crate::schema::NullableString;

#[derive(Debug, Clone)]
pub struct NotificationRequest {
    pub session_id: ThreadId,
    pub turn_id: String,
    pub cwd: PathBuf,
    pub transcript_path: Option<PathBuf>,
    pub model: String,
    pub notification_type: String,
    pub message: String,
}

#[derive(Debug)]
pub struct NotificationOutcome {
    pub hook_events: Vec<HookCompletedEvent>,
}

pub(crate) fn preview(
    handlers: &[ConfiguredHandler],
    request: &NotificationRequest,
) -> Vec<HookRunSummary> {
    matching_handlers(handlers, request)
        .into_iter()
        .map(|handler| dispatcher::running_summary(&handler))
        .collect()
}

pub(crate) async fn run(
    handlers: &[ConfiguredHandler],
    shell: &CommandShell,
    request: NotificationRequest,
) -> NotificationOutcome {
    let matched = matching_handlers(handlers, &request);
    if matched.is_empty() {
        return NotificationOutcome {
            hook_events: Vec::new(),
        };
    }

    let input_json = match serde_json::to_string(&NotificationCommandInput {
        session_id: request.session_id.to_string(),
        turn_id: request.turn_id.clone(),
        transcript_path: NullableString::from_path(request.transcript_path.clone()),
        cwd: request.cwd.display().to_string(),
        hook_event_name: "Notification".to_string(),
        model: request.model.clone(),
        notification_type: request.notification_type.clone(),
        message: request.message.clone(),
    }) {
        Ok(input_json) => input_json,
        Err(error) => {
            return NotificationOutcome {
                hook_events: common::serialization_failure_hook_events(
                    matched,
                    Some(request.turn_id),
                    format!("failed to serialize notification hook input: {error}"),
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

    NotificationOutcome {
        hook_events: results.into_iter().map(|result| result.completed).collect(),
    }
}

fn matching_handlers(
    handlers: &[ConfiguredHandler],
    request: &NotificationRequest,
) -> Vec<ConfiguredHandler> {
    dispatcher::select_handlers(
        handlers,
        HookEventName::Notification,
        Some(&request.notification_type),
    )
    .into_iter()
    .filter(|handler| {
        common::matches_command_handler_condition(
            handler.condition.as_deref(),
            Some("Notification"),
            Some(&request.notification_type),
        )
    })
    .collect()
}

fn parse_completed(
    handler: &ConfiguredHandler,
    run_result: CommandRunResult,
    turn_id: Option<String>,
) -> dispatcher::ParsedHandler<()> {
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
                let trimmed_stdout = run_result.stdout.trim();
                if trimmed_stdout.is_empty() {
                } else if let Some(parsed) = output_parser::parse_notification(&run_result.stdout)
                {
                    if let Some(system_message) = parsed.universal.system_message {
                        entries.push(HookOutputEntry {
                            kind: HookOutputEntryKind::Warning,
                            text: system_message,
                        });
                    }
                    if let Some(additional_context) = parsed.additional_context {
                        entries.push(HookOutputEntry {
                            kind: HookOutputEntryKind::Context,
                            text: additional_context,
                        });
                    }
                    let _ = parsed.universal.suppress_output;
                    if !parsed.universal.continue_processing {
                        status = HookRunStatus::Stopped;
                        if let Some(stop_reason_text) = parsed.universal.stop_reason {
                            entries.push(HookOutputEntry {
                                kind: HookOutputEntryKind::Stop,
                                text: stop_reason_text,
                            });
                        }
                    }
                } else if trimmed_stdout.starts_with('{') || trimmed_stdout.starts_with('[') {
                    status = HookRunStatus::Failed;
                    entries.push(HookOutputEntry {
                        kind: HookOutputEntryKind::Error,
                        text: "hook returned invalid notification JSON output".to_string(),
                    });
                } else {
                    entries.push(HookOutputEntry {
                        kind: HookOutputEntryKind::Context,
                        text: trimmed_stdout.to_string(),
                    });
                }
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

    let completed = HookCompletedEvent {
        turn_id,
        run: dispatcher::completed_summary(handler, &run_result, status, entries),
    };

    dispatcher::ParsedHandler {
        completed,
        data: (),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use codex_protocol::ThreadId;
    use codex_protocol::protocol::HookEventName;
    use codex_protocol::protocol::HookOutputEntry;
    use codex_protocol::protocol::HookOutputEntryKind;
    use codex_protocol::protocol::HookRunStatus;
    use pretty_assertions::assert_eq;

    use super::NotificationRequest;
    use super::matching_handlers;
    use super::parse_completed;
    use crate::engine::ConfiguredHandler;
    use crate::engine::command_runner::CommandRunResult;

    #[test]
    fn handler_level_if_filters_notification_handlers() {
        let handlers = vec![
            ConfiguredHandler {
                event_name: HookEventName::Notification,
                matcher: Some("permission_prompt".to_string()),
                condition: Some("Notification(permission_prompt)".to_string()),
                command: "echo notify".to_string(),
                timeout_sec: 5,
                status_message: None,
                source_path: PathBuf::from("/tmp/hooks.json"),
                display_order: 0,
            },
            ConfiguredHandler {
                event_name: HookEventName::Notification,
                matcher: Some("permission_prompt".to_string()),
                condition: Some("Notification(idle_prompt)".to_string()),
                command: "echo skip".to_string(),
                timeout_sec: 5,
                status_message: None,
                source_path: PathBuf::from("/tmp/hooks.json"),
                display_order: 1,
            },
        ];

        let request = NotificationRequest {
            session_id: ThreadId::new(),
            turn_id: "turn-1".to_string(),
            cwd: PathBuf::from("/tmp"),
            transcript_path: None,
            model: "gpt-5.4".to_string(),
            notification_type: "permission_prompt".to_string(),
            message: "need approval".to_string(),
        };

        let selected = matching_handlers(&handlers, &request);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].display_order, 0);
    }

    #[test]
    fn plain_stdout_becomes_context_entry() {
        let parsed = parse_completed(
            &handler(),
            run_result(Some(0), "needs attention\n", ""),
            None,
        );

        assert_eq!(parsed.completed.run.status, HookRunStatus::Completed);
        assert_eq!(
            parsed.completed.run.entries,
            vec![HookOutputEntry {
                kind: HookOutputEntryKind::Context,
                text: "needs attention".to_string(),
            }]
        );
    }

    fn handler() -> ConfiguredHandler {
        ConfiguredHandler {
            event_name: HookEventName::Notification,
            matcher: Some("permission_prompt".to_string()),
            condition: None,
            command: "echo notification".to_string(),
            timeout_sec: 5,
            status_message: None,
            source_path: PathBuf::from("/tmp/hooks.json"),
            display_order: 0,
        }
    }

    fn run_result(exit_code: Option<i32>, stdout: &str, stderr: &str) -> CommandRunResult {
        CommandRunResult {
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            exit_code,
            error: None,
            started_at: 100,
            completed_at: 101,
            duration_ms: 1,
        }
    }
}
