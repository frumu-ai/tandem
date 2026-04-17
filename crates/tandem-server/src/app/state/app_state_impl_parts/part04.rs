impl AppState {
    pub async fn apply_provider_usage_to_runs(
        &self,
        session_id: &str,
        prompt_tokens: u64,
        completion_tokens: u64,
        total_tokens: u64,
    ) {
        if let Some(policy) = self.routine_session_policy(session_id).await {
            let rate = self.token_cost_per_1k_usd.max(0.0);
            let delta_cost = (total_tokens as f64 / 1000.0) * rate;
            let mut guard = self.routine_runs.write().await;
            if let Some(run) = guard.get_mut(&policy.run_id) {
                run.prompt_tokens = run.prompt_tokens.saturating_add(prompt_tokens);
                run.completion_tokens = run.completion_tokens.saturating_add(completion_tokens);
                run.total_tokens = run.total_tokens.saturating_add(total_tokens);
                run.estimated_cost_usd += delta_cost;
                run.updated_at_ms = now_ms();
            }
            drop(guard);
            let _ = self.persist_routine_runs().await;
        }

        let maybe_v2_run_id = self
            .automation_v2_session_runs
            .read()
            .await
            .get(session_id)
            .cloned();
        if let Some(run_id) = maybe_v2_run_id {
            let rate = self.token_cost_per_1k_usd.max(0.0);
            let delta_cost = (total_tokens as f64 / 1000.0) * rate;
            let mut guard = self.automation_v2_runs.write().await;
            if let Some(run) = guard.get_mut(&run_id) {
                run.prompt_tokens = run.prompt_tokens.saturating_add(prompt_tokens);
                run.completion_tokens = run.completion_tokens.saturating_add(completion_tokens);
                run.total_tokens = run.total_tokens.saturating_add(total_tokens);
                run.estimated_cost_usd += delta_cost;
                run.updated_at_ms = now_ms();
            }
            drop(guard);
            let _ = self.persist_automation_v2_runs().await;
        }
    }

    pub async fn evaluate_automation_v2_misfires(&self, now_ms: u64) -> Vec<String> {
        let mut fired = Vec::new();
        let mut guard = self.automations_v2.write().await;
        for automation in guard.values_mut() {
            if automation.status != AutomationV2Status::Active {
                continue;
            }
            let Some(next_fire_at_ms) = automation.next_fire_at_ms else {
                automation.next_fire_at_ms =
                    automation_schedule_next_fire_at_ms(&automation.schedule, now_ms);
                continue;
            };
            if now_ms < next_fire_at_ms {
                continue;
            }
            let run_count =
                automation_schedule_due_count(&automation.schedule, now_ms, next_fire_at_ms);
            let next = automation_schedule_next_fire_at_ms(&automation.schedule, now_ms);
            automation.next_fire_at_ms = next;
            automation.last_fired_at_ms = Some(now_ms);
            for _ in 0..run_count {
                fired.push(automation.automation_id.clone());
            }
        }
        drop(guard);
        let _ = self.persist_automations_v2().await;
        fired
    }

    /// Evaluate watch conditions for all active automations and return the IDs of
    /// automations whose conditions are met, along with a human-readable trigger reason
    /// and the handoff that triggered it (if any).
    ///
    /// An automation is skipped if it already has a `Queued` or `Running` run (dedup).
    pub async fn evaluate_automation_v2_watches(
        &self,
    ) -> Vec<(
        String,
        String,
        Option<crate::automation_v2::types::HandoffArtifact>,
    )> {
        use crate::automation_v2::types::{AutomationRunStatus, WatchCondition};

        // Snapshot of automations that have watch conditions and are Active.
        let candidates: Vec<crate::automation_v2::types::AutomationV2Spec> = {
            let guard = self.automations_v2.read().await;
            guard
                .values()
                .filter(|a| {
                    a.status == crate::automation_v2::types::AutomationV2Status::Active
                        && a.has_watch_conditions()
                })
                .cloned()
                .collect()
        };

        // Snapshot active run statuses to implement dedup.
        let active_automation_ids: std::collections::HashSet<String> = {
            let runs = self.automation_v2_runs.read().await;
            runs.values()
                .filter(|r| {
                    matches!(
                        r.status,
                        AutomationRunStatus::Queued | AutomationRunStatus::Running
                    )
                })
                .map(|r| r.automation_id.clone())
                .collect()
        };

        let workspace_root = self.workspace_index.snapshot().await.root;
        let mut results = Vec::new();

        'outer: for automation in candidates {
            // Dedup: skip if already queued or running.
            if active_automation_ids.contains(&automation.automation_id) {
                continue;
            }

            let handoff_cfg = automation.effective_handoff_config();
            let approved_dir =
                std::path::Path::new(&workspace_root).join(&handoff_cfg.approved_dir);

            for condition in &automation.watch_conditions {
                match condition {
                    WatchCondition::HandoffAvailable {
                        source_automation_id,
                        artifact_type,
                    } => {
                        if let Some(handoff) = find_matching_handoff(
                            &approved_dir,
                            &automation.automation_id,
                            source_automation_id.as_deref(),
                            artifact_type.as_deref(),
                        )
                        .await
                        {
                            let reason = format!(
                                "handoff `{}` of type `{}` from `{}` is available",
                                handoff.handoff_id,
                                handoff.artifact_type,
                                handoff.source_automation_id
                            );
                            results.push((automation.automation_id.clone(), reason, Some(handoff)));
                            continue 'outer;
                        }
                    }
                }
            }
        }

        results
    }

    /// Create a run triggered by a watch condition, recording the trigger reason
    /// and the consumed handoff ID (if any).
    pub async fn create_automation_v2_watch_run(
        &self,
        automation: &crate::automation_v2::types::AutomationV2Spec,
        trigger_reason: String,
        consumed_handoff_id: Option<String>,
    ) -> anyhow::Result<crate::automation_v2::types::AutomationV2RunRecord> {
        use crate::automation_v2::types::{
            AutomationRunCheckpoint, AutomationRunStatus, AutomationV2RunRecord,
        };
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
        let run = AutomationV2RunRecord {
            run_id: format!("automation-v2-run-{}", uuid::Uuid::new_v4()),
            automation_id: automation.automation_id.clone(),
            tenant_context: TenantContext::local_implicit(),
            trigger_type: "watch_condition".to_string(),
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
                blocked_nodes: Vec::new(),
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: Vec::new(),
                last_failure: None,
            },
            runtime_context,
            automation_snapshot: Some(automation.clone()),
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
            trigger_reason: Some(trigger_reason),
            consumed_handoff_id,
            learning_summary: None,
        };
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

    /// Deposit a handoff artifact into the workspace `inbox/` directory.
    /// If `auto_approve` is true (Phase 1 default), the file is immediately
    /// moved to `approved/` so the downstream watch condition can fire on the next tick.
    pub async fn deposit_automation_v2_handoff(
        &self,
        workspace_root: &str,
        handoff: &crate::automation_v2::types::HandoffArtifact,
        handoff_cfg: &crate::automation_v2::types::AutomationHandoffConfig,
    ) -> anyhow::Result<()> {
        use tokio::fs;
        let root = std::path::Path::new(workspace_root);
        let inbox = root.join(&handoff_cfg.inbox_dir);
        fs::create_dir_all(&inbox).await?;

        let filename = handoff_filename(&handoff.handoff_id);
        let content = serde_json::to_string_pretty(handoff)?;

        if handoff_cfg.auto_approve {
            // Write directly to approved/ (bypass inbox).
            let approved = root.join(&handoff_cfg.approved_dir);
            fs::create_dir_all(&approved).await?;
            fs::write(approved.join(&filename), &content).await?;
            tracing::info!(
                handoff_id = %handoff.handoff_id,
                target = %handoff.target_automation_id,
                artifact_type = %handoff.artifact_type,
                "handoff deposited (auto-approved)"
            );
        } else {
            fs::write(inbox.join(&filename), &content).await?;
            tracing::info!(
                handoff_id = %handoff.handoff_id,
                target = %handoff.target_automation_id,
                artifact_type = %handoff.artifact_type,
                "handoff deposited to inbox (awaiting approval)"
            );
        }
        Ok(())
    }

    /// Atomically consume a handoff artifact: rename it from `approved/` to
    /// `archived/`, stamping the consuming run's metadata into the file for audit.
    /// Returns the updated artifact. This is idempotent — if the file is already
    /// gone from `approved/`, it returns `None` (race-safe).
    pub async fn consume_automation_v2_handoff(
        &self,
        workspace_root: &str,
        handoff: &crate::automation_v2::types::HandoffArtifact,
        handoff_cfg: &crate::automation_v2::types::AutomationHandoffConfig,
        consuming_run_id: &str,
        consuming_automation_id: &str,
    ) -> anyhow::Result<Option<crate::automation_v2::types::HandoffArtifact>> {
        use tokio::fs;
        let root = std::path::Path::new(workspace_root);
        let filename = handoff_filename(&handoff.handoff_id);
        let approved_path = root.join(&handoff_cfg.approved_dir).join(&filename);

        if !approved_path.exists() {
            // Already consumed by another run (race).
            tracing::warn!(
                handoff_id = %handoff.handoff_id,
                "handoff already consumed or missing from approved dir"
            );
            return Ok(None);
        }

        let archived_dir = root.join(&handoff_cfg.archived_dir);
        fs::create_dir_all(&archived_dir).await?;

        let mut archived = handoff.clone();
        archived.consumed_by_run_id = Some(consuming_run_id.to_string());
        archived.consumed_by_automation_id = Some(consuming_automation_id.to_string());
        archived.consumed_at_ms = Some(now_ms());

        // Write the updated envelope to archived/ first, then remove from approved/.
        // This ordering means we never lose the record even if the remove fails.
        let archived_path = archived_dir.join(&filename);
        fs::write(&archived_path, serde_json::to_string_pretty(&archived)?).await?;
        let _ = fs::remove_file(&approved_path).await;

        tracing::info!(
            handoff_id = %handoff.handoff_id,
            run_id = %consuming_run_id,
            "handoff consumed and archived"
        );
        Ok(Some(archived))
    }
}
