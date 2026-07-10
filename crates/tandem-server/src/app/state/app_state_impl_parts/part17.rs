fn routine_store_index(
    routines: std::collections::HashMap<String, RoutineSpec>,
) -> std::collections::HashMap<String, RoutineSpec> {
    routines
        .into_values()
        .map(|routine| {
            let identity = RoutineIdentity::new(&routine.routine_id, &routine.tenant_context);
            (identity.storage_key(), routine)
        })
        .collect()
}

fn routine_history_index(
    history: std::collections::HashMap<String, Vec<RoutineHistoryEvent>>,
) -> std::collections::HashMap<String, Vec<RoutineHistoryEvent>> {
    let mut indexed = std::collections::HashMap::<String, Vec<RoutineHistoryEvent>>::new();
    for event in history.into_values().flatten() {
        let identity = RoutineIdentity::new(&event.routine_id, &event.tenant_context);
        indexed
            .entry(identity.storage_key())
            .or_default()
            .push(event);
    }
    indexed
}

fn normalize_routine(mut routine: RoutineSpec) -> Result<RoutineSpec, RoutineStoreError> {
    if routine.routine_id.trim().is_empty() {
        return Err(RoutineStoreError::InvalidRoutineId {
            routine_id: routine.routine_id,
        });
    }
    routine.allowed_tools = config::channels::normalize_allowed_tools(routine.allowed_tools);
    routine.output_targets = normalize_non_empty_list(routine.output_targets);
    let next_schedule_fire =
        compute_next_schedule_fire_at_ms(&routine.schedule, &routine.timezone, now_ms())
            .ok_or_else(|| RoutineStoreError::InvalidSchedule {
                detail: "invalid schedule or timezone".to_string(),
            })?;
    if matches!(
        routine.schedule,
        RoutineSchedule::IntervalSeconds { seconds: 0 }
    ) {
        return Err(RoutineStoreError::InvalidSchedule {
            detail: "interval_seconds must be > 0".to_string(),
        });
    }
    if routine.next_fire_at_ms.is_none() {
        routine.next_fire_at_ms = Some(next_schedule_fire);
    }
    Ok(routine)
}

impl AppState {
    pub async fn load_routines(&self) -> anyhow::Result<()> {
        let _operation = self.routine_persistence.lock().await;
        let Some(raw) = read_state_file_with_legacy(&self.routines_path, "routines.json").await?
        else {
            return Ok(());
        };
        match serde_json::from_str::<std::collections::HashMap<String, RoutineSpec>>(&raw) {
            Ok(parsed) => {
                *self.routines.write().await = routine_store_index(parsed);
                Ok(())
            }
            Err(primary_err) => {
                let backup_path = config::paths::sibling_backup_path(&self.routines_path);
                if backup_path.exists() {
                    let backup_raw = fs::read_to_string(&backup_path).await?;
                    if let Ok(parsed) = serde_json::from_str::<
                        std::collections::HashMap<String, RoutineSpec>,
                    >(&backup_raw)
                    {
                        *self.routines.write().await = routine_store_index(parsed);
                        return Ok(());
                    }
                }
                Err(anyhow::anyhow!(
                    "failed to parse routines store {}: {primary_err}",
                    self.routines_path.display()
                ))
            }
        }
    }

    pub async fn load_routine_history(&self) -> anyhow::Result<()> {
        if !self.routine_history_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.routine_history_path).await?;
        let parsed = serde_json::from_str::<
            std::collections::HashMap<String, Vec<RoutineHistoryEvent>>,
        >(&raw)
        .unwrap_or_default();
        *self.routine_history.write().await = routine_history_index(parsed);
        Ok(())
    }

    pub async fn load_routine_runs(&self) -> anyhow::Result<()> {
        let Some(raw) =
            read_state_file_with_legacy(&self.routine_runs_path, "routine_runs.json").await?
        else {
            return Ok(());
        };
        let parsed =
            serde_json::from_str::<std::collections::HashMap<String, RoutineRunRecord>>(&raw)
                .unwrap_or_default();
        *self.routine_runs.write().await = parsed;
        Ok(())
    }

    async fn persist_routines_inner(&self, allow_empty_overwrite: bool) -> anyhow::Result<()> {
        if let Some(parent) = self.routines_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let (payload, is_empty) = {
            let guard = self.routines.read().await;
            (serde_json::to_string_pretty(&*guard)?, guard.is_empty())
        };
        if is_empty && !allow_empty_overwrite && self.routines_path.exists() {
            let existing_raw = fs::read_to_string(&self.routines_path)
                .await
                .unwrap_or_default();
            let existing_has_rows = serde_json::from_str::<
                std::collections::HashMap<String, RoutineSpec>,
            >(&existing_raw)
            .map(|rows| !rows.is_empty())
            .unwrap_or(true);
            if existing_has_rows {
                return Err(anyhow::anyhow!(
                    "refusing to overwrite non-empty routines store {} with empty in-memory state",
                    self.routines_path.display()
                ));
            }
        }
        let backup_path = config::paths::sibling_backup_path(&self.routines_path);
        if self.routines_path.exists() {
            let _ = fs::copy(&self.routines_path, &backup_path).await;
        }
        let tmp_path = config::paths::sibling_tmp_path(&self.routines_path);
        fs::write(&tmp_path, payload).await?;
        fs::rename(&tmp_path, &self.routines_path).await?;
        Ok(())
    }

    pub async fn persist_routines(&self) -> anyhow::Result<()> {
        let _operation = self.routine_persistence.lock().await;
        self.persist_routines_inner(false).await
    }

    pub async fn persist_routine_history(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.routine_history_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = serde_json::to_string_pretty(&*self.routine_history.read().await)?;
        fs::write(&self.routine_history_path, payload).await?;
        Ok(())
    }

    pub async fn persist_routine_runs(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.routine_runs_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = serde_json::to_string_pretty(&*self.routine_runs.read().await)?;
        fs::write(&self.routine_runs_path, payload).await?;
        Ok(())
    }

    pub async fn put_routine(
        &self,
        routine: RoutineSpec,
    ) -> Result<RoutineSpec, RoutineStoreError> {
        let routine = normalize_routine(routine)?;
        let identity = RoutineIdentity::new(&routine.routine_id, &routine.tenant_context);
        let storage_key = identity.storage_key();
        let _operation = self.routine_persistence.lock().await;
        let previous = self
            .routines
            .write()
            .await
            .insert(storage_key.clone(), routine.clone());
        if let Err(error) = self.persist_routines_inner(false).await {
            let mut rollback = self.routines.write().await;
            if let Some(previous) = previous {
                rollback.insert(storage_key, previous);
            } else {
                rollback.remove(&storage_key);
            }
            return Err(RoutineStoreError::PersistFailed {
                message: error.to_string(),
            });
        }
        Ok(routine)
    }

    pub async fn update_routine_for_tenant<F>(
        &self,
        routine_id: &str,
        tenant_context: &TenantContext,
        update: F,
    ) -> Result<Option<RoutineSpec>, RoutineStoreError>
    where
        F: FnOnce(&mut RoutineSpec),
    {
        let identity = RoutineIdentity::new(routine_id, tenant_context);
        let storage_key = identity.storage_key();
        let _operation = self.routine_persistence.lock().await;
        let Some(previous) = self.routines.read().await.get(&storage_key).cloned() else {
            return Ok(None);
        };
        let mut routine = previous.clone();
        update(&mut routine);
        routine.routine_id = previous.routine_id.clone();
        routine.tenant_context = previous.tenant_context.clone();
        let routine = normalize_routine(routine)?;
        self.routines
            .write()
            .await
            .insert(storage_key.clone(), routine.clone());
        if let Err(error) = self.persist_routines_inner(false).await {
            self.routines.write().await.insert(storage_key, previous);
            return Err(RoutineStoreError::PersistFailed {
                message: error.to_string(),
            });
        }
        Ok(Some(routine))
    }

    pub async fn list_routines(&self) -> Vec<RoutineSpec> {
        let mut rows = self
            .routines
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| {
            a.routine_id
                .cmp(&b.routine_id)
                .then_with(|| a.tenant_context.org_id.cmp(&b.tenant_context.org_id))
                .then_with(|| {
                    a.tenant_context
                        .workspace_id
                        .cmp(&b.tenant_context.workspace_id)
                })
        });
        rows
    }

    pub async fn list_routines_for_tenant(
        &self,
        tenant_context: &TenantContext,
    ) -> Vec<RoutineSpec> {
        let mut rows = self
            .routines
            .read()
            .await
            .values()
            .filter(|routine| {
                RoutineIdentity::new(&routine.routine_id, &routine.tenant_context)
                    .matches_tenant(tenant_context)
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.routine_id.cmp(&b.routine_id));
        rows
    }

    pub async fn get_routine(&self, routine_id: &str) -> Option<RoutineSpec> {
        self.get_routine_for_tenant(routine_id, &TenantContext::local_implicit())
            .await
    }

    pub async fn get_routine_for_tenant(
        &self,
        routine_id: &str,
        tenant_context: &TenantContext,
    ) -> Option<RoutineSpec> {
        let identity = RoutineIdentity::new(routine_id, tenant_context);
        self.get_routine_by_identity(&identity).await
    }

    pub async fn get_routine_by_identity(&self, identity: &RoutineIdentity) -> Option<RoutineSpec> {
        self.routines
            .read()
            .await
            .get(&identity.storage_key())
            .filter(|routine| {
                RoutineIdentity::new(&routine.routine_id, &routine.tenant_context) == *identity
            })
            .cloned()
    }

    pub async fn delete_routine(
        &self,
        routine_id: &str,
    ) -> Result<Option<RoutineSpec>, RoutineStoreError> {
        self.delete_routine_for_tenant(routine_id, &TenantContext::local_implicit())
            .await
    }

    pub async fn delete_routine_for_tenant(
        &self,
        routine_id: &str,
        tenant_context: &TenantContext,
    ) -> Result<Option<RoutineSpec>, RoutineStoreError> {
        let identity = RoutineIdentity::new(routine_id, tenant_context);
        let storage_key = identity.storage_key();
        let _operation = self.routine_persistence.lock().await;
        let removed = self.routines.write().await.remove(&storage_key);
        let allow_empty_overwrite = self.routines.read().await.is_empty();
        if let Err(error) = self.persist_routines_inner(allow_empty_overwrite).await {
            if let Some(removed) = removed.clone() {
                self.routines.write().await.insert(storage_key, removed);
            }
            return Err(RoutineStoreError::PersistFailed {
                message: error.to_string(),
            });
        }
        Ok(removed)
    }

    pub async fn evaluate_routine_misfires(&self, now_ms: u64) -> Vec<RoutineTriggerPlan> {
        let _operation = self.routine_persistence.lock().await;
        let mut plans = Vec::new();
        let mut guard = self.routines.write().await;
        for routine in guard.values_mut() {
            if routine.status != RoutineStatus::Active {
                continue;
            }
            let Some(next_fire_at_ms) = routine.next_fire_at_ms else {
                continue;
            };
            if now_ms < next_fire_at_ms {
                continue;
            }
            let (run_count, next_fire_at_ms) = compute_misfire_plan_for_schedule(
                now_ms,
                next_fire_at_ms,
                &routine.schedule,
                &routine.timezone,
                &routine.misfire_policy,
            );
            routine.next_fire_at_ms = Some(next_fire_at_ms);
            if run_count == 0 {
                continue;
            }
            plans.push(RoutineTriggerPlan {
                identity: RoutineIdentity::new(&routine.routine_id, &routine.tenant_context),
                tenant_context: routine.tenant_context.clone(),
                run_count,
                scheduled_at_ms: now_ms,
                next_fire_at_ms,
            });
        }
        drop(guard);
        let _ = self.persist_routines_inner(false).await;
        plans
    }

    pub async fn mark_routine_fired(
        &self,
        routine_id: &str,
        fired_at_ms: u64,
    ) -> Option<RoutineSpec> {
        self.mark_routine_fired_for_tenant(
            routine_id,
            &TenantContext::local_implicit(),
            fired_at_ms,
        )
        .await
        .ok()
        .flatten()
    }

    pub async fn mark_routine_fired_for_tenant(
        &self,
        routine_id: &str,
        tenant_context: &TenantContext,
        fired_at_ms: u64,
    ) -> Result<Option<RoutineSpec>, RoutineStoreError> {
        let identity = RoutineIdentity::new(routine_id, tenant_context);
        self.mark_routine_fired_by_identity(&identity, fired_at_ms)
            .await
    }

    pub async fn mark_routine_fired_by_identity(
        &self,
        identity: &RoutineIdentity,
        fired_at_ms: u64,
    ) -> Result<Option<RoutineSpec>, RoutineStoreError> {
        let storage_key = identity.storage_key();
        let _operation = self.routine_persistence.lock().await;
        let mut guard = self.routines.write().await;
        let Some(routine) = guard.get_mut(&storage_key) else {
            return Ok(None);
        };
        let previous = routine.clone();
        routine.last_fired_at_ms = Some(fired_at_ms);
        let updated = routine.clone();
        drop(guard);
        if let Err(error) = self.persist_routines_inner(false).await {
            self.routines.write().await.insert(storage_key, previous);
            return Err(RoutineStoreError::PersistFailed {
                message: error.to_string(),
            });
        }
        Ok(Some(updated))
    }

    pub async fn append_routine_history(&self, event: RoutineHistoryEvent) {
        let identity = RoutineIdentity::new(&event.routine_id, &event.tenant_context);
        self.routine_history
            .write()
            .await
            .entry(identity.storage_key())
            .or_default()
            .push(event);
        let _ = self.persist_routine_history().await;
    }

    pub async fn list_routine_history_for_tenant(
        &self,
        routine_id: &str,
        tenant_context: &TenantContext,
        limit: usize,
    ) -> Vec<RoutineHistoryEvent> {
        let identity = RoutineIdentity::new(routine_id, tenant_context);
        let mut rows = self
            .routine_history
            .read()
            .await
            .get(&identity.storage_key())
            .cloned()
            .unwrap_or_default();
        rows.sort_by(|a, b| b.fired_at_ms.cmp(&a.fired_at_ms));
        rows.truncate(limit.clamp(1, 500));
        rows
    }

    pub async fn create_routine_run(
        &self,
        routine: &RoutineSpec,
        trigger_type: &str,
        run_count: u32,
        status: RoutineRunStatus,
        detail: Option<String>,
    ) -> RoutineRunRecord {
        let now = now_ms();
        let record = RoutineRunRecord {
            run_id: format!("routine-run-{}", uuid::Uuid::new_v4()),
            routine_id: routine.routine_id.clone(),
            tenant_context: routine.tenant_context.clone(),
            trigger_type: trigger_type.to_string(),
            run_count,
            status,
            created_at_ms: now,
            updated_at_ms: now,
            fired_at_ms: Some(now),
            started_at_ms: None,
            finished_at_ms: None,
            requires_approval: routine.requires_approval,
            approval_reason: None,
            denial_reason: None,
            paused_reason: None,
            detail,
            entrypoint: routine.entrypoint.clone(),
            args: routine.args.clone(),
            allowed_tools: routine.allowed_tools.clone(),
            output_targets: routine.output_targets.clone(),
            artifacts: Vec::new(),
            active_session_ids: Vec::new(),
            latest_session_id: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
        };
        self.routine_runs
            .write()
            .await
            .insert(record.run_id.clone(), record.clone());
        let _ = self.persist_routine_runs().await;
        record
    }

    pub async fn get_routine_run(&self, run_id: &str) -> Option<RoutineRunRecord> {
        self.routine_runs.read().await.get(run_id).cloned()
    }

    pub async fn get_routine_run_for_tenant(
        &self,
        run_id: &str,
        tenant_context: &TenantContext,
    ) -> Option<RoutineRunRecord> {
        self.routine_runs
            .read()
            .await
            .get(run_id)
            .filter(|run| {
                RoutineIdentity::new(&run.routine_id, &run.tenant_context)
                    .matches_tenant(tenant_context)
            })
            .cloned()
    }

    pub async fn list_routine_runs(
        &self,
        routine_id: Option<&str>,
        limit: usize,
    ) -> Vec<RoutineRunRecord> {
        self.list_routine_runs_matching(routine_id, None, limit)
            .await
    }

    pub async fn list_routine_runs_for_tenant(
        &self,
        routine_id: Option<&str>,
        tenant_context: &TenantContext,
        limit: usize,
    ) -> Vec<RoutineRunRecord> {
        self.list_routine_runs_matching(routine_id, Some(tenant_context), limit)
            .await
    }

    async fn list_routine_runs_matching(
        &self,
        routine_id: Option<&str>,
        tenant_context: Option<&TenantContext>,
        limit: usize,
    ) -> Vec<RoutineRunRecord> {
        let mut rows = self
            .routine_runs
            .read()
            .await
            .values()
            .filter(|row| routine_id.is_none_or(|id| row.routine_id == id))
            .filter(|row| {
                tenant_context.is_none_or(|tenant| {
                    RoutineIdentity::new(&row.routine_id, &row.tenant_context)
                        .matches_tenant(tenant)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
        rows.truncate(limit.clamp(1, 500));
        rows
    }

    pub async fn claim_next_queued_routine_run(&self) -> Option<RoutineRunRecord> {
        let mut guard = self.routine_runs.write().await;
        let next_run_id = guard
            .values()
            .filter(|row| row.status == RoutineRunStatus::Queued)
            .min_by(|a, b| {
                a.created_at_ms
                    .cmp(&b.created_at_ms)
                    .then_with(|| a.run_id.cmp(&b.run_id))
            })
            .map(|row| row.run_id.clone())?;
        let now = now_ms();
        let row = guard.get_mut(&next_run_id)?;
        row.status = RoutineRunStatus::Running;
        row.updated_at_ms = now;
        row.started_at_ms = Some(now);
        let claimed = row.clone();
        drop(guard);
        let _ = self.persist_routine_runs().await;
        Some(claimed)
    }

    pub async fn set_routine_session_policy(
        &self,
        session_id: String,
        run_id: String,
        routine_id: String,
        tenant_context: TenantContext,
        allowed_tools: Vec<String>,
    ) {
        let policy = RoutineSessionPolicy {
            session_id: session_id.clone(),
            run_id,
            routine_id,
            tenant_context,
            allowed_tools: config::channels::normalize_allowed_tools(allowed_tools),
        };
        self.routine_session_policies
            .write()
            .await
            .insert(session_id, policy);
    }

    pub async fn routine_session_policy(&self, session_id: &str) -> Option<RoutineSessionPolicy> {
        self.routine_session_policies
            .read()
            .await
            .get(session_id)
            .cloned()
    }

    pub async fn clear_routine_session_policy(&self, session_id: &str) {
        self.routine_session_policies
            .write()
            .await
            .remove(session_id);
    }

    pub async fn update_routine_run_status(
        &self,
        run_id: &str,
        status: RoutineRunStatus,
        reason: Option<String>,
    ) -> Option<RoutineRunRecord> {
        self.update_routine_run_status_matching(run_id, None, status, reason)
            .await
    }

    pub async fn update_routine_run_status_for_tenant(
        &self,
        run_id: &str,
        tenant_context: &TenantContext,
        status: RoutineRunStatus,
        reason: Option<String>,
    ) -> Option<RoutineRunRecord> {
        self.update_routine_run_status_matching(run_id, Some(tenant_context), status, reason)
            .await
    }

    async fn update_routine_run_status_matching(
        &self,
        run_id: &str,
        tenant_context: Option<&TenantContext>,
        status: RoutineRunStatus,
        reason: Option<String>,
    ) -> Option<RoutineRunRecord> {
        let mut guard = self.routine_runs.write().await;
        let row = guard.get_mut(run_id)?;
        if tenant_context.is_some_and(|tenant| {
            !RoutineIdentity::new(&row.routine_id, &row.tenant_context).matches_tenant(tenant)
        }) {
            return None;
        }
        row.status = status.clone();
        row.updated_at_ms = now_ms();
        match status {
            RoutineRunStatus::PendingApproval => row.approval_reason = reason,
            RoutineRunStatus::Running => {
                row.started_at_ms.get_or_insert_with(now_ms);
                if let Some(detail) = reason {
                    row.detail = Some(detail);
                }
            }
            RoutineRunStatus::Denied => row.denial_reason = reason,
            RoutineRunStatus::Paused => row.paused_reason = reason,
            RoutineRunStatus::Completed
            | RoutineRunStatus::Failed
            | RoutineRunStatus::Cancelled => {
                row.finished_at_ms = Some(now_ms());
                if let Some(detail) = reason {
                    row.detail = Some(detail);
                }
            }
            _ => {
                if let Some(detail) = reason {
                    row.detail = Some(detail);
                }
            }
        }
        let updated = row.clone();
        drop(guard);
        let _ = self.persist_routine_runs().await;
        Some(updated)
    }

    pub async fn append_routine_run_artifact(
        &self,
        run_id: &str,
        artifact: RoutineRunArtifact,
    ) -> Option<RoutineRunRecord> {
        self.append_routine_run_artifact_matching(run_id, None, artifact)
            .await
    }

    pub async fn append_routine_run_artifact_for_tenant(
        &self,
        run_id: &str,
        tenant_context: &TenantContext,
        artifact: RoutineRunArtifact,
    ) -> Option<RoutineRunRecord> {
        self.append_routine_run_artifact_matching(run_id, Some(tenant_context), artifact)
            .await
    }

    async fn append_routine_run_artifact_matching(
        &self,
        run_id: &str,
        tenant_context: Option<&TenantContext>,
        artifact: RoutineRunArtifact,
    ) -> Option<RoutineRunRecord> {
        let mut guard = self.routine_runs.write().await;
        let row = guard.get_mut(run_id)?;
        if tenant_context.is_some_and(|tenant| {
            !RoutineIdentity::new(&row.routine_id, &row.tenant_context).matches_tenant(tenant)
        }) {
            return None;
        }
        row.updated_at_ms = now_ms();
        row.artifacts.push(artifact);
        let updated = row.clone();
        drop(guard);
        let _ = self.persist_routine_runs().await;
        Some(updated)
    }

    pub async fn add_active_session_id(
        &self,
        run_id: &str,
        session_id: String,
    ) -> Option<RoutineRunRecord> {
        let mut guard = self.routine_runs.write().await;
        let row = guard.get_mut(run_id)?;
        if !row.active_session_ids.iter().any(|id| id == &session_id) {
            row.active_session_ids.push(session_id);
        }
        row.latest_session_id = row.active_session_ids.last().cloned();
        row.updated_at_ms = now_ms();
        let updated = row.clone();
        drop(guard);
        let _ = self.persist_routine_runs().await;
        Some(updated)
    }

    pub async fn clear_active_session_id(
        &self,
        run_id: &str,
        session_id: &str,
    ) -> Option<RoutineRunRecord> {
        let mut guard = self.routine_runs.write().await;
        let row = guard.get_mut(run_id)?;
        row.active_session_ids.retain(|id| id != session_id);
        row.updated_at_ms = now_ms();
        let updated = row.clone();
        drop(guard);
        let _ = self.persist_routine_runs().await;
        Some(updated)
    }
}
