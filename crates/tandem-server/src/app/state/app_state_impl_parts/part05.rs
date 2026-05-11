const DEFAULT_STALE_AUTO_RESUME_WINDOW_MS: u64 = 20 * 60 * 1000;

fn stale_auto_resume_window_ms() -> u64 {
    std::env::var("TANDEM_STALE_AUTO_RESUME_WINDOW_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_STALE_AUTO_RESUME_WINDOW_MS)
}

fn latest_stale_reap_recorded_at_ms(run: &AutomationV2RunRecord) -> Option<u64> {
    run.checkpoint
        .lifecycle_history
        .iter()
        .rev()
        .find(|record| {
            record.event == "run_paused_stale_no_provider_activity"
                || record.stop_kind == Some(AutomationStopKind::StaleReaped)
        })
        .map(|record| record.recorded_at_ms)
}

fn stale_reap_is_within_auto_resume_window(
    now: u64,
    stale_reaped_at_ms: u64,
    auto_resume_window_ms: u64,
) -> bool {
    now.saturating_sub(stale_reaped_at_ms) <= auto_resume_window_ms
}

#[cfg(test)]
mod stale_auto_resume_window_tests {
    use super::stale_reap_is_within_auto_resume_window;

    #[test]
    fn stale_auto_resume_window_allows_fresh_reaped_runs() {
        assert!(stale_reap_is_within_auto_resume_window(
            10_000, 9_500, 1_000,
        ));
    }

    #[test]
    fn stale_auto_resume_window_rejects_old_reaped_runs() {
        assert!(!stale_reap_is_within_auto_resume_window(
            10_000, 7_000, 1_000,
        ));
    }
}

impl AppState {
    pub async fn recover_in_flight_runs(&self) -> usize {
        let runs = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut recovered = 0usize;
        for run in runs {
            match run.status {
                AutomationRunStatus::Running => {
                    let detail = "automation run interrupted by server restart".to_string();
                    if self
                        .update_automation_v2_run(&run.run_id, |row| {
                            row.status = AutomationRunStatus::Failed;
                            row.detail = Some(detail.clone());
                            row.stop_kind = Some(AutomationStopKind::ServerRestart);
                            row.stop_reason = Some(detail.clone());
                            automation::record_automation_lifecycle_event(
                                row,
                                "run_failed_server_restart",
                                Some(detail.clone()),
                                Some(AutomationStopKind::ServerRestart),
                            );
                        })
                        .await
                        .is_some()
                    {
                        recovered += 1;
                    }
                }
                AutomationRunStatus::Pausing => {
                    // `Pausing` is a transient state — the executor task that
                    // was about to finish pausing is gone after a restart and
                    // will never complete the transition. Settle the run to
                    // `Paused` so it (a) releases its workspace lock (Pausing
                    // holds it, Paused does not) and (b) becomes eligible for
                    // `/recover` via the API. Without this, the Pausing lock
                    // perpetuates across every restart and blocks every new
                    // run on the same workspace.
                    let detail =
                        "automation run settled to paused after server restart".to_string();
                    if self
                        .update_automation_v2_run(&run.run_id, |row| {
                            row.status = AutomationRunStatus::Paused;
                            if row.pause_reason.is_none() {
                                row.pause_reason = Some(detail.clone());
                            }
                            automation::record_automation_lifecycle_event(
                                row,
                                "run_pausing_settled_on_restart",
                                Some(detail.clone()),
                                None,
                            );
                        })
                        .await
                        .is_some()
                    {
                        recovered += 1;
                    }
                }
                AutomationRunStatus::Paused | AutomationRunStatus::AwaitingApproval => {
                    let workspace_root = if automation_status_holds_workspace_lock(&run.status) {
                        self.automation_v2_run_workspace_root(&run).await
                    } else {
                        None
                    };
                    let mut scheduler = self.automation_scheduler.write().await;
                    if automation_status_holds_workspace_lock(&run.status) {
                        scheduler.reserve_workspace(&run.run_id, workspace_root.as_deref());
                    }
                    for (node_id, output) in &run.checkpoint.node_outputs {
                        if let Some((path, content_digest)) =
                            automation::node_output::automation_output_validated_artifact(output)
                        {
                            scheduler.preexisting_registry.register_validated(
                                &run.run_id,
                                node_id,
                                automation::scheduler::ValidatedArtifact {
                                    path,
                                    content_digest,
                                },
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        recovered
    }

    pub async fn auto_resume_stale_reaped_runs(&self) -> usize {
        // Stale reaping is provider/session infrastructure failure, not proof
        // that the workflow contract failed. Keep the retry bounded so a truly
        // wedged provider cannot loop forever, but default to recovery while
        // the node still has attempt budget.
        if std::env::var_os("TANDEM_DISABLE_STALE_AUTO_RESUME").is_some() {
            return 0;
        }

        let candidate_runs = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .filter(|run| run.status == AutomationRunStatus::Paused)
            .filter(|run| run.stop_kind == Some(AutomationStopKind::StaleReaped))
            .cloned()
            .collect::<Vec<_>>();
        let mut resumed = 0usize;
        let now = now_ms();
        let auto_resume_window_ms = stale_auto_resume_window_ms();
        for run in candidate_runs {
            let Some(stale_reaped_at_ms) = latest_stale_reap_recorded_at_ms(&run) else {
                continue;
            };
            if !stale_reap_is_within_auto_resume_window(
                now,
                stale_reaped_at_ms,
                auto_resume_window_ms,
            ) {
                continue;
            }
            let auto_resume_count = run
                .checkpoint
                .lifecycle_history
                .iter()
                .filter(|event| event.event == "run_auto_resumed")
                .count();
            if auto_resume_count >= 2 {
                continue;
            }
            let automation = self.get_automation_v2(&run.automation_id).await;
            let automation = match automation.or(run.automation_snapshot.clone()) {
                Some(a) => a,
                None => continue,
            };
            let has_repairable_nodes = automation.flow.nodes.iter().any(|node| {
                if run.checkpoint.completed_nodes.contains(&node.node_id) {
                    return false;
                }
                if run.checkpoint.node_outputs.contains_key(&node.node_id) {
                    let status = run.checkpoint.node_outputs[&node.node_id]
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    if status != "needs_repair" {
                        return false;
                    }
                } else {
                    return false;
                }
                let attempts = run
                    .checkpoint
                    .node_attempts
                    .get(&node.node_id)
                    .copied()
                    .unwrap_or(0);
                let max_attempts = automation_node_max_attempts(node);
                attempts < max_attempts
            });
            if !has_repairable_nodes {
                continue;
            }
            if self
                .update_automation_v2_run(&run.run_id, |row| {
                    row.status = AutomationRunStatus::Queued;
                    row.pause_reason = None;
                    row.detail = None;
                    row.stop_kind = None;
                    row.stop_reason = None;
                    automation::record_automation_lifecycle_event_with_metadata(
                        row,
                        "run_auto_resumed",
                        Some("auto_resume_after_stale_reap".to_string()),
                        None,
                        Some(json!({
                            "auto_resume_window_ms": auto_resume_window_ms,
                            "stale_reaped_at_ms": stale_reaped_at_ms,
                        })),
                    );
                })
                .await
                .is_some()
            {
                resumed += 1;
            }
        }
        resumed
    }

    pub fn is_automation_scheduler_stopping(&self) -> bool {
        self.automation_scheduler_stopping.load(Ordering::Relaxed)
    }

    pub fn set_automation_scheduler_stopping(&self, stopping: bool) {
        self.automation_scheduler_stopping
            .store(stopping, Ordering::Relaxed);
    }

    pub async fn fail_running_automation_runs_for_shutdown(&self) -> usize {
        let run_ids = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .filter(|run| matches!(run.status, AutomationRunStatus::Running))
            .map(|run| run.run_id.clone())
            .collect::<Vec<_>>();
        let mut failed = 0usize;
        for run_id in run_ids {
            let detail = "automation run stopped during server shutdown".to_string();
            if self
                .update_automation_v2_run(&run_id, |row| {
                    row.status = AutomationRunStatus::Failed;
                    row.detail = Some(detail.clone());
                    row.stop_kind = Some(AutomationStopKind::Shutdown);
                    row.stop_reason = Some(detail.clone());
                    automation::record_automation_lifecycle_event(
                        row,
                        "run_failed_shutdown",
                        Some(detail.clone()),
                        Some(AutomationStopKind::Shutdown),
                    );
                })
                .await
                .is_some()
            {
                failed += 1;
            }
        }
        failed
    }

    pub async fn claim_next_queued_automation_v2_run(&self) -> Option<AutomationV2RunRecord> {
        let run_id = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .filter(|row| row.status == AutomationRunStatus::Queued)
            .min_by(|a, b| a.created_at_ms.cmp(&b.created_at_ms))
            .map(|row| row.run_id.clone())?;
        self.claim_specific_automation_v2_run(&run_id).await
    }
    pub async fn claim_specific_automation_v2_run(
        &self,
        run_id: &str,
    ) -> Option<AutomationV2RunRecord> {
        const STARTUP_RUNTIME_CONTEXT_MISSING: &str =
            "runtime context partition missing for automation run";
        const STARTUP_RUNTIME_CONTEXT_FAILURE_NODE: &str = "runtime_context";

        let (automation_snapshot, previous_status, automation_id, stored_runtime_context) = {
            let mut guard = self.automation_v2_runs.write().await;
            let run = guard.get_mut(run_id)?;
            if run.status != AutomationRunStatus::Queued {
                return None;
            }
            (
                run.automation_snapshot.clone(),
                run.status.clone(),
                run.automation_id.clone(),
                run.runtime_context.clone(),
            )
        };
        let automation_for_context = if let Some(automation) = automation_snapshot {
            Some(automation)
        } else {
            self.get_automation_v2(&automation_id).await
        };
        let runtime_context_required = automation_for_context
            .as_ref()
            .map(crate::automation_v2::types::AutomationV2Spec::requires_runtime_context)
            .unwrap_or(false);
        let computed_runtime_context = match automation_for_context.as_ref() {
            Some(automation) => self
                .automation_v2_effective_runtime_context(
                    automation,
                    automation
                        .runtime_context_materialization()
                        .or_else(|| automation.approved_plan_runtime_context_materialization()),
                )
                .await
                .ok()
                .flatten(),
            None => None,
        };
        let runtime_context = computed_runtime_context.or(stored_runtime_context);
        if runtime_context_required && runtime_context.is_none() {
            let mut guard = self.automation_v2_runs.write().await;
            let run = guard.get_mut(run_id)?;
            if run.status != AutomationRunStatus::Queued {
                return None;
            }
            let previous_status = run.status.clone();
            let now = now_ms();
            run.status = AutomationRunStatus::Failed;
            run.updated_at_ms = now;
            run.finished_at_ms.get_or_insert(now);
            run.scheduler = None;
            run.detail = Some(STARTUP_RUNTIME_CONTEXT_MISSING.to_string());
            if run.checkpoint.last_failure.is_none() {
                run.checkpoint.last_failure = Some(crate::AutomationFailureRecord {
                    node_id: STARTUP_RUNTIME_CONTEXT_FAILURE_NODE.to_string(),
                    reason: STARTUP_RUNTIME_CONTEXT_MISSING.to_string(),
                    failed_at_ms: now,
                });
            }
            let claimed = run.clone();
            drop(guard);
            self.sync_automation_scheduler_for_run_transition(previous_status, &claimed)
                .await;
            let _ = self.persist_automation_v2_runs().await;
            return None;
        }

        let mut guard = self.automation_v2_runs.write().await;
        let run = guard.get_mut(run_id)?;
        if run.status != AutomationRunStatus::Queued {
            return None;
        }
        let now = now_ms();
        if run.automation_snapshot.is_none() {
            run.automation_snapshot = automation_for_context.clone();
        }
        run.runtime_context = runtime_context;
        run.status = AutomationRunStatus::Running;
        run.updated_at_ms = now;
        run.started_at_ms.get_or_insert(now);
        run.scheduler = None;
        let claimed = run.clone();
        drop(guard);
        self.sync_automation_scheduler_for_run_transition(previous_status, &claimed)
            .await;
        let _ = self.persist_automation_v2_runs().await;
        Some(claimed)
    }
    pub async fn update_automation_v2_run(
        &self,
        run_id: &str,
        update: impl FnOnce(&mut AutomationV2RunRecord),
    ) -> Option<AutomationV2RunRecord> {
        let mut guard = self.automation_v2_runs.write().await;
        if !guard.contains_key(run_id) {
            drop(guard);
            let history =
                load_automation_v2_run_history_shard(&self.automation_v2_runs_path, run_id).await?;
            guard = self.automation_v2_runs.write().await;
            guard.entry(run_id.to_string()).or_insert(history);
        }
        let run = guard.get_mut(run_id)?;
        let previous_status = run.status.clone();
        update(run);
        if run.status != AutomationRunStatus::Queued {
            run.scheduler = None;
        }
        run.updated_at_ms = now_ms();
        if matches!(
            run.status,
            AutomationRunStatus::Completed
                | AutomationRunStatus::Blocked
                | AutomationRunStatus::Failed
                | AutomationRunStatus::Cancelled
        ) {
            run.finished_at_ms.get_or_insert_with(now_ms);
        }
        let out = run.clone();
        drop(guard);
        self.sync_automation_scheduler_for_run_transition(previous_status.clone(), &out)
            .await;
        let _ = self.persist_automation_v2_runs().await;
        let _ = self.persist_automation_v2_run_status_json(&out).await;
        if matches!(
            out.status,
            AutomationRunStatus::Completed
                | AutomationRunStatus::Blocked
                | AutomationRunStatus::Failed
                | AutomationRunStatus::Cancelled
        ) {
            let _ = self
                .finalize_terminal_automation_v2_run_learning(&out)
                .await;
            if !Self::automation_run_is_terminal(&previous_status) {
                let _ = self
                    .record_automation_review_progress(
                        &out.automation_id,
                        crate::automation_v2::governance::AutomationLifecycleReviewKind::RunDrift,
                        Some(out.run_id.clone()),
                        out.detail.clone().or_else(|| out.stop_reason.clone()),
                    )
                    .await;
            }
        }
        Some(out)
    }

    async fn persist_automation_v2_run_status_json(
        &self,
        run: &AutomationV2RunRecord,
    ) -> anyhow::Result<()> {
        let default_workspace = self.workspace_index.snapshot().await.root.clone();
        let automation = run.automation_snapshot.as_ref();
        let workspace_root = if let Some(ref a) = automation {
            if let Some(ref wr) = a.workspace_root {
                if !wr.trim().is_empty() {
                    wr.trim().to_string()
                } else {
                    a.metadata
                        .as_ref()
                        .and_then(|m| m.get("workspace_root"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| default_workspace.clone())
                }
            } else {
                a.metadata
                    .as_ref()
                    .and_then(|m| m.get("workspace_root"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| default_workspace.clone())
            }
        } else {
            default_workspace
        };
        let run_dir = PathBuf::from(&workspace_root)
            .join(".tandem")
            .join("runs")
            .join(&run.run_id);
        let status_path = run_dir.join("status.json");
        let status_json = json!({
            "run_id": run.run_id,
            "automation_id": run.automation_id,
            "status": run.status,
            "detail": run.detail,
            "completed_nodes": run.checkpoint.completed_nodes,
            "pending_nodes": run.checkpoint.pending_nodes,
            "blocked_nodes": run.checkpoint.blocked_nodes,
            "node_attempts": run.checkpoint.node_attempts,
            "last_failure": run.checkpoint.last_failure,
            "learning_summary": run.learning_summary,
            "updated_at_ms": run.updated_at_ms,
        });
        fs::create_dir_all(&run_dir).await?;
        fs::write(&status_path, serde_json::to_string_pretty(&status_json)?).await?;
        Ok(())
    }

    pub async fn set_automation_v2_run_scheduler_metadata(
        &self,
        run_id: &str,
        meta: automation::SchedulerMetadata,
    ) -> Option<AutomationV2RunRecord> {
        self.update_automation_v2_run(run_id, |row| {
            row.scheduler = Some(meta);
        })
        .await
    }

    pub async fn clear_automation_v2_run_scheduler_metadata(
        &self,
        run_id: &str,
    ) -> Option<AutomationV2RunRecord> {
        self.update_automation_v2_run(run_id, |row| {
            row.scheduler = None;
        })
        .await
    }

    pub async fn add_automation_v2_session(
        &self,
        run_id: &str,
        session_id: &str,
    ) -> Option<AutomationV2RunRecord> {
        let updated = self
            .update_automation_v2_run(run_id, |row| {
                if !row.active_session_ids.iter().any(|id| id == session_id) {
                    row.active_session_ids.push(session_id.to_string());
                }
                row.latest_session_id = Some(session_id.to_string());
            })
            .await;
        self.automation_v2_session_runs
            .write()
            .await
            .insert(session_id.to_string(), run_id.to_string());
        updated
    }

    pub async fn set_automation_v2_session_mcp_servers(
        &self,
        session_id: &str,
        servers: Vec<String>,
    ) {
        if servers.is_empty() {
            self.automation_v2_session_mcp_servers
                .write()
                .await
                .remove(session_id);
        } else {
            self.automation_v2_session_mcp_servers
                .write()
                .await
                .insert(session_id.to_string(), servers);
        }
    }

    pub async fn clear_automation_v2_session_mcp_servers(&self, session_id: &str) {
        self.automation_v2_session_mcp_servers
            .write()
            .await
            .remove(session_id);
    }

    pub async fn clear_automation_v2_session(
        &self,
        run_id: &str,
        session_id: &str,
    ) -> Option<AutomationV2RunRecord> {
        self.automation_v2_session_runs
            .write()
            .await
            .remove(session_id);
        self.update_automation_v2_run(run_id, |row| {
            row.active_session_ids.retain(|id| id != session_id);
        })
        .await
    }

    pub async fn forget_automation_v2_sessions(&self, session_ids: &[String]) {
        let mut guard = self.automation_v2_session_runs.write().await;
        for session_id in session_ids {
            guard.remove(session_id);
        }
        let mut mcp_guard = self.automation_v2_session_mcp_servers.write().await;
        for session_id in session_ids {
            mcp_guard.remove(session_id);
        }
    }

    pub async fn add_automation_v2_instance(
        &self,
        run_id: &str,
        instance_id: &str,
    ) -> Option<AutomationV2RunRecord> {
        self.update_automation_v2_run(run_id, |row| {
            if !row.active_instance_ids.iter().any(|id| id == instance_id) {
                row.active_instance_ids.push(instance_id.to_string());
            }
        })
        .await
    }

    pub async fn clear_automation_v2_instance(
        &self,
        run_id: &str,
        instance_id: &str,
    ) -> Option<AutomationV2RunRecord> {
        self.update_automation_v2_run(run_id, |row| {
            row.active_instance_ids.retain(|id| id != instance_id);
        })
        .await
    }
}
