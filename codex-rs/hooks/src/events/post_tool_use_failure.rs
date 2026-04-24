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
use crate::engine::output_parser;
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
    pub command: String,
    pub error: String,
    pub is_interrupt: bool,
}

#[derive(Debug)]
pub struct PostToolUseFailureOutcome {
    pub hook_events: Vec<HookCompletedEvent>,
    pub should_stop: bool,
    pub stop_reason: Option<String>,
    pub additional_contexts: Vec<String>,
    pub feedback_message: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct PostToolUseFailureHandlerData {
    should_stop: bool,
    stop_reason: Option<String>,
    additional_contexts_for_model: Vec<String>,
    feedback_messages_for_model: Vec<String>,
}

pub(crate) fn preview(
    handlers: &[ConfiguredHandler],
    request: &PostToolUseFailureRequest,
) -> Vec<HookRunSummary> {
    matching_handlers(handlers, request)
        .into_iter()
        .map(|handler| dispatcher::running_summary(&handler))
        .collect()
}

pub(crate) async fn run(
    handlers: &[ConfiguredHandler],
    shell: &CommandShell,
    request: PostToolUseFailureRequest,
) -> PostToolUseFailureOutcome {
    let matched = matching_handlers(handlers, &request);
    if matched.is_empty() {
        return PostToolUseFailureOutcome {
            hook_events: Vec::new(),
            should_stop: false,
            stop_reason: None,
            additional_contexts: Vec::new(),
            feedback_message: None,
        };
    }

    let input_json = match serde_json::to_string(&PostToolUseFailureCommandInput {
        session_id: request.session_id.to_string(),
        turn_id: request.turn_id.clone(),
        transcript_path: crate::schema::NullableString::from_path(request.transcript_path.clone()),
        cwd: request.cwd.display().to_string(),
        hook_event_name: "PostToolUseFailure".to_string(),
        model: request.model.clone(),
        permission_mode: request.permission_mode.clone(),
        tool_name: request.tool_name.clone(),
        tool_input: request.tool_input.clone(),
        tool_use_id: request.tool_use_id.clone(),
        error: request.error.clone(),
        is_interrupt: request.is_interrupt,
    }) {
        Ok(input_json) => input_json,
        Err(error) => {
            return serialization_failure_outcome(common::serialization_failure_hook_events(
                matched,
                Some(request.turn_id),
                format!("failed to serialize post tool use failure hook input: {error}"),
            ));
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

    let additional_contexts = common::flatten_additional_contexts(
        results
            .iter()
            .map(|result| result.data.additional_contexts_for_model.as_slice()),
    );
    let should_stop = results.iter().any(|result| result.data.should_stop);
    let stop_reason = results
        .iter()
        .find_map(|result| result.data.stop_reason.clone());
    let feedback_message = common::join_text_chunks(
        results
            .iter()
            .flat_map(|result| result.data.feedback_messages_for_model.clone())
            .collect(),
    );

    PostToolUseFailureOutcome {
        hook_events: results.into_iter().map(|result| result.completed).collect(),
        should_stop,
        stop_reason,
        additional_contexts,
        feedback_message,
    }
}

fn matching_handlers(
    handlers: &[ConfiguredHandler],
    request: &PostToolUseFailureRequest,
) -> Vec<ConfiguredHandler> {
    dispatcher::select_handlers(
        handlers,
        HookEventName::PostToolUseFailure,
        Some(&request.tool_name),
    )
    .into_iter()
    .filter(|handler| {
        common::matches_command_handler_condition(
            handler.condition.as_deref(),
            Some(&request.tool_name),
            Some(&request.command),
        )
    })
    .collect()
}

fn parse_completed(
    handler: &ConfiguredHandler,
    run_result: CommandRunResult,
    turn_id: Option<String>,
) -> dispatcher::ParsedHandler<PostToolUseFailureHandlerData> {
    let mut entries = Vec::new();
    let mut status = HookRunStatus::Completed;
    let mut should_stop = false;
    let mut stop_reason = None;
    let mut additional_contexts_for_model = Vec::new();
    let mut feedback_messages_for_model = Vec::new();

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
                } else if let Some(parsed) =
                    output_parser::parse_post_tool_use_failure(&run_result.stdout)
                {
                    if let Some(system_message) = parsed.universal.system_message {
                        entries.push(HookOutputEntry {
                            kind: HookOutputEntryKind::Warning,
                            text: system_message,
                        });
                    }
                    if parsed.invalid_reason.is_none()
                        && parsed.invalid_block_reason.is_none()
                        && let Some(additional_context) = parsed.additional_context
                    {
                        common::append_additional_context(
                            &mut entries,
                            &mut additional_contexts_for_model,
                            additional_context,
                        );
                    }
                    if !parsed.universal.continue_processing {
                        status = HookRunStatus::Stopped;
                        should_stop = true;
                        stop_reason = parsed.universal.stop_reason.clone();
                        let stop_text = parsed.universal.stop_reason.unwrap_or_else(|| {
                            "PostToolUseFailure hook stopped execution".to_string()
                        });
                        entries.push(HookOutputEntry {
                            kind: HookOutputEntryKind::Stop,
                            text: stop_text.clone(),
                        });
                        let model_feedback = parsed
                            .reason
                            .as_deref()
                            .and_then(common::trimmed_non_empty)
                            .unwrap_or(stop_text);
                        feedback_messages_for_model.push(model_feedback);
                    } else if let Some(invalid_reason) = parsed.invalid_reason {
                        status = HookRunStatus::Failed;
                        entries.push(HookOutputEntry {
                            kind: HookOutputEntryKind::Error,
                            text: invalid_reason,
                        });
                    } else if let Some(invalid_block_reason) = parsed.invalid_block_reason {
                        status = HookRunStatus::Failed;
                        entries.push(HookOutputEntry {
                            kind: HookOutputEntryKind::Error,
                            text: invalid_block_reason,
                        });
                    } else if parsed.should_block {
                        status = HookRunStatus::Stopped;
                        should_stop = true;
                        stop_reason = parsed.reason.clone();
                        let stop_text = parsed.reason.unwrap_or_else(|| {
                            "PostToolUseFailure hook blocked execution".to_string()
                        });
                        entries.push(HookOutputEntry {
                            kind: HookOutputEntryKind::Stop,
                            text: stop_text.clone(),
                        });
                        feedback_messages_for_model.push(stop_text);
                    }
                } else {
                    entries.push(HookOutputEntry {
                        kind: HookOutputEntryKind::Context,
                        text: trimmed_stdout.to_string(),
                    });
                }
            }
            Some(2) => {
                status = HookRunStatus::Stopped;
                should_stop = true;
                let feedback = common::trimmed_non_empty(&run_result.stderr).unwrap_or_else(|| {
                    "PostToolUseFailure hook exited with code 2 but did not write feedback to stderr"
                        .to_string()
                });
                stop_reason = Some(feedback.clone());
                feedback_messages_for_model.push(feedback.clone());
                entries.push(HookOutputEntry {
                    kind: HookOutputEntryKind::Stop,
                    text: feedback,
                });
            }
            Some(code) => {
                status = HookRunStatus::Failed;
                entries.push(HookOutputEntry {
                    kind: HookOutputEntryKind::Error,
                    text: format!("hook exited with code {code}"),
                });
            }
            None => {
                status = HookRunStatus::Failed;
                entries.push(HookOutputEntry {
                    kind: HookOutputEntryKind::Error,
                    text: "hook terminated without exit code".to_string(),
                });
            }
        },
    }

    dispatcher::ParsedHandler {
        completed: HookCompletedEvent {
            turn_id,
            run: dispatcher::completed_summary(handler, &run_result, status, entries),
        },
        data: PostToolUseFailureHandlerData {
            should_stop,
            stop_reason,
            additional_contexts_for_model,
            feedback_messages_for_model,
        },
    }
}

fn serialization_failure_outcome(
    hook_events: Vec<HookCompletedEvent>,
) -> PostToolUseFailureOutcome {
    PostToolUseFailureOutcome {
        hook_events,
        should_stop: false,
        stop_reason: None,
        additional_contexts: Vec::new(),
        feedback_message: None,
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
    use serde_json::json;

    use super::PostToolUseFailureRequest;
    use super::matching_handlers;
    use super::parse_completed;
    use crate::engine::ConfiguredHandler;
    use crate::engine::command_runner::CommandRunResult;

    #[test]
    fn handler_level_if_filters_post_tool_use_failure_handlers() {
        let handlers = vec![
            ConfiguredHandler {
                event_name: HookEventName::PostToolUseFailure,
                matcher: Some("Bash".to_string()),
                condition: Some("Bash(git push*)".to_string()),
                command: "echo notify".to_string(),
                timeout_sec: 5,
                status_message: None,
                source_path: PathBuf::from("/tmp/hooks.json"),
                display_order: 0,
            },
            ConfiguredHandler {
                event_name: HookEventName::PostToolUseFailure,
                matcher: Some("Bash".to_string()),
                condition: Some("Bash(gh pr create*)".to_string()),
                command: "echo skip".to_string(),
                timeout_sec: 5,
                status_message: None,
                source_path: PathBuf::from("/tmp/hooks.json"),
                display_order: 1,
            },
        ];

        let request = PostToolUseFailureRequest {
            session_id: ThreadId::new(),
            turn_id: "turn-1".to_string(),
            cwd: PathBuf::from("/tmp"),
            transcript_path: None,
            model: "gpt-5.4".to_string(),
            permission_mode: "default".to_string(),
            tool_name: "Bash".to_string(),
            tool_use_id: "tool-1".to_string(),
            tool_input: json!({ "command": "git push origin main" }),
            command: "git push origin main".to_string(),
            error: "push failed".to_string(),
            is_interrupt: false,
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
            event_name: HookEventName::PostToolUseFailure,
            matcher: Some("Bash".to_string()),
            condition: None,
            command: "echo failure".to_string(),
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
            started_at: 1,
            completed_at: 2,
            duration_ms: 1,
        }
    }
}
