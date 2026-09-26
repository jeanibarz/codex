//! Report real provider output without making model sampling wait for hooks.
//!
//! A single worker owns delivery and keeps only the latest pending observation.
//! Deadlines and cancellation also cover handlers configured as asynchronous.
//! No timer creates observations, and the wire timestamp always describes the
//! output's arrival, never the later delivery time.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::Hooks;
use crate::NotificationRequest;

// Ten seconds is comfortably inside the supervisor's thirty-second stale
// window. Limit each attempt to two seconds, including all handler cleanup.
const DELIVERY_INTERVAL: Duration = Duration::from_secs(10);
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_OBSERVATION_AGE: Duration = Duration::from_secs(10);

#[derive(Clone, Copy)]
struct Observation {
    at: Instant,
    wall_time_ms: i64,
}

/// Sampling-scoped progress delivery. Dropping it cancels pending hooks; `stop`
/// also waits for their cleanup before the caller reports the end of sampling.
pub struct ProviderProgressReporter {
    observations: watch::Sender<Option<Observation>>,
    worker: JoinHandle<()>,
}

impl ProviderProgressReporter {
    pub fn new(
        hooks: Arc<Hooks>,
        mut request: NotificationRequest,
        cancelled: impl Future<Output = ()> + Send + 'static,
    ) -> Self {
        request.notification_type = "provider_progress".to_string();
        request.message = "Provider output received".to_string();
        Self::spawn(cancelled, move |observation| {
            let hooks = Arc::clone(&hooks);
            let mut request = request.clone();
            request.observed_at_ms = Some(observation.wall_time_ms);
            async move {
                // Deliberately omit HookStarted/HookCompleted events: persisting
                // handler activity would make slow hooks look like model output.
                hooks.run_notification(request).await;
            }
        })
    }

    fn spawn<F, D>(cancelled: impl Future<Output = ()> + Send + 'static, deliver: D) -> Self
    where
        F: Future<Output = ()> + Send,
        D: Fn(Observation) -> F + Send + 'static,
    {
        let (observations, mut pending) = watch::channel::<Option<Observation>>(None);
        let worker = tokio::spawn(async move {
            let delivery = async move {
                let mut next_delivery = Instant::now();
                while pending.changed().await.is_ok() {
                    tokio::time::sleep_until(next_delivery).await;
                    let Some(observation) = *pending.borrow_and_update() else {
                        continue;
                    };
                    if observation.at.elapsed() >= MAX_OBSERVATION_AGE {
                        continue;
                    }
                    next_delivery = Instant::now() + DELIVERY_INTERVAL;
                    let _ = tokio::time::timeout(DELIVERY_TIMEOUT, deliver(observation)).await;
                }
            };
            tokio::select! {
                biased;
                _ = cancelled => {}
                _ = delivery => {}
            }
        });
        Self {
            observations,
            worker,
        }
    }

    /// Call only for provider output deltas. The contents are never retained or
    /// sent to a hook; empty deltas and transport keepalives do not count.
    pub fn observe_delta(&self, delta: &str) {
        if !delta.is_empty() {
            self.observations.send_replace(Some(Observation {
                at: Instant::now(),
                wall_time_ms: chrono::Utc::now().timestamp_millis(),
            }));
        }
    }

    pub async fn stop(mut self) {
        self.worker.abort();
        let _ = (&mut self.worker).await;
    }
}

impl Drop for ProviderProgressReporter {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

#[cfg(test)]
#[path = "provider_progress_tests.rs"]
mod tests;
