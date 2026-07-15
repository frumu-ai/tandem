// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(crate) struct SlackEventTaskRuntime {
    cancellation: CancellationToken,
    tasks: Arc<Mutex<JoinSet<()>>>,
    recovery_started: Arc<AtomicBool>,
}

impl Default for SlackEventTaskRuntime {
    fn default() -> Self {
        Self {
            cancellation: CancellationToken::new(),
            tasks: Arc::new(Mutex::new(JoinSet::new())),
            recovery_started: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl SlackEventTaskRuntime {
    pub(crate) async fn spawn<F, Fut>(&self, build: F) -> anyhow::Result<()>
    where
        F: FnOnce(CancellationToken) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        if self.cancellation.is_cancelled() {
            anyhow::bail!("Slack event runtime is shutting down");
        }
        let mut tasks = self.tasks.lock().await;
        if self.cancellation.is_cancelled() {
            anyhow::bail!("Slack event runtime is shutting down");
        }
        while tasks.try_join_next().is_some() {}
        let child = self.cancellation.child_token();
        tasks.spawn(build(child));
        Ok(())
    }

    pub(crate) async fn start_recovery_worker<F, Fut>(&self, build: F) -> anyhow::Result<bool>
    where
        F: FnOnce(CancellationToken) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        if self
            .recovery_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(false);
        }
        let started = self.recovery_started.clone();
        if let Err(error) = self
            .spawn(move |cancel| async move {
                build(cancel).await;
                started.store(false, Ordering::Release);
            })
            .await
        {
            self.recovery_started.store(false, Ordering::Release);
            return Err(error);
        }
        Ok(true)
    }

    pub(crate) async fn shutdown(&self, timeout: Duration) {
        self.cancellation.cancel();
        let mut tasks = self.tasks.lock().await;
        let drained = tokio::time::timeout(timeout, async {
            while let Some(result) = tasks.join_next().await {
                if let Err(error) = result {
                    tracing::warn!(%error, "tracked Slack event task failed during shutdown");
                }
            }
        })
        .await
        .is_ok();
        if !drained {
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
        }
    }

    #[cfg(test)]
    pub(crate) async fn active_count(&self) -> usize {
        let mut tasks = self.tasks.lock().await;
        while tasks.try_join_next().is_some() {}
        tasks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_cancels_and_drains_tracked_tasks() {
        let runtime = SlackEventTaskRuntime::default();
        let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let task_finished = finished.clone();
        runtime
            .spawn(move |cancel| async move {
                cancel.cancelled().await;
                task_finished.store(true, std::sync::atomic::Ordering::SeqCst);
            })
            .await
            .unwrap();
        assert_eq!(runtime.active_count().await, 1);

        runtime.shutdown(Duration::from_secs(1)).await;

        assert!(finished.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(runtime.active_count().await, 0);
        assert!(runtime.spawn(|_| async {}).await.is_err());
    }

    #[tokio::test]
    async fn recovery_worker_is_singleton_and_tracked_through_shutdown() {
        let runtime = SlackEventTaskRuntime::default();
        assert!(runtime
            .start_recovery_worker(|cancel| async move { cancel.cancelled().await })
            .await
            .unwrap());
        assert!(!runtime.start_recovery_worker(|_| async {}).await.unwrap());
        assert_eq!(runtime.active_count().await, 1);

        runtime.shutdown(Duration::from_secs(1)).await;
        assert_eq!(runtime.active_count().await, 0);
    }
}
