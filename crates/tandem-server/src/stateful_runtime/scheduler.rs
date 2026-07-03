use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tandem_types::TenantContext;

use super::phases::{guarded_phase_state_from_status, StatefulWorkflowPhaseState};
use super::reliability::{
    stateful_reliability_path_from_runtime_events_path, upsert_stateful_dead_letter,
    StatefulDeadLetterRecord, StatefulDeadLetterStatus, StatefulRecoveryOption,
};
use super::store::{
    append_stateful_run_event_once_with_next_seq, list_stateful_run_snapshots,
    load_stateful_run_events, write_stateful_run_snapshot, StatefulRuntimeStoragePaths,
};
use super::types::{
    StatefulRunEventRecord, StatefulRunSnapshotRecord, StatefulWaitRecord, StatefulWaitStatus,
    StatefulWaitTimeoutAction, StatefulWorkflowRunStatus,
};
use super::waits::{
    begin_claimed_stateful_wait_reminder_completion,
    begin_claimed_stateful_wait_timeout_completion, begin_claimed_stateful_wait_wake_completion,
    cancel_stateful_wait_after_phase_guard_denial, claim_due_stateful_wait_with_lease_clock,
    due_stateful_waits, finish_claimed_stateful_wait_completion,
    finish_claimed_stateful_wait_reminder_completion, load_stateful_waits,
};

pub const STATEFUL_WAIT_SCHEDULER_CLAIMANT: &str = "stateful-wait-scheduler";
pub const DEFAULT_STATEFUL_WAIT_SCHEDULER_LEASE_MS: u64 = 60_000;
pub const DEFAULT_STATEFUL_WAIT_SCHEDULER_LIMIT: usize = 100;

static SCHEDULER_CLOCK_STATE: OnceLock<Mutex<HashMap<String, SchedulerClockState>>> =
    OnceLock::new();

#[derive(Debug, Clone, Default)]
struct SchedulerClockState {
    last_seen_ms: u64,
    observed_waits: HashSet<String>,
}

#[derive(Debug, Clone)]
struct SchedulerClockObservation {
    effective_now_ms: u64,
    regression_observed_waits: Option<HashSet<String>>,
}

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
    pub clock_regressions: usize,
    pub max_clock_regression_ms: u64,
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
    Reminder {
        due_at_ms: u64,
        remind_every_ms: u64,
    },
}

impl SchedulerAction {
    fn due_at_ms(&self) -> u64 {
        match self {
            Self::WakeTimer { due_at_ms }
            | Self::Timeout { due_at_ms, .. }
            | Self::Reminder { due_at_ms, .. } => *due_at_ms,
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
            }
            | Self::Reminder { .. } => "stateful_runtime.wait.timeout_reminded",
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
            Self::Reminder { .. } => StatefulWaitStatus::Waiting,
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
            Self::Timeout { .. } | Self::Reminder { .. } => StatefulWorkflowRunStatus::Paused,
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
            Self::Reminder { due_at_ms, .. } => {
                format!(
                    "timeout:Remind:{}:{}:{due_at_ms}",
                    wait.run_id, wait.wait_id
                )
            }
        }
    }

    fn dead_letter_reason(&self) -> Option<&'static str> {
        match self {
            Self::Timeout {
                timeout_action: StatefulWaitTimeoutAction::Cancel,
                ..
            } => Some("stateful wait timeout cancelled the run"),
            Self::Timeout {
                timeout_action: StatefulWaitTimeoutAction::Escalate,
                ..
            } => Some("stateful wait timeout escalated for operator review"),
            Self::Timeout {
                timeout_action: StatefulWaitTimeoutAction::Remind,
                ..
            } => Some("stateful wait timeout reminder requires operator review"),
            Self::Timeout {
                timeout_action: StatefulWaitTimeoutAction::Resume,
                ..
            }
            | Self::Reminder { .. }
            | Self::WakeTimer { .. } => None,
        }
    }
}

pub async fn process_due_stateful_waits(
    paths: &StatefulRuntimeStoragePaths,
    now_ms: u64,
    config: StatefulWaitSchedulerConfig,
) -> StatefulWaitSchedulerTick {
    let tenant = TenantContext::local_implicit();
    let mut tick = StatefulWaitSchedulerTick::default();
    let observed_waits = scheduler_observed_wait_ids(paths, &tenant);
    let clock = observe_scheduler_wall_time(paths, &config, now_ms, &observed_waits, &mut tick);
    let regression_tick = clock.regression_observed_waits.is_some();
    let candidate_limit = if regression_tick {
        None
    } else {
        Some(config.limit)
    };
    let mut candidates = due_stateful_waits(
        &paths.waits_path,
        &tenant,
        clock.effective_now_ms,
        candidate_limit,
    )
    .into_iter()
    .filter_map(|candidate| {
        let action = scheduler_action(&candidate, clock.effective_now_ms)?;
        let processing_now_ms = scheduler_processing_now_ms(
            &candidate,
            &action,
            now_ms,
            clock.effective_now_ms,
            clock.regression_observed_waits.as_ref(),
        )?;
        Some((candidate, action, processing_now_ms))
    })
    .collect::<Vec<_>>();
    if regression_tick && config.limit > 0 && candidates.len() > config.limit {
        candidates.truncate(config.limit);
    }
    tick.checked = candidates.len();

    for (candidate, action, processing_now_ms) in candidates {
        if let Err(error) =
            guarded_phase_state_for_wait_action(paths, &candidate, &action, processing_now_ms)
        {
            let reason = error.to_string();
            if let Err(cancel_error) = cancel_stateful_wait_after_phase_guard_denial(
                &paths.waits_path,
                &candidate.scope.tenant_context,
                &candidate,
                &reason,
                processing_now_ms,
            )
            .await
            {
                tick.errors.push(format!(
                    "failed to cancel phase-denied wait {} for run {}: {cancel_error}",
                    candidate.wait_id, candidate.run_id
                ));
            }
            tick.failed += 1;
            tick.errors.push(format!(
                "failed to validate phase transition for wait {} for run {}: {reason}",
                candidate.wait_id, candidate.run_id
            ));
            continue;
        }
        let tenant_context = candidate.scope.tenant_context.clone();
        let claimed = match claim_due_stateful_wait_with_lease_clock(
            &paths.waits_path,
            &tenant_context,
            &candidate.run_id,
            &candidate.wait_id,
            &config.claimant_id,
            processing_now_ms,
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
        match complete_claimed_wait(paths, &claimed, &action, processing_now_ms, now_ms).await {
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

fn observe_scheduler_wall_time(
    paths: &StatefulRuntimeStoragePaths,
    config: &StatefulWaitSchedulerConfig,
    now_ms: u64,
    observed_waits: &HashSet<String>,
    tick: &mut StatefulWaitSchedulerTick,
) -> SchedulerClockObservation {
    let key = scheduler_clock_key(paths, config);
    let (effective_now_ms, regression_ms, regression_observed_waits) = {
        let mut state = SCHEDULER_CLOCK_STATE
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .expect("stateful wait scheduler clock state mutex poisoned");
        match state.get_mut(&key) {
            Some(clock) if now_ms < clock.last_seen_ms => {
                let regression_ms = clock.last_seen_ms.saturating_sub(now_ms);
                (
                    clock.last_seen_ms,
                    Some(regression_ms),
                    Some(clock.observed_waits.clone()),
                )
            }
            Some(clock) => {
                clock.last_seen_ms = now_ms;
                clock.observed_waits = observed_waits.clone();
                (now_ms, None, None)
            }
            None => {
                state.insert(
                    key.clone(),
                    SchedulerClockState {
                        last_seen_ms: now_ms,
                        observed_waits: observed_waits.clone(),
                    },
                );
                (now_ms, None, None)
            }
        }
    };

    if let Some(regression_ms) = regression_ms {
        tick.clock_regressions = tick.clock_regressions.saturating_add(1);
        tick.max_clock_regression_ms = tick.max_clock_regression_ms.max(regression_ms);
        tandem_observability::record_scheduler_clock_regression_ms(regression_ms);
        tracing::warn!(
            waits_path = %paths.waits_path.display(),
            claimant_id = %config.claimant_id,
            now_ms,
            effective_now_ms,
            regression_ms,
            "stateful wait scheduler observed a backward wall-clock step"
        );
    }

    SchedulerClockObservation {
        effective_now_ms,
        regression_observed_waits,
    }
}

fn scheduler_observed_wait_ids(
    paths: &StatefulRuntimeStoragePaths,
    tenant: &TenantContext,
) -> HashSet<String> {
    load_stateful_waits(&paths.waits_path)
        .into_iter()
        .filter(|wait| wait.visible_to_tenant(tenant))
        .filter(|wait| !wait.status.is_terminal())
        .map(|wait| scheduler_wait_identity_key(&wait))
        .collect()
}

fn scheduler_processing_now_ms(
    wait: &StatefulWaitRecord,
    action: &SchedulerAction,
    now_ms: u64,
    effective_now_ms: u64,
    regression_observed_waits: Option<&HashSet<String>>,
) -> Option<u64> {
    let Some(observed_waits) = regression_observed_waits else {
        return Some(effective_now_ms);
    };
    if observed_waits.contains(&scheduler_wait_identity_key(wait)) {
        return Some(effective_now_ms);
    }
    (action.due_at_ms() <= now_ms).then_some(now_ms)
}

fn scheduler_wait_identity_key(wait: &StatefulWaitRecord) -> String {
    let tenant = &wait.scope.tenant_context;
    format!(
        "{}:{}:{}:{}:{}:{}",
        tenant.org_id,
        tenant.workspace_id,
        tenant.deployment_id.as_deref().unwrap_or(""),
        wait.run_id,
        wait.wait_id,
        wait.created_at_ms
    )
}

fn scheduler_clock_key(
    paths: &StatefulRuntimeStoragePaths,
    config: &StatefulWaitSchedulerConfig,
) -> String {
    format!(
        "{}:{}",
        config.claimant_id,
        paths.waits_path.to_string_lossy()
    )
}

async fn complete_claimed_wait(
    paths: &StatefulRuntimeStoragePaths,
    wait: &StatefulWaitRecord,
    action: &SchedulerAction,
    event_now_ms: u64,
    lease_now_ms: u64,
) -> anyhow::Result<StatefulWaitSchedulerOutcome> {
    let completion_key = action.completion_key(wait);
    let event_id = format!("stateful-wait-{completion_key}");
    let lag_ms = event_now_ms.saturating_sub(action.due_at_ms());
    let wait_status = action.wait_status();
    let run_status = action.run_status();

    let reserved = match action {
        SchedulerAction::WakeTimer { .. }
        | SchedulerAction::Timeout {
            timeout_action: StatefulWaitTimeoutAction::Resume,
            ..
        } => {
            begin_claimed_stateful_wait_wake_completion(
                &paths.waits_path,
                &wait.scope.tenant_context,
                wait,
                &completion_key,
                lease_now_ms,
            )
            .await?
        }
        SchedulerAction::Reminder { .. } => {
            begin_claimed_stateful_wait_reminder_completion(
                &paths.waits_path,
                &wait.scope.tenant_context,
                wait,
                &completion_key,
                lease_now_ms,
            )
            .await?
        }
        SchedulerAction::Timeout { .. } => {
            begin_claimed_stateful_wait_timeout_completion(
                &paths.waits_path,
                &wait.scope.tenant_context,
                wait,
                &completion_key,
                wait_status.clone(),
                lease_now_ms,
            )
            .await?
        }
    }
    .ok_or_else(|| anyhow::anyhow!("stateful wait completion conflict"))?;

    let phase_state = match guarded_phase_state_for_wait_action(
        paths,
        &reserved,
        action,
        event_now_ms,
    ) {
        Ok(phase_state) => phase_state,
        Err(error) => {
            let reason = error.to_string();
            match cancel_stateful_wait_after_phase_guard_denial(
                &paths.waits_path,
                &reserved.scope.tenant_context,
                &reserved,
                &reason,
                event_now_ms,
            )
            .await
            {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Err(anyhow::anyhow!(
                        "{error}; additionally failed to cancel claimed wait after phase guard denial: wait no longer matched current claim"
                    ));
                }
                Err(cancel_error) => {
                    return Err(anyhow::anyhow!(
                        "{error}; additionally failed to cancel claimed wait after phase guard denial: {cancel_error}"
                    ));
                }
            }
            return Err(error);
        }
    };

    let event = StatefulRunEventRecord {
        schema_version: 1,
        event_id: event_id.clone(),
        run_id: wait.run_id.clone(),
        seq: 0,
        event_type: action.event_type().to_string(),
        occurred_at_ms: event_now_ms,
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
            "completion_key": &completion_key,
            "lag_ms": lag_ms,
            "scheduler": STATEFUL_WAIT_SCHEDULER_CLAIMANT,
        }),
    };
    let (_appended, seq) = append_stateful_run_event_once_with_next_seq(
        &paths.run_events_path,
        &wait.scope.tenant_context,
        &event,
    )
    .await?;

    let snapshot = StatefulRunSnapshotRecord {
        schema_version: 1,
        snapshot_id: event_id,
        run_id: wait.run_id.clone(),
        seq,
        created_at_ms: event_now_ms,
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
    let completed = match action {
        SchedulerAction::Reminder {
            remind_every_ms, ..
        } => {
            finish_claimed_stateful_wait_reminder_completion(
                &paths.waits_path,
                &wait.scope.tenant_context,
                &reserved,
                &completion_key,
                seq,
                event_now_ms.saturating_add((*remind_every_ms).max(1)),
                event_now_ms,
            )
            .await?
        }
        _ => {
            finish_claimed_stateful_wait_completion(
                &paths.waits_path,
                &wait.scope.tenant_context,
                &reserved,
                &completion_key,
                seq,
                wait_status.clone(),
                event_now_ms,
            )
            .await?
        }
    }
    .ok_or_else(|| anyhow::anyhow!("stateful wait completion conflict"))?;

    if let Err(error) =
        record_wait_terminal_dead_letter(paths, &completed, action, seq, event_now_ms, lag_ms).await
    {
        tracing::warn!(
            wait_id = %completed.wait_id,
            run_id = %completed.run_id,
            error = %error,
            "failed to record terminal stateful wait dead letter"
        );
    }

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

fn guarded_phase_state_for_wait_action(
    paths: &StatefulRuntimeStoragePaths,
    wait: &StatefulWaitRecord,
    action: &SchedulerAction,
    now_ms: u64,
) -> anyhow::Result<StatefulWorkflowPhaseState> {
    let run_status = action.run_status();
    let previous_snapshot = list_stateful_run_snapshots(
        &paths.snapshots_root,
        &wait.scope.tenant_context,
        &wait.run_id,
        Some(1),
    )
    .pop();
    let previous_history = previous_snapshot
        .as_ref()
        .map(|snapshot| snapshot.phase_history.as_slice())
        .unwrap_or(&[]);
    guarded_phase_state_from_status(
        &wait.run_id,
        &run_status,
        now_ms,
        wait.phase_id.as_deref(),
        previous_snapshot.as_ref().map(|snapshot| snapshot.phase),
        previous_history,
        Some(format!("stateful_wait_scheduler:{}", action.event_type())),
    )
    .map_err(anyhow::Error::from)
}

async fn record_wait_terminal_dead_letter(
    paths: &StatefulRuntimeStoragePaths,
    wait: &StatefulWaitRecord,
    action: &SchedulerAction,
    event_seq: u64,
    now_ms: u64,
    lag_ms: u64,
) -> anyhow::Result<()> {
    let Some(reason) = action.dead_letter_reason() else {
        return Ok(());
    };
    let reliability_path =
        stateful_reliability_path_from_runtime_events_path(&paths.run_events_path);
    let digest = crate::sha256_hex(&["stateful_wait", &wait.run_id, &wait.wait_id]);
    let detail = wait
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let reason = detail
        .map(|detail| format!("{reason}: {detail}"))
        .unwrap_or_else(|| reason.to_string());
    let record = StatefulDeadLetterRecord {
        schema_version: 1,
        dead_letter_id: format!("dead-letter-wait-{}", &digest[..16]),
        source_type: "stateful_wait".to_string(),
        source_id: wait.wait_id.clone(),
        run_id: Some(wait.run_id.clone()),
        scope: wait.scope.clone(),
        reason,
        status: StatefulDeadLetterStatus::Open,
        recovery_options: vec![
            StatefulRecoveryOption::Ignore,
            StatefulRecoveryOption::Compensate,
        ],
        payload_pointer: Some(format!("stateful-wait://{}/{}", wait.run_id, wait.wait_id)),
        compensation_id: None,
        attempts: 1,
        created_at_ms: now_ms,
        updated_at_ms: now_ms,
        operator_disposition: None,
        disposition_reason: None,
        disposition_actor: None,
        disposition_at_ms: None,
        metadata: Some(json!({
            "source": "stateful_wait_scheduler",
            "event_seq": event_seq,
            "wait_status": &wait.status,
            "wait_kind": &wait.wait_kind,
            "timeout_policy": &wait.timeout_policy,
            "lag_ms": lag_ms,
        })),
    };
    upsert_stateful_dead_letter(&reliability_path, record).await?;
    Ok(())
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
        .map(|policy| match &policy.on_timeout {
            StatefulWaitTimeoutAction::Remind => policy
                .remind_every_ms
                .filter(|remind_every_ms| *remind_every_ms > 0)
                .map(|remind_every_ms| SchedulerAction::Reminder {
                    due_at_ms: policy.timeout_at_ms,
                    remind_every_ms,
                })
                .unwrap_or_else(|| SchedulerAction::Timeout {
                    due_at_ms: policy.timeout_at_ms,
                    timeout_action: policy.on_timeout.clone(),
                }),
            _ => SchedulerAction::Timeout {
                due_at_ms: policy.timeout_at_ms,
                timeout_action: policy.on_timeout.clone(),
            },
        });

    match (wake_due, timeout_due) {
        (Some(wake), Some(timeout)) if timeout.due_at_ms() <= wake.due_at_ms() => Some(timeout),
        (Some(wake), _) => Some(wake),
        (None, Some(timeout)) => Some(timeout),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tandem_types::TenantContext;
    use uuid::Uuid;

    use super::*;
    use crate::stateful_runtime::{
        claim_due_stateful_wait, list_stateful_dead_letters, list_stateful_run_snapshots,
        list_stateful_waits, stateful_reliability_path_from_runtime_events_path,
        upsert_stateful_wait, StatefulRecoveryOption, StatefulReliabilityQuery,
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
    async fn scheduler_assigns_completion_sequence_under_append_lock() {
        let paths = paths("stateful-wait-scheduler-seq");
        let tenant = tenant("org-a", "workspace-a");
        let seed = StatefulRunEventRecord {
            schema_version: 1,
            event_id: "seed-event".to_string(),
            run_id: "run-a".to_string(),
            seq: 0,
            event_type: "stateful_runtime.seed".to_string(),
            occurred_at_ms: 1_000,
            scope: StatefulRuntimeScope::from_tenant_context(tenant.clone()),
            actor: None,
            phase_id: None,
            phase_transition: None,
            wait_kind: None,
            causation_id: None,
            correlation_id: None,
            payload: json!({ "seed": true }),
        };
        let (_appended, seed_seq) =
            append_stateful_run_event_once_with_next_seq(&paths.run_events_path, &tenant, &seed)
                .await
                .expect("seed event");
        assert_eq!(seed_seq, 1);

        upsert_stateful_wait(&paths.waits_path, timer_wait("wait-a", 1_250))
            .await
            .expect("insert wait");
        let tick = process_due_stateful_waits(
            &paths,
            1_500,
            StatefulWaitSchedulerConfig {
                claimant_id: "scheduler-test".to_string(),
                lease_ms: 500,
                limit: 10,
            },
        )
        .await;

        assert_eq!(tick.completed, 1);
        assert_eq!(tick.outcomes[0].event_seq, 2);
        let events = load_stateful_run_events(&paths.run_events_path);
        assert_eq!(
            events.iter().map(|event| event.seq).collect::<Vec<_>>(),
            vec![1, 2]
        );
        let waits = list_stateful_waits(
            &paths.waits_path,
            &tenant,
            StatefulWaitQuery {
                run_id: Some("run-a"),
                ..StatefulWaitQuery::default()
            },
        );
        assert_eq!(waits[0].status, StatefulWaitStatus::Woken);
        assert_eq!(waits[0].event_seq, Some(2));
        let snapshots = list_stateful_run_snapshots(&paths.snapshots_root, &tenant, "run-a", None);
        assert_eq!(snapshots[0].seq, 2);
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
    async fn scheduler_rejects_wake_after_terminal_phase_without_appending_event() {
        let paths = paths("stateful-wait-scheduler-terminal-phase");
        let tenant = tenant("org-a", "workspace-a");
        let phase_state = crate::stateful_runtime::phase_state_from_status(
            "run-a",
            &StatefulWorkflowRunStatus::Completed,
            900,
            None,
        );
        let completed_snapshot = StatefulRunSnapshotRecord {
            schema_version: 1,
            snapshot_id: "completed-snapshot".to_string(),
            run_id: "run-a".to_string(),
            seq: 1,
            created_at_ms: 900,
            scope: StatefulRuntimeScope::from_tenant_context(tenant.clone()),
            status: StatefulWorkflowRunStatus::Completed,
            phase: phase_state.phase,
            phase_history: phase_state.phase_history,
            allowed_next_phases: phase_state.allowed_next_phases,
            phase_id: None,
            source_record_kind: None,
            checkpoint: None,
            payload_digest: None,
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            metadata: None,
        };
        crate::stateful_runtime::write_stateful_run_snapshot(
            &paths.snapshots_root,
            &completed_snapshot,
        )
        .await
        .expect("write completed snapshot");
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
        assert_eq!(tick.claimed, 0);
        assert_eq!(tick.completed, 0);
        assert_eq!(tick.failed, 1);
        assert!(tick.errors[0].contains("terminal phase completed"));
        assert!(load_stateful_run_events(&paths.run_events_path).is_empty());
        let waits = list_stateful_waits(
            &paths.waits_path,
            &tenant,
            StatefulWaitQuery {
                run_id: Some("run-a"),
                ..StatefulWaitQuery::default()
            },
        );
        assert_eq!(waits[0].status, StatefulWaitStatus::Cancelled);
        assert_eq!(
            waits[0].wake_idempotency_key.as_deref(),
            Some("phase-guard-denied:wait-a")
        );
        assert_eq!(
            waits[0]
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("phase_guard_denied"))
                .and_then(|denied| denied.as_bool()),
            Some(true)
        );
        let next_tick = process_due_stateful_waits(
            &paths,
            1_300,
            StatefulWaitSchedulerConfig {
                claimant_id: "scheduler-test".to_string(),
                lease_ms: 500,
                limit: 10,
            },
        )
        .await;
        assert_eq!(next_tick.checked, 0);
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
    async fn scheduler_revalidates_terminal_phase_after_claim_before_completion() {
        let paths = paths("stateful-wait-scheduler-post-claim-terminal-phase");
        let tenant = tenant("org-a", "workspace-a");
        upsert_stateful_wait(&paths.waits_path, timer_wait("wait-a", 1_000))
            .await
            .expect("insert wait");
        let claimed = claim_due_stateful_wait(
            &paths.waits_path,
            &tenant,
            "run-a",
            "wait-a",
            "scheduler-test",
            1_250,
            500,
        )
        .await
        .expect("claim wait")
        .expect("claimed wait");
        let phase_state = crate::stateful_runtime::phase_state_from_status(
            "run-a",
            &StatefulWorkflowRunStatus::Completed,
            1_300,
            None,
        );
        let completed_snapshot = StatefulRunSnapshotRecord {
            schema_version: 1,
            snapshot_id: "completed-after-claim".to_string(),
            run_id: "run-a".to_string(),
            seq: 1,
            created_at_ms: 1_300,
            scope: StatefulRuntimeScope::from_tenant_context(tenant.clone()),
            status: StatefulWorkflowRunStatus::Completed,
            phase: phase_state.phase,
            phase_history: phase_state.phase_history,
            allowed_next_phases: phase_state.allowed_next_phases,
            phase_id: None,
            source_record_kind: None,
            checkpoint: None,
            payload_digest: None,
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            metadata: None,
        };
        crate::stateful_runtime::write_stateful_run_snapshot(
            &paths.snapshots_root,
            &completed_snapshot,
        )
        .await
        .expect("write completed snapshot");

        let err = complete_claimed_wait(
            &paths,
            &claimed,
            &SchedulerAction::WakeTimer { due_at_ms: 1_000 },
            1_350,
            1_350,
        )
        .await
        .expect_err("post-claim terminal phase should block completion");

        assert!(err.to_string().contains("terminal phase completed"));
        assert!(load_stateful_run_events(&paths.run_events_path).is_empty());
        let waits = list_stateful_waits(
            &paths.waits_path,
            &tenant,
            StatefulWaitQuery {
                run_id: Some("run-a"),
                ..StatefulWaitQuery::default()
            },
        );
        assert_eq!(waits[0].status, StatefulWaitStatus::Cancelled);
        assert!(waits[0].claimed_by.is_none());
        assert_eq!(
            waits[0]
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("phase_guard_denied"))
                .and_then(|denied| denied.as_bool()),
            Some(true)
        );
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
        let dead_letters = list_stateful_dead_letters(
            &stateful_reliability_path_from_runtime_events_path(&paths.run_events_path),
            &tenant,
            StatefulReliabilityQuery {
                run_id: Some("run-a"),
                limit: Some(10),
                ..Default::default()
            },
        );
        assert_eq!(dead_letters.len(), 1);
        assert_eq!(dead_letters[0].source_type, "stateful_wait");
        assert_eq!(dead_letters[0].source_id, "wait-a");
        assert_eq!(
            dead_letters[0].recovery_options,
            vec![
                StatefulRecoveryOption::Ignore,
                StatefulRecoveryOption::Compensate
            ]
        );
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
    async fn scheduler_reschedules_remind_timeout_without_terminal_wait() {
        let paths = paths("stateful-wait-scheduler-reminder");
        let tenant = tenant("org-a", "workspace-a");
        let mut wait = timeout_wait("wait-a", 1_000, StatefulWaitTimeoutAction::Remind);
        wait.timeout_policy
            .as_mut()
            .expect("timeout policy")
            .remind_every_ms = Some(500);
        upsert_stateful_wait(&paths.waits_path, wait)
            .await
            .expect("insert wait");

        let tick = process_due_stateful_waits(
            &paths,
            1_250,
            StatefulWaitSchedulerConfig {
                claimant_id: "scheduler-reminder-test".to_string(),
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
            "stateful_runtime.wait.timeout_reminded"
        );
        assert_eq!(tick.outcomes[0].wait_status, StatefulWaitStatus::Waiting);
        assert_eq!(
            tick.outcomes[0].run_status,
            StatefulWorkflowRunStatus::Paused
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
        assert_eq!(waits.len(), 1);
        assert_eq!(waits[0].status, StatefulWaitStatus::Waiting);
        assert!(waits[0].claimed_by.is_none());
        assert_eq!(
            waits[0].wake_idempotency_key.as_deref(),
            Some("timeout:Remind:run-a:wait-a:1000")
        );
        let timeout_policy = waits[0].timeout_policy.as_ref().expect("timeout policy");
        assert_eq!(timeout_policy.timeout_at_ms, 1_750);
        assert_eq!(timeout_policy.remind_every_ms, Some(500));
        let metadata = timeout_policy.metadata.as_ref().expect("timeout metadata");
        assert_eq!(
            metadata.get("source").and_then(|value| value.as_str()),
            Some("test")
        );
        assert_eq!(
            metadata
                .get("reminder_count")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        assert_eq!(
            metadata
                .get("last_reminded_at_ms")
                .and_then(|value| value.as_u64()),
            Some(1_250)
        );
        assert_eq!(
            metadata
                .get("next_reminder_at_ms")
                .and_then(|value| value.as_u64()),
            Some(1_750)
        );
        let dead_letters = list_stateful_dead_letters(
            &stateful_reliability_path_from_runtime_events_path(&paths.run_events_path),
            &tenant,
            StatefulReliabilityQuery {
                run_id: Some("run-a"),
                limit: Some(10),
                ..Default::default()
            },
        );
        assert!(dead_letters.is_empty());

        let early_tick = process_due_stateful_waits(
            &paths,
            1_500,
            StatefulWaitSchedulerConfig {
                claimant_id: "scheduler-reminder-test".to_string(),
                lease_ms: 500,
                limit: 10,
            },
        )
        .await;
        assert_eq!(early_tick.checked, 0);

        let second_tick = process_due_stateful_waits(
            &paths,
            1_750,
            StatefulWaitSchedulerConfig {
                claimant_id: "scheduler-reminder-test".to_string(),
                lease_ms: 500,
                limit: 10,
            },
        )
        .await;
        assert_eq!(second_tick.completed, 1);
        let waits = list_stateful_waits(
            &paths.waits_path,
            &tenant,
            StatefulWaitQuery {
                run_id: Some("run-a"),
                ..StatefulWaitQuery::default()
            },
        );
        let timeout_policy = waits[0].timeout_policy.as_ref().expect("timeout policy");
        assert_eq!(timeout_policy.timeout_at_ms, 2_250);
        assert_eq!(
            timeout_policy
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("reminder_count"))
                .and_then(|value| value.as_u64()),
            Some(2)
        );
        assert_eq!(load_stateful_run_events(&paths.run_events_path).len(), 2);
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
    async fn scheduler_compensates_backward_clock_regression_for_due_waits() {
        let paths = paths("stateful-wait-scheduler-clock-regression");
        let tenant = tenant("org-a", "workspace-a");
        let config = StatefulWaitSchedulerConfig {
            claimant_id: "scheduler-clock-regression-test".to_string(),
            lease_ms: 500,
            limit: 1,
        };
        upsert_stateful_wait(&paths.waits_path, timer_wait("wait-a", 1_800))
            .await
            .expect("insert first wait");
        upsert_stateful_wait(&paths.waits_path, timer_wait("wait-b", 1_900))
            .await
            .expect("insert second wait");

        let seed_tick = process_due_stateful_waits(&paths, 2_000, config.clone()).await;
        assert_eq!(seed_tick.checked, 1);
        assert_eq!(seed_tick.completed, 1);
        assert_eq!(seed_tick.clock_regressions, 0);
        let waits = list_stateful_waits(
            &paths.waits_path,
            &tenant,
            StatefulWaitQuery {
                run_id: Some("run-a"),
                ..StatefulWaitQuery::default()
            },
        );
        assert_eq!(
            waits
                .iter()
                .find(|wait| wait.wait_id == "wait-a")
                .expect("first wait")
                .status,
            StatefulWaitStatus::Woken
        );
        assert_eq!(
            waits
                .iter()
                .find(|wait| wait.wait_id == "wait-b")
                .expect("second wait")
                .status,
            StatefulWaitStatus::Waiting
        );

        let mut new_wait = timer_wait("wait-new", 1_500);
        new_wait.created_at_ms = 1_050;
        new_wait.updated_at_ms = 1_050;
        upsert_stateful_wait(&paths.waits_path, new_wait)
            .await
            .expect("insert new wait after regression");

        let tick = process_due_stateful_waits(&paths, 1_000, config).await;

        assert_eq!(tick.clock_regressions, 1);
        assert_eq!(tick.max_clock_regression_ms, 1_000);
        assert_eq!(tick.checked, 1);
        assert_eq!(tick.claimed, 1);
        assert_eq!(tick.completed, 1);
        assert_eq!(tick.failed, 0);
        assert_eq!(tick.outcomes[0].wait_status, StatefulWaitStatus::Woken);
        assert_eq!(tick.outcomes[0].lag_ms, 100);

        let waits = list_stateful_waits(
            &paths.waits_path,
            &tenant,
            StatefulWaitQuery {
                run_id: Some("run-a"),
                ..StatefulWaitQuery::default()
            },
        );
        assert_eq!(
            waits
                .iter()
                .find(|wait| wait.wait_id == "wait-b")
                .expect("second wait")
                .status,
            StatefulWaitStatus::Woken
        );
        assert_eq!(
            waits
                .iter()
                .find(|wait| wait.wait_id == "wait-new")
                .expect("new wait")
                .status,
            StatefulWaitStatus::Waiting
        );
        let events = load_stateful_run_events(&paths.run_events_path);
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].occurred_at_ms, 2_000);
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
    async fn scheduler_does_not_apply_regressed_future_tick_to_new_waits() {
        let paths = paths("stateful-wait-scheduler-clock-regression-new-wait");
        let tenant = tenant("org-a", "workspace-a");
        let config = StatefulWaitSchedulerConfig {
            claimant_id: "scheduler-clock-regression-new-wait-test".to_string(),
            lease_ms: 500,
            limit: 10,
        };
        let seed_tick = process_due_stateful_waits(&paths, 2_000, config.clone()).await;
        assert_eq!(seed_tick.checked, 0);
        assert_eq!(seed_tick.clock_regressions, 0);

        let mut wait = timer_wait("wait-new", 1_500);
        wait.created_at_ms = 1_050;
        wait.updated_at_ms = 1_050;
        upsert_stateful_wait(&paths.waits_path, wait)
            .await
            .expect("insert new wait after regression");

        let early_tick = process_due_stateful_waits(&paths, 1_100, config.clone()).await;
        assert_eq!(early_tick.clock_regressions, 1);
        assert_eq!(early_tick.checked, 0);
        assert_eq!(early_tick.completed, 0);
        let waits = list_stateful_waits(
            &paths.waits_path,
            &tenant,
            StatefulWaitQuery {
                run_id: Some("run-a"),
                ..StatefulWaitQuery::default()
            },
        );
        assert_eq!(waits[0].status, StatefulWaitStatus::Waiting);

        let due_tick = process_due_stateful_waits(&paths, 1_500, config).await;
        assert_eq!(due_tick.clock_regressions, 1);
        assert_eq!(due_tick.checked, 1);
        assert_eq!(due_tick.completed, 1);
        assert_eq!(due_tick.outcomes[0].lag_ms, 0);
        let events = load_stateful_run_events(&paths.run_events_path);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].occurred_at_ms, 1_500);
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
    async fn scheduler_regression_identity_includes_wait_generation() {
        let paths = paths("stateful-wait-scheduler-clock-regression-generation");
        let tenant = tenant("org-a", "workspace-a");
        let config = StatefulWaitSchedulerConfig {
            claimant_id: "scheduler-clock-regression-generation-test".to_string(),
            lease_ms: 500,
            limit: 10,
        };
        let mut old_generation = timer_wait("wait-reused", 2_500);
        old_generation.created_at_ms = 1_900;
        old_generation.updated_at_ms = 1_900;
        upsert_stateful_wait(&paths.waits_path, old_generation)
            .await
            .expect("insert old generation");
        let seed_tick = process_due_stateful_waits(&paths, 2_000, config.clone()).await;
        assert_eq!(seed_tick.checked, 0);
        assert_eq!(seed_tick.clock_regressions, 0);

        let mut new_generation = timer_wait("wait-reused", 1_500);
        new_generation.created_at_ms = 1_050;
        new_generation.updated_at_ms = 1_050;
        upsert_stateful_wait(&paths.waits_path, new_generation)
            .await
            .expect("replace with new generation");

        let early_tick = process_due_stateful_waits(&paths, 1_100, config).await;
        assert_eq!(early_tick.clock_regressions, 1);
        assert_eq!(early_tick.checked, 0);
        assert_eq!(early_tick.completed, 0);
        let waits = list_stateful_waits(
            &paths.waits_path,
            &tenant,
            StatefulWaitQuery {
                run_id: Some("run-a"),
                ..StatefulWaitQuery::default()
            },
        );
        assert_eq!(waits.len(), 1);
        assert_eq!(waits[0].wait_id, "wait-reused");
        assert_eq!(waits[0].created_at_ms, 1_050);
        assert_eq!(waits[0].status, StatefulWaitStatus::Waiting);
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
    async fn scheduler_regression_claim_lease_uses_wall_time() {
        let paths = paths("stateful-wait-scheduler-clock-regression-lease");
        let tenant = tenant("org-a", "workspace-a");
        upsert_stateful_wait(&paths.waits_path, timer_wait("wait-a", 1_900))
            .await
            .expect("insert wait");

        let claimed = claim_due_stateful_wait_with_lease_clock(
            &paths.waits_path,
            &tenant,
            "run-a",
            "wait-a",
            "scheduler-clock-regression-lease-test",
            2_000,
            1_000,
            500,
        )
        .await
        .expect("claim wait")
        .expect("claimed wait");

        assert_eq!(claimed.claimed_at_ms, Some(1_000));
        assert_eq!(claimed.claim_expires_at_ms, Some(1_500));
        assert_eq!(claimed.updated_at_ms, 1_000);
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
