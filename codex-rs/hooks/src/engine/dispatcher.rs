use std::path::Path;

use futures::StreamExt;
use futures::stream::FuturesUnordered;

use codex_protocol::protocol::HookCompletedEvent;
use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::HookExecutionMode;
use codex_protocol::protocol::HookHandlerType;
use codex_protocol::protocol::HookRunStatus;
use codex_protocol::protocol::HookRunSummary;
use codex_protocol::protocol::HookScope;

use super::ClaudeHooksEngine;
use super::ConfiguredHandler;
use super::ConfiguredHandlerKind;
use super::HandlerRunResult;
use super::command_runner::run_command;
use crate::events::common::matches_matcher;

const CLAUDE_CONDITIONAL_MATCHER_PREFIX: &str = "__codex_claude_conditional_matcher__:";

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub(crate) enum ClaudeHookCondition {
    ToolCommandGlob {
        tool_name: String,
        command_glob: String,
    },
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct ClaudeConditionalMatcher {
    matcher: Option<String>,
    conditions: Vec<ClaudeHookCondition>,
}

#[derive(Debug)]
pub(crate) struct ParsedHandler<T> {
    pub completed: HookCompletedEvent,
    pub data: T,
    pub completion_order: usize,
}

pub(crate) fn encode_claude_conditional_matcher(
    matcher: Option<&str>,
    conditions: &[ClaudeHookCondition],
) -> Option<String> {
    if conditions.is_empty() {
        return matcher.map(ToOwned::to_owned);
    }
    let Ok(encoded) = serde_json::to_string(&ClaudeConditionalMatcher {
        matcher: matcher.map(ToOwned::to_owned),
        conditions: conditions.to_vec(),
    }) else {
        return matcher.map(ToOwned::to_owned);
    };
    Some(format!("{CLAUDE_CONDITIONAL_MATCHER_PREFIX}{encoded}"))
}

pub(crate) fn select_handlers(
    handlers: &[ConfiguredHandler],
    event_name: HookEventName,
    matcher_input: Option<&str>,
) -> Vec<ConfiguredHandler> {
    let matcher_inputs = matcher_input.into_iter().collect::<Vec<_>>();
    select_handlers_for_matcher_inputs(handlers, event_name, &matcher_inputs)
}

pub(crate) fn select_handlers_for_matcher_inputs(
    handlers: &[ConfiguredHandler],
    event_name: HookEventName,
    matcher_inputs: &[&str],
) -> Vec<ConfiguredHandler> {
    select_handlers_for_matcher_inputs_and_tool_input(
        handlers,
        event_name,
        matcher_inputs,
        /*tool_input*/ None,
    )
}

pub(crate) fn select_handlers_for_tool_use(
    handlers: &[ConfiguredHandler],
    event_name: HookEventName,
    matcher_inputs: &[&str],
    tool_input: &serde_json::Value,
) -> Vec<ConfiguredHandler> {
    select_handlers_for_matcher_inputs_and_tool_input(
        handlers,
        event_name,
        matcher_inputs,
        Some(tool_input),
    )
}

fn select_handlers_for_matcher_inputs_and_tool_input(
    handlers: &[ConfiguredHandler],
    event_name: HookEventName,
    matcher_inputs: &[&str],
    tool_input: Option<&serde_json::Value>,
) -> Vec<ConfiguredHandler> {
    // Check each configured handler once, even when several compatibility names
    // match the same regex. A hook like `apply_patch|Write|Edit` should run a
    // single time for one tool call, not once per matching alias.
    handlers
        .iter()
        .filter(|handler| handler.event_name == event_name)
        .filter(|handler| {
            let parsed = parse_claude_conditional_matcher(handler.matcher.as_deref());
            let matcher = parsed
                .as_ref()
                .map_or(handler.matcher.as_deref(), |parsed| {
                    parsed.matcher.as_deref()
                });
            match event_name {
                HookEventName::PreToolUse
                | HookEventName::PermissionRequest
                | HookEventName::PostToolUse
                | HookEventName::PreCompact
                | HookEventName::PostCompact
                | HookEventName::PostToolUseFailure
                | HookEventName::SessionStart
                | HookEventName::SessionEnd
                | HookEventName::SubagentStart
                | HookEventName::SubagentStop
                | HookEventName::FileChanged => {
                    if matcher_inputs.is_empty() {
                        matches_matcher(matcher, /*input*/ None)
                    } else {
                        matcher_inputs
                            .iter()
                            .any(|input| matches_matcher(matcher, Some(input)))
                    }
                }
                HookEventName::Notification
                | HookEventName::UserPromptSubmit
                | HookEventName::Stop
                | HookEventName::StopFailure => true,
            }
        })
        .filter(|handler| {
            let Some(parsed) = parse_claude_conditional_matcher(handler.matcher.as_deref()) else {
                return true;
            };
            tool_input.is_some_and(|tool_input| {
                parsed.conditions.iter().all(|condition| {
                    condition_matches_tool_use(condition, matcher_inputs, tool_input)
                })
            })
        })
        .cloned()
        .collect()
}

fn parse_claude_conditional_matcher(matcher: Option<&str>) -> Option<ClaudeConditionalMatcher> {
    let encoded = matcher?.strip_prefix(CLAUDE_CONDITIONAL_MATCHER_PREFIX)?;
    serde_json::from_str(encoded).ok()
}

fn condition_matches_tool_use(
    condition: &ClaudeHookCondition,
    matcher_inputs: &[&str],
    tool_input: &serde_json::Value,
) -> bool {
    match condition {
        ClaudeHookCondition::ToolCommandGlob {
            tool_name,
            command_glob,
        } => {
            matcher_inputs.iter().any(|input| input == tool_name)
                && tool_input
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|command| command_matches_glob(command_glob, command))
        }
    }
}

fn command_matches_glob(pattern: &str, command: &str) -> bool {
    if glob_matches(pattern, command) {
        return true;
    }
    let Some(commands) = split_top_level_shell_commands(command) else {
        return true;
    };
    commands
        .iter()
        .any(|command| glob_matches(pattern, command.trim()))
}

fn split_top_level_shell_commands(command: &str) -> Option<Vec<&str>> {
    let mut commands = Vec::new();
    let mut start = 0;
    let mut chars = command.char_indices().peekable();
    let mut quote = None;
    let mut escaped = false;

    while let Some((index, ch)) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            ';' | '\n' | '&' | '|' => {
                let next_len = if matches!(ch, '&' | '|')
                    && chars.peek().is_some_and(|(_, next)| *next == ch)
                {
                    chars.next();
                    ch.len_utf8() * 2
                } else {
                    ch.len_utf8()
                };
                commands.push(&command[start..index]);
                start = index + next_len;
            }
            '$' if chars.peek().is_some_and(|(_, next)| *next == '(') => return None,
            '`' | '(' | ')' | '{' | '}' => return None,
            _ => {}
        }
    }

    if escaped || quote.is_some() {
        return None;
    }
    commands.push(&command[start..]);
    Some(commands)
}

fn glob_matches(pattern: &str, input: &str) -> bool {
    let mut regex_pattern = String::from("^");
    for ch in pattern.chars() {
        match ch {
            '*' => regex_pattern.push_str(".*"),
            '?' => regex_pattern.push('.'),
            _ => regex_pattern.push_str(&regex::escape(&ch.to_string())),
        }
    }
    regex_pattern.push('$');
    regex::Regex::new(&regex_pattern)
        .map(|regex| regex.is_match(input))
        .unwrap_or(false)
}

pub(crate) fn running_summary(handler: &ConfiguredHandler) -> HookRunSummary {
    HookRunSummary {
        id: handler.run_id(),
        event_name: handler.event_name,
        handler_type: handler.handler_type(),
        execution_mode: handler.execution_mode(),
        scope: scope_for_event(handler.event_name),
        source_path: handler.source_path.clone(),
        source: handler.source,
        display_order: handler.display_order,
        status: HookRunStatus::Running,
        status_message: handler.status_message.clone(),
        started_at: chrono::Utc::now().timestamp(),
        completed_at: None,
        duration_ms: None,
        entries: Vec::new(),
    }
}

pub(crate) async fn execute_handlers<T: 'static>(
    engine: &ClaudeHooksEngine,
    handlers: Vec<ConfiguredHandler>,
    input_json: String,
    cwd: &Path,
    turn_id: Option<String>,
    parse: fn(&ConfiguredHandler, HandlerRunResult, Option<String>) -> ParsedHandler<T>,
) -> Vec<ParsedHandler<T>> {
    let mut pending = FuturesUnordered::new();
    for (configured_order, handler) in handlers.into_iter().enumerate() {
        if handler.execution_mode() == HookExecutionMode::Async {
            engine.command_runtime.schedule_async_hook(
                handler,
                input_json.clone(),
                cwd.to_path_buf(),
                turn_id.clone(),
                parse,
            );
            continue;
        }
        let input_json = input_json.clone();
        let turn_id = turn_id.clone();
        pending.push(async move {
            let result = match &handler.kind {
                ConfiguredHandlerKind::Command { command, env, .. } => {
                    run_command(
                        &engine.command_runtime,
                        &handler,
                        command,
                        env,
                        &input_json,
                        cwd,
                    )
                    .await
                }
            };
            (configured_order, parse(&handler, result, turn_id))
        });
    }

    let mut completed = Vec::new();
    let mut completion_order = 0;
    while let Some((configured_order, mut parsed)) = pending.next().await {
        parsed.completion_order = completion_order;
        completion_order += 1;
        completed.push((configured_order, parsed));
    }
    completed.sort_by_key(|(configured_order, _)| *configured_order);
    completed.into_iter().map(|(_, parsed)| parsed).collect()
}

pub(crate) fn completed_summary(
    handler: &ConfiguredHandler,
    run_result: &HandlerRunResult,
    status: HookRunStatus,
    entries: Vec<codex_protocol::protocol::HookOutputEntry>,
) -> HookRunSummary {
    HookRunSummary {
        id: handler.run_id(),
        event_name: handler.event_name,
        handler_type: handler.handler_type(),
        execution_mode: handler.execution_mode(),
        scope: scope_for_event(handler.event_name),
        source_path: handler.source_path.clone(),
        source: handler.source,
        display_order: handler.display_order,
        status,
        status_message: handler.status_message.clone(),
        started_at: run_result.started_at,
        completed_at: Some(run_result.completed_at),
        duration_ms: Some(run_result.duration_ms),
        entries,
    }
}

pub(crate) fn scope_for_event(event_name: HookEventName) -> HookScope {
    match event_name {
        HookEventName::SessionStart
        | HookEventName::SessionEnd
        | HookEventName::Notification
        | HookEventName::SubagentStart => HookScope::Thread,
        HookEventName::PreToolUse
        | HookEventName::PermissionRequest
        | HookEventName::PostToolUse
        | HookEventName::PreCompact
        | HookEventName::PostCompact
        | HookEventName::PostToolUseFailure
        | HookEventName::UserPromptSubmit
        | HookEventName::SubagentStop
        | HookEventName::FileChanged
        | HookEventName::Stop
        | HookEventName::StopFailure => HookScope::Turn,
    }
}

pub(crate) fn hook_event_name_label(event_name: HookEventName) -> &'static str {
    match event_name {
        HookEventName::PreToolUse => "PreToolUse",
        HookEventName::PermissionRequest => "PermissionRequest",
        HookEventName::PostToolUse => "PostToolUse",
        HookEventName::PreCompact => "PreCompact",
        HookEventName::PostCompact => "PostCompact",
        HookEventName::PostToolUseFailure => "PostToolUseFailure",
        HookEventName::Notification => "Notification",
        HookEventName::SessionStart => "SessionStart",
        HookEventName::SessionEnd => "SessionEnd",
        HookEventName::UserPromptSubmit => "UserPromptSubmit",
        HookEventName::SubagentStart => "SubagentStart",
        HookEventName::SubagentStop => "SubagentStop",
        HookEventName::Stop => "Stop",
        HookEventName::StopFailure => "StopFailure",
        HookEventName::FileChanged => "FileChanged",
    }
}

pub(crate) fn hook_execution_mode_label(mode: HookExecutionMode) -> &'static str {
    match mode {
        HookExecutionMode::Sync => "sync",
        HookExecutionMode::Async => "async",
    }
}

pub(crate) fn hook_handler_type_label(handler_type: HookHandlerType) -> &'static str {
    match handler_type {
        HookHandlerType::Command => "command",
        HookHandlerType::Prompt => "prompt",
        HookHandlerType::Agent => "agent",
    }
}

pub(crate) fn hook_scope_label(scope: HookScope) -> &'static str {
    match scope {
        HookScope::Thread => "thread",
        HookScope::Turn => "turn",
    }
}

pub(crate) fn hook_source_label(source: codex_protocol::protocol::HookSource) -> &'static str {
    match source {
        codex_protocol::protocol::HookSource::System => "system",
        codex_protocol::protocol::HookSource::User => "user",
        codex_protocol::protocol::HookSource::Project => "project",
        codex_protocol::protocol::HookSource::Mdm => "mdm",
        codex_protocol::protocol::HookSource::SessionFlags => "session_flags",
        codex_protocol::protocol::HookSource::SupervisorSettings => "supervisor_settings",
        codex_protocol::protocol::HookSource::Plugin => "plugin",
        codex_protocol::protocol::HookSource::CloudRequirements => "cloud_requirements",
        codex_protocol::protocol::HookSource::CloudManagedConfig => "cloud_managed_config",
        codex_protocol::protocol::HookSource::LegacyManagedConfigFile => {
            "legacy_managed_config_file"
        }
        codex_protocol::protocol::HookSource::LegacyManagedConfigMdm => "legacy_managed_config_mdm",
        codex_protocol::protocol::HookSource::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use codex_protocol::protocol::HookEventName;
    use codex_protocol::protocol::HookSource;
    use codex_utils_absolute_path::test_support::PathBufExt;
    use codex_utils_absolute_path::test_support::test_path_buf;
    use pretty_assertions::assert_eq;

    use super::ConfiguredHandler;
    use super::ConfiguredHandlerKind;
    use super::select_handlers;
    use super::select_handlers_for_matcher_inputs;

    fn make_handler(
        event_name: HookEventName,
        matcher: Option<&str>,
        command: &str,
        display_order: i64,
    ) -> ConfiguredHandler {
        ConfiguredHandler {
            event_name,
            matcher: matcher.map(str::to_owned),
            timeout_sec: 5,
            status_message: None,
            additional_context_limit: Default::default(),
            source_path: test_path_buf("/tmp/hooks.json").abs(),
            source: HookSource::User,
            display_order,
            kind: ConfiguredHandlerKind::Command {
                command: command.to_string(),
                r#async: false,
                env: std::collections::HashMap::new(),
            },
        }
    }

    #[test]
    fn select_handlers_keeps_duplicate_stop_handlers() {
        let handlers = vec![
            make_handler(
                HookEventName::Stop,
                /*matcher*/ None,
                "echo same",
                /*display_order*/ 0,
            ),
            make_handler(
                HookEventName::Stop,
                /*matcher*/ None,
                "echo same",
                /*display_order*/ 1,
            ),
        ];

        let selected = select_handlers(&handlers, HookEventName::Stop, /*matcher_input*/ None);

        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].display_order, 0);
        assert_eq!(selected[1].display_order, 1);
    }

    #[test]
    fn select_handlers_keeps_overlapping_session_start_matchers() {
        let handlers = vec![
            make_handler(
                HookEventName::SessionStart,
                Some("start.*"),
                "echo same",
                /*display_order*/ 0,
            ),
            make_handler(
                HookEventName::SessionStart,
                Some("^startup$"),
                "echo same",
                /*display_order*/ 1,
            ),
        ];

        let selected = select_handlers(&handlers, HookEventName::SessionStart, Some("startup"));

        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].display_order, 0);
        assert_eq!(selected[1].display_order, 1);
    }

    #[test]
    fn compact_hooks_match_trigger() {
        let handlers = vec![
            make_handler(
                HookEventName::PreCompact,
                Some("manual"),
                "echo manual",
                /*display_order*/ 0,
            ),
            make_handler(
                HookEventName::PreCompact,
                Some("auto"),
                "echo auto",
                /*display_order*/ 1,
            ),
        ];

        let selected = select_handlers(&handlers, HookEventName::PreCompact, Some("manual"));

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].display_order, 0);
    }

    #[test]
    fn pre_tool_use_matches_tool_name() {
        let handlers = vec![
            make_handler(
                HookEventName::PreToolUse,
                Some("^Bash$"),
                "echo same",
                /*display_order*/ 0,
            ),
            make_handler(
                HookEventName::PreToolUse,
                Some("^Edit$"),
                "echo same",
                /*display_order*/ 1,
            ),
        ];

        let selected = select_handlers(&handlers, HookEventName::PreToolUse, Some("Bash"));

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].display_order, 0);
    }

    #[test]
    fn post_tool_use_matches_tool_name() {
        let handlers = vec![
            make_handler(
                HookEventName::PostToolUse,
                Some("^Bash$"),
                "echo same",
                /*display_order*/ 0,
            ),
            make_handler(
                HookEventName::PostToolUse,
                Some("^Edit$"),
                "echo same",
                /*display_order*/ 1,
            ),
        ];

        let selected = select_handlers(&handlers, HookEventName::PostToolUse, Some("Bash"));

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].display_order, 0);
    }

    #[test]
    fn pre_tool_use_star_matcher_matches_all_tools() {
        let handlers = vec![
            make_handler(
                HookEventName::PreToolUse,
                Some("*"),
                "echo same",
                /*display_order*/ 0,
            ),
            make_handler(
                HookEventName::PreToolUse,
                Some("^Edit$"),
                "echo same",
                /*display_order*/ 1,
            ),
        ];

        let selected = select_handlers(&handlers, HookEventName::PreToolUse, Some("Bash"));

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].display_order, 0);
    }

    #[test]
    fn pre_tool_use_regex_alternation_matches_each_tool_name() {
        let handlers = vec![make_handler(
            HookEventName::PreToolUse,
            Some("Edit|Write"),
            "echo same",
            /*display_order*/ 0,
        )];

        let selected_edit = select_handlers(&handlers, HookEventName::PreToolUse, Some("Edit"));
        let selected_write = select_handlers(&handlers, HookEventName::PreToolUse, Some("Write"));
        let selected_bash = select_handlers(&handlers, HookEventName::PreToolUse, Some("Bash"));

        assert_eq!(selected_edit.len(), 1);
        assert_eq!(selected_write.len(), 1);
        assert_eq!(selected_bash.len(), 0);
    }

    #[test]
    fn pre_tool_use_aliases_match_once_per_handler() {
        let handlers = vec![
            make_handler(
                HookEventName::PreToolUse,
                Some("^apply_patch$"),
                "echo apply_patch",
                /*display_order*/ 0,
            ),
            make_handler(
                HookEventName::PreToolUse,
                Some("^Write$"),
                "echo write",
                /*display_order*/ 1,
            ),
            make_handler(
                HookEventName::PreToolUse,
                Some("^Edit$"),
                "echo edit",
                /*display_order*/ 2,
            ),
            make_handler(
                HookEventName::PreToolUse,
                Some("apply_patch|Write|Edit"),
                "echo combined",
                /*display_order*/ 3,
            ),
        ];

        let selected = select_handlers_for_matcher_inputs(
            &handlers,
            HookEventName::PreToolUse,
            &["apply_patch", "Write", "Edit"],
        );

        assert_eq!(selected.len(), 4);
        assert_eq!(
            selected
                .iter()
                .map(|handler| handler.display_order)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3],
        );
    }

    #[test]
    fn user_prompt_submit_ignores_matcher() {
        let handlers = vec![
            make_handler(
                HookEventName::UserPromptSubmit,
                Some("^hello"),
                "echo first",
                /*display_order*/ 0,
            ),
            make_handler(
                HookEventName::UserPromptSubmit,
                Some("["),
                "echo second",
                /*display_order*/ 1,
            ),
        ];

        let selected = select_handlers(
            &handlers,
            HookEventName::UserPromptSubmit,
            /*matcher_input*/ None,
        );

        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].display_order, 0);
        assert_eq!(selected[1].display_order, 1);
    }

    #[test]
    fn select_handlers_preserves_declaration_order() {
        let handlers = vec![
            make_handler(
                HookEventName::Stop,
                /*matcher*/ None,
                "first",
                /*display_order*/ 0,
            ),
            make_handler(
                HookEventName::Stop,
                /*matcher*/ None,
                "second",
                /*display_order*/ 1,
            ),
            make_handler(
                HookEventName::Stop,
                /*matcher*/ None,
                "third",
                /*display_order*/ 2,
            ),
        ];

        let selected = select_handlers(&handlers, HookEventName::Stop, /*matcher_input*/ None);

        assert_eq!(selected, handlers);
    }
}
