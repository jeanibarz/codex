#![cfg(not(target_os = "windows"))]
#![allow(clippy::expect_used)]

use core_test_support::responses;
use core_test_support::test_codex_exec::test_codex_exec;
use pretty_assertions::assert_eq;
use serde_json::Value;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_settings_file_hooks_fire_for_shell_command() -> anyhow::Result<()> {
    let test = test_codex_exec();
    let hook_log = test.cwd_path().join("hook-fired.jsonl");
    let settings_path = test.cwd_path().join("settings.json");
    let hook_log_display = hook_log.display();
    let hook_command = format!(
        "payload=$(cat); printf '%s\\n' \"$payload\" >> {hook_log_display}"
    );
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
