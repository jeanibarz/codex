use super::*;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use pretty_assertions::assert_eq;

async fn tick(duration: Duration) {
    tokio::time::advance(duration).await;
    tokio::task::yield_now().await;
}

#[tokio::test(start_paused = true)]
async fn four_minutes_of_output_are_throttled_and_silence_does_not_create_progress() {
    let delivered = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&delivered);
    let reporter = ProviderProgressReporter::spawn(std::future::pending(), move |observation| {
        seen.lock().unwrap().push(observation);
        std::future::ready(())
    });
    for _ in 0..250 {
        reporter.observe_delta("synthetic private input must never leave the observer");
        tick(Duration::from_secs(1)).await;
    }
    let observations = delivered.lock().unwrap().clone();
    assert!((24..=26).contains(&observations.len()));
    assert!(observations.windows(2).all(|pair| {
        pair[1].at.duration_since(pair[0].at) >= DELIVERY_INTERVAL - Duration::from_secs(1)
    }));
    // A coalesced final observation may still be delivered, but no repeated
    // timer tick is allowed to invent another one during actual silence.
    tick(DELIVERY_INTERVAL).await;
    let count = delivered.lock().unwrap().len();
    for _ in 0..120 {
        reporter.observe_delta("");
        tick(Duration::from_secs(1)).await;
    }
    assert_eq!(delivered.lock().unwrap().len(), count);
    reporter.stop().await;
}

#[tokio::test(start_paused = true)]
async fn coalesces_to_latest_observation_and_preserves_its_timestamp() {
    let delivered = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&delivered);
    let reporter = ProviderProgressReporter::spawn(std::future::pending(), move |observation| {
        seen.lock().unwrap().push(observation.wall_time_ms);
        std::future::ready(())
    });
    reporter.observe_delta("first");
    tick(Duration::from_millis(1)).await;
    tick(Duration::from_secs(5)).await;
    for wall_time_ms in 1..=10_000 {
        reporter.observations.send_replace(Some(Observation {
            at: Instant::now(),
            wall_time_ms,
        }));
    }
    tick(Duration::from_secs(5)).await;
    assert_eq!(delivered.lock().unwrap().len(), 2);
    assert_eq!(delivered.lock().unwrap()[1], 10_000);
    reporter.stop().await;
}

#[cfg(unix)]
struct NoMcp;

#[cfg(unix)]
impl crate::HookMcpExecutor for NoMcp {
    fn execute(
        &self,
        _: crate::HookMcpCall,
    ) -> futures::future::BoxFuture<'_, anyhow::Result<String>> {
        Box::pin(async { anyhow::bail!("unexpected MCP hook") })
    }
}

#[cfg(unix)]
fn command_hooks(
    directory: &std::path::Path,
    command: String,
    asynchronous: bool,
    session_id: codex_protocol::ThreadId,
    turn_id: &str,
) -> (
    Arc<Hooks>,
    NotificationRequest,
    async_channel::Receiver<codex_protocol::protocol::HookCompletedEvent>,
) {
    let settings = directory.join(format!("{turn_id}.json"));
    std::fs::write(
        &settings,
        serde_json::to_vec(&serde_json::json!({
            "permissions": {},
            "hooks": { "Notification": [{ "hooks": [{
                "type": "command", "command": command, "async": asynchronous, "timeout": 600
            }] }] }
        }))
        .unwrap(),
    )
    .unwrap();
    let (hooks, completions) = Hooks::new(
        crate::HooksConfig {
            feature_enabled: true,
            bypass_hook_trust: true,
            settings_file: Some(settings),
            shell_program: Some("/bin/sh".to_string()),
            shell_args: vec!["-c".to_string()],
            ..Default::default()
        },
        session_id,
        Arc::new(NoMcp),
    )
    .unwrap();
    let request = NotificationRequest {
        session_id,
        turn_id: turn_id.to_string(),
        cwd: directory.to_path_buf(),
        transcript_path: None,
        model: "test-model".to_string(),
        notification_type: "provider_progress".to_string(),
        message: String::new(),
        observed_at_ms: None,
    };
    (Arc::new(hooks), request, completions)
}

#[cfg(unix)]
async fn wait_for_payload(path: &std::path::Path) -> serde_json::Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(bytes) = tokio::fs::read(path).await
                && let Ok(payload) = serde_json::from_slice(&bytes)
            {
                break payload;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("hook payload")
}

#[cfg(unix)]
#[tokio::test]
async fn async_command_handlers_cannot_escape_deadline_or_emit_late_progress() {
    let directory = tempfile::tempdir().unwrap();
    let capture = directory.path().join("capture.json");
    let late = directory.path().join("late");
    let (hooks, request, completions) = command_hooks(
        directory.path(),
        format!(
            "cat > '{}'; sleep 3; printf late > '{}'",
            capture.display(),
            late.display()
        ),
        /*asynchronous*/ true,
        codex_protocol::ThreadId::new(),
        "slow",
    );
    let reporter = ProviderProgressReporter::new(hooks, request, std::future::pending());
    reporter.observe_delta("private model output");
    let payload = wait_for_payload(&capture).await;
    assert_eq!(payload["notification_type"], "provider_progress");
    assert_eq!(payload["message"], "Provider output received");
    assert!(payload["observed_at_ms"].as_i64().unwrap() > 0);
    assert!(!payload.to_string().contains("private model output"));
    tokio::time::sleep(Duration::from_millis(3300)).await;
    assert!(!late.exists(), "timed out handler left a live descendant");
    assert!(
        completions.is_empty(),
        "progress must not generate transcript activity"
    );
    reporter.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn concurrent_sessions_keep_their_identity_even_when_a_handler_fails() {
    let directory = tempfile::tempdir().unwrap();
    let parent_path = directory.path().join("parent-output.json");
    let child_path = directory.path().join("child-output.json");
    let parent_id = codex_protocol::ThreadId::new();
    let child_id = codex_protocol::ThreadId::new();
    let (parent_hooks, parent_request, parent_completions) = command_hooks(
        directory.path(),
        format!("cat > '{}'; exit 1", parent_path.display()),
        /*asynchronous*/ false,
        parent_id,
        "parent-turn",
    );
    let (child_hooks, child_request, child_completions) = command_hooks(
        directory.path(),
        format!("cat > '{}'", child_path.display()),
        /*asynchronous*/ true,
        child_id,
        "child-turn",
    );
    let parent =
        ProviderProgressReporter::new(parent_hooks, parent_request, std::future::pending());
    let child = ProviderProgressReporter::new(child_hooks, child_request, std::future::pending());
    parent.observe_delta("parent secret");
    child.observe_delta("child secret");
    let parent_payload = wait_for_payload(&parent_path).await;
    let child_payload = wait_for_payload(&child_path).await;
    assert_eq!(parent_payload["session_id"], parent_id.to_string());
    assert_eq!(parent_payload["turn_id"], "parent-turn");
    assert_eq!(child_payload["session_id"], child_id.to_string());
    assert_eq!(child_payload["turn_id"], "child-turn");
    assert!(parent_completions.is_empty());
    assert!(child_completions.is_empty());
    parent.stop().await;
    child.stop().await;
}

#[tokio::test]
async fn progress_skips_uncancellable_mcp_but_ordinary_notifications_keep_it() {
    struct CountingMcp(Arc<AtomicUsize>);
    impl crate::HookMcpExecutor for CountingMcp {
        fn execute(
            &self,
            _: crate::HookMcpCall,
        ) -> futures::future::BoxFuture<'_, anyhow::Result<String>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(String::new()) })
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let settings = directory.path().join("settings.json");
    std::fs::write(
        &settings,
        serde_json::json!({"hooks": {"Notification": [{"hooks": [{
            "type": "mcp_tool", "server": "test", "tool": "notify", "input": {}
        }]}]}})
        .to_string(),
    )
    .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let session_id = codex_protocol::ThreadId::new();
    let (hooks, _) = Hooks::new(
        crate::HooksConfig {
            feature_enabled: true,
            bypass_hook_trust: true,
            settings_file: Some(settings),
            ..Default::default()
        },
        session_id,
        Arc::new(CountingMcp(Arc::clone(&calls))),
    )
    .unwrap();
    let mut request = NotificationRequest {
        session_id,
        turn_id: "turn-1".to_string(),
        cwd: directory.path().to_path_buf(),
        transcript_path: None,
        model: "test".to_string(),
        notification_type: "provider_progress".to_string(),
        message: "Provider output received".to_string(),
        observed_at_ms: Some(1),
    };
    assert_eq!(hooks.preview_notification(&request).len(), 1);
    hooks.run_notification(request.clone()).await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    request.notification_type = "ordinary_notification".to_string();
    request.observed_at_ms = None;
    hooks.run_notification(request).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

struct InFlight(Arc<AtomicUsize>);

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[tokio::test(start_paused = true)]
async fn slow_delivery_is_bounded_and_expired_pending_work_is_discarded() {
    let active = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(AtomicUsize::new(0));
    let delivery_active = Arc::clone(&active);
    let delivery_started = Arc::clone(&started);
    let reporter = ProviderProgressReporter::spawn(std::future::pending(), move |_| {
        assert_eq!(delivery_active.fetch_add(1, Ordering::SeqCst), 0);
        delivery_started.fetch_add(1, Ordering::SeqCst);
        let guard = InFlight(Arc::clone(&delivery_active));
        async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        }
    });
    reporter.observe_delta("first");
    tick(Duration::from_millis(1)).await;
    assert_eq!(active.load(Ordering::SeqCst), 1);
    // Observation is synchronous and has only one pending slot even when a
    // handler never completes. These updates cannot wait for its stdin/output.
    for _ in 0..10_000 {
        reporter.observe_delta("more");
    }
    tick(DELIVERY_TIMEOUT).await;
    assert_eq!(active.load(Ordering::SeqCst), 0);
    // Simulate the executor not scheduling the worker until the observation is
    // too old. Starting that old work must not freshen a silent provider.
    tick(Duration::from_secs(60)).await;
    assert_eq!(started.load(Ordering::SeqCst), 1);
    reporter.observe_delta("new");
    tick(Duration::from_millis(1)).await;
    assert_eq!(started.load(Ordering::SeqCst), 2);
    reporter.stop().await;
    assert_eq!(active.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn cancellation_and_completion_discard_in_flight_and_pending_delivery() {
    for cancel_turn in [true, false] {
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
        let active = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(AtomicUsize::new(0));
        let seen_active = Arc::clone(&active);
        let seen_started = Arc::clone(&started);
        let reporter = ProviderProgressReporter::spawn(
            async {
                let _ = cancel_rx.await;
            },
            move |_| {
                seen_started.fetch_add(1, Ordering::SeqCst);
                seen_active.fetch_add(1, Ordering::SeqCst);
                let guard = InFlight(Arc::clone(&seen_active));
                async move {
                    let _guard = guard;
                    std::future::pending::<()>().await;
                }
            },
        );
        reporter.observe_delta("first");
        tick(Duration::from_millis(1)).await;
        reporter.observe_delta("pending");
        if cancel_turn {
            cancel_tx.send(()).unwrap();
            tokio::task::yield_now().await;
        } else {
            reporter.stop().await;
        }
        tick(Duration::from_secs(120)).await;
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(started.load(Ordering::SeqCst), 1);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn only_progress_commands_reap_background_children_after_successful_exit() {
    for notification_type in ["provider_progress", "ordinary_notification"] {
        let directory = tempfile::tempdir().unwrap();
        let capture = directory.path().join("capture.json");
        let late = directory.path().join("late");
        let (hooks, mut request, _) = command_hooks(
            directory.path(),
            format!(
                "cat > '{}'; (sleep 1; printf late > '{}') </dev/null >/dev/null 2>&1 &",
                capture.display(),
                late.display()
            ),
            /*asynchronous*/ false,
            codex_protocol::ThreadId::new(),
            "background-child",
        );
        request.notification_type = notification_type.to_string();
        // Returning means the root shell exited. Its redirected background
        // child must remain owned for progress, even on this success path.
        hooks.run_notification(request).await;
        assert!(capture.exists(), "the command never consumed its payload");
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert_eq!(
            late.exists(),
            notification_type == "ordinary_notification",
            "only ordinary notifications may leave background children running"
        );
    }
}
