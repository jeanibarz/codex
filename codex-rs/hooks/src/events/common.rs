use codex_protocol::protocol::HookCompletedEvent;
use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::HookOutputEntry;
use codex_protocol::protocol::HookOutputEntryKind;
use codex_protocol::protocol::HookRunStatus;

use crate::engine::dispatcher;
use crate::engine::ConfiguredHandler;

pub(crate) fn join_text_chunks(chunks: Vec<String>) -> Option<String> {
    if chunks.is_empty() {
        None
    } else {
        Some(chunks.join("\n\n"))
    }
}

pub(crate) fn trimmed_non_empty(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(crate) fn append_additional_context(
    entries: &mut Vec<HookOutputEntry>,
    additional_contexts_for_model: &mut Vec<String>,
    additional_context: String,
) {
    entries.push(HookOutputEntry {
        kind: HookOutputEntryKind::Context,
        text: additional_context.clone(),
    });
    additional_contexts_for_model.push(additional_context);
}

pub(crate) fn flatten_additional_contexts<'a>(
    additional_contexts: impl IntoIterator<Item = &'a [String]>,
) -> Vec<String> {
    additional_contexts
        .into_iter()
        .flat_map(|chunk| chunk.iter().cloned())
        .collect()
}

pub(crate) fn serialization_failure_hook_events(
    handlers: Vec<ConfiguredHandler>,
    turn_id: Option<String>,
    error_message: String,
) -> Vec<HookCompletedEvent> {
    handlers
        .into_iter()
        .map(|handler| {
            let mut run = dispatcher::running_summary(&handler);
            run.status = HookRunStatus::Failed;
            run.completed_at = Some(run.started_at);
            run.duration_ms = Some(0);
            run.entries = vec![HookOutputEntry {
                kind: HookOutputEntryKind::Error,
                text: error_message.clone(),
            }];
            HookCompletedEvent {
                turn_id: turn_id.clone(),
                run,
            }
        })
        .collect()
}

pub(crate) fn matcher_pattern_for_event(
    event_name: HookEventName,
    matcher: Option<&str>,
) -> Option<&str> {
    match event_name {
        HookEventName::PreToolUse
        | HookEventName::PostToolUse
        | HookEventName::PostToolUseFailure
        | HookEventName::Notification
        | HookEventName::SessionStart
        | HookEventName::PermissionRequest => matcher,
        HookEventName::UserPromptSubmit
        | HookEventName::Stop
        | HookEventName::StopFailure
        | HookEventName::SessionEnd => None,
    }
}

pub(crate) fn validate_command_handler_condition(condition: &str) -> Result<(), String> {
    parse_command_handler_condition(condition).map(|_| ())
}

pub(crate) fn matches_command_handler_condition(
    condition: Option<&str>,
    tool_name: Option<&str>,
    command: Option<&str>,
) -> bool {
    let Some(condition) = condition else {
        return true;
    };

    let Ok((expected_tool_name, command_pattern)) = parse_command_handler_condition(condition) else {
        return false;
    };

    if tool_name != Some(expected_tool_name) {
        return false;
    }

    match command_pattern {
        Some(pattern) => command
            .map(|command| wildcard_pattern_matches(pattern, command))
            .unwrap_or(false),
        None => true,
    }
}

pub(crate) fn validate_matcher_pattern(matcher: &str) -> Result<(), regex::Error> {
    if is_match_all_matcher(matcher) {
        return Ok(());
    }
    regex::Regex::new(matcher).map(|_| ())
}

pub(crate) fn matches_matcher(matcher: Option<&str>, input: Option<&str>) -> bool {
    match matcher {
        None => true,
        Some(matcher) if is_match_all_matcher(matcher) => true,
        Some(matcher) => input
            .and_then(|input| {
                regex::Regex::new(matcher)
                    .ok()
                    .map(|regex| regex.is_match(input))
            })
            .unwrap_or(false),
    }
}

fn is_match_all_matcher(matcher: &str) -> bool {
    matcher.is_empty() || matcher == "*"
}

fn parse_command_handler_condition(condition: &str) -> Result<(&str, Option<&str>), String> {
    let trimmed = condition.trim();
    if trimmed.is_empty() {
        return Err("condition cannot be empty".to_string());
    }

    match trimmed.split_once('(') {
        Some((tool_name, rest)) => {
            if tool_name.trim().is_empty() {
                return Err("condition is missing a tool name".to_string());
            }
            if !rest.ends_with(')') {
                return Err("condition must end with ')'".to_string());
            }
            let pattern = &rest[..rest.len() - 1];
            if pattern.contains('(') {
                return Err("nested '(' is not supported in conditions".to_string());
            }
            Ok((tool_name.trim(), (!pattern.is_empty()).then_some(pattern)))
        }
        None => Ok((trimmed, None)),
    }
}

fn wildcard_pattern_matches(pattern: &str, value: &str) -> bool {
    if is_match_all_matcher(pattern) {
        return true;
    }

    let mut regex_pattern = String::from("^");
    for ch in pattern.chars() {
        match ch {
            '*' => regex_pattern.push_str(".*"),
            _ => regex_pattern.push_str(&regex::escape(&ch.to_string())),
        }
    }
    regex_pattern.push('$');

    regex::Regex::new(&regex_pattern)
        .map(|regex| regex.is_match(value))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use codex_protocol::protocol::HookEventName;
    use pretty_assertions::assert_eq;

    use super::matcher_pattern_for_event;
    use super::matches_command_handler_condition;
    use super::matches_matcher;
    use super::validate_command_handler_condition;
    use super::validate_matcher_pattern;

    #[test]
    fn matcher_omitted_matches_all_occurrences() {
        assert!(matches_matcher(/*matcher*/ None, Some("Bash")));
        assert!(matches_matcher(/*matcher*/ None, Some("Write")));
    }

    #[test]
    fn matcher_star_matches_all_occurrences() {
        assert!(matches_matcher(Some("*"), Some("Bash")));
        assert!(matches_matcher(Some("*"), Some("Edit")));
        assert_eq!(validate_matcher_pattern("*"), Ok(()));
    }

    #[test]
    fn matcher_empty_string_matches_all_occurrences() {
        assert!(matches_matcher(Some(""), Some("Bash")));
        assert!(matches_matcher(Some(""), Some("SessionStart")));
        assert_eq!(validate_matcher_pattern(""), Ok(()));
    }

    #[test]
    fn matcher_uses_regex_matching() {
        assert!(matches_matcher(Some("Edit|Write"), Some("Edit")));
        assert!(matches_matcher(Some("Edit|Write"), Some("Write")));
        assert!(!matches_matcher(Some("Edit|Write"), Some("Bash")));
        assert_eq!(validate_matcher_pattern("Edit|Write"), Ok(()));
    }

    #[test]
    fn matcher_supports_anchored_regexes() {
        assert!(matches_matcher(Some("^Bash$"), Some("Bash")));
        assert!(!matches_matcher(Some("^Bash$"), Some("BashOutput")));
        assert_eq!(validate_matcher_pattern("^Bash$"), Ok(()));
    }

    #[test]
    fn invalid_regex_is_rejected() {
        assert!(validate_matcher_pattern("[").is_err());
        assert!(!matches_matcher(Some("["), Some("Bash")));
    }

    #[test]
    fn unsupported_events_ignore_matchers() {
        assert_eq!(
            matcher_pattern_for_event(HookEventName::UserPromptSubmit, Some("^hello")),
            None
        );
        assert_eq!(
            matcher_pattern_for_event(HookEventName::Stop, Some("^done$")),
            None
        );
    }

    #[test]
    fn supported_events_keep_matchers() {
        assert_eq!(
            matcher_pattern_for_event(HookEventName::PreToolUse, Some("Bash")),
            Some("Bash")
        );
        assert_eq!(
            matcher_pattern_for_event(HookEventName::PostToolUse, Some("Edit|Write")),
            Some("Edit|Write")
        );
        assert_eq!(
            matcher_pattern_for_event(HookEventName::Notification, Some("permission_prompt")),
            Some("permission_prompt")
        );
        assert_eq!(
            matcher_pattern_for_event(HookEventName::SessionStart, Some("startup|resume")),
            Some("startup|resume")
        );
        assert_eq!(
            matcher_pattern_for_event(HookEventName::PermissionRequest, Some("WorkspaceTrust")),
            Some("WorkspaceTrust")
        );
    }

    #[test]
    fn validates_command_handler_conditions() {
        assert_eq!(validate_command_handler_condition("Bash(git push*)"), Ok(()));
        assert_eq!(validate_command_handler_condition("Notification(permission_prompt)"), Ok(()));
        assert!(validate_command_handler_condition("").is_err());
        assert!(validate_command_handler_condition("(git push*)").is_err());
        assert!(validate_command_handler_condition("Bash(git push*").is_err());
    }

    #[test]
    fn command_handler_conditions_match_tool_name_and_command_pattern() {
        assert!(matches_command_handler_condition(
            Some("Bash(gh pr create*)"),
            Some("Bash"),
            Some("gh pr create --fill")
        ));
        assert!(!matches_command_handler_condition(
            Some("Bash(gh pr create*)"),
            Some("Bash"),
            Some("git push origin main")
        ));
        assert!(matches_command_handler_condition(
            Some("Notification(permission_prompt)"),
            Some("Notification"),
            Some("permission_prompt")
        ));
        assert!(matches_command_handler_condition(
            Some("WorkspaceTrust"),
            Some("WorkspaceTrust"),
            None
        ));
    }
}
