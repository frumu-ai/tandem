//! Notification fan-out for newly pending approvals.
//!
//! When a workflow run pauses on a `HumanApprovalGate`, surfaces (Slack,
//! Discord, Telegram, control-panel inbox) need to learn about it without
//! polling the engine themselves. This task closes the loop: it polls the
//! cross-subsystem aggregator (`/approvals/pending`, W1.5) on a short
//! interval, dedups against an in-memory set of `request_id`s already
//! announced, and dispatches each new request to the registered
//! [`ApprovalNotifier`] implementations.
//!
//! # Why polling, not the broadcast event bus
//!
//! The Plan agent flagged the existing event-bus pattern
//! (`workflows.rs:628`, `app/tasks.rs:392`) as wrong for approvals: those
//! subscribers drop on `tokio::sync::broadcast::error::RecvError::Lagged(_)`
//! and a missed approval means a stuck run. Polling the aggregator avoids
//! that failure mode entirely — the aggregator is an idempotent read of
//! durable state, so a slow notifier or a process restart cannot lose a
//! pending approval. We get correctness over latency (5s worst-case
//! notification delay vs. milliseconds), which is the right trade-off for
//! human approval flows.
//!
//! # Surface-side delivery
//!
//! The task itself is surface-agnostic: it accepts any `Vec<Arc<dyn
//! ApprovalNotifier>>`. The Slack/Discord/Telegram channel adapters supply
//! concrete notifiers that translate `ApprovalRequest` into the rich
//! interactive cards built in W2/W4. A future hosted-control-plane sidecar
//! can register its own notifier without touching this module.
//!
//! # Dedup
//!
//! The task maintains an in-memory `HashSet<String>` keyed by
//! `request_id`. When a request is decided (and therefore disappears from
//! the aggregator's pending list), the dedup set entry is pruned on the
//! next sweep so that the same `request_id` could in principle resurface
//! (it never should — `request_id`s are stable per gate — but the prune is
//! a safety net for misconfigured aggregators).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tandem_types::{ApprovalListFilter, ApprovalRequest};

/// Default polling interval. 5 seconds keeps human-approval latency tight
/// without overloading the aggregator (which is an in-memory walk).
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Hard cap on the in-memory dedup set so a runaway emit never grows
/// unboundedly. When the cap is hit, the oldest-inserted entries are
/// evicted via FIFO (we wrap a `VecDeque` next to the `HashSet`).
pub const DEDUP_CAP: usize = 8192;

/// Implemented by anything that wants to be notified of a new pending
/// approval. The notifier is responsible for surface-specific filtering
/// (e.g. only deliver to channels whose tenant matches the request) and for
/// retry/backoff against transient platform failures.
///
/// `Notifier::notify` MUST NOT block the fan-out task. Implementations
/// should spawn their own work or use a bounded internal queue.
#[async_trait]
pub trait ApprovalNotifier: Send + Sync {
    /// Stable name used for logging (`"slack"`, `"discord"`, etc.).
    fn name(&self) -> &str;

    /// Deliver a single approval request to this surface. Errors are logged
    /// by the fan-out task but do not stop the polling loop — a flaky
    /// channel must not delay other surfaces.
    async fn notify(&self, request: &ApprovalRequest) -> Result<(), NotifierError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifierError {
    /// Transient failure (rate limit, network blip). Caller logs, fan-out
    /// retries on the next sweep automatically because the request is
    /// still in the aggregator.
    Transient(String),
    /// Permanent failure (invalid card, channel not configured). The
    /// request will keep appearing in the aggregator until decided; the
    /// notifier should suppress its own retries to avoid log spam.
    Permanent(String),
}

impl core::fmt::Display for NotifierError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Transient(reason) => write!(f, "transient: {reason}"),
            Self::Permanent(reason) => write!(f, "permanent: {reason}"),
        }
    }
}

impl std::error::Error for NotifierError {}

/// Source of pending approvals. Used by tests to inject a mock aggregator
/// without spinning up the full HTTP stack. The production wiring uses the
/// `tandem-server` crate's `list_pending_approvals` directly.
#[async_trait]
pub trait PendingApprovalsSource: Send + Sync {
    async fn list_pending(&self, filter: &ApprovalListFilter) -> Vec<ApprovalRequest>;
}

/// In-memory dedup with FIFO eviction at the cap. Public for direct use in
/// tests; the fan-out task wraps it internally.
pub struct DedupRing {
    seen: HashSet<String>,
    order: std::collections::VecDeque<String>,
    cap: usize,
}

impl DedupRing {
    pub fn with_cap(cap: usize) -> Self {
        Self {
            seen: HashSet::with_capacity(cap.min(1024)),
            order: std::collections::VecDeque::with_capacity(cap.min(1024)),
            cap,
        }
    }

    /// `true` if the key is new (and now recorded). `false` if already seen.
    pub fn record_new(&mut self, key: &str) -> bool {
        if self.seen.contains(key) {
            return false;
        }
        if self.order.len() >= self.cap {
            if let Some(oldest) = self.order.pop_front() {
                self.seen.remove(&oldest);
            }
        }
        self.seen.insert(key.to_string());
        self.order.push_back(key.to_string());
        true
    }

    /// Drop entries whose request_id no longer appears in the latest
    /// aggregator snapshot — those approvals have been decided and the
    /// notifier should never see them again.
    pub fn prune_to(&mut self, current_request_ids: &HashSet<&str>) {
        let to_remove: Vec<String> = self
            .order
            .iter()
            .filter(|id| !current_request_ids.contains(id.as_str()))
            .cloned()
            .collect();
        for id in to_remove {
            self.seen.remove(&id);
            // O(N) drain; acceptable since order length is bounded.
            self.order.retain(|existing| existing != &id);
        }
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

/// One sweep of the fan-out: poll the aggregator, dispatch new requests to
/// every notifier, prune decided requests from the dedup ring.
///
/// Returned counts are the number of (a) new requests dispatched and (b)
/// total notifier calls attempted — useful for metrics + tests.
pub async fn run_one_sweep(
    source: &dyn PendingApprovalsSource,
    notifiers: &[Arc<dyn ApprovalNotifier>],
    filter: &ApprovalListFilter,
    dedup: &mut DedupRing,
) -> SweepResult {
    let pending = source.list_pending(filter).await;
    let current_ids: HashSet<&str> = pending.iter().map(|r| r.request_id.as_str()).collect();

    let mut new_count = 0usize;
    let mut notify_attempts = 0usize;
    let mut notify_failures = 0usize;

    for request in &pending {
        if !dedup.record_new(&request.request_id) {
            continue;
        }
        new_count += 1;
        for notifier in notifiers {
            notify_attempts += 1;
            match notifier.notify(request).await {
                Ok(()) => {}
                Err(error) => {
                    notify_failures += 1;
                    tracing::warn!(
                        target: "tandem_server::approval_outbound",
                        notifier = notifier.name(),
                        request_id = %request.request_id,
                        ?error,
                        "approval notifier returned an error"
                    );
                }
            }
        }
    }

    dedup.prune_to(&current_ids);

    SweepResult {
        pending_count: pending.len(),
        new_count,
        notify_attempts,
        notify_failures,
        dedup_size: dedup.len(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepResult {
    pub pending_count: usize,
    pub new_count: usize,
    pub notify_attempts: usize,
    pub notify_failures: usize,
    pub dedup_size: usize,
}

/// Long-running polling loop. Calls `run_one_sweep` every `interval`.
///
/// Dropping the returned `JoinHandle` does not stop the loop; cancellation
/// is intentional via the cooperative `cancel` flag (Arc<AtomicBool>) so
/// shutdown is deterministic without `tokio::task::abort`.
pub async fn run_polling_loop(
    source: Arc<dyn PendingApprovalsSource>,
    notifiers: Arc<Vec<Arc<dyn ApprovalNotifier>>>,
    filter: ApprovalListFilter,
    interval: Duration,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) {
    let mut dedup = DedupRing::with_cap(DEDUP_CAP);
    let mut tick = tokio::time::interval(interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            tracing::info!(
                target: "tandem_server::approval_outbound",
                "polling loop received cancel signal, exiting"
            );
            break;
        }
        let result = run_one_sweep(source.as_ref(), notifiers.as_ref(), &filter, &mut dedup).await;
        if result.new_count > 0 || result.notify_failures > 0 {
            tracing::info!(
                target: "tandem_server::approval_outbound",
                pending = result.pending_count,
                new = result.new_count,
                attempts = result.notify_attempts,
                failures = result.notify_failures,
                dedup = result.dedup_size,
                "approval fan-out sweep complete"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tandem_types::{ApprovalDecision, ApprovalSourceKind, ApprovalTenantRef};

    fn fake_request(request_id: &str) -> ApprovalRequest {
        ApprovalRequest {
            request_id: request_id.to_string(),
            source: ApprovalSourceKind::AutomationV2,
            tenant: ApprovalTenantRef {
                org_id: "local-default-org".to_string(),
                workspace_id: "local-default-workspace".to_string(),
                user_id: None,
            },
            run_id: format!("run-{request_id}"),
            node_id: Some("send_email".to_string()),
            workflow_name: Some("sales-research-outreach".to_string()),
            action_kind: Some("send_email".to_string()),
            action_preview_markdown: Some("Will email alice@example.com".to_string()),
            surface_payload: None,
            requested_at_ms: 1_700_000_000_000,
            expires_at_ms: None,
            decisions: vec![
                ApprovalDecision::Approve,
                ApprovalDecision::Rework,
                ApprovalDecision::Cancel,
            ],
            rework_targets: vec![],
            instructions: None,
            decided_by: None,
            decided_at_ms: None,
            decision: None,
            rework_feedback: None,
        }
    }

    struct CountingNotifier {
        name: &'static str,
        seen: Mutex<Vec<String>>,
        fail_with: Option<NotifierError>,
    }

    impl CountingNotifier {
        fn ok(name: &'static str) -> Arc<Self> {
            Arc::new(Self {
                name,
                seen: Mutex::new(Vec::new()),
                fail_with: None,
            })
        }
        fn failing(name: &'static str, error: NotifierError) -> Arc<Self> {
            Arc::new(Self {
                name,
                seen: Mutex::new(Vec::new()),
                fail_with: Some(error),
            })
        }
        fn seen_ids(&self) -> Vec<String> {
            self.seen.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ApprovalNotifier for CountingNotifier {
        fn name(&self) -> &str {
            self.name
        }
        async fn notify(&self, request: &ApprovalRequest) -> Result<(), NotifierError> {
            self.seen.lock().unwrap().push(request.request_id.clone());
            if let Some(err) = &self.fail_with {
                return Err(err.clone());
            }
            Ok(())
        }
    }

    struct VecSource {
        requests: Mutex<Vec<ApprovalRequest>>,
    }

    impl VecSource {
        fn new(initial: Vec<ApprovalRequest>) -> Arc<Self> {
            Arc::new(Self {
                requests: Mutex::new(initial),
            })
        }
        fn set(&self, requests: Vec<ApprovalRequest>) {
            *self.requests.lock().unwrap() = requests;
        }
    }

    #[async_trait]
    impl PendingApprovalsSource for VecSource {
        async fn list_pending(&self, _filter: &ApprovalListFilter) -> Vec<ApprovalRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    #[tokio::test]
    async fn first_sweep_dispatches_all_pending_to_every_notifier() {
        let source = VecSource::new(vec![fake_request("a"), fake_request("b")]);
        let n1 = CountingNotifier::ok("slack");
        let n2 = CountingNotifier::ok("discord");
        let notifiers: Vec<Arc<dyn ApprovalNotifier>> =
            vec![n1.clone() as Arc<dyn ApprovalNotifier>, n2.clone()];
        let mut dedup = DedupRing::with_cap(16);

        let result = run_one_sweep(
            source.as_ref(),
            &notifiers,
            &ApprovalListFilter::default(),
            &mut dedup,
        )
        .await;

        assert_eq!(result.pending_count, 2);
        assert_eq!(result.new_count, 2);
        assert_eq!(result.notify_attempts, 4);
        assert_eq!(result.notify_failures, 0);
        assert_eq!(n1.seen_ids(), vec!["a", "b"]);
        assert_eq!(n2.seen_ids(), vec!["a", "b"]);
    }

    #[tokio::test]
    async fn second_sweep_with_same_pending_does_not_redispatch() {
        let source = VecSource::new(vec![fake_request("a")]);
        let n1 = CountingNotifier::ok("slack");
        let notifiers: Vec<Arc<dyn ApprovalNotifier>> = vec![n1.clone()];
        let mut dedup = DedupRing::with_cap(16);

        let _ = run_one_sweep(
            source.as_ref(),
            &notifiers,
            &ApprovalListFilter::default(),
            &mut dedup,
        )
        .await;
        let second = run_one_sweep(
            source.as_ref(),
            &notifiers,
            &ApprovalListFilter::default(),
            &mut dedup,
        )
        .await;

        assert_eq!(second.new_count, 0);
        assert_eq!(second.notify_attempts, 0);
        assert_eq!(n1.seen_ids(), vec!["a"]);
    }

    #[tokio::test]
    async fn newly_added_pending_in_later_sweep_is_dispatched() {
        let source = VecSource::new(vec![fake_request("a")]);
        let n1 = CountingNotifier::ok("slack");
        let notifiers: Vec<Arc<dyn ApprovalNotifier>> = vec![n1.clone()];
        let mut dedup = DedupRing::with_cap(16);

        run_one_sweep(
            source.as_ref(),
            &notifiers,
            &ApprovalListFilter::default(),
            &mut dedup,
        )
        .await;
        source.set(vec![fake_request("a"), fake_request("b")]);
        let second = run_one_sweep(
            source.as_ref(),
            &notifiers,
            &ApprovalListFilter::default(),
            &mut dedup,
        )
        .await;

        assert_eq!(second.new_count, 1);
        assert_eq!(n1.seen_ids(), vec!["a", "b"]);
    }

    #[tokio::test]
    async fn decided_request_is_pruned_so_resurfacing_fires_again() {
        let source = VecSource::new(vec![fake_request("a")]);
        let n1 = CountingNotifier::ok("slack");
        let notifiers: Vec<Arc<dyn ApprovalNotifier>> = vec![n1.clone()];
        let mut dedup = DedupRing::with_cap(16);

        run_one_sweep(
            source.as_ref(),
            &notifiers,
            &ApprovalListFilter::default(),
            &mut dedup,
        )
        .await;
        // "a" was decided and disappears from pending.
        source.set(vec![]);
        let cleared = run_one_sweep(
            source.as_ref(),
            &notifiers,
            &ApprovalListFilter::default(),
            &mut dedup,
        )
        .await;
        assert_eq!(cleared.dedup_size, 0);
        assert!(dedup.is_empty());

        // If for some reason the same request_id appears again (it
        // shouldn't, but the system must not get stuck), it dispatches.
        source.set(vec![fake_request("a")]);
        let resurfaced = run_one_sweep(
            source.as_ref(),
            &notifiers,
            &ApprovalListFilter::default(),
            &mut dedup,
        )
        .await;
        assert_eq!(resurfaced.new_count, 1);
        assert_eq!(n1.seen_ids(), vec!["a", "a"]);
    }

    #[tokio::test]
    async fn failing_notifier_does_not_block_other_notifiers() {
        let source = VecSource::new(vec![fake_request("a")]);
        let bad = CountingNotifier::failing(
            "discord",
            NotifierError::Transient("rate limit".to_string()),
        );
        let good = CountingNotifier::ok("slack");
        let notifiers: Vec<Arc<dyn ApprovalNotifier>> = vec![bad.clone(), good.clone()];
        let mut dedup = DedupRing::with_cap(16);

        let result = run_one_sweep(
            source.as_ref(),
            &notifiers,
            &ApprovalListFilter::default(),
            &mut dedup,
        )
        .await;

        assert_eq!(result.notify_attempts, 2);
        assert_eq!(result.notify_failures, 1);
        assert_eq!(bad.seen_ids(), vec!["a"]);
        assert_eq!(good.seen_ids(), vec!["a"]);
    }

    #[tokio::test]
    async fn empty_pending_is_a_noop() {
        let source = VecSource::new(vec![]);
        let n1 = CountingNotifier::ok("slack");
        let notifiers: Vec<Arc<dyn ApprovalNotifier>> = vec![n1.clone()];
        let mut dedup = DedupRing::with_cap(16);

        let result = run_one_sweep(
            source.as_ref(),
            &notifiers,
            &ApprovalListFilter::default(),
            &mut dedup,
        )
        .await;

        assert_eq!(result.pending_count, 0);
        assert_eq!(result.new_count, 0);
        assert_eq!(result.notify_attempts, 0);
        assert!(n1.seen_ids().is_empty());
    }

    #[tokio::test]
    async fn dedup_evicts_at_cap() {
        let mut dedup = DedupRing::with_cap(3);
        assert!(dedup.record_new("a"));
        assert!(dedup.record_new("b"));
        assert!(dedup.record_new("c"));
        assert!(!dedup.record_new("a"));
        // Insert one more — oldest ("a") should be evicted.
        assert!(dedup.record_new("d"));
        // "a" is no longer remembered, so re-inserting is a new record.
        assert!(dedup.record_new("a"));
    }

    #[test]
    fn dedup_prune_to_removes_absent_entries() {
        let mut dedup = DedupRing::with_cap(8);
        dedup.record_new("a");
        dedup.record_new("b");
        dedup.record_new("c");
        let mut current = HashSet::new();
        current.insert("b");
        dedup.prune_to(&current);
        assert!(!dedup.record_new("b"), "b should still be deduped");
        assert!(dedup.record_new("a"), "a should be re-droppable");
        assert!(dedup.record_new("c"), "c should be re-droppable");
    }

    #[test]
    fn notifier_error_display_is_informative() {
        assert_eq!(
            format!("{}", NotifierError::Transient("rate".to_string())),
            "transient: rate"
        );
        assert_eq!(
            format!("{}", NotifierError::Permanent("misconfigured".to_string())),
            "permanent: misconfigured"
        );
    }
}
