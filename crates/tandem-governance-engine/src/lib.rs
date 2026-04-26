// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde_json::json;
use tandem_enterprise_contract::governance::{
    AgentCreationReviewSummary, AgentSpendSummary, AutomationDeclaredCapabilities,
    AutomationGovernanceRecord, AutomationLifecycleFinding, AutomationLifecycleFindingSeverity,
    AutomationLifecycleReviewKind, AutomationProvenanceRecord, GovernanceActorKind,
    GovernanceActorRef, GovernanceApprovalDraftInput, GovernanceApprovalRequest,
    GovernanceApprovalRequestType, GovernanceApprovalStatus, GovernanceAutomationReviewEvaluation,
    GovernanceContextSnapshot, GovernanceCreationReviewEvaluation,
    GovernanceDependencyRevocationInput, GovernanceError, GovernanceHealthCheckEvaluation,
    GovernanceHealthCheckInput, GovernancePolicyEngine, GovernanceResourceRef,
    GovernanceRetirementExtensionInput, GovernanceRetirementInput, GovernanceSpendEvaluation,
    GovernanceSpendHardStopRecord, GovernanceSpendInput, GovernanceSpendWarningRecord,
};
use uuid::Uuid;

#[derive(Default)]
pub struct DefaultGovernanceEngine;

impl DefaultGovernanceEngine {
    fn default_record(
        &self,
        automation_id: String,
        provenance: AutomationProvenanceRecord,
        declared_capabilities: AutomationDeclaredCapabilities,
        now_ms: u64,
    ) -> AutomationGovernanceRecord {
        AutomationGovernanceRecord {
            automation_id,
            provenance,
            declared_capabilities,
            modify_grants: Vec::new(),
            capability_grants: Vec::new(),
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            deleted_at_ms: None,
            delete_retention_until_ms: None,
            published_externally: false,
            creation_paused: false,
            review_required: false,
            review_kind: None,
            review_requested_at_ms: None,
            review_request_id: None,
            last_reviewed_at_ms: None,
            runs_since_review: 0,
            expires_at_ms: None,
            expired_at_ms: None,
            retired_at_ms: None,
            retire_reason: None,
            paused_for_lifecycle: false,
            health_last_checked_at_ms: None,
            health_findings: Vec::new(),
        }
    }

    fn validate_declared_capabilities_for_agent(
        &self,
        snapshot: &GovernanceContextSnapshot,
        agent_id: &str,
        declared_capabilities: &AutomationDeclaredCapabilities,
        previous_capabilities: Option<&AutomationDeclaredCapabilities>,
        now_ms: u64,
    ) -> Result<(), GovernanceError> {
        let previous = previous_capabilities.cloned().unwrap_or_default();
        for capability in declared_capabilities.escalates_from(&previous) {
            if !snapshot.has_approved_agent_capability(agent_id, capability, now_ms) {
                return Err(GovernanceError::forbidden(
                    "AUTOMATION_V2_CAPABILITY_ESCALATION_FORBIDDEN",
                    format!(
                        "agent {} lacks approval for capability {}",
                        agent_id, capability
                    ),
                ));
            }
        }
        Ok(())
    }
}

impl GovernancePolicyEngine for DefaultGovernanceEngine {
    fn premium_enabled(&self) -> bool {
        true
    }

    fn authorize_create(
        &self,
        snapshot: &GovernanceContextSnapshot,
        actor: &GovernanceActorRef,
        provenance: &AutomationProvenanceRecord,
        declared_capabilities: &AutomationDeclaredCapabilities,
        now_ms: u64,
    ) -> Result<(), GovernanceError> {
        let limits = &snapshot.limits;
        if !limits.creation_enabled {
            return Err(GovernanceError::forbidden(
                "AUTOMATION_V2_CREATION_DISABLED",
                "agent automation creation is disabled for this tenant",
            ));
        }
        if matches!(actor.kind, GovernanceActorKind::Agent) {
            let agent_id = actor.actor_id.as_deref().unwrap_or_default();
            if agent_id.is_empty() {
                return Err(GovernanceError::forbidden(
                    "AUTOMATION_V2_AGENT_ID_REQUIRED",
                    "agent automation creation requires an agent identifier",
                ));
            }
            if snapshot.is_agent_paused(agent_id) {
                return Err(GovernanceError::forbidden(
                    "AUTOMATION_V2_AGENT_CREATION_PAUSED",
                    "this agent is paused from creating automations",
                ));
            }
            if snapshot.is_agent_spend_paused(agent_id)
                && !snapshot.has_approved_agent_quota_override(agent_id, now_ms)
            {
                return Err(GovernanceError::too_many_requests(
                    "AUTOMATION_V2_AGENT_SPEND_CAP_EXCEEDED",
                    "this agent is paused after reaching its spend cap",
                ));
            }
            if snapshot
                .agent_creation_reviews
                .get(agent_id)
                .is_some_and(|summary| summary.review_required)
            {
                return Err(GovernanceError::too_many_requests(
                    "AUTOMATION_V2_AGENT_REVIEW_REQUIRED",
                    format!(
                        "agent {} must be reviewed before creating additional automations",
                        agent_id
                    ),
                ));
            }
            self.validate_declared_capabilities_for_agent(
                snapshot,
                agent_id,
                declared_capabilities,
                None,
                now_ms,
            )?;
            if provenance.depth > limits.lineage_depth_limit {
                return Err(GovernanceError::forbidden(
                    "AUTOMATION_V2_LINEAGE_DEPTH_EXCEEDED",
                    format!(
                        "lineage depth {} exceeds configured limit {}",
                        provenance.depth, limits.lineage_depth_limit
                    ),
                ));
            }
            let window_start = now_ms.saturating_sub(24 * 60 * 60 * 1000);
            let created_today = snapshot
                .records
                .values()
                .filter(|record| {
                    record.deleted_at_ms.is_none()
                        && record.provenance.creator.kind == GovernanceActorKind::Agent
                        && record
                            .provenance
                            .creator
                            .actor_id
                            .as_deref()
                            .is_some_and(|value| value == agent_id)
                        && record.created_at_ms >= window_start
                })
                .count() as u64;
            if created_today >= limits.per_agent_daily_creation_limit {
                return Err(GovernanceError::too_many_requests(
                    "AUTOMATION_V2_AGENT_DAILY_QUOTA_EXCEEDED",
                    format!(
                        "agent {} has reached the daily automation creation quota",
                        agent_id
                    ),
                ));
            }
            let active_agent_authored = snapshot
                .records
                .values()
                .filter(|record| {
                    record.deleted_at_ms.is_none()
                        && record.provenance.creator.kind == GovernanceActorKind::Agent
                })
                .count() as u64;
            if active_agent_authored >= limits.active_agent_automation_cap {
                return Err(GovernanceError::too_many_requests(
                    "AUTOMATION_V2_AGENT_CAP_EXCEEDED",
                    "tenant has reached the active agent-authored automation cap",
                ));
            }
        }
        Ok(())
    }

    fn authorize_capability_escalation(
        &self,
        snapshot: &GovernanceContextSnapshot,
        actor: &GovernanceActorRef,
        previous: &AutomationDeclaredCapabilities,
        next: &AutomationDeclaredCapabilities,
        now_ms: u64,
    ) -> Result<(), GovernanceError> {
        if matches!(actor.kind, GovernanceActorKind::Human) {
            return Ok(());
        }
        let Some(agent_id) = actor.actor_id.as_deref() else {
            return Err(GovernanceError::forbidden(
                "AUTOMATION_V2_AGENT_ID_REQUIRED",
                "agent automation requests require an agent identifier",
            ));
        };
        self.validate_declared_capabilities_for_agent(
            snapshot,
            agent_id,
            next,
            Some(previous),
            now_ms,
        )
    }

    fn authorize_mutation(
        &self,
        record: &AutomationGovernanceRecord,
        actor: &GovernanceActorRef,
        destructive: bool,
    ) -> Result<(), GovernanceError> {
        if matches!(actor.kind, GovernanceActorKind::Human) {
            return Ok(());
        }
        let Some(actor_id) = actor.actor_id.as_deref() else {
            return Err(GovernanceError::forbidden(
                "AUTOMATION_V2_AGENT_ID_REQUIRED",
                "agent automation requests require an agent identifier",
            ));
        };
        if record.retired_at_ms.is_some() {
            return Err(GovernanceError::forbidden(
                "AUTOMATION_V2_RETIRED",
                "retired automations are not mutable by agents",
            ));
        }
        if record.expired_at_ms.is_some() && record.paused_for_lifecycle {
            return Err(GovernanceError::forbidden(
                "AUTOMATION_V2_EXPIRED",
                "expired automations are paused pending human review",
            ));
        }
        if record.paused_for_lifecycle {
            return Err(GovernanceError::forbidden(
                "AUTOMATION_V2_LIFECYCLE_PAUSED",
                "paused automations are not mutable by agents",
            ));
        }
        if destructive {
            if record.provenance.creator.kind != GovernanceActorKind::Agent {
                return Err(GovernanceError::forbidden(
                    "AUTOMATION_V2_DELETE_HUMAN_CREATED_DENIED",
                    "agents cannot delete human-created automations",
                ));
            }
            if record.provenance.creator.actor_id.as_deref() != Some(actor_id) {
                return Err(GovernanceError::forbidden(
                    "AUTOMATION_V2_DELETE_NOT_OWNER",
                    "agents can only delete automations they created",
                ));
            }
            return Ok(());
        }
        if record.provenance.creator.kind == GovernanceActorKind::Agent
            && record.provenance.creator.actor_id.as_deref() == Some(actor_id)
        {
            return Ok(());
        }
        if record.has_modify_grant(actor_id) {
            return Ok(());
        }
        Err(GovernanceError::forbidden(
            "AUTOMATION_V2_MODIFY_FORBIDDEN",
            "agent lacks modify rights for this automation",
        ))
    }

    fn create_approval_request(
        &self,
        snapshot: &GovernanceContextSnapshot,
        input: GovernanceApprovalDraftInput,
        now_ms: u64,
    ) -> Result<GovernanceApprovalRequest, GovernanceError> {
        let expires_at_ms = input
            .expires_at_ms
            .unwrap_or_else(|| now_ms.saturating_add(snapshot.limits.approval_ttl_ms));
        Ok(GovernanceApprovalRequest {
            approval_id: format!("apr_{}", Uuid::new_v4().simple()),
            request_type: input.request_type,
            requested_by: input.requested_by,
            target_resource: input.target_resource,
            rationale: input.rationale,
            context: input.context,
            status: GovernanceApprovalStatus::Pending,
            expires_at_ms,
            reviewed_by: None,
            reviewed_at_ms: None,
            review_notes: None,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        })
    }

    fn decide_approval_request(
        &self,
        existing: &GovernanceApprovalRequest,
        reviewer: GovernanceActorRef,
        approved: bool,
        notes: Option<String>,
        now_ms: u64,
    ) -> Result<GovernanceApprovalRequest, GovernanceError> {
        if existing.status != GovernanceApprovalStatus::Pending {
            return Ok(existing.clone());
        }
        let mut next = existing.clone();
        next.status = if approved {
            GovernanceApprovalStatus::Approved
        } else {
            GovernanceApprovalStatus::Denied
        };
        next.reviewed_by = Some(reviewer);
        next.reviewed_at_ms = Some(now_ms);
        next.review_notes = notes;
        next.updated_at_ms = now_ms;
        Ok(next)
    }

    fn evaluate_creation_review_progress(
        &self,
        snapshot: &GovernanceContextSnapshot,
        agent_id: &str,
        automation_id: &str,
        now_ms: u64,
    ) -> Result<GovernanceCreationReviewEvaluation, GovernanceError> {
        let threshold = snapshot.limits.per_agent_creation_review_threshold;
        let mut summary = snapshot
            .agent_creation_reviews
            .get(agent_id)
            .cloned()
            .unwrap_or_else(|| AgentCreationReviewSummary::new(agent_id.to_string(), now_ms));
        summary.created_since_review = summary.created_since_review.saturating_add(1);
        summary.updated_at_ms = now_ms;

        let should_request =
            threshold > 0 && summary.created_since_review >= threshold && !summary.review_required;

        let approval_request = if should_request {
            summary.review_required = true;
            summary.review_kind = Some(AutomationLifecycleReviewKind::CreationQuota);
            summary.review_requested_at_ms = Some(now_ms);
            let request = self.create_approval_request(
                snapshot,
                GovernanceApprovalDraftInput {
                    request_type: GovernanceApprovalRequestType::LifecycleReview,
                    requested_by: GovernanceActorRef::system("automation_creation_review"),
                    target_resource: GovernanceResourceRef {
                        resource_type: "agent".to_string(),
                        id: agent_id.to_string(),
                    },
                    rationale: format!(
                        "Human acknowledgment required after agent {agent_id} created {} automations",
                        summary.created_since_review
                    ),
                    context: json!({
                        "trigger": "creation_quota",
                        "agentID": agent_id,
                        "automationID": automation_id,
                        "createdSinceReview": summary.created_since_review,
                        "creationReviewThreshold": threshold,
                    }),
                    expires_at_ms: None,
                },
                now_ms,
            )?;
            summary.review_request_id = Some(request.approval_id.clone());
            Some(request)
        } else {
            None
        };

        Ok(GovernanceCreationReviewEvaluation {
            summary,
            approval_request,
        })
    }

    fn evaluate_run_review_progress(
        &self,
        snapshot: &GovernanceContextSnapshot,
        automation_id: &str,
        reason: AutomationLifecycleReviewKind,
        run_id: Option<String>,
        detail: Option<String>,
        now_ms: u64,
    ) -> Result<Option<GovernanceAutomationReviewEvaluation>, GovernanceError> {
        let threshold = snapshot.limits.run_review_threshold;
        let Some(existing) = snapshot.records.get(automation_id).cloned() else {
            return Ok(None);
        };
        let mut record = existing;
        record.runs_since_review = record.runs_since_review.saturating_add(1);
        record.health_last_checked_at_ms = Some(now_ms);
        record.updated_at_ms = now_ms;
        let should_request =
            threshold > 0 && record.runs_since_review >= threshold && !record.review_required;
        let approval_request = if should_request {
            record.review_required = true;
            record.review_kind = Some(reason);
            record.review_requested_at_ms = Some(now_ms);
            let approval = self.create_approval_request(
                snapshot,
                GovernanceApprovalDraftInput {
                    request_type: GovernanceApprovalRequestType::LifecycleReview,
                    requested_by: GovernanceActorRef::system("automation_lifecycle_review"),
                    target_resource: GovernanceResourceRef {
                        resource_type: "automation".to_string(),
                        id: automation_id.to_string(),
                    },
                    rationale: format!(
                        "Human review required after automation {automation_id} completed {} runs without acknowledgment",
                        record.runs_since_review
                    ),
                    context: json!({
                        "trigger": "run_drift",
                        "automationID": automation_id,
                        "runID": run_id,
                        "detail": detail,
                        "runCountSinceReview": record.runs_since_review,
                        "reviewKind": "run_drift",
                    }),
                    expires_at_ms: None,
                },
                now_ms,
            )?;
            record.review_request_id = Some(approval.approval_id.clone());
            Some(approval)
        } else {
            None
        };
        Ok(Some(GovernanceAutomationReviewEvaluation {
            record,
            approval_request,
        }))
    }

    fn evaluate_health_check(
        &self,
        snapshot: &GovernanceContextSnapshot,
        input: GovernanceHealthCheckInput,
        now_ms: u64,
    ) -> Result<Option<GovernanceHealthCheckEvaluation>, GovernanceError> {
        let limits = &snapshot.limits;
        let mut record = input.current_record.unwrap_or_else(|| {
            self.default_record(
                input.automation_id.clone(),
                input.default_provenance,
                input.declared_capabilities.clone(),
                now_ms,
            )
        });
        record.declared_capabilities = input.declared_capabilities;
        record.health_last_checked_at_ms = Some(now_ms);

        let mut findings = Vec::new();
        if input.terminal_run_count > 0 {
            let failure_rate = input.failure_count as f64 / input.terminal_run_count as f64;
            if failure_rate >= limits.health_failure_rate_threshold && input.terminal_run_count >= 5
            {
                findings.push(AutomationLifecycleFinding {
                    finding_id: format!("finding-{}", Uuid::new_v4().simple()),
                    kind: AutomationLifecycleReviewKind::HealthDrift,
                    severity: if failure_rate >= 0.75 {
                        AutomationLifecycleFindingSeverity::Critical
                    } else {
                        AutomationLifecycleFindingSeverity::Warning
                    },
                    summary: "high failure rate across recent runs".to_string(),
                    detail: Some(format!(
                        "{} of {} recent terminal runs failed or were blocked ({:.0}% failure rate)",
                        input.failure_count,
                        input.terminal_run_count,
                        failure_rate * 100.0
                    )),
                    observed_at_ms: now_ms,
                    automation_run_id: input.last_terminal_run_id.clone(),
                    approval_id: None,
                    evidence: Some(json!({
                        "failureCount": input.failure_count,
                        "terminalRunCount": input.terminal_run_count,
                        "failureRate": failure_rate,
                    })),
                });
            }
        }
        if input.empty_output_count > 0 {
            findings.push(AutomationLifecycleFinding {
                finding_id: format!("finding-{}", Uuid::new_v4().simple()),
                kind: AutomationLifecycleReviewKind::HealthDrift,
                severity: AutomationLifecycleFindingSeverity::Warning,
                summary: "completed runs emitted empty outputs".to_string(),
                detail: Some(format!(
                    "{} recent completed runs produced no node outputs",
                    input.empty_output_count
                )),
                observed_at_ms: now_ms,
                automation_run_id: input.last_terminal_run_id.clone(),
                approval_id: None,
                evidence: Some(json!({
                    "emptyOutputCount": input.empty_output_count,
                })),
            });
        }
        if limits.health_guardrail_stop_threshold > 0
            && input.guardrail_stop_count >= limits.health_guardrail_stop_threshold
        {
            findings.push(AutomationLifecycleFinding {
                finding_id: format!("finding-{}", Uuid::new_v4().simple()),
                kind: AutomationLifecycleReviewKind::HealthDrift,
                severity: AutomationLifecycleFindingSeverity::Warning,
                summary: "repeated guardrail stops detected".to_string(),
                detail: Some(format!(
                    "{} recent terminal runs stopped on guardrails",
                    input.guardrail_stop_count
                )),
                observed_at_ms: now_ms,
                automation_run_id: input.last_terminal_run_id.clone(),
                approval_id: None,
                evidence: Some(json!({
                    "guardrailStopCount": input.guardrail_stop_count,
                })),
            });
        }

        let mut approval_requests = Vec::new();
        let mut pause_automation = false;
        let has_pending_lifecycle_review = snapshot.has_pending_approval_request(
            GovernanceApprovalRequestType::LifecycleReview,
            "automation",
            &input.automation_id,
            now_ms,
        );
        let has_pending_retirement_request = snapshot.has_pending_approval_request(
            GovernanceApprovalRequestType::RetirementAction,
            "automation",
            &input.automation_id,
            now_ms,
        );

        if !findings.is_empty() {
            record.review_required = true;
            record.review_kind = Some(AutomationLifecycleReviewKind::HealthDrift);
            if record.review_requested_at_ms.is_none() {
                record.review_requested_at_ms = Some(now_ms);
            }
            if !has_pending_lifecycle_review {
                approval_requests.push(self.create_approval_request(
                    snapshot,
                    GovernanceApprovalDraftInput {
                        request_type: GovernanceApprovalRequestType::LifecycleReview,
                        requested_by: GovernanceActorRef::system("automation_health_check"),
                        target_resource: GovernanceResourceRef {
                            resource_type: "automation".to_string(),
                            id: input.automation_id.clone(),
                        },
                        rationale: format!(
                            "Human review required after health check detected drift in automation {}",
                            input.automation_id
                        ),
                        context: json!({
                            "trigger": "health_drift",
                            "automationID": input.automation_id,
                            "findingCount": findings.len(),
                        }),
                        expires_at_ms: None,
                    },
                    now_ms,
                )?);
            }
        }

        if let Some(expires_at_ms) = record.expires_at_ms {
            if now_ms >= expires_at_ms && record.expired_at_ms.is_none() {
                record.expired_at_ms = Some(now_ms);
                record.review_required = true;
                record.review_kind = Some(AutomationLifecycleReviewKind::Expired);
                record.review_requested_at_ms = Some(now_ms);
                record.paused_for_lifecycle = true;
                pause_automation = true;
                findings.push(AutomationLifecycleFinding {
                    finding_id: format!("finding-{}", Uuid::new_v4().simple()),
                    kind: AutomationLifecycleReviewKind::Expired,
                    severity: AutomationLifecycleFindingSeverity::Critical,
                    summary: "automation has expired and was paused".to_string(),
                    detail: Some(format!(
                        "automation expired at {} and has been paused for human review",
                        expires_at_ms
                    )),
                    observed_at_ms: now_ms,
                    automation_run_id: input.last_terminal_run_id.clone(),
                    approval_id: None,
                    evidence: Some(json!({
                        "expiresAtMs": expires_at_ms,
                        "expiredAtMs": now_ms,
                    })),
                });
            } else if expires_at_ms > now_ms
                && expires_at_ms.saturating_sub(now_ms) <= limits.expiration_warning_window_ms
            {
                record.review_required = true;
                record.review_kind = Some(AutomationLifecycleReviewKind::ExpirationSoon);
                if record.review_requested_at_ms.is_none() {
                    record.review_requested_at_ms = Some(now_ms);
                }
                findings.push(AutomationLifecycleFinding {
                    finding_id: format!("finding-{}", Uuid::new_v4().simple()),
                    kind: AutomationLifecycleReviewKind::ExpirationSoon,
                    severity: AutomationLifecycleFindingSeverity::Info,
                    summary: "automation is approaching its expiration date".to_string(),
                    detail: Some(format!(
                        "automation expires in {}ms",
                        expires_at_ms.saturating_sub(now_ms)
                    )),
                    observed_at_ms: now_ms,
                    automation_run_id: None,
                    approval_id: None,
                    evidence: Some(json!({
                        "expiresAtMs": expires_at_ms,
                        "warningWindowMs": limits.expiration_warning_window_ms,
                    })),
                });
            }

            if (pause_automation
                || (expires_at_ms > now_ms
                    && expires_at_ms.saturating_sub(now_ms) <= limits.expiration_warning_window_ms))
                && !has_pending_retirement_request
            {
                approval_requests.push(self.create_approval_request(
                    snapshot,
                    GovernanceApprovalDraftInput {
                        request_type: GovernanceApprovalRequestType::RetirementAction,
                        requested_by: GovernanceActorRef::system("automation_expiration"),
                        target_resource: GovernanceResourceRef {
                            resource_type: "automation".to_string(),
                            id: input.automation_id.clone(),
                        },
                        rationale: format!(
                            "Automation {} is expiring or has expired and needs operator action",
                            input.automation_id
                        ),
                        context: json!({
                            "trigger": if pause_automation { "expired" } else { "expiration_soon" },
                            "automationID": input.automation_id,
                            "expiresAtMs": expires_at_ms,
                        }),
                        expires_at_ms: None,
                    },
                    now_ms,
                )?);
            }
        }

        record.health_findings = findings;
        record.updated_at_ms = now_ms;

        Ok(Some(GovernanceHealthCheckEvaluation {
            record,
            approval_requests,
            pause_automation,
        }))
    }

    fn evaluate_dependency_revocation(
        &self,
        snapshot: &GovernanceContextSnapshot,
        input: GovernanceDependencyRevocationInput,
        now_ms: u64,
    ) -> Result<GovernanceAutomationReviewEvaluation, GovernanceError> {
        let dependency_context = json!({
            "trigger": "dependency_revoked",
            "reason": input.reason,
            "evidence": input.evidence,
        });
        let mut record = input.current_record.unwrap_or_else(|| {
            self.default_record(
                input.automation_id.clone(),
                input.default_provenance,
                input.declared_capabilities.clone(),
                now_ms,
            )
        });
        record.declared_capabilities = input.declared_capabilities;
        record.paused_for_lifecycle = true;
        record.review_required = true;
        record.review_kind = Some(AutomationLifecycleReviewKind::DependencyRevoked);
        record.review_requested_at_ms = Some(now_ms);
        record.health_last_checked_at_ms = Some(now_ms);
        record.health_findings.push(AutomationLifecycleFinding {
            finding_id: format!("finding-{}", Uuid::new_v4().simple()),
            kind: AutomationLifecycleReviewKind::DependencyRevoked,
            severity: AutomationLifecycleFindingSeverity::Critical,
            summary: "automation paused after dependency revocation".to_string(),
            detail: Some(
                "an owned grant or connected MCP capability was removed and the automation was paused pending review"
                    .to_string(),
            ),
            observed_at_ms: now_ms,
            automation_run_id: None,
            approval_id: None,
            evidence: Some(dependency_context.clone()),
        });
        record.updated_at_ms = now_ms;

        let has_pending_lifecycle_review = snapshot.has_pending_approval_request(
            GovernanceApprovalRequestType::LifecycleReview,
            "automation",
            &input.automation_id,
            now_ms,
        );
        let approval_request = if has_pending_lifecycle_review {
            None
        } else {
            let approval = self.create_approval_request(
                snapshot,
                GovernanceApprovalDraftInput {
                    request_type: GovernanceApprovalRequestType::LifecycleReview,
                    requested_by: GovernanceActorRef::system("automation_dependency_revocation"),
                    target_resource: GovernanceResourceRef {
                        resource_type: "automation".to_string(),
                        id: input.automation_id.clone(),
                    },
                    rationale: format!(
                        "Human review required after dependency revocation paused automation {}",
                        input.automation_id
                    ),
                    context: dependency_context,
                    expires_at_ms: None,
                },
                now_ms,
            )?;
            record.review_request_id = Some(approval.approval_id.clone());
            Some(approval)
        };
        Ok(GovernanceAutomationReviewEvaluation {
            record,
            approval_request,
        })
    }

    fn evaluate_retirement(
        &self,
        input: GovernanceRetirementInput,
        now_ms: u64,
    ) -> Result<AutomationGovernanceRecord, GovernanceError> {
        let mut record = input.current_record.unwrap_or_else(|| {
            self.default_record(
                input.automation_id,
                input.default_provenance,
                input.declared_capabilities.clone(),
                now_ms,
            )
        });
        record.declared_capabilities = input.declared_capabilities;
        record.retired_at_ms = Some(now_ms);
        record.retire_reason = Some(input.reason);
        record.paused_for_lifecycle = true;
        record.review_required = false;
        record.review_kind = Some(AutomationLifecycleReviewKind::Retired);
        record.review_requested_at_ms = Some(now_ms);
        record.updated_at_ms = now_ms;
        Ok(record)
    }

    fn evaluate_retirement_extension(
        &self,
        input: GovernanceRetirementExtensionInput,
        now_ms: u64,
    ) -> Result<AutomationGovernanceRecord, GovernanceError> {
        let mut record = input.current_record.unwrap_or_else(|| {
            self.default_record(
                input.automation_id,
                input.default_provenance,
                input.declared_capabilities.clone(),
                now_ms,
            )
        });
        record.declared_capabilities = input.declared_capabilities;
        record.expires_at_ms = Some(input.expires_at_ms);
        record.expired_at_ms = None;
        record.retired_at_ms = None;
        record.retire_reason = None;
        record.paused_for_lifecycle = false;
        record.review_required = false;
        record.review_kind = None;
        record.review_requested_at_ms = None;
        record.review_request_id = None;
        record.last_reviewed_at_ms = Some(now_ms);
        record.health_last_checked_at_ms = Some(now_ms);
        record.updated_at_ms = now_ms;
        Ok(record)
    }

    fn evaluate_spend_usage(
        &self,
        snapshot: &GovernanceContextSnapshot,
        input: &GovernanceSpendInput,
        now_ms: u64,
    ) -> Result<GovernanceSpendEvaluation, GovernanceError> {
        let mut evaluation = GovernanceSpendEvaluation::default();
        let weekly_cap = snapshot.limits.weekly_spend_cap_usd;
        let warning_threshold_ratio = snapshot.limits.spend_warning_threshold_ratio;

        for agent_id in &input.agent_ids {
            let has_override = snapshot.has_approved_agent_quota_override(agent_id, now_ms);
            let mut summary = snapshot
                .agent_spend
                .get(agent_id)
                .cloned()
                .unwrap_or_else(|| AgentSpendSummary::new(agent_id.clone(), now_ms));
            summary.apply_usage(
                now_ms,
                Some(&input.automation_id),
                Some(&input.run_id),
                input.prompt_tokens,
                input.completion_tokens,
                input.total_tokens,
                input.delta_cost_usd,
            );

            if let Some(limit) = weekly_cap {
                if summary.weekly_warning_threshold_reached(limit, warning_threshold_ratio)
                    && summary.weekly.soft_warning_at_ms.is_none()
                {
                    summary.weekly.soft_warning_at_ms = Some(now_ms);
                    evaluation.warnings.push(GovernanceSpendWarningRecord {
                        agent_id: agent_id.clone(),
                        weekly_cost_usd: summary.weekly.cost_usd,
                        weekly_spend_cap_usd: limit,
                    });
                }

                if summary.weekly_limit_reached(limit)
                    && summary.weekly.hard_stop_at_ms.is_none()
                    && !has_override
                {
                    summary.weekly.hard_stop_at_ms = Some(now_ms);
                    summary.paused_at_ms = Some(now_ms);
                    summary.pause_reason =
                        Some(format!("weekly spend cap {:.2} USD reached", limit));
                    evaluation.hard_stops.push(GovernanceSpendHardStopRecord {
                        agent_id: agent_id.clone(),
                        weekly_cost_usd: summary.weekly.cost_usd,
                        weekly_spend_cap_usd: limit,
                    });
                    evaluation.spend_paused_agents.push(agent_id.clone());

                    if !snapshot.has_pending_agent_quota_override(agent_id, now_ms)
                        && !snapshot.has_approved_agent_quota_override(agent_id, now_ms)
                    {
                        let approval = self.create_approval_request(
                            snapshot,
                            GovernanceApprovalDraftInput {
                                request_type: GovernanceApprovalRequestType::QuotaOverride,
                                requested_by: GovernanceActorRef::system("automation_spend_cap"),
                                target_resource: GovernanceResourceRef {
                                    resource_type: "agent".to_string(),
                                    id: agent_id.clone(),
                                },
                                rationale: format!(
                                    "Approve temporary quota override after agent {agent_id} reached weekly spend cap"
                                ),
                                context: json!({
                                    "automationID": input.automation_id,
                                    "runID": input.run_id,
                                    "agentID": agent_id,
                                    "weeklyCostUsd": summary.weekly.cost_usd,
                                    "weeklySpendCapUsd": limit,
                                    "reason": "agent weekly spend cap exceeded",
                                }),
                                expires_at_ms: None,
                            },
                            now_ms,
                        )?;
                        evaluation.approvals.push(approval);
                    }
                }
            }

            evaluation.updated_summaries.push(summary);
        }

        Ok(evaluation)
    }
}
