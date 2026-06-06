// Workflow Learning v1 run finalization (GCL-04 / TAN-44). Split out of part02.rs
// to keep that file under the line-count gate.

impl AppState {
    async fn finalize_terminal_automation_v2_run_learning(
        &self,
        run: &AutomationV2RunRecord,
    ) -> anyhow::Result<()> {
        let learning_policy =
            crate::workflow_learning_policy::WorkflowLearningPromotionPolicy::from_env();
        let automation = if let Some(snapshot) = run.automation_snapshot.clone() {
            snapshot
        } else if let Some(current) = self.get_automation_v2(&run.automation_id).await {
            current
        } else {
            return Ok(());
        };
        let recent_runs = self
            .list_automation_v2_runs(Some(&run.automation_id), 50)
            .await;
        let metrics =
            crate::app::state::automation::workflow_learning_metrics_snapshot(&recent_runs);
        let existing_candidates = self
            .list_workflow_learning_candidates(Some(&run.automation_id), None, None)
            .await;
        let generated =
            crate::app::state::automation::workflow_learning_candidates_for_terminal_run(
                &automation,
                run,
                &recent_runs,
                &existing_candidates,
            );
        let mut generated_candidate_ids = Vec::new();
        for candidate in generated {
            let stored = self.upsert_workflow_learning_candidate(candidate).await?;
            // Consult the promotion policy. Fail closed: with the default
            // (auto-apply disabled) this is always RequireHumanReview/Block, so
            // candidates stay `Proposed` for the human review endpoint exactly as
            // before. Only an explicit opt-in can auto-apply a low-risk candidate.
            if stored.status == WorkflowLearningCandidateStatus::Proposed
                && learning_policy
                    .evaluate_promotion(&stored, &metrics)
                    .is_auto_apply()
            {
                let baseline = metrics.clone();
                let _ = self
                    .update_workflow_learning_candidate(&stored.candidate_id, |candidate| {
                        candidate.status = WorkflowLearningCandidateStatus::Applied;
                        // Capture the baseline the same way the review endpoint
                        // does, so the before/after regression gate can run.
                        candidate.baseline_before = Some(baseline.clone());
                    })
                    .await;
                self.event_bus.publish(EngineEvent::new(
                    "workflow_learning.candidate.auto_applied",
                    serde_json::json!({
                        "candidate_id": stored.candidate_id,
                        "workflow_id": stored.workflow_id,
                        "kind": format!("{:?}", stored.kind),
                        "confidence": stored.confidence,
                    }),
                ));
            }
            generated_candidate_ids.push(stored.candidate_id);
        }
        let candidate_ids = self
            .list_workflow_learning_candidates(Some(&run.automation_id), None, None)
            .await
            .into_iter()
            .filter(|candidate| {
                matches!(
                    candidate.status,
                    WorkflowLearningCandidateStatus::Approved
                        | WorkflowLearningCandidateStatus::Applied
                ) && candidate.baseline_before.is_some()
            })
            .map(|candidate| candidate.candidate_id)
            .collect::<Vec<_>>();
        for candidate_id in candidate_ids {
            let _ = self
                .update_workflow_learning_candidate(&candidate_id, |candidate| {
                    candidate.latest_observed_metrics = Some(metrics.clone());
                    if candidate.status == WorkflowLearningCandidateStatus::Applied {
                        if let Some(baseline) = candidate.baseline_before.as_ref() {
                            // Count terminal runs that finished *after* the baseline
                            // was captured. This is uncapped by the rolling window:
                            // subtracting capped snapshot sample sizes would pin the
                            // post-apply count at 0 on mature workflows, so a
                            // regression could never be detected.
                            let post_apply_sample_size = recent_runs
                                .iter()
                                .filter(|candidate_run| {
                                    candidate_run
                                        .finished_at_ms
                                        .is_some_and(|finished| finished > baseline.computed_at_ms)
                                })
                                .count();
                            // Route the before/after gate through the policy so the
                            // thresholds are centralized and testable. Default
                            // thresholds reproduce the prior inline behavior.
                            if learning_policy
                                .evaluate_regression(baseline, &metrics, post_apply_sample_size)
                                .is_regressed()
                            {
                                candidate.status = WorkflowLearningCandidateStatus::Regressed;
                            }
                        }
                    }
                })
                .await;
        }
        let updated_run = {
            let mut guard = self.automation_v2_runs.write().await;
            let Some(stored_run) = guard.get_mut(&run.run_id) else {
                return Ok(());
            };
            let summary = stored_run
                .learning_summary
                .get_or_insert_with(WorkflowLearningRunSummary::default);
            for candidate_id in generated_candidate_ids {
                if !summary
                    .generated_candidate_ids
                    .iter()
                    .any(|value| value == &candidate_id)
                {
                    summary.generated_candidate_ids.push(candidate_id);
                }
            }
            summary.post_run_metrics = Some(metrics);
            stored_run.clone()
        };
        self.persist_automation_v2_runs().await?;
        self.persist_automation_v2_run_status_json(&updated_run)
            .await?;
        Ok(())
    }
}
