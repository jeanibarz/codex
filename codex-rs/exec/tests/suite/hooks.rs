#![cfg(not(target_os = "windows"))]
#![allow(clippy::expect_used)]

use core_test_support::responses;
use core_test_support::test_codex_exec::test_codex_exec;
use pretty_assertions::assert_eq;
use serde_json::Value;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_plugin_dir_hooks_fire_for_shell_command() -> anyhow::Result<()> {
    let test = test_codex_exec();
    let plugin_root = test.cwd_path().join("plugin");
    let hooks_dir = plugin_root.join("hooks");
    let manifest_dir = plugin_root.join(".claude-plugin");
    std::fs::create_dir_all(&hooks_dir)?;
    std::fs::create_dir_all(&manifest_dir)?;
    std::fs::write(
        manifest_dir.join("plugin.json"),
        r#"{"name":"exec-plugin-dir-smoke","version":"0.0.0"}"#,
    )?;

    let hook_log = test.cwd_path().join("plugin-hook-fired.jsonl");
    let hook_script = hooks_dir.join("pre_tool_use_hook.py");
    std::fs::write(
        &hook_script,
        format!(
            r#"import json
from pathlib import Path
import sys

payload = json.load(sys.stdin)
with Path(r"{hook_log}").open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(payload) + "\n")

print(json.dumps({{
    "hookSpecificOutput": {{
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": "blocked by plugin-dir hook"
    }}
}}))
"#,
            hook_log = hook_log.display(),
        ),
    )?;
    std::fs::write(
        hooks_dir.join("hooks.json"),
        r#"{
  "hooks": {
    "PreToolUse": [{
      "matcher": "^Bash$",
      "hooks": [{
        "type": "command",
        "command": "python3 ${PLUGIN_ROOT}/hooks/pre_tool_use_hook.py"
      }]
    }]
  }
}"#,
    )?;

    let server = responses::start_mock_server().await;
    let server_uri = server.uri();
    std::fs::write(
        test.home_path().join("config.toml"),
        format!(
            r#"model_provider = "mock"

[model_providers.mock]
name = "mock"
base_url = "{server_uri}/v1"
env_key = "CODEX_API_KEY"
wire_api = "responses"
supports_websockets = false
"#
        ),
    )?;
    responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("resp-1"),
                responses::ev_shell_command_call("call-1", "echo blocked"),
                responses::ev_completed("resp-1"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("resp-2"),
                responses::ev_assistant_message("msg-1", "done"),
                responses::ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let assert = test
        .cmd()
        .arg("--plugin-dir")
        .arg(&plugin_root)
        .arg("--dangerously-bypass-hook-trust")
        .arg("-c")
        .arg("features.codex_hooks=true")
        .arg("-c")
        .arg("features.plugin_hooks=true")
        .arg("--skip-git-repo-check")
        .arg("-s")
        .arg("danger-full-access")
        .arg("-m")
        .arg("gpt-5.1")
        .arg("run a shell command")
        .assert()
        .success();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        hook_log.exists(),
        "expected plugin-dir hook to write log at {}\nstderr:\n{stderr}",
        hook_log.display()
    );

    assert!(
        stderr.contains("Command blocked by PreToolUse hook: blocked by plugin-dir hook")
            || stderr.contains("blocked by plugin-dir hook"),
        "expected plugin-dir hook block to surface in stderr, got:\n{stderr}\nhook log:\n{}",
        std::fs::read_to_string(&hook_log).unwrap_or_default()
    );

    let hook_log_contents = std::fs::read_to_string(&hook_log)?;
    let hook_events = hook_log_contents
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;

    assert_eq!(hook_events.len(), 1);
    assert_eq!(hook_events[0]["hook_event_name"], "PreToolUse");
    assert_eq!(hook_events[0]["tool_name"], "Bash");
    assert_eq!(hook_events[0]["tool_use_id"], "call-1");
    assert_eq!(hook_events[0]["tool_input"]["command"], "echo blocked");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_settings_file_hooks_fire_for_shell_command() -> anyhow::Result<()> {
    let test = test_codex_exec();
    let hook_log = test.cwd_path().join("hook-fired.jsonl");
    let settings_path = test.cwd_path().join("settings.json");
    let hook_log_display = hook_log.display();
    let hook_command = format!("payload=$(cat); printf '%s\\n' \"$payload\" >> {hook_log_display}");
    let settings = serde_json::json!({
        "hooks": {
            "PreToolUse": [{
                "matcher": "*",
                "hooks": [{ "type": "command", "command": hook_command }]
            }],
            "PostToolUse": [{
                "matcher": "*",
                "hooks": [{ "type": "command", "command": hook_command }]
            }]
        }
    });
    std::fs::write(&settings_path, serde_json::to_vec_pretty(&settings)?)?;

    let server = responses::start_mock_server().await;
    let server_uri = server.uri();
    std::fs::write(
        test.home_path().join("config.toml"),
        format!(
            r#"model_provider = "mock"

[model_providers.mock]
name = "mock"
base_url = "{server_uri}/v1"
env_key = "CODEX_API_KEY"
wire_api = "responses"
supports_websockets = false
"#
        ),
    )?;
    responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("resp-1"),
                responses::ev_shell_command_call("call-1", "echo hello"),
                responses::ev_completed("resp-1"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("resp-2"),
                responses::ev_assistant_message("msg-1", "done"),
                responses::ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    test.cmd()
        .arg("--settings")
        .arg(&settings_path)
        .arg("-c")
        .arg("features.codex_hooks=true")
        .arg("--skip-git-repo-check")
        .arg("-s")
        .arg("danger-full-access")
        .arg("-m")
        .arg("gpt-5.1")
        .arg("run a shell command")
        .assert()
        .success();

    let hook_log_contents = std::fs::read_to_string(&hook_log)?;
    let hook_events = hook_log_contents
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;

    assert_eq!(
        hook_events
            .iter()
            .map(|event| event["hook_event_name"].as_str().expect("hook event name"))
            .collect::<Vec<_>>(),
        vec!["PreToolUse", "PostToolUse"]
    );
    assert_eq!(hook_events[0]["tool_name"], "Bash");
    assert_eq!(hook_events[0]["tool_use_id"], "call-1");
    assert_eq!(hook_events[0]["tool_input"]["command"], "echo hello");
    assert_eq!(hook_events[1]["tool_name"], "Bash");
    assert_eq!(hook_events[1]["tool_use_id"], "call-1");
    assert_eq!(hook_events[1]["tool_input"]["command"], "echo hello");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_user_claude_settings_hooks_fire_for_shell_command() -> anyhow::Result<()> {
    let test = test_codex_exec();
    let real_home = tempfile::tempdir()?;
    let hook_log = test.cwd_path().join("claude-settings-hook-fired.jsonl");
    let claude_dir = real_home.path().join(".claude");
    std::fs::create_dir_all(&claude_dir)?;
    let hook_log_display = hook_log.display();
    let hook_command = format!("payload=$(cat); printf '%s\\n' \"$payload\" >> {hook_log_display}");
    let settings = serde_json::json!({
        "hooks": {
            "PreToolUse": [{
                "matcher": "Bash",
                "hooks": [{ "type": "command", "command": hook_command }]
            }]
        }
    });
    std::fs::write(
        claude_dir.join("settings.json"),
        serde_json::to_vec_pretty(&settings)?,
    )?;

    let server = responses::start_mock_server().await;
    let server_uri = server.uri();
    std::fs::write(
        test.home_path().join("config.toml"),
        format!(
            r#"model_provider = "mock"

[model_providers.mock]
name = "mock"
base_url = "{server_uri}/v1"
env_key = "CODEX_API_KEY"
wire_api = "responses"
supports_websockets = false
"#
        ),
    )?;
    responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("resp-1"),
                responses::ev_shell_command_call("call-1", "echo from claude settings"),
                responses::ev_completed("resp-1"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("resp-2"),
                responses::ev_assistant_message("msg-1", "done"),
                responses::ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    test.cmd()
        .env("HOME", real_home.path())
        .arg("-c")
        .arg("features.codex_hooks=true")
        .arg("--skip-git-repo-check")
        .arg("-s")
        .arg("danger-full-access")
        .arg("-m")
        .arg("gpt-5.1")
        .arg("run a shell command")
        .assert()
        .success();

    let hook_log_contents = std::fs::read_to_string(&hook_log)?;
    let hook_events = hook_log_contents
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;

    assert_eq!(
        hook_events
            .iter()
            .map(|event| event["hook_event_name"].as_str().expect("hook event name"))
            .collect::<Vec<_>>(),
        vec!["PreToolUse"]
    );
    assert_eq!(hook_events[0]["tool_name"], "Bash");
    assert_eq!(hook_events[0]["tool_use_id"], "call-1");
    assert_eq!(
        hook_events[0]["tool_input"]["command"],
        "echo from claude settings"
    );

    Ok(())
}
