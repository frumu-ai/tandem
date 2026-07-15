// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::{Deserialize, Serialize};
use serde_json::json;
use tandem_types::PrincipalRef;

use super::types::{
    StatefulRunEventRecord, StatefulRuntimeScope, StatefulWorkflowRunStatus,
    STATEFUL_RUNTIME_SCHEMA_VERSION,
};

pub const PHASE_OBSERVED_EVENT_TYPE: &str = "stateful_runtime.phase.observed";
pub const PHASE_TRANSITION_EVENT_TYPE: &str = "stateful_runtime.phase.transition";

const CREATED_NEXT: &[StatefulWorkflowPhase] = &[
    StatefulWorkflowPhase::Queued,
    StatefulWorkflowPhase::Cancelled,
];
const QUEUED_NEXT: &[StatefulWorkflowPhase] = &[
    StatefulWorkflowPhase::RunningPhase,
    StatefulWorkflowPhase::PausedAttentionRequired,
    StatefulWorkflowPhase::Cancelled,
];
const RUNNING_NEXT: &[StatefulWorkflowPhase] = &[
    StatefulWorkflowPhase::Sleeping,
    StatefulWorkflowPhase::WaitingWebhook,
    StatefulWorkflowPhase::AwaitingApproval,
    StatefulWorkflowPhase::Retrying,
    StatefulWorkflowPhase::PausedAttentionRequired,
    StatefulWorkflowPhase::Completed,
    StatefulWorkflowPhase::Failed,
    StatefulWorkflowPhase::Cancelled,
];
const SLEEPING_NEXT: &[StatefulWorkflowPhase] = &[
    StatefulWorkflowPhase::RunningPhase,
    StatefulWorkflowPhase::PausedAttentionRequired,
    StatefulWorkflowPhase::Cancelled,
];
const WAITING_WEBHOOK_NEXT: &[StatefulWorkflowPhase] = &[
    StatefulWorkflowPhase::RunningPhase,
    StatefulWorkflowPhase::Retrying,
    StatefulWorkflowPhase::PausedAttentionRequired,
    StatefulWorkflowPhase::Failed,
    StatefulWorkflowPhase::Cancelled,
];
const AWAITING_APPROVAL_NEXT: &[StatefulWorkflowPhase] = &[
    StatefulWorkflowPhase::RunningPhase,
    StatefulWorkflowPhase::PausedAttentionRequired,
    StatefulWorkflowPhase::Failed,
    StatefulWorkflowPhase::Cancelled,
];
const RETRYING_NEXT: &[StatefulWorkflowPhase] = &[
    StatefulWorkflowPhase::RunningPhase,
    StatefulWorkflowPhase::PausedAttentionRequired,
    StatefulWorkflowPhase::Failed,
    StatefulWorkflowPhase::Cancelled,
];
const PAUSED_ATTENTION_NEXT: &[StatefulWorkflowPhase] = &[
    StatefulWorkflowPhase::Queued,
    StatefulWorkflowPhase::RunningPhase,
    StatefulWorkflowPhase::Failed,
    StatefulWorkflowPhase::Cancelled,
];
const TERMINAL_NEXT: &[StatefulWorkflowPhase] = &[];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum StatefulWorkflowPhase {
    Created,
    Queued,
    RunningPhase,
    Sleeping,
    WaitingWebhook,
    AwaitingApproval,
    Retrying,
    PausedAttentionRequired,
    Failed,
    Completed,
    Cancelled,
}

impl Default for StatefulWorkflowPhase {
    fn default() -> Self {
        Self::Created
    }
}

impl StatefulWorkflowPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Queued => "queued",
            Self::RunningPhase => "running_phase",
            Self::Sleeping => "sleeping",
            Self::WaitingWebhook => "waiting_webhook",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Retrying => "retrying",
            Self::PausedAttentionRequired => "paused_attention_required",
            Self::Failed => "failed",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }

    pub const fn allowed_next_phases(self) -> &'static [Self] {
        match self {
            Self::Created => CREATED_NEXT,
            Self::Queued => QUEUED_NEXT,
            Self::RunningPhase => RUNNING_NEXT,
            Self::Sleeping => SLEEPING_NEXT,
            Self::WaitingWebhook => WAITING_WEBHOOK_NEXT,
            Self::AwaitingApproval => AWAITING_APPROVAL_NEXT,
            Self::Retrying => RETRYING_NEXT,
            Self::PausedAttentionRequired => PAUSED_ATTENTION_NEXT,
            Self::Failed | Self::Completed | Self::Cancelled => TERMINAL_NEXT,
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Failed | Self::Completed | Self::Cancelled)
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        self.allowed_next_phases().contains(&next)
    }

    pub fn validate_transition_to(
        self,
        next: Self,
    ) -> Result<(), StatefulWorkflowPhaseTransitionError> {
        if self.can_transition_to(next) {
            Ok(())
        } else {
            Err(StatefulWorkflowPhaseTransitionError {
                from_phase: self,
                to_phase: next,
            })
        }
    }
}

impl std::fmt::Display for StatefulWorkflowPhase {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatefulWorkflowPhaseTransitionError {
    pub from_phase: StatefulWorkflowPhase,
    pub to_phase: StatefulWorkflowPhase,
}

impl std::fmt::Display for StatefulWorkflowPhaseTransitionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.from_phase.is_terminal() {
            write!(
                formatter,
                "workflow phase transition from terminal phase {} to {} is not allowed",
                self.from_phase, self.to_phase
            )
        } else {
            write!(
                formatter,
                "workflow phase transition from {} to {} is not allowed",
                self.from_phase, self.to_phase
            )
        }
    }
}

impl std::error::Error for StatefulWorkflowPhaseTransitionError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatefulWorkflowPhaseTransitionRecord {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub event_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_phase: Option<StatefulWorkflowPhase>,
    pub to_phase: StatefulWorkflowPhase,
    pub event_type: String,
    pub occurred_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl StatefulWorkflowPhaseTransitionRecord {
    pub fn new(
        event_id: impl Into<String>,
        from_phase: StatefulWorkflowPhase,
        to_phase: StatefulWorkflowPhase,
        occurred_at_ms: u64,
        phase_id: Option<String>,
        reason: Option<String>,
    ) -> Result<Self, StatefulWorkflowPhaseTransitionError> {
        from_phase.validate_transition_to(to_phase)?;
        Ok(Self {
            schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
            event_id: event_id.into(),
            from_phase: Some(from_phase),
            to_phase,
            event_type: PHASE_TRANSITION_EVENT_TYPE.to_string(),
            occurred_at_ms,
            phase_id,
            reason,
        })
    }

    pub fn observed(
        event_id: impl Into<String>,
        to_phase: StatefulWorkflowPhase,
        occurred_at_ms: u64,
        phase_id: Option<String>,
        reason: Option<String>,
    ) -> Self {
        Self {
            schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
            event_id: event_id.into(),
            from_phase: None,
            to_phase,
            event_type: PHASE_OBSERVED_EVENT_TYPE.to_string(),
            occurred_at_ms,
            phase_id,
            reason,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatefulWorkflowPhaseState {
    pub phase: StatefulWorkflowPhase,
    pub phase_history: Vec<StatefulWorkflowPhaseTransitionRecord>,
    pub allowed_next_phases: Vec<StatefulWorkflowPhase>,
}

pub fn phase_from_stateful_status(status: &StatefulWorkflowRunStatus) -> StatefulWorkflowPhase {
    match status {
        StatefulWorkflowRunStatus::Queued => StatefulWorkflowPhase::Queued,
        StatefulWorkflowRunStatus::Running => StatefulWorkflowPhase::RunningPhase,
        StatefulWorkflowRunStatus::Sleeping => StatefulWorkflowPhase::Sleeping,
        StatefulWorkflowRunStatus::AwaitingWebhook => StatefulWorkflowPhase::WaitingWebhook,
        StatefulWorkflowRunStatus::AwaitingApproval => StatefulWorkflowPhase::AwaitingApproval,
        StatefulWorkflowRunStatus::Pausing
        | StatefulWorkflowRunStatus::Paused
        | StatefulWorkflowRunStatus::Blocked => StatefulWorkflowPhase::PausedAttentionRequired,
        StatefulWorkflowRunStatus::Retrying => StatefulWorkflowPhase::Retrying,
        StatefulWorkflowRunStatus::Completed | StatefulWorkflowRunStatus::DryRun => {
            StatefulWorkflowPhase::Completed
        }
        StatefulWorkflowRunStatus::Failed | StatefulWorkflowRunStatus::DeadLettered => {
            StatefulWorkflowPhase::Failed
        }
        StatefulWorkflowRunStatus::Cancelled => StatefulWorkflowPhase::Cancelled,
    }
}

pub fn phase_state_from_status(
    run_id: &str,
    status: &StatefulWorkflowRunStatus,
    occurred_at_ms: u64,
    phase_id: Option<&str>,
) -> StatefulWorkflowPhaseState {
    let phase = phase_from_stateful_status(status);
    let transition = StatefulWorkflowPhaseTransitionRecord::observed(
        format!("{run_id}:phase:{}:{occurred_at_ms}", phase.as_str()),
        phase,
        occurred_at_ms,
        phase_id.map(ToOwned::to_owned),
        Some(format!(
            "observed_status:{}",
            stateful_status_as_str(status)
        )),
    );
    StatefulWorkflowPhaseState {
        phase,
        phase_history: vec![transition],
        allowed_next_phases: phase.allowed_next_phases().to_vec(),
    }
}

pub fn guarded_phase_state_from_status(
    run_id: &str,
    status: &StatefulWorkflowRunStatus,
    occurred_at_ms: u64,
    phase_id: Option<&str>,
    previous_phase: Option<StatefulWorkflowPhase>,
    previous_history: &[StatefulWorkflowPhaseTransitionRecord],
    reason: impl Into<Option<String>>,
) -> Result<StatefulWorkflowPhaseState, StatefulWorkflowPhaseTransitionError> {
    let next_phase = phase_from_stateful_status(status);
    let Some(from_phase) = previous_phase else {
        return Ok(phase_state_from_status(
            run_id,
            status,
            occurred_at_ms,
            phase_id,
        ));
    };

    let mut history = previous_history.to_vec();
    if from_phase == next_phase {
        if history.is_empty() {
            history.push(StatefulWorkflowPhaseTransitionRecord::observed(
                phase_observed_event_id(run_id, next_phase, occurred_at_ms),
                next_phase,
                occurred_at_ms,
                phase_id.map(ToOwned::to_owned),
                Some(format!(
                    "observed_status:{}",
                    stateful_status_as_str(status)
                )),
            ));
        }
        return Ok(StatefulWorkflowPhaseState {
            phase: next_phase,
            phase_history: history,
            allowed_next_phases: next_phase.allowed_next_phases().to_vec(),
        });
    }

    let transition = StatefulWorkflowPhaseTransitionRecord::new(
        phase_transition_event_id(run_id, from_phase, next_phase, occurred_at_ms),
        from_phase,
        next_phase,
        occurred_at_ms,
        phase_id.map(ToOwned::to_owned),
        reason.into().or_else(|| {
            Some(format!(
                "status_transition:{}",
                stateful_status_as_str(status)
            ))
        }),
    )?;
    history.push(transition);
    Ok(StatefulWorkflowPhaseState {
        phase: next_phase,
        phase_history: history,
        allowed_next_phases: next_phase.allowed_next_phases().to_vec(),
    })
}

pub fn phase_transition_event(
    run_id: impl Into<String>,
    seq: u64,
    scope: StatefulRuntimeScope,
    actor: Option<PrincipalRef>,
    transition: StatefulWorkflowPhaseTransitionRecord,
) -> StatefulRunEventRecord {
    StatefulRunEventRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        event_id: transition.event_id.clone(),
        run_id: run_id.into(),
        seq,
        event_type: transition.event_type.clone(),
        occurred_at_ms: transition.occurred_at_ms,
        scope,
        actor,
        phase_id: transition.phase_id.clone(),
        phase_transition: Some(transition.clone()),
        wait_kind: None,
        causation_id: None,
        correlation_id: None,
        payload: json!({
            "from_phase": transition.from_phase,
            "to_phase": transition.to_phase,
            "reason": transition.reason,
        }),
    }
}

fn phase_observed_event_id(
    run_id: &str,
    phase: StatefulWorkflowPhase,
    occurred_at_ms: u64,
) -> String {
    format!("{run_id}:phase:{}:{occurred_at_ms}", phase.as_str())
}

fn phase_transition_event_id(
    run_id: &str,
    from_phase: StatefulWorkflowPhase,
    to_phase: StatefulWorkflowPhase,
    occurred_at_ms: u64,
) -> String {
    format!(
        "{run_id}:phase_transition:{}:{}:{occurred_at_ms}",
        from_phase.as_str(),
        to_phase.as_str()
    )
}

fn stateful_status_as_str(status: &StatefulWorkflowRunStatus) -> &'static str {
    match status {
        StatefulWorkflowRunStatus::Queued => "queued",
        StatefulWorkflowRunStatus::Running => "running",
        StatefulWorkflowRunStatus::Sleeping => "sleeping",
        StatefulWorkflowRunStatus::AwaitingWebhook => "awaiting_webhook",
        StatefulWorkflowRunStatus::AwaitingApproval => "awaiting_approval",
        StatefulWorkflowRunStatus::Pausing => "pausing",
        StatefulWorkflowRunStatus::Paused => "paused",
        StatefulWorkflowRunStatus::Retrying => "retrying",
        StatefulWorkflowRunStatus::Blocked => "blocked",
        StatefulWorkflowRunStatus::Completed => "completed",
        StatefulWorkflowRunStatus::Failed => "failed",
        StatefulWorkflowRunStatus::Cancelled => "cancelled",
        StatefulWorkflowRunStatus::DeadLettered => "dead_lettered",
        StatefulWorkflowRunStatus::DryRun => "dry_run",
    }
}

fn default_schema_version() -> u32 {
    STATEFUL_RUNTIME_SCHEMA_VERSION
}

#[cfg(test)]
mod tests {
    use tandem_types::TenantContext;

    use super::*;

    #[test]
    fn transition_matrix_allows_expected_runtime_paths() {
        let running_next = StatefulWorkflowPhase::RunningPhase.allowed_next_phases();
        assert!(running_next.contains(&StatefulWorkflowPhase::Sleeping));
        assert!(running_next.contains(&StatefulWorkflowPhase::WaitingWebhook));
        assert!(running_next.contains(&StatefulWorkflowPhase::AwaitingApproval));
        assert!(running_next.contains(&StatefulWorkflowPhase::Completed));
        assert!(StatefulWorkflowPhase::Sleeping
            .validate_transition_to(StatefulWorkflowPhase::RunningPhase)
            .is_ok());
        assert!(StatefulWorkflowPhase::Queued
            .validate_transition_to(StatefulWorkflowPhase::RunningPhase)
            .is_ok());
    }

    #[test]
    fn invalid_and_terminal_transitions_are_rejected() {
        let invalid = StatefulWorkflowPhase::Completed
            .validate_transition_to(StatefulWorkflowPhase::RunningPhase)
            .expect_err("terminal transition must fail");
        assert_eq!(invalid.from_phase, StatefulWorkflowPhase::Completed);
        assert!(invalid.to_string().contains("terminal phase"));

        let backward = StatefulWorkflowPhaseTransitionRecord::new(
            "event-a",
            StatefulWorkflowPhase::AwaitingApproval,
            StatefulWorkflowPhase::Queued,
            42,
            Some("approve-plan".to_string()),
            None,
        );
        assert!(backward.is_err());
    }

    #[test]
    fn existing_statuses_map_to_explicit_phases() {
        use StatefulWorkflowRunStatus as Status;

        let cases = [
            (Status::Queued, StatefulWorkflowPhase::Queued),
            (Status::Running, StatefulWorkflowPhase::RunningPhase),
            (Status::Sleeping, StatefulWorkflowPhase::Sleeping),
            (
                Status::AwaitingWebhook,
                StatefulWorkflowPhase::WaitingWebhook,
            ),
            (
                Status::AwaitingApproval,
                StatefulWorkflowPhase::AwaitingApproval,
            ),
            (
                Status::Pausing,
                StatefulWorkflowPhase::PausedAttentionRequired,
            ),
            (
                Status::Paused,
                StatefulWorkflowPhase::PausedAttentionRequired,
            ),
            (
                Status::Blocked,
                StatefulWorkflowPhase::PausedAttentionRequired,
            ),
            (Status::Retrying, StatefulWorkflowPhase::Retrying),
            (Status::Completed, StatefulWorkflowPhase::Completed),
            (Status::DryRun, StatefulWorkflowPhase::Completed),
            (Status::Failed, StatefulWorkflowPhase::Failed),
            (Status::DeadLettered, StatefulWorkflowPhase::Failed),
            (Status::Cancelled, StatefulWorkflowPhase::Cancelled),
        ];

        for (status, phase) in cases {
            assert_eq!(phase_from_stateful_status(&status), phase);
        }
    }

    #[test]
    fn phase_state_from_status_exposes_history_and_allowed_transitions() {
        let state = phase_state_from_status(
            "run-a",
            &StatefulWorkflowRunStatus::AwaitingApproval,
            123,
            Some("approve-plan"),
        );

        assert_eq!(state.phase, StatefulWorkflowPhase::AwaitingApproval);
        assert_eq!(
            state.allowed_next_phases,
            vec![
                StatefulWorkflowPhase::RunningPhase,
                StatefulWorkflowPhase::PausedAttentionRequired,
                StatefulWorkflowPhase::Failed,
                StatefulWorkflowPhase::Cancelled,
            ]
        );
        assert_eq!(state.phase_history.len(), 1);
        assert_eq!(
            state.phase_history[0].reason.as_deref(),
            Some("observed_status:awaiting_approval")
        );
    }

    #[test]
    fn guarded_phase_state_accumulates_and_rejects_illegal_transitions() {
        let queued =
            phase_state_from_status("run-a", &StatefulWorkflowRunStatus::Queued, 100, None);
        let running = guarded_phase_state_from_status(
            "run-a",
            &StatefulWorkflowRunStatus::Running,
            200,
            Some("node-a"),
            Some(queued.phase),
            &queued.phase_history,
            Some("executor claimed run".to_string()),
        )
        .expect("queued to running");

        assert_eq!(running.phase, StatefulWorkflowPhase::RunningPhase);
        assert_eq!(running.phase_history.len(), 2);
        assert_eq!(
            running.phase_history[1].from_phase,
            Some(StatefulWorkflowPhase::Queued)
        );
        assert_eq!(
            running.phase_history[1].to_phase,
            StatefulWorkflowPhase::RunningPhase
        );

        let completed = guarded_phase_state_from_status(
            "run-a",
            &StatefulWorkflowRunStatus::Completed,
            300,
            None,
            Some(running.phase),
            &running.phase_history,
            Some("executor completed run".to_string()),
        )
        .expect("running to completed");
        let err = guarded_phase_state_from_status(
            "run-a",
            &StatefulWorkflowRunStatus::Running,
            400,
            None,
            Some(completed.phase),
            &completed.phase_history,
            Some("scheduler attempted wake".to_string()),
        )
        .expect_err("terminal completed run must not transition to running");
        assert_eq!(err.from_phase, StatefulWorkflowPhase::Completed);
        assert_eq!(err.to_phase, StatefulWorkflowPhase::RunningPhase);
    }

    #[test]
    fn validated_transition_can_be_emitted_as_run_event() {
        let transition = StatefulWorkflowPhaseTransitionRecord::new(
            "event-a",
            StatefulWorkflowPhase::RunningPhase,
            StatefulWorkflowPhase::AwaitingApproval,
            99,
            Some("approve-plan".to_string()),
            Some("approval gate opened".to_string()),
        )
        .expect("transition");

        let event = phase_transition_event(
            "run-a",
            7,
            StatefulRuntimeScope::from_tenant_context(TenantContext::explicit_user_workspace(
                "org-a",
                "workspace-a",
                None,
                "user-a",
            )),
            None,
            transition,
        );

        assert_eq!(event.event_type, PHASE_TRANSITION_EVENT_TYPE);
        assert_eq!(event.phase_id.as_deref(), Some("approve-plan"));
        assert_eq!(
            event
                .phase_transition
                .as_ref()
                .map(|transition| transition.to_phase),
            Some(StatefulWorkflowPhase::AwaitingApproval)
        );
        assert_eq!(
            event
                .payload
                .get("to_phase")
                .and_then(|value| value.as_str()),
            Some("awaiting_approval")
        );
    }
}
