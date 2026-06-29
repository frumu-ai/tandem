use serde::{Deserialize, Serialize};
use serde_json::json;
use tandem_types::TenantContext;

use super::phases::phase_state_from_status;
use super::store::{
    append_stateful_run_event_once, load_stateful_run_events, query_stateful_run_events,
    write_stateful_run_snapshot, StatefulRunEventQuery, StatefulRuntimeStoragePaths,
};
use super::types::{
    StatefulRunEventRecord, StatefulRunSnapshotRecord, StatefulWaitRecord, StatefulWaitStatus,
    StatefulWaitTimeoutAction, StatefulWorkflowRunStatus,
};
use super::waits::{
    claim_due_stateful_wait, due_stateful_waits, mark_stateful_wait_timeout_result,
    mark_stateful_wait_woken,
};

pub const STATEFUL_WAIT_SCHEDULER_CLAIMANT: &str = "stateful-wait-scheduler";
pub const DEFAULT_STATEFUL_WAIT_SCHEDULER_LEASE_MS: u64 = 60_000;
pub const DEFAULT_STATEFUL_WAIT_SCHEDULER_LIMIT: usize = 100;

#[derive(Debug, Clone)]
pub struct StatefulWaitSchedulerConfig {
    pub claimant_id: String,
    pub lease_ms: u64,
    pub limit: usize,
}

impl Default for StatefulWaitSchedulerConfig {
    fn default() -> Self {
        Self {
            claimant_id: format!(
                "tandem-server:{STATEFUL_WAIT_SCHEDULER_CLAIMANT}:{}",
                std::process::id()
            ),
            lease_ms: DEFAULT_STATEFUL_WAIT_SCHEDULER_LEASE_MS,
            limit: DEFAULT_STATEFUL_WAIT_SCHEDULER_LIMIT,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatefulWaitSchedulerOutcome {
    pub run_id: String,
    pub wait_id: String,
    pub tenant_context: TenantContext,
    pub event_type: String,
    pub event_seq: u64,
    pub wait_status: StatefulWaitStatus,
    pub run_status: StatefulWorkflowRunStatus,
    pub lag_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct StatefulWaitSchedulerTick {
    pub checked: usize,
    pub claimed: usize,
    pub completed: usize,
    pub failed: usize,
    pub max_lag_ms: u64,
    pub outcomes: Vec<StatefulWaitSchedulerOutcome>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
enum SchedulerAction {
    WakeTimer {
        due_at_ms: u64,
    },
    Timeout {
        due_at_ms: u64,
        timeout_action: StatefulWaitTimeoutAction,
    },
}

impl SchedulerAction {
    fn due_at_ms(&self) -> u64 {
        match self {
            Self::WakeTimer { due_at_ms } | Self::Timeout { due_at_ms, .. } => *due_at_ms,
        }
    }

    fn event_type(&self) -> &'static str {
        match self {
            Self::WakeTimer { .. } => "stateful_runtime.wait.timer_woken",
            Self::Timeout {
                timeout_action: StatefulWaitTimeoutAction::Cancel,
                ..
            } => "stateful_runtime.wait.timeout_cancelled",
            Self::Timeout {
                timeout_action: StatefulWaitTimeoutAction::Escalate,
                ..
            } => "stateful_runtime.wait.timeout_escalated",
            Self::Timeout {
                timeout_action: StatefulWaitTimeoutAction::Remind,
                ..
            } => "stateful_runtime.wait.timeout_reminded",
            Self::Timeout {
                timeout_action: StatefulWaitTimeoutAction::Resume,
                ..
            } => "stateful_runtime.wait.timeout_resumed",
        }
    }

    fn wait_status(&self) -> StatefulWaitStatus {
        match self {
            Self::WakeTimer { .. } => StatefulWaitStatus::Woken,
            Self::Timeout {
                timeout_action: StatefulWaitTimeoutAction::Cancel,
                ..
            } => StatefulWaitStatus::Cancelled,
            Self::Timeout {
                timeout_action: StatefulWaitTimeoutAction::Escalate,
                ..
            } => StatefulWaitStatus::Escalated,
            Self::Timeout {
                timeout_action: StatefulWaitTimeoutAction::Remind,
                ..
            } => StatefulWaitStatus::TimedOut,
            Self::Timeout {
                timeout_action: StatefulWaitTimeoutAction::Resume,
                ..
            } => StatefulWaitStatus::Woken,
        }
    }

    fn run_status(&self) -> StatefulWorkflowRunStatus {
        match self {
            Self::WakeTimer { .. }
            | Self::Timeout {
                timeout_action: StatefulWaitTimeoutAction::Resume,
                ..
            } => StatefulWorkflowRunStatus::Running,
            Self::Timeout {
                timeout_action: StatefulWaitTimeoutAction::Cancel,
                ..
            } => StatefulWorkflowRunStatus::Cancelled,
            Self::Timeout { .. } => StatefulWorkflowRunStatus::Paused,
        }
    }

    fn completion_key(&self, wait: &StatefulWaitRecord) -> String {
        match self {
            Self::WakeTimer { due_at_ms } => {
                format!("timer:{}:{}:{due_at_ms}", wait.run_id, wait.wait_id)
            }
            Self::Timeout {
                due_at_ms,
                timeout_action,
            } => format!(
                "timeout:{timeout_action:?}:{}:{}:{due_at_ms}",
                wait.run_id, wait.wait_id
            ),
        }
    }
}

pub async fn process_due_stateful_waits(
    paths: &StatefulRuntimeStoragePaths,
    now_ms: u64,
    config: StatefulWaitSchedulerConfig,
) -> StatefulWaitSchedulerTick {
    let tenant = TenantContext::local_implicit();
    let candidates = due_stateful_waits(&paths.waits_path, &tenant, now_ms, Some(config.limit));
    let mut tick = StatefulWaitSchedulerTick {
        checked: candidates.len(),
        ..StatefulWaitSchedulerTick::default()
    };

    for candidate in candidates {
        let Some(action) = scheduler_action(&candidate, now_ms) else {
            continue;
        };
        let tenant_context = candidate.scope.tenant_context.clone();
        let claimed = match claim_due_stateful_wait(
            &paths.waits_path,
            &tenant_context,
            &candidate.run_id,
            &candidate.wait_id,
            &config.claimant_id,
            now_ms,
            config.lease_ms,
        )
        .await
        {
            Ok(Some(claimed)) => claimed,
            Ok(None) => continue,
            Err(error) => {
                tick.failed += 1;
                tick.errors.push(format!(
                    "failed to claim wait {} for run {}: {error}",
                    candidate.wait_id, candidate.run_id
                ));
                continue;
            }
        };
        tick.claimed += 1;
        match complete_claimed_wait(paths, &claimed, &action, now_ms).await {
            Ok(outcome) => {
                tick.max_lag_ms = tick.max_lag_ms.max(outcome.lag_ms);
                tick.completed += 1;
                tick.outcomes.push(outcome);
            }
            Err(error) => {
                tick.failed += 1;
                tick.errors.push(format!(
                    "failed to complete wait {} for run {}: {error}",
                    claimed.wait_id, claimed.run_id
                ));
            }
        }
    }

    tick
}

async fn complete_claimed_wait(
    paths: &StatefulRuntimeStoragePaths,
    wait: &StatefulWaitRecord,
    action: &SchedulerAction,
    now_ms: u64,
) -> anyhow::Result<StatefulWaitSchedulerOutcome> {
    let completion_key = action.completion_key(wait);
    let event_id = format!("stateful-wait-{completion_key}");
    let mut seq = next_stateful_run_event_seq(
        &paths.run_events_path,
        &wait.scope.tenant_context,
        &wait.run_id,
    );
    let lag_ms = now_ms.saturating_sub(action.due_at_ms());
    let event = StatefulRunEventRecord {
        schema_version: 1,
        event_id: event_id.clone(),
        run_id: wait.run_id.clone(),
        seq,
        event_type: action.event_type().to_string(),
        occurred_at_ms: now_ms,
        scope: wait.scope.clone(),
        actor: None,
        phase_id: wait.phase_id.clone(),
        phase_transition: None,
        wait_kind: Some(wait.wait_kind.clone()),
        causation_id: Some(wait.wait_id.clone()),
        correlation_id: Some(completion_key.clone()),
        payload: json!({
            "wait_id": &wait.wait_id,
            "wait_kind": &wait.wait_kind,
            "wake_at_ms": wait.wake_at_ms,
            "timeout_policy": &wait.timeout_policy,
            "completion_key": completion_key,
            "lag_ms": lag_ms,
            "scheduler": STATEFUL_WAIT_SCHEDULER_CLAIMANT,
        }),
    };
    if !append_stateful_run_event_once(&paths.run_events_path, &event).await? {
        if let Some(existing_seq) = stateful_run_event_seq_by_id(
            &paths.run_events_path,
            &wait.scope.tenant_context,
            &wait.run_id,
            &event_id,
        ) {
            seq = existing_seq;
        }
    }

    let run_status = action.run_status();
    let phase_state =
        phase_state_from_status(&wait.run_id, &run_status, now_ms, wait.phase_id.as_deref());
    let snapshot = StatefulRunSnapshotRecord {
        schema_version: 1,
        snapshot_id: event_id,
        run_id: wait.run_id.clone(),
        seq,
        created_at_ms: now_ms,
        scope: wait.scope.clone(),
        status: run_status.clone(),
        phase: phase_state.phase,
        phase_history: phase_state.phase_history,
        allowed_next_phases: phase_state.allowed_next_phases,
        phase_id: wait.phase_id.clone(),
        source_record_kind: None,
        checkpoint: None,
        payload_digest: None,
        workflow_definition_version: None,
        workflow_definition_snapshot_hash: None,
        metadata: Some(json!({
            "source": "stateful_wait_scheduler",
            "event_type": action.event_type(),
            "wait_id": &wait.wait_id,
            "wait_kind": &wait.wait_kind,
            "lag_ms": lag_ms,
        })),
    };
    write_stateful_run_snapshot(&paths.snapshots_root, &snapshot).await?;

    let wait_status = action.wait_status();
    let completed = if wait_status == StatefulWaitStatus::Woken {
        mark_stateful_wait_woken(
            &paths.waits_path,
            &wait.scope.tenant_context,
            &wait.run_id,
            &wait.wait_id,
            &completion_key,
            seq,
            now_ms,
        )
        .await?
    } else {
        mark_stateful_wait_timeout_result(
            &paths.waits_path,
            &wait.scope.tenant_context,
            &wait.run_id,
            &wait.wait_id,
            &completion_key,
            seq,
            wait_status.clone(),
            now_ms,
        )
        .await?
    }
    .ok_or_else(|| anyhow::anyhow!("stateful wait completion conflict"))?;

    Ok(StatefulWaitSchedulerOutcome {
        run_id: completed.run_id,
        wait_id: completed.wait_id,
        tenant_context: completed.scope.tenant_context,
        event_type: action.event_type().to_string(),
        event_seq: seq,
        wait_status,
        run_status,
        lag_ms,
    })
}

fn scheduler_action(wait: &StatefulWaitRecord, now_ms: u64) -> Option<SchedulerAction> {
    let wake_due = wait
        .wake_at_ms
        .filter(|wake_at_ms| *wake_at_ms <= now_ms)
        .map(|due_at_ms| SchedulerAction::WakeTimer { due_at_ms });
    let timeout_due = wait
        .timeout_policy
        .as_ref()
        .filter(|policy| policy.timeout_at_ms <= now_ms)
        .map(|policy| SchedulerAction::Timeout {
            due_at_ms: policy.timeout_at_ms,
            timeout_action: policy.on_timeout.clone(),
        });

    match (wake_due, timeout_due) {
        (Some(wake), Some(timeout)) if timeout.due_at_ms() <= wake.due_at_ms() => Some(timeout),
        (Some(wake), _) => Some(wake),
        (None, Some(timeout)) => Some(timeout),
        (None, None) => None,
    }
}

fn next_stateful_run_event_seq(
    path: &std::path::Path,
    tenant_context: &TenantContext,
    run_id: &str,
) -> u64 {
    query_stateful_run_events(
        path,
        tenant_context,
        StatefulRunEventQuery {
            run_id,
            after_seq: None,
            limit: None,
        },
    )
    .last()
    .map(|event| event.seq.saturating_add(1))
    .unwrap_or(1)
}

fn stateful_run_event_seq_by_id(
    path: &std::path::Path,
    tenant_context: &TenantContext,
    run_id: &str,
    event_id: &str,
) -> Option<u64> {
    query_stateful_run_events(
        path,
        tenant_context,
        StatefulRunEventQuery {
            run_id,
            after_seq: None,
            limit: None,
        },
    )
    .into_iter()
    .find(|event| event.event_id == event_id)
    .map(|event| event.seq)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tandem_types::TenantContext;
    use uuid::Uuid;

    use super::*;
    use crate::stateful_runtime::{
        list_stateful_run_snapshots, list_stateful_waits, upsert_stateful_wait,
        StatefulRuntimeScope, StatefulWaitKind, StatefulWaitQuery, StatefulWaitTimeoutAction,
        StatefulWaitTimeoutPolicy,
    };

    fn tenant(org: &str, workspace: &str) -> TenantContext {
        TenantContext::explicit_user_workspace(org, workspace, None, "user-a")
    }

    fn paths(name: &str) -> StatefulRuntimeStoragePaths {
        let root = std::env::temp_dir().join(format!("{name}-{}", Uuid::new_v4()));
        StatefulRuntimeStoragePaths::new(
            root.join("stateful_events.jsonl"),
            root.join("stateful_snapshots"),
            root.join("stateful_waits.json"),
        )
    }

    fn timer_wait(wait_id: &str, wake_at_ms: u64) -> StatefulWaitRecord {
        StatefulWaitRecord {
            schema_version: 1,
            wait_id: wait_id.to_string(),
            run_id: "run-a".to_string(),
            wait_kind: StatefulWaitKind::Timer,
            status: StatefulWaitStatus::Waiting,
            scope: StatefulRuntimeScope::from_tenant_context(tenant("org-a", "workspace-a")),
            phase_id: Some("phase-a".to_string()),
            reason: Some("sleep until durable wake".to_string()),
            created_at_ms: wake_at_ms.saturating_sub(100),
            updated_at_ms: wake_at_ms.saturating_sub(100),
            wake_at_ms: Some(wake_at_ms),
            timeout_policy: None,
            event_seq: None,
            wake_idempotency_key: None,
            claimed_by: None,
            claimed_at_ms: None,
            claim_expires_at_ms: None,
            completed_at_ms: None,
            metadata: None,
        }
    }

    fn timeout_wait(
        wait_id: &str,
        timeout_at_ms: u64,
        timeout_action: StatefulWaitTimeoutAction,
    ) -> StatefulWaitRecord {
        StatefulWaitRecord {
            wake_at_ms: None,
            timeout_policy: Some(StatefulWaitTimeoutPolicy {
                timeout_at_ms,
                on_timeout: timeout_action,
                escalate_to: Some("ops".to_string()),
                remind_every_ms: None,
                metadata: Some(json!({ "source": "test" })),
            }),
            wait_kind: StatefulWaitKind::Approval,
            ..timer_wait(wait_id, timeout_at_ms.saturating_add(10_000))
        }
    }

    #[tokio::test]
    async fn scheduler_wakes_due_timer_wait_once() {
        let paths = paths("stateful-wait-scheduler-timer");
        let tenant = tenant("org-a", "workspace-a");
        upsert_stateful_wait(&paths.waits_path, timer_wait("wait-a", 1_000))
            .await
            .expect("insert wait");

        let tick = process_due_stateful_waits(
            &paths,
            1_250,
            StatefulWaitSchedulerConfig {
                claimant_id: "scheduler-test".to_string(),
                lease_ms: 500,
                limit: 10,
            },
        )
        .await;

        assert_eq!(tick.checked, 1);
        assert_eq!(tick.claimed, 1);
        assert_eq!(tick.completed, 1);
        assert_eq!(tick.failed, 0);
        assert_eq!(
            tick.outcomes[0].event_type,
            "stateful_runtime.wait.timer_woken"
        );
        assert_eq!(tick.outcomes[0].lag_ms, 250);

        let waits = list_stateful_waits(
            &paths.waits_path,
            &tenant,
            StatefulWaitQuery {
                run_id: Some("run-a"),
                ..StatefulWaitQuery::default()
            },
        );
        assert_eq!(waits[0].status, StatefulWaitStatus::Woken);
        assert_eq!(load_stateful_run_events(&paths.run_events_path).len(), 1);
        assert_eq!(
            list_stateful_run_snapshots(&paths.snapshots_root, &tenant, "run-a", None).len(),
            1
        );

        let duplicate = process_due_stateful_waits(&paths, 2_000, Default::default()).await;
        assert_eq!(duplicate.checked, 0);
        assert_eq!(load_stateful_run_events(&paths.run_events_path).len(), 1);
        let _ = tokio::fs::remove_dir_all(
            paths
                .run_events_path
                .parent()
                .expect("runtime root")
                .to_path_buf(),
        )
        .await;
    }

    #[tokio::test]
    async fn scheduler_applies_timeout_action_before_later_wake() {
        let paths = paths("stateful-wait-scheduler-timeout");
        let tenant = tenant("org-a", "workspace-a");
        upsert_stateful_wait(
            &paths.waits_path,
            timeout_wait("wait-a", 1_000, StatefulWaitTimeoutAction::Escalate),
        )
        .await
        .expect("insert wait");

        let tick = process_due_stateful_waits(
            &paths,
            1_250,
            StatefulWaitSchedulerConfig {
                claimant_id: "scheduler-test".to_string(),
                lease_ms: 500,
                limit: 10,
            },
        )
        .await;

        assert_eq!(tick.completed, 1);
        assert_eq!(
            tick.outcomes[0].event_type,
            "stateful_runtime.wait.timeout_escalated"
        );
        assert_eq!(tick.outcomes[0].wait_status, StatefulWaitStatus::Escalated);
        assert_eq!(
            tick.outcomes[0].run_status,
            StatefulWorkflowRunStatus::Paused
        );

        let waits = list_stateful_waits(
            &paths.waits_path,
            &tenant,
            StatefulWaitQuery {
                status: Some(StatefulWaitStatus::Escalated),
                ..StatefulWaitQuery::default()
            },
        );
        assert_eq!(waits.len(), 1);
        let _ = tokio::fs::remove_dir_all(
            paths
                .run_events_path
                .parent()
                .expect("runtime root")
                .to_path_buf(),
        )
        .await;
    }

    #[tokio::test]
    async fn scheduler_reclaims_expired_wait_claims_after_restart_gap() {
        let paths = paths("stateful-wait-scheduler-reclaim");
        upsert_stateful_wait(&paths.waits_path, timer_wait("wait-a", 1_000))
            .await
            .expect("insert wait");
        let tenant = tenant("org-a", "workspace-a");
        let claimed = claim_due_stateful_wait(
            &paths.waits_path,
            &tenant,
            "run-a",
            "wait-a",
            "scheduler-that-died",
            1_250,
            500,
        )
        .await
        .expect("claim wait");
        assert!(claimed.is_some());

        let tick = process_due_stateful_waits(
            &paths,
            2_000,
            StatefulWaitSchedulerConfig {
                claimant_id: "scheduler-restarted".to_string(),
                lease_ms: 500,
                limit: 10,
            },
        )
        .await;

        assert_eq!(tick.checked, 1);
        assert_eq!(tick.claimed, 1);
        assert_eq!(tick.completed, 1);
        assert_eq!(tick.outcomes[0].wait_status, StatefulWaitStatus::Woken);
        let _ = tokio::fs::remove_dir_all(
            paths
                .run_events_path
                .parent()
                .expect("runtime root")
                .to_path_buf(),
        )
        .await;
    }
}
