//! Provider progress must survive stream branches that have no UI consumer.

use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_features::Feature;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use core_test_support::fs_wait;
use core_test_support::hooks::trust_discovered_hooks;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_custom_tool_call;
use core_test_support::responses::ev_message_item_added;
use core_test_support::responses::ev_output_text_delta;
use core_test_support::responses::ev_reasoning_item_added;
use core_test_support::responses::ev_reasoning_summary_text_delta;
use core_test_support::responses::ev_reasoning_text_delta;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::skip_if_wine_exec;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::StreamingSseServer;
use core_test_support::streaming_sse::start_streaming_sse_server;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use core_test_support::wait_for_event_match;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio::sync::oneshot;
use tokio::time::sleep;
use tokio::time::timeout;

fn output_events(kind: &str, delta: &str) -> Vec<Value> {
    match kind {
        "custom_exec" => {
            let mut added = ev_custom_tool_call("call-1", "exec", "");
            added["type"] = json!("response.output_item.added");
            added["item"]["id"] = json!("item-1");
            vec![
                added,
                json!({
                    "type": "response.custom_tool_call_input.delta",
                    "item_id": "item-1",
                    "call_id": "call-1",
                    "delta": delta,
                }),
            ]
        }
        "text" => vec![
            ev_message_item_added("message-1", ""),
            ev_output_text_delta(delta),
        ],
        "reasoning_summary" => vec![
            ev_reasoning_item_added("reasoning-1", &[]),
            ev_reasoning_summary_text_delta(delta),
        ],
        "reasoning_content" => vec![
            ev_reasoning_item_added("reasoning-1", &[]),
            ev_reasoning_text_delta(delta),
        ],
        _ => panic!("unknown provider output kind: {kind}"),
    }
}

enum RecorderMode {
    Immediate,
    BlockingFailure,
}

async fn build_progress_test(
    server: &StreamingSseServer,
    recorder: RecorderMode,
) -> Result<TestCodex> {
    test_codex()
        .with_model("gpt-5.4")
        .with_pre_build_hook(move |home| {
            let script = home.join("capture_progress.py");
            let mut body = r#"import json, pathlib, sys, time
p = json.load(sys.stdin)
if p.get('notification_type') != 'provider_progress':
    sys.exit(0)
base = pathlib.Path(__file__)
with base.with_suffix('.jsonl').open('a') as f:
    f.write(json.dumps(p) + '\n')
"#
            .to_string();
            if let RecorderMode::BlockingFailure = recorder {
                body.push_str(
                    r#"while not base.with_suffix('.release').exists():
    time.sleep(0.01)
base.with_suffix('.failed').write_text('hook exits with status 7')
sys.exit(7)
"#,
                );
            }
            fs::write(&script, body).expect("write notification recorder");
            fs::write(
                home.join("hooks.json"),
                json!({"hooks": {"Notification": [{"hooks": [{
                    "type": "command",
                    "command": format!("python3 {}", script.display()),
                }]}]}})
                .to_string(),
            )
            .expect("write notification hook settings");
        })
        .with_config(|config| {
            trust_discovered_hooks(config);
            config
                .features
                .enable(Feature::CodeMode)
                .expect("code mode");
        })
        .build_with_streaming_server(server)
        .await
}

async fn start_turn(test: &TestCodex) -> Result<String> {
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Exercise provider progress.".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    Ok(wait_for_event_match(&test.codex, |event| match event {
        EventMsg::TurnStarted(event) => Some(event.turn_id.clone()),
        _ => None,
    })
    .await)
}

fn progress_inputs(home: &Path) -> Result<Vec<Value>> {
    let log = home.join("capture_progress.jsonl");
    if !log.exists() {
        return Ok(Vec::new());
    }
    fs::read_to_string(log)?
        .lines()
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}

async fn wait_for_progress(home: &Path) -> Result<Value> {
    timeout(Duration::from_secs(5), async {
        loop {
            if let Some(input) = progress_inputs(home)?.into_iter().next() {
                return Ok(input);
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await?
}

#[test_case::test_case("custom_exec"; "custom exec without argument diff consumer")]
#[test_case::test_case("text"; "assistant text")]
#[test_case::test_case("reasoning_summary"; "reasoning summary")]
#[test_case::test_case("reasoning_content"; "reasoning content")]
#[tokio::test]
async fn nonempty_provider_output_emits_content_free_progress(kind: &str) -> Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_wine_exec!(Ok(()), "command hooks require a host-native executor");
    let (_finish_tx, finish_rx) = oneshot::channel();
    let mut events = vec![ev_response_created("response-1")];
    events.extend(output_events(
        kind,
        "private model output never sent to hooks",
    ));
    let (server, _) = start_streaming_sse_server(vec![vec![
        StreamingSseChunk {
            gate: None,
            body: sse(events),
        },
        StreamingSseChunk {
            gate: Some(finish_rx),
            body: sse(vec![ev_completed("response-1")]),
        },
    ]])
    .await;
    let test = build_progress_test(&server, RecorderMode::Immediate).await?;
    let earliest_observation = chrono::Utc::now().timestamp_millis();
    let turn_id = start_turn(&test).await?;

    // `exec` has no ToolArgumentDiffConsumer. Its delta must reach the hook
    // while the tool input is still being generated, before any tool runs.
    let input = wait_for_progress(test.codex_home_path()).await?;
    let observed_at_ms = input["observed_at_ms"].as_i64().expect("observation time");
    assert!(
        (earliest_observation..=chrono::Utc::now().timestamp_millis()).contains(&observed_at_ms)
    );
    assert_eq!(
        input,
        json!({
            "session_id": test.session_configured.thread_id.to_string(),
            "turn_id": turn_id,
            "transcript_path": test.codex.rollout_path(),
            "cwd": test.config.cwd.display().to_string(),
            "hook_event_name": "Notification",
            "model": "gpt-5.4",
            "notification_type": "provider_progress",
            "message": "Provider output received",
            "observed_at_ms": observed_at_ms,
        })
    );

    test.codex.submit(Op::Interrupt).await?;
    wait_for_event(&test.codex, |event| {
        match event {
            EventMsg::HookStarted(event) => {
                assert_ne!(event.run.event_name, HookEventName::Notification)
            }
            EventMsg::HookCompleted(event) => {
                assert_ne!(event.run.event_name, HookEventName::Notification)
            }
            _ => {}
        }
        matches!(event, EventMsg::TurnAborted(_))
    })
    .await;
    assert_eq!(server.requests().await.len(), 1);
    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn empty_deltas_and_transport_keepalives_do_not_emit_progress() -> Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_wine_exec!(Ok(()), "command hooks require a host-native executor");
    let (finish_tx, finish_rx) = oneshot::channel();
    let mut events = vec![ev_response_created("response-1")];
    for kind in [
        "text",
        "reasoning_summary",
        "reasoning_content",
        "custom_exec",
    ] {
        events.extend(output_events(kind, ""));
    }
    let (server, _) = start_streaming_sse_server(vec![vec![
        StreamingSseChunk {
            gate: None,
            body: format!(": keepalive\n\n{}", sse(events)),
        },
        StreamingSseChunk {
            gate: Some(finish_rx),
            body: sse(vec![ev_completed("response-1")]),
        },
    ]])
    .await;
    let test = build_progress_test(&server, RecorderMode::Immediate).await?;
    start_turn(&test).await?;
    server.wait_for_request_count(1).await;
    // Keep the stream open long enough for an incorrectly scheduled immediate
    // notification to run, then drain completion before checking the log.
    sleep(Duration::from_millis(250)).await;
    finish_tx.send(()).expect("release response completion");
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    assert_eq!(
        progress_inputs(test.codex_home_path())?,
        Vec::<Value>::new()
    );
    server.shutdown().await;
    Ok(())
}

#[test_case::test_case("complete"; "turn completes")]
#[test_case::test_case("interrupt"; "turn is interrupted")]
#[tokio::test]
async fn ending_turn_cancels_queued_progress(ending: &str) -> Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_wine_exec!(Ok(()), "command hooks require a host-native executor");
    let (next_tx, next_rx) = oneshot::channel();
    let (finish_tx, finish_rx) = oneshot::channel();
    let (server, _) = start_streaming_sse_server(vec![vec![
        StreamingSseChunk {
            gate: None,
            body: sse(vec![
                ev_response_created("response-1"),
                ev_message_item_added("message-1", ""),
                ev_output_text_delta("first"),
            ]),
        },
        StreamingSseChunk {
            gate: Some(next_rx),
            body: sse(vec![ev_output_text_delta(" queued")]),
        },
        StreamingSseChunk {
            gate: Some(finish_rx),
            body: sse(vec![
                ev_assistant_message("message-1", "first queued"),
                ev_completed("response-1"),
            ]),
        },
    ]])
    .await;
    let test = build_progress_test(&server, RecorderMode::Immediate).await?;
    start_turn(&test).await?;
    let first = wait_for_progress(test.codex_home_path()).await?;
    next_tx.send(()).expect("release queued output");
    wait_for_event(&test.codex, |event| matches!(event, EventMsg::AgentMessageContentDelta(event) if event.delta == " queued")).await;

    match ending {
        "complete" => {
            finish_tx.send(()).expect("release response completion");
            wait_for_event(&test.codex, |event| {
                matches!(event, EventMsg::TurnComplete(_))
            })
            .await;
        }
        "interrupt" => {
            test.codex.submit(Op::Interrupt).await?;
            wait_for_event(&test.codex, |event| {
                matches!(event, EventMsg::TurnAborted(_))
            })
            .await;
        }
        _ => panic!("unknown turn ending: {ending}"),
    }
    sleep(Duration::from_secs(11)).await;
    assert_eq!(progress_inputs(test.codex_home_path())?, vec![first]);
    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn blocked_and_failing_progress_hook_does_not_block_sampling() -> Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_wine_exec!(Ok(()), "command hooks require a host-native executor");
    let (while_blocked_tx, while_blocked_rx) = oneshot::channel();
    let (after_failure_tx, after_failure_rx) = oneshot::channel();
    let (_finish_tx, finish_rx) = oneshot::channel();
    let (server, _) = start_streaming_sse_server(vec![vec![
        StreamingSseChunk {
            gate: None,
            body: sse(vec![
                ev_response_created("response-1"),
                ev_message_item_added("message-1", ""),
                ev_output_text_delta("first"),
            ]),
        },
        StreamingSseChunk {
            gate: Some(while_blocked_rx),
            body: sse(vec![ev_output_text_delta(" while hook is blocked")]),
        },
        StreamingSseChunk {
            gate: Some(after_failure_rx),
            body: sse(vec![ev_output_text_delta(" after hook failure")]),
        },
        StreamingSseChunk {
            gate: Some(finish_rx),
            body: sse(vec![ev_completed("response-1")]),
        },
    ]])
    .await;
    let test = build_progress_test(&server, RecorderMode::BlockingFailure).await?;
    start_turn(&test).await?;
    let first = wait_for_progress(test.codex_home_path()).await?;
    let failed_path = test.codex_home_path().join("capture_progress.failed");

    // The recorder has consumed stdin and cannot finish until its file gate
    // opens. Awaiting it in the sampling loop would prevent this next delta.
    while_blocked_tx
        .send(())
        .expect("release output while hook blocks");
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::AgentMessageContentDelta(event) if event.delta == " while hook is blocked")
    }).await;
    assert!(
        !failed_path.exists(),
        "hook finished before sampling advanced"
    );

    fs::write(test.codex_home_path().join("capture_progress.release"), "")?;
    fs_wait::wait_for_path_exists(failed_path, Duration::from_secs(1)).await?;
    after_failure_tx
        .send(())
        .expect("release output after hook failure");
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::AgentMessageContentDelta(event) if event.delta == " after hook failure")
    }).await;

    test.codex.submit(Op::Interrupt).await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnAborted(_))
    })
    .await;
    sleep(Duration::from_secs(11)).await;
    assert_eq!(progress_inputs(test.codex_home_path())?, vec![first]);
    assert_eq!(server.requests().await.len(), 1);
    server.shutdown().await;
    Ok(())
}
