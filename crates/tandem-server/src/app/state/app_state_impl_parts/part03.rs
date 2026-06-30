impl AppState {
    fn build_optimization_apply_patch(
        baseline: &crate::AutomationV2Spec,
        candidate: &crate::AutomationV2Spec,
        mutation: &crate::OptimizationValidatedMutation,
        approved_at_ms: u64,
    ) -> Result<Value, String> {
        let baseline_node = baseline
            .flow
            .nodes
            .iter()
            .find(|node| node.node_id == mutation.node_id)
            .ok_or_else(|| format!("baseline node `{}` not found", mutation.node_id))?;
        let candidate_node = candidate
            .flow
            .nodes
            .iter()
            .find(|node| node.node_id == mutation.node_id)
            .ok_or_else(|| format!("candidate node `{}` not found", mutation.node_id))?;
        let before = Self::optimization_node_field_value(baseline_node, mutation.field)?;
        let after = Self::optimization_node_field_value(candidate_node, mutation.field)?;
        Ok(json!({
            "node_id": mutation.node_id,
            "field": mutation.field,
            "field_path": Self::optimization_mutation_field_path(mutation.field),
            "expected_before": before,
            "apply_value": after,
            "approved_at_ms": approved_at_ms,
        }))
    }

    pub async fn apply_optimization_winner(
        &self,
        optimization_id: &str,
        experiment_id: &str,
    ) -> Result<
        (
            OptimizationCampaignRecord,
            OptimizationExperimentRecord,
            crate::AutomationV2Spec,
        ),
        String,
    > {
        let campaign = self
            .get_optimization_campaign(optimization_id)
            .await
            .ok_or_else(|| "optimization not found".to_string())?;
        let mut experiment = self
            .get_optimization_experiment(optimization_id, experiment_id)
            .await
            .ok_or_else(|| "experiment not found".to_string())?;
        if experiment.status != OptimizationExperimentStatus::PromotionApproved {
            return Err("only approved winner experiments may be applied".to_string());
        }
        if campaign.baseline_snapshot_hash != experiment.candidate_snapshot_hash {
            return Err(
                "only the latest approved winner may be applied to the live workflow".to_string(),
            );
        }
        let patch = experiment
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("apply_patch"))
            .cloned()
            .ok_or_else(|| "approved experiment is missing apply_patch metadata".to_string())?;
        let node_id = patch
            .get("node_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "apply_patch.node_id is required".to_string())?;
        let field: OptimizationMutableField = serde_json::from_value(
            patch
                .get("field")
                .cloned()
                .ok_or_else(|| "apply_patch.field is required".to_string())?,
        )
        .map_err(|error| format!("invalid apply_patch.field: {error}"))?;
        let expected_before = patch
            .get("expected_before")
            .cloned()
            .ok_or_else(|| "apply_patch.expected_before is required".to_string())?;
        let apply_value = patch
            .get("apply_value")
            .cloned()
            .ok_or_else(|| "apply_patch.apply_value is required".to_string())?;
        let mut live = self
            .get_automation_v2(&campaign.source_workflow_id)
            .await
            .ok_or_else(|| "source workflow not found".to_string())?;
        let current_value = {
            let live_node = live
                .flow
                .nodes
                .iter()
                .find(|node| node.node_id == node_id)
                .ok_or_else(|| format!("live workflow node `{node_id}` not found"))?;
            Self::optimization_node_field_value(live_node, field)?
        };
        if current_value != expected_before {
            return Err(format!(
                "live workflow drift detected for node `{node_id}` {}",
                Self::optimization_mutation_field_path(field)
            ));
        }
        let live_node = live
            .flow
            .nodes
            .iter_mut()
            .find(|node| node.node_id == node_id)
            .ok_or_else(|| format!("live workflow node `{node_id}` not found"))?;
        Self::set_optimization_node_field_value(live_node, field, &apply_value)?;
        let applied_at_ms = now_ms();
        let apply_record = json!({
            "optimization_id": campaign.optimization_id,
            "experiment_id": experiment.experiment_id,
            "node_id": node_id,
            "field": field,
            "field_path": Self::optimization_mutation_field_path(field),
            "previous_value": expected_before,
            "new_value": apply_value,
            "applied_at_ms": applied_at_ms,
        });
        live.metadata =
            Self::append_optimization_apply_metadata(live.metadata.clone(), apply_record)?;
        let stored_live = self
            .put_automation_v2(live)
            .await
            .map_err(|error| error.to_string())?;
        let mut metadata = match experiment.metadata.take() {
            Some(Value::Object(map)) => map,
            Some(_) => return Err("experiment metadata must be a JSON object".to_string()),
            None => serde_json::Map::new(),
        };
        metadata.insert(
            "applied_to_live".to_string(),
            json!({
                "automation_id": stored_live.automation_id,
                "applied_at_ms": applied_at_ms,
                "field": field,
                "node_id": node_id,
            }),
        );
        experiment.metadata = Some(Value::Object(metadata));
        let stored_experiment = self
            .put_optimization_experiment(experiment)
            .await
            .map_err(|error| error.to_string())?;
        Ok((campaign, stored_experiment, stored_live))
    }

    fn optimization_objective_hint(text: &str) -> String {
        let cleaned = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect::<Vec<_>>()
            .join(" ");
        let hint = if cleaned.is_empty() {
            "Prioritize validator-complete output with explicit evidence."
        } else {
            cleaned.as_str()
        };
        let trimmed = hint.trim();
        let clipped = if trimmed.len() > 140 {
            trimmed[..140].trim_end()
        } else {
            trimmed
        };
        let mut sentence = clipped.trim_end_matches('.').to_string();
        if sentence.is_empty() {
            sentence = "Prioritize validator-complete output with explicit evidence".to_string();
        }
        sentence.push('.');
        sentence
    }

    fn build_phase1_candidate_options(
        baseline: &crate::AutomationV2Spec,
        phase1: &crate::OptimizationPhase1Config,
    ) -> Vec<(
        crate::AutomationV2Spec,
        crate::OptimizationValidatedMutation,
    )> {
        let mut options = Vec::new();
        let hint = Self::optimization_objective_hint(&phase1.objective_markdown);
        for (index, node) in baseline.flow.nodes.iter().enumerate() {
            if phase1
                .mutation_policy
                .allowed_text_fields
                .contains(&OptimizationMutableField::Objective)
            {
                let addition = if node.objective.contains(&hint) {
                    "Prioritize validator-complete output with concrete evidence."
                } else {
                    &hint
                };
                let mut candidate = baseline.clone();
                candidate.flow.nodes[index].objective =
                    format!("{} {}", node.objective.trim(), addition.trim())
                        .trim()
                        .to_string();
                if let Ok(validated) =
                    validate_phase1_candidate_mutation(baseline, &candidate, phase1)
                {
                    options.push((candidate, validated));
                }
            }
            if phase1
                .mutation_policy
                .allowed_text_fields
                .contains(&OptimizationMutableField::OutputContractSummaryGuidance)
            {
                if let Some(summary_guidance) = node
                    .output_contract
                    .as_ref()
                    .and_then(|contract| contract.summary_guidance.as_ref())
                {
                    let addition = if summary_guidance.contains("Cite concrete evidence") {
                        "Keep evidence explicit."
                    } else {
                        "Cite concrete evidence in the summary."
                    };
                    let mut candidate = baseline.clone();
                    if let Some(contract) = candidate.flow.nodes[index].output_contract.as_mut() {
                        contract.summary_guidance = Some(
                            format!("{} {}", summary_guidance.trim(), addition)
                                .trim()
                                .to_string(),
                        );
                    }
                    if let Ok(validated) =
                        validate_phase1_candidate_mutation(baseline, &candidate, phase1)
                    {
                        options.push((candidate, validated));
                    }
                }
            }
            if phase1
                .mutation_policy
                .allowed_knob_fields
                .contains(&OptimizationMutableField::TimeoutMs)
            {
                if let Some(timeout_ms) = node.timeout_ms {
                    let delta_by_percent = ((timeout_ms as f64)
                        * phase1.mutation_policy.timeout_delta_percent)
                        .round() as u64;
                    let delta = delta_by_percent
                        .min(phase1.mutation_policy.timeout_delta_ms)
                        .max(1);
                    let next = timeout_ms
                        .saturating_add(delta)
                        .min(phase1.mutation_policy.timeout_max_ms);
                    if next != timeout_ms {
                        let mut candidate = baseline.clone();
                        candidate.flow.nodes[index].timeout_ms = Some(next);
                        if let Ok(validated) =
                            validate_phase1_candidate_mutation(baseline, &candidate, phase1)
                        {
                            options.push((candidate, validated));
                        }
                    }
                }
            }
            if phase1
                .mutation_policy
                .allowed_knob_fields
                .contains(&OptimizationMutableField::RetryPolicyMaxAttempts)
            {
                let current = node
                    .retry_policy
                    .as_ref()
                    .and_then(Value::as_object)
                    .and_then(|row| row.get("max_attempts"))
                    .and_then(Value::as_i64);
                if let Some(before) = current {
                    let next = (before + 1).min(phase1.mutation_policy.retry_max as i64);
                    if next != before {
                        let mut candidate = baseline.clone();
                        let policy = candidate.flow.nodes[index]
                            .retry_policy
                            .get_or_insert_with(|| json!({}));
                        if let Some(object) = policy.as_object_mut() {
                            object.insert("max_attempts".to_string(), json!(next));
                        }
                        if let Ok(validated) =
                            validate_phase1_candidate_mutation(baseline, &candidate, phase1)
                        {
                            options.push((candidate, validated));
                        }
                    }
                }
            }
            if phase1
                .mutation_policy
                .allowed_knob_fields
                .contains(&OptimizationMutableField::RetryPolicyRetries)
            {
                let current = node
                    .retry_policy
                    .as_ref()
                    .and_then(Value::as_object)
                    .and_then(|row| row.get("retries"))
                    .and_then(Value::as_i64);
                if let Some(before) = current {
                    let next = (before + 1).min(phase1.mutation_policy.retry_max as i64);
                    if next != before {
                        let mut candidate = baseline.clone();
                        let policy = candidate.flow.nodes[index]
                            .retry_policy
                            .get_or_insert_with(|| json!({}));
                        if let Some(object) = policy.as_object_mut() {
                            object.insert("retries".to_string(), json!(next));
                        }
                        if let Ok(validated) =
                            validate_phase1_candidate_mutation(baseline, &candidate, phase1)
                        {
                            options.push((candidate, validated));
                        }
                    }
                }
            }
        }
        options
    }

    async fn maybe_queue_phase1_candidate_experiment(
        &self,
        campaign: &mut OptimizationCampaignRecord,
    ) -> Result<bool, String> {
        let Some(phase1) = campaign.phase1.as_ref() else {
            return Ok(false);
        };
        let experiment_count = self
            .count_optimization_experiments(&campaign.optimization_id)
            .await;
        if experiment_count >= phase1.budget.max_experiments as usize {
            campaign.status = OptimizationCampaignStatus::Completed;
            campaign.last_pause_reason = Some("phase 1 experiment budget exhausted".to_string());
            campaign.updated_at_ms = now_ms();
            return Ok(true);
        }
        if campaign.baseline_metrics.is_none() || campaign.pending_promotion_experiment_id.is_some()
        {
            return Ok(false);
        }
        let existing = self
            .list_optimization_experiments(&campaign.optimization_id)
            .await;
        let active_eval_exists = existing.iter().any(|experiment| {
            matches!(experiment.status, OptimizationExperimentStatus::Draft)
                && experiment
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("eval_run_id"))
                    .and_then(Value::as_str)
                    .is_some()
        });
        if active_eval_exists {
            return Ok(false);
        }
        let existing_hashes = existing
            .iter()
            .map(|experiment| experiment.candidate_snapshot_hash.clone())
            .collect::<std::collections::HashSet<_>>();
        let options = Self::build_phase1_candidate_options(&campaign.baseline_snapshot, phase1);
        let Some((candidate_snapshot, mutation)) = options.into_iter().find(|(candidate, _)| {
            !existing_hashes.contains(&optimization_snapshot_hash(candidate))
        }) else {
            campaign.status = OptimizationCampaignStatus::Completed;
            campaign.last_pause_reason = Some(
                "phase 1 deterministic candidate mutator exhausted available mutations".to_string(),
            );
            campaign.updated_at_ms = now_ms();
            return Ok(true);
        };
        let eval_run = self
            .create_automation_v2_run(&candidate_snapshot, "optimization_candidate_eval")
            .await
            .map_err(|error| error.to_string())?;
        let now = now_ms();
        let experiment = OptimizationExperimentRecord {
            experiment_id: format!("opt-exp-{}", uuid::Uuid::new_v4()),
            optimization_id: campaign.optimization_id.clone(),
            status: OptimizationExperimentStatus::Draft,
            candidate_snapshot: candidate_snapshot.clone(),
            candidate_snapshot_hash: optimization_snapshot_hash(&candidate_snapshot),
            baseline_snapshot_hash: campaign.baseline_snapshot_hash.clone(),
            mutation_summary: Some(mutation.summary.clone()),
            metrics: None,
            phase1_metrics: None,
            promotion_recommendation: None,
            promotion_decision: None,
            created_at_ms: now,
            updated_at_ms: now,
            metadata: Some(json!({
                "generator": "phase1_deterministic_v1",
                "eval_run_id": eval_run.run_id,
                "mutation": mutation,
            })),
        };
        self.put_optimization_experiment(experiment)
            .await
            .map_err(|error| error.to_string())?;
        campaign.last_pause_reason = Some("waiting for phase 1 candidate evaluation".to_string());
        campaign.updated_at_ms = now_ms();
        Ok(true)
    }

    async fn reconcile_phase1_candidate_experiments(
        &self,
        campaign: &mut OptimizationCampaignRecord,
    ) -> Result<bool, String> {
        let Some(phase1) = campaign.phase1.as_ref() else {
            return Ok(false);
        };
        let Some(baseline_metrics) = campaign.baseline_metrics.as_ref() else {
            return Ok(false);
        };
        let experiments = self
            .list_optimization_experiments(&campaign.optimization_id)
            .await;
        let mut changed = false;
        for mut experiment in experiments {
            if experiment.status != OptimizationExperimentStatus::Draft {
                continue;
            }
            let Some(eval_run_id) = experiment
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("eval_run_id"))
                .and_then(Value::as_str)
                .map(str::to_string)
            else {
                continue;
            };
            let Some(run) = self.get_automation_v2_run(&eval_run_id).await else {
                continue;
            };
            if !Self::automation_run_is_terminal(&run.status) {
                continue;
            }
            if run.status != crate::AutomationRunStatus::Completed {
                experiment.status = OptimizationExperimentStatus::Failed;
                let mut metadata = match experiment.metadata.take() {
                    Some(Value::Object(map)) => map,
                    Some(_) => serde_json::Map::new(),
                    None => serde_json::Map::new(),
                };
                metadata.insert(
                    "eval_failure".to_string(),
                    json!({
                        "run_id": run.run_id,
                        "status": run.status,
                    }),
                );
                experiment.metadata = Some(Value::Object(metadata));
                self.put_optimization_experiment(experiment)
                    .await
                    .map_err(|error| error.to_string())?;
                changed = true;
                continue;
            }
            if experiment.baseline_snapshot_hash != campaign.baseline_snapshot_hash {
                experiment.status = OptimizationExperimentStatus::Failed;
                let mut metadata = match experiment.metadata.take() {
                    Some(Value::Object(map)) => map,
                    Some(_) => serde_json::Map::new(),
                    None => serde_json::Map::new(),
                };
                metadata.insert(
                    "eval_failure".to_string(),
                    json!({
                        "run_id": run.run_id,
                        "status": run.status,
                        "reason": "experiment baseline_snapshot_hash does not match current campaign baseline",
                    }),
                );
                experiment.metadata = Some(Value::Object(metadata));
                self.put_optimization_experiment(experiment)
                    .await
                    .map_err(|error| error.to_string())?;
                changed = true;
                continue;
            }
            let metrics =
                match derive_phase1_metrics_from_run(&run, &campaign.baseline_snapshot, phase1) {
                    Ok(metrics) => metrics,
                    Err(error) => {
                        experiment.status = OptimizationExperimentStatus::Failed;
                        let mut metadata = match experiment.metadata.take() {
                            Some(Value::Object(map)) => map,
                            Some(_) => serde_json::Map::new(),
                            None => serde_json::Map::new(),
                        };
                        metadata.insert(
                            "eval_failure".to_string(),
                            json!({
                                "run_id": run.run_id,
                                "status": run.status,
                                "reason": error,
                            }),
                        );
                        experiment.metadata = Some(Value::Object(metadata));
                        self.put_optimization_experiment(experiment)
                            .await
                            .map_err(|error| error.to_string())?;
                        changed = true;
                        continue;
                    }
                };
            let decision = evaluate_phase1_promotion(baseline_metrics, &metrics);
            experiment.phase1_metrics = Some(metrics.clone());
            experiment.metrics = Some(json!({
                "artifact_validator_pass_rate": metrics.artifact_validator_pass_rate,
                "unmet_requirement_count": metrics.unmet_requirement_count,
                "blocked_node_rate": metrics.blocked_node_rate,
                "budget_within_limits": metrics.budget_within_limits,
            }));
            experiment.promotion_recommendation = Some(
                match decision.decision {
                    OptimizationPromotionDecisionKind::Promote => "promote",
                    OptimizationPromotionDecisionKind::Discard => "discard",
                    OptimizationPromotionDecisionKind::NeedsOperatorReview => {
                        "needs_operator_review"
                    }
                }
                .to_string(),
            );
            experiment.promotion_decision = Some(decision.clone());
            match decision.decision {
                OptimizationPromotionDecisionKind::Promote
                | OptimizationPromotionDecisionKind::NeedsOperatorReview => {
                    experiment.status = OptimizationExperimentStatus::PromotionRecommended;
                    campaign.pending_promotion_experiment_id =
                        Some(experiment.experiment_id.clone());
                    campaign.status = OptimizationCampaignStatus::AwaitingPromotionApproval;
                    campaign.last_pause_reason = Some(decision.reason.clone());
                }
                OptimizationPromotionDecisionKind::Discard => {
                    experiment.status = OptimizationExperimentStatus::Discarded;
                    if campaign.status == OptimizationCampaignStatus::Running {
                        campaign.last_pause_reason = Some(decision.reason.clone());
                    }
                }
            }
            self.put_optimization_experiment(experiment)
                .await
                .map_err(|error| error.to_string())?;
            changed = true;
        }
        let refreshed = self
            .list_optimization_experiments(&campaign.optimization_id)
            .await;
        let consecutive_failures = Self::optimization_consecutive_failure_count(&refreshed);
        if consecutive_failures >= phase1.budget.max_consecutive_failures as usize
            && phase1.budget.max_consecutive_failures > 0
        {
            campaign.status = OptimizationCampaignStatus::Failed;
            campaign.last_pause_reason = Some(format!(
                "phase 1 candidate evaluations reached {} consecutive failures",
                consecutive_failures
            ));
            changed = true;
        }
        Ok(changed)
    }

    async fn reconcile_pending_baseline_replays(
        &self,
        campaign: &mut OptimizationCampaignRecord,
    ) -> Result<bool, String> {
        let Some(phase1) = campaign.phase1.as_ref() else {
            return Ok(false);
        };
        let mut changed = false;
        let mut remaining = Vec::new();
        for run_id in campaign.pending_baseline_run_ids.clone() {
            let Some(run) = self.get_automation_v2_run(&run_id).await else {
                campaign.status = OptimizationCampaignStatus::PausedEvaluatorUnstable;
                campaign.last_pause_reason = Some(format!(
                    "baseline replay run `{run_id}` was not found during optimization reconciliation"
                ));
                changed = true;
                continue;
            };
            if !Self::automation_run_is_terminal(&run.status) {
                remaining.push(run_id);
                continue;
            }
            if run.status != crate::AutomationRunStatus::Completed {
                campaign.status = OptimizationCampaignStatus::PausedEvaluatorUnstable;
                campaign.last_pause_reason = Some(format!(
                    "baseline replay run `{}` finished with status `{:?}`",
                    run.run_id, run.status
                ));
                changed = true;
                continue;
            }
            if run.automation_id != campaign.source_workflow_id {
                campaign.status = OptimizationCampaignStatus::PausedEvaluatorUnstable;
                campaign.last_pause_reason = Some(
                    "baseline replay run must belong to the optimization source workflow"
                        .to_string(),
                );
                changed = true;
                continue;
            }
            let snapshot = run.automation_snapshot.as_ref().ok_or_else(|| {
                "baseline replay run must include an automation snapshot".to_string()
            })?;
            if optimization_snapshot_hash(snapshot) != campaign.baseline_snapshot_hash {
                campaign.status = OptimizationCampaignStatus::PausedEvaluatorUnstable;
                campaign.last_pause_reason = Some(
                    "baseline replay run does not match the current campaign baseline snapshot"
                        .to_string(),
                );
                changed = true;
                continue;
            }
            let metrics =
                derive_phase1_metrics_from_run(&run, &campaign.baseline_snapshot, phase1)?;
            let validator_case_outcomes = derive_phase1_validator_case_outcomes_from_run(&run);
            campaign
                .baseline_replays
                .push(OptimizationBaselineReplayRecord {
                    replay_id: format!("baseline-replay-{}", uuid::Uuid::new_v4()),
                    automation_run_id: Some(run.run_id.clone()),
                    phase1_metrics: metrics,
                    validator_case_outcomes,
                    experiment_count_at_recording: self
                        .count_optimization_experiments(&campaign.optimization_id)
                        .await as u64,
                    recorded_at_ms: now_ms(),
                });
            changed = true;
        }
        if remaining != campaign.pending_baseline_run_ids {
            campaign.pending_baseline_run_ids = remaining;
            changed = true;
        }
        Ok(changed)
    }

    pub async fn reconcile_optimization_campaigns(&self) -> Result<usize, String> {
        let campaigns = self.list_optimization_campaigns().await;
        let mut updated = 0usize;
        for campaign in campaigns {
            let Some(mut latest) = self
                .get_optimization_campaign(&campaign.optimization_id)
                .await
            else {
                continue;
            };
            let Some(phase1) = latest.phase1.clone() else {
                continue;
            };
            let mut changed = self.reconcile_pending_baseline_replays(&mut latest).await?;
            changed |= self
                .reconcile_phase1_candidate_experiments(&mut latest)
                .await?;
            let experiment_count = self
                .count_optimization_experiments(&latest.optimization_id)
                .await;
            if latest.pending_baseline_run_ids.is_empty() {
                if phase1_baseline_replay_due(
                    &latest.baseline_replays,
                    latest.pending_baseline_run_ids.len(),
                    &phase1,
                    experiment_count,
                    now_ms(),
                ) {
                    if self.maybe_queue_phase1_baseline_replay(&mut latest).await? {
                        latest.status = OptimizationCampaignStatus::Draft;
                        changed = true;
                    }
                } else if latest.baseline_replays.len()
                    >= phase1.eval.campaign_start_baseline_runs.max(1) as usize
                {
                    match establish_phase1_baseline(&latest.baseline_replays, &phase1) {
                        Ok(metrics) => {
                            if latest.baseline_metrics.as_ref() != Some(&metrics) {
                                latest.baseline_metrics = Some(metrics);
                                changed = true;
                            }
                            if matches!(
                                latest.status,
                                OptimizationCampaignStatus::Draft
                                    | OptimizationCampaignStatus::PausedEvaluatorUnstable
                            ) || (latest.status == OptimizationCampaignStatus::Running
                                && latest.last_pause_reason.is_some())
                            {
                                latest.status = OptimizationCampaignStatus::Running;
                                latest.last_pause_reason = None;
                                changed = true;
                            }
                        }
                        Err(error) => {
                            if matches!(
                                latest.status,
                                OptimizationCampaignStatus::Draft
                                    | OptimizationCampaignStatus::Running
                                    | OptimizationCampaignStatus::PausedEvaluatorUnstable
                            ) && (latest.status
                                != OptimizationCampaignStatus::PausedEvaluatorUnstable
                                || latest.last_pause_reason.as_deref() != Some(error.as_str()))
                            {
                                latest.status = OptimizationCampaignStatus::PausedEvaluatorUnstable;
                                latest.last_pause_reason = Some(error);
                                changed = true;
                            }
                        }
                    }
                }
            } else if latest.last_pause_reason.as_deref()
                != Some("waiting for phase 1 baseline replay completion")
            {
                latest.last_pause_reason =
                    Some("waiting for phase 1 baseline replay completion".to_string());
                changed = true;
            }
            if latest.status == OptimizationCampaignStatus::Running
                && latest.pending_baseline_run_ids.is_empty()
            {
                changed |= self
                    .maybe_queue_phase1_candidate_experiment(&mut latest)
                    .await?;
            }
            if changed {
                self.put_optimization_campaign(latest)
                    .await
                    .map_err(|error| error.to_string())?;
                updated = updated.saturating_add(1);
            }
        }
        Ok(updated)
    }

    async fn maybe_queue_phase1_baseline_replay(
        &self,
        campaign: &mut OptimizationCampaignRecord,
    ) -> Result<bool, String> {
        let Some(phase1) = campaign.phase1.as_ref() else {
            return Ok(false);
        };
        if !campaign.pending_baseline_run_ids.is_empty() {
            campaign.last_pause_reason =
                Some("waiting for phase 1 baseline replay completion".into());
            campaign.updated_at_ms = now_ms();
            return Ok(true);
        }
        let experiment_count = self
            .count_optimization_experiments(&campaign.optimization_id)
            .await;
        if !phase1_baseline_replay_due(
            &campaign.baseline_replays,
            campaign.pending_baseline_run_ids.len(),
            phase1,
            experiment_count,
            now_ms(),
        ) {
            return Ok(false);
        }
        let replay_run = self
            .create_automation_v2_run(&campaign.baseline_snapshot, "optimization_baseline_replay")
            .await
            .map_err(|error| error.to_string())?;
        if !campaign
            .pending_baseline_run_ids
            .iter()
            .any(|value| value == &replay_run.run_id)
        {
            campaign
                .pending_baseline_run_ids
                .push(replay_run.run_id.clone());
        }
        campaign.last_pause_reason = Some("waiting for phase 1 baseline replay completion".into());
        campaign.updated_at_ms = now_ms();
        Ok(true)
    }

    async fn maybe_queue_initial_phase1_baseline_replay(
        &self,
        campaign: &mut OptimizationCampaignRecord,
    ) -> Result<bool, String> {
        let Some(phase1) = campaign.phase1.as_ref() else {
            return Ok(false);
        };
        let required_runs = phase1.eval.campaign_start_baseline_runs.max(1) as usize;
        if campaign.baseline_replays.len() >= required_runs {
            return Ok(false);
        }
        self.maybe_queue_phase1_baseline_replay(campaign).await
    }

    pub async fn apply_optimization_action(
        &self,
        optimization_id: &str,
        action: &str,
        experiment_id: Option<&str>,
        run_id: Option<&str>,
        reason: Option<&str>,
    ) -> Result<OptimizationCampaignRecord, String> {
        let normalized = action.trim().to_ascii_lowercase();
        let mut campaign = self
            .get_optimization_campaign(optimization_id)
            .await
            .ok_or_else(|| "optimization not found".to_string())?;
        match normalized.as_str() {
            "start" => {
                if campaign.phase1.is_some() {
                    if self
                        .maybe_queue_initial_phase1_baseline_replay(&mut campaign)
                        .await?
                    {
                        campaign.status = OptimizationCampaignStatus::Draft;
                    } else {
                        let phase1 = campaign
                            .phase1
                            .as_ref()
                            .ok_or_else(|| "phase 1 config is required".to_string())?;
                        match establish_phase1_baseline(&campaign.baseline_replays, phase1) {
                            Ok(metrics) => {
                                campaign.baseline_metrics = Some(metrics);
                                campaign.status = OptimizationCampaignStatus::Running;
                                campaign.last_pause_reason = None;
                            }
                            Err(error) => {
                                campaign.status =
                                    OptimizationCampaignStatus::PausedEvaluatorUnstable;
                                campaign.last_pause_reason = Some(error);
                            }
                        }
                    }
                } else {
                    campaign.status = OptimizationCampaignStatus::Running;
                    campaign.last_pause_reason = None;
                }
            }
            "pause" => {
                campaign.status = OptimizationCampaignStatus::PausedManual;
                campaign.last_pause_reason = reason
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
            }
            "resume" => {
                if self
                    .maybe_queue_initial_phase1_baseline_replay(&mut campaign)
                    .await?
                {
                    campaign.status = OptimizationCampaignStatus::Draft;
                } else {
                    campaign.status = OptimizationCampaignStatus::Running;
                    campaign.last_pause_reason = None;
                }
            }
            "queue_baseline_replay" => {
                let replay_run = self
                    .create_automation_v2_run(
                        &campaign.baseline_snapshot,
                        "optimization_baseline_replay",
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                if !campaign
                    .pending_baseline_run_ids
                    .iter()
                    .any(|value| value == &replay_run.run_id)
                {
                    campaign
                        .pending_baseline_run_ids
                        .push(replay_run.run_id.clone());
                }
                campaign.updated_at_ms = now_ms();
            }
            "record_baseline_replay" => {
                let run_id = run_id
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| "run_id is required for record_baseline_replay".to_string())?;
                let phase1 = campaign
                    .phase1
                    .as_ref()
                    .ok_or_else(|| "phase 1 config is required for baseline replay".to_string())?;
                let run = self
                    .get_automation_v2_run(run_id)
                    .await
                    .ok_or_else(|| "automation run not found".to_string())?;
                if run.automation_id != campaign.source_workflow_id {
                    return Err(
                        "baseline replay run must belong to the optimization source workflow"
                            .to_string(),
                    );
                }
                let snapshot = run.automation_snapshot.as_ref().ok_or_else(|| {
                    "baseline replay run must include an automation snapshot".to_string()
                })?;
                if optimization_snapshot_hash(snapshot) != campaign.baseline_snapshot_hash {
                    return Err(
                        "baseline replay run does not match the current campaign baseline snapshot"
                            .to_string(),
                    );
                }
                let metrics =
                    derive_phase1_metrics_from_run(&run, &campaign.baseline_snapshot, phase1)?;
                let validator_case_outcomes = derive_phase1_validator_case_outcomes_from_run(&run);
                campaign
                    .baseline_replays
                    .push(OptimizationBaselineReplayRecord {
                        replay_id: format!("baseline-replay-{}", uuid::Uuid::new_v4()),
                        automation_run_id: Some(run.run_id.clone()),
                        phase1_metrics: metrics,
                        validator_case_outcomes,
                        experiment_count_at_recording: self
                            .count_optimization_experiments(&campaign.optimization_id)
                            .await as u64,
                        recorded_at_ms: now_ms(),
                    });
                campaign
                    .pending_baseline_run_ids
                    .retain(|value| value != run_id);
                campaign.updated_at_ms = now_ms();
            }
            "approve_winner" => {
                let experiment_id = experiment_id
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| "experiment_id is required for approve_winner".to_string())?;
                let mut experiment = self
                    .get_optimization_experiment(optimization_id, experiment_id)
                    .await
                    .ok_or_else(|| "experiment not found".to_string())?;
                if experiment.baseline_snapshot_hash != campaign.baseline_snapshot_hash {
                    return Err(
                        "experiment baseline_snapshot_hash does not match current campaign baseline"
                            .to_string(),
                    );
                }
                if let Some(phase1) = campaign.phase1.as_ref() {
                    let validated = validate_phase1_candidate_mutation(
                        &campaign.baseline_snapshot,
                        &experiment.candidate_snapshot,
                        phase1,
                    )?;
                    if experiment.mutation_summary.is_none() {
                        experiment.mutation_summary = Some(validated.summary.clone());
                    }
                    let approved_at_ms = now_ms();
                    let apply_patch = Self::build_optimization_apply_patch(
                        &campaign.baseline_snapshot,
                        &experiment.candidate_snapshot,
                        &validated,
                        approved_at_ms,
                    )?;
                    let mut metadata = match experiment.metadata.take() {
                        Some(Value::Object(map)) => map,
                        Some(_) => {
                            return Err("experiment metadata must be a JSON object".to_string());
                        }
                        None => serde_json::Map::new(),
                    };
                    metadata.insert("apply_patch".to_string(), apply_patch);
                    experiment.metadata = Some(Value::Object(metadata));
                    if let Some(baseline_metrics) = campaign.baseline_metrics.as_ref() {
                        let candidate_metrics = experiment
                            .phase1_metrics
                            .clone()
                            .or_else(|| {
                                experiment
                                    .metrics
                                    .as_ref()
                                    .and_then(|metrics| parse_phase1_metrics(metrics).ok())
                            })
                            .ok_or_else(|| {
                                "phase 1 candidate is missing promotion metrics".to_string()
                            })?;
                        let decision =
                            evaluate_phase1_promotion(baseline_metrics, &candidate_metrics);
                        experiment.promotion_recommendation = Some(
                            match decision.decision {
                                OptimizationPromotionDecisionKind::Promote => "promote",
                                OptimizationPromotionDecisionKind::Discard => "discard",
                                OptimizationPromotionDecisionKind::NeedsOperatorReview => {
                                    "needs_operator_review"
                                }
                            }
                            .to_string(),
                        );
                        experiment.promotion_decision = Some(decision.clone());
                        if decision.decision != OptimizationPromotionDecisionKind::Promote {
                            let _ = self
                                .put_optimization_experiment(experiment)
                                .await
                                .map_err(|e| e.to_string())?;
                            return Err(decision.reason);
                        }
                        campaign.baseline_metrics = Some(candidate_metrics);
                    }
                }
                campaign.baseline_snapshot = experiment.candidate_snapshot.clone();
                campaign.baseline_snapshot_hash = experiment.candidate_snapshot_hash.clone();
                campaign.baseline_replays.clear();
                campaign.pending_baseline_run_ids.clear();
                campaign.pending_promotion_experiment_id = None;
                campaign.status = OptimizationCampaignStatus::Draft;
                campaign.last_pause_reason = None;
                experiment.status = OptimizationExperimentStatus::PromotionApproved;
                let _ = self
                    .put_optimization_experiment(experiment)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            "reject_winner" => {
                let experiment_id = experiment_id
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| "experiment_id is required for reject_winner".to_string())?;
                let mut experiment = self
                    .get_optimization_experiment(optimization_id, experiment_id)
                    .await
                    .ok_or_else(|| "experiment not found".to_string())?;
                campaign.pending_promotion_experiment_id = None;
                campaign.status = OptimizationCampaignStatus::Draft;
                campaign.last_pause_reason = reason
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                experiment.status = OptimizationExperimentStatus::PromotionRejected;
                let _ = self
                    .put_optimization_experiment(experiment)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            _ => return Err("unsupported optimization action".to_string()),
        }
        self.put_optimization_campaign(campaign)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn list_automations_v2(&self) -> Vec<AutomationV2Spec> {
        let mut rows = self
            .automations_v2
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.automation_id.cmp(&b.automation_id));
        rows
    }

    pub async fn delete_automation_v2(
        &self,
        automation_id: &str,
    ) -> anyhow::Result<Option<AutomationV2Spec>> {
        let deleted_by =
            crate::automation_v2::governance::GovernanceActorRef::system("automation_v2_delete");
        let removed = self
            .delete_automation_v2_with_governance(automation_id, deleted_by)
            .await?;
        let removed_run_count = {
            let mut runs = self.automation_v2_runs.write().await;
            let before = runs.len();
            runs.retain(|_, run| run.automation_id != automation_id);
            before.saturating_sub(runs.len())
        };
        if removed_run_count > 0 {
            self.persist_automation_v2_runs().await?;
        }
        self.verify_automation_v2_persisted_locked(automation_id, false)
            .await?;
        Ok(removed)
    }

    pub async fn create_automation_v2_run(
        &self,
        automation: &AutomationV2Spec,
        trigger_type: &str,
    ) -> anyhow::Result<AutomationV2RunRecord> {
        self.create_automation_v2_run_with_profile(automation, trigger_type, None)
            .await
    }

    pub async fn create_automation_v2_run_with_profile(
        &self,
        automation: &AutomationV2Spec,
        trigger_type: &str,
        requested_execution_profile: Option<
            crate::automation_v2::execution_profile::ExecutionProfile,
        >,
    ) -> anyhow::Result<AutomationV2RunRecord> {
        let now = now_ms();
        let runtime_context = self
            .automation_v2_effective_runtime_context(
                automation,
                automation
                    .runtime_context_materialization()
                    .or_else(|| automation.approved_plan_runtime_context_materialization()),
            )
            .await?;
        let pending_nodes = automation
            .flow
            .nodes
            .iter()
            .map(|n| n.node_id.clone())
            .collect::<Vec<_>>();
        let effective_execution_profile =
            crate::automation_v2::types::resolve_effective_execution_profile(
                automation,
                requested_execution_profile,
            );
        // Stamp the resolved profile onto the snapshot so downstream
        // validation logic that reads `automation.execution.profile`
        // (e.g. the repair-budget multiplier in
        // validate_automation_artifact_output_with_context) honors the
        // run-level override without needing the run record itself.
        let mut snapshot = automation.clone();
        snapshot.execution.profile = Some(effective_execution_profile);
        snapshot.stamp_enterprise_scope_metadata();
        let mut run = AutomationV2RunRecord {
            run_id: format!("automation-v2-run-{}", uuid::Uuid::new_v4()),
            automation_id: automation.automation_id.clone(),
            tenant_context: automation.tenant_context(),
            trigger_type: trigger_type.to_string(),
            status: AutomationRunStatus::Queued,
            created_at_ms: now,
            updated_at_ms: now,
            started_at_ms: None,
            finished_at_ms: None,
            active_session_ids: Vec::new(),
            latest_session_id: None,
            active_instance_ids: Vec::new(),
            checkpoint: AutomationRunCheckpoint {
                completed_nodes: Vec::new(),
                pending_nodes,
                node_outputs: std::collections::HashMap::new(),
                node_attempts: std::collections::HashMap::new(),
                node_attempt_verdicts: std::collections::HashMap::new(),
                blocked_nodes: Vec::new(),
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: Vec::new(),
                last_failure: None,
            },
            runtime_context,
            automation_snapshot: Some(snapshot),
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            execution_claim: None,
            execution_claim_epoch: 0,
            pause_reason: None,
            resume_reason: None,
            detail: None,
            stop_kind: None,
            stop_reason: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
            scheduler: None,
            trigger_reason: None,
            consumed_handoff_id: None,
            learning_summary: None,
            effective_execution_profile,
            requested_execution_profile,
        };
        crate::stateful_runtime::ensure_automation_run_definition_metadata(&mut run);
        self.automation_v2_runs
            .write()
            .await
            .insert(run.run_id.clone(), run.clone());
        self.persist_automation_v2_runs().await?;
        crate::http::context_runs::sync_automation_v2_run_blackboard(self, automation, &run)
            .await
            .map_err(|status| anyhow::anyhow!("failed to sync automation context run: {status}"))?;
        Ok(run)
    }

    pub async fn create_automation_v2_dry_run(
        &self,
        automation: &AutomationV2Spec,
        trigger_type: &str,
    ) -> anyhow::Result<AutomationV2RunRecord> {
        self.create_automation_v2_dry_run_with_profile(automation, trigger_type, None)
            .await
    }

    pub async fn create_automation_v2_dry_run_with_profile(
        &self,
        automation: &AutomationV2Spec,
        trigger_type: &str,
        requested_execution_profile: Option<
            crate::automation_v2::execution_profile::ExecutionProfile,
        >,
    ) -> anyhow::Result<AutomationV2RunRecord> {
        let now = now_ms();
        let runtime_context = self
            .automation_v2_effective_runtime_context(
                automation,
                automation
                    .runtime_context_materialization()
                    .or_else(|| automation.approved_plan_runtime_context_materialization()),
            )
            .await?;
        let effective_execution_profile =
            crate::automation_v2::types::resolve_effective_execution_profile(
                automation,
                requested_execution_profile,
            );
        // Stamp the resolved profile onto the snapshot — see
        // create_automation_v2_run_with_profile for rationale.
        let mut snapshot = automation.clone();
        snapshot.execution.profile = Some(effective_execution_profile);
        snapshot.stamp_enterprise_scope_metadata();
        let mut run = AutomationV2RunRecord {
            run_id: format!("automation-v2-run-{}", uuid::Uuid::new_v4()),
            automation_id: automation.automation_id.clone(),
            tenant_context: automation.tenant_context(),
            trigger_type: format!("{trigger_type}_dry_run"),
            status: AutomationRunStatus::Completed,
            created_at_ms: now,
            updated_at_ms: now,
            started_at_ms: Some(now),
            finished_at_ms: Some(now),
            active_session_ids: Vec::new(),
            latest_session_id: None,
            active_instance_ids: Vec::new(),
            checkpoint: AutomationRunCheckpoint {
                completed_nodes: Vec::new(),
                pending_nodes: Vec::new(),
                node_outputs: std::collections::HashMap::new(),
                node_attempts: std::collections::HashMap::new(),
                node_attempt_verdicts: std::collections::HashMap::new(),
                blocked_nodes: Vec::new(),
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: Vec::new(),
                last_failure: None,
            },
            runtime_context,
            automation_snapshot: Some(snapshot),
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            execution_claim: None,
            execution_claim_epoch: 0,
            pause_reason: None,
            resume_reason: None,
            detail: Some("dry_run".to_string()),
            stop_kind: None,
            stop_reason: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
            scheduler: None,
            trigger_reason: None,
            consumed_handoff_id: None,
            learning_summary: None,
            effective_execution_profile,
            requested_execution_profile,
        };
        crate::stateful_runtime::ensure_automation_run_definition_metadata(&mut run);
        self.automation_v2_runs
            .write()
            .await
            .insert(run.run_id.clone(), run.clone());
        self.persist_automation_v2_runs().await?;
        crate::http::context_runs::sync_automation_v2_run_blackboard(self, automation, &run)
            .await
            .map_err(|status| anyhow::anyhow!("failed to sync automation context run: {status}"))?;
        Ok(run)
    }

    pub async fn get_automation_v2_run(&self, run_id: &str) -> Option<AutomationV2RunRecord> {
        let hot = self
            .automation_v2_runs
            .read()
            .await
            .get(run_id)
            .cloned()
            .filter(|run| !automation_v2_run_is_nonterminal_recovered_context_run(run));
        let history =
            load_automation_v2_run_history_shard(&self.automation_v2_runs_path, run_id).await;
        match (hot, history) {
            (Some(hot), Some(history)) => {
                let history_has_pending_gate = history
                    .checkpoint
                    .awaiting_gate
                    .as_ref()
                    .is_some_and(|gate| {
                        !Self::automation_run_is_terminal(&hot.status)
                            && hot.checkpoint.awaiting_gate.is_none()
                            && !crate::app::state::automation_gate_has_settled_decision(
                                &hot,
                                &gate.node_id,
                            )
                    });
                let history_has_more_detail = history.checkpoint.node_outputs.len()
                    > hot.checkpoint.node_outputs.len()
                    || (hot.runtime_context.is_none() && history.runtime_context.is_some())
                    || (hot.automation_snapshot.is_none() && history.automation_snapshot.is_some());
                if history_has_pending_gate || history_has_more_detail {
                    Some(history)
                } else {
                    Some(hot)
                }
            }
            (Some(hot), None) => Some(hot),
            (None, Some(history)) => Some(history),
            (None, None) => {
                automation_v2_context_recovery::get_recovered_automation_v2_run(self, run_id).await
            }
        }
    }

    pub async fn list_automation_v2_runs(
        &self,
        automation_id: Option<&str>,
        limit: usize,
    ) -> Vec<AutomationV2RunRecord> {
        let mut merged = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .cloned()
            .filter(|run| !automation_v2_run_is_nonterminal_recovered_context_run(run))
            .map(|run| (run.run_id.clone(), run))
            .collect::<std::collections::HashMap<_, _>>();
        for history_run in
            load_automation_v2_run_history_shards(&self.automation_v2_runs_path).await
        {
            match merged.get(&history_run.run_id) {
                Some(existing) if existing.updated_at_ms >= history_run.updated_at_ms => {}
                _ => {
                    merged.insert(history_run.run_id.clone(), history_run);
                }
            }
        }
        automation_v2_context_recovery::merge_recovered_automation_v2_runs(self, &mut merged)
            .await;
        let mut rows = merged
            .into_values()
            .filter(|row| automation_id.is_none_or(|id| row.automation_id == id))
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
        rows.truncate(limit.clamp(1, 500));
        rows
    }

    async fn automation_v2_run_workspace_root(
        &self,
        run: &AutomationV2RunRecord,
    ) -> Option<String> {
        if let Some(root) = run
            .automation_snapshot
            .as_ref()
            .and_then(|automation| automation.workspace_root.as_ref())
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            return Some(root.to_string());
        }
        self.get_automation_v2(&run.automation_id)
            .await
            .and_then(|automation| automation.workspace_root)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    async fn sync_automation_scheduler_for_run_transition(
        &self,
        previous_status: AutomationRunStatus,
        run: &AutomationV2RunRecord,
    ) {
        let had_capacity = automation_status_uses_scheduler_capacity(&previous_status);
        let has_capacity = automation_status_uses_scheduler_capacity(&run.status);
        let had_lock = automation_status_holds_workspace_lock(&previous_status);
        let has_lock = automation_status_holds_workspace_lock(&run.status);
        let workspace_root = self.automation_v2_run_workspace_root(run).await;
        let mut scheduler = self.automation_scheduler.write().await;

        if (had_capacity || had_lock) && !has_capacity && !has_lock {
            scheduler.release_run(&run.run_id);
            return;
        }
        if had_capacity && !has_capacity {
            scheduler.release_capacity(&run.run_id);
        }
        if had_lock && !has_lock {
            scheduler.release_workspace(&run.run_id);
        }
        if !had_lock && has_lock {
            if has_capacity {
                scheduler.admit_run(&run.run_id, workspace_root.as_deref());
            } else {
                scheduler.reserve_workspace(&run.run_id, workspace_root.as_deref());
            }
            return;
        }
        if !had_capacity && has_capacity {
            scheduler.admit_run(&run.run_id, workspace_root.as_deref());
        }
    }

}
