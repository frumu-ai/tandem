// Continuation of the AppState impl split from part01.rs for the file-size gate.
// A second `impl AppState` block (Rust permits multiple); included via mod.rs.

impl AppState {

    async fn recover_automation_definitions_from_run_snapshots(&self) -> anyhow::Result<usize> {
        let runs = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut guard = self.automations_v2.write().await;
        let mut recovered = 0usize;
        for run in runs {
            let Some(snapshot) = run.automation_snapshot.clone() else {
                continue;
            };
            let should_replace = match guard.get(&run.automation_id) {
                Some(existing) => existing.updated_at_ms < snapshot.updated_at_ms,
                None => true,
            };
            if should_replace {
                if !guard.contains_key(&run.automation_id) {
                    recovered += 1;
                }
                guard.insert(run.automation_id.clone(), snapshot);
            }
        }
        drop(guard);
        if recovered > 0 {
            let active_path = self.automations_v2_path.display().to_string();
            tracing::warn!(
                recovered,
                active_path,
                "recovered automation v2 definitions from run snapshots"
            );
            self.persist_automations_v2().await?;
        }
        Ok(recovered)
    }

    pub async fn load_bug_monitor_config(&self) -> anyhow::Result<()> {
        let path = if self.bug_monitor_config_path.exists() {
            self.bug_monitor_config_path.clone()
        } else if let Some(path) =
            config::paths::resolve_legacy_root_file_path("bug_monitor_config.json")
        {
            if path.exists() {
                path
            } else if config::paths::legacy_failure_reporter_path("failure_reporter_config.json")
                .exists()
            {
                config::paths::legacy_failure_reporter_path("failure_reporter_config.json")
            } else {
                return Ok(());
            }
        } else if config::paths::legacy_failure_reporter_path("failure_reporter_config.json")
            .exists()
        {
            config::paths::legacy_failure_reporter_path("failure_reporter_config.json")
        } else {
            return Ok(());
        };
        check_file_permissions(&path);
        let raw = fs::read_to_string(path).await?;
        let parsed = serde_json::from_str::<BugMonitorConfig>(&raw)
            .unwrap_or_else(|_| config::env::resolve_bug_monitor_env_config());
        *self.bug_monitor_config.write().await = parsed;
        Ok(())
    }

    pub async fn persist_bug_monitor_config(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.bug_monitor_config_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.bug_monitor_config.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.bug_monitor_config_path, payload).await?;
        Ok(())
    }

    pub async fn bug_monitor_config(&self) -> BugMonitorConfig {
        self.bug_monitor_config.read().await.clone()
    }

    pub async fn put_bug_monitor_config(
        &self,
        mut config: BugMonitorConfig,
    ) -> anyhow::Result<BugMonitorConfig> {
        config.workspace_root = config
            .workspace_root
            .as_ref()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        if let Some(repo) = config.repo.as_ref() {
            if !repo.is_empty() && !is_valid_owner_repo_slug(repo) {
                anyhow::bail!("repo must be in owner/repo format");
            }
        }
        if let Some(server) = config.mcp_server.as_ref() {
            let servers = self.mcp.list().await;
            if !servers.contains_key(server) {
                anyhow::bail!("unknown mcp server `{server}`");
            }
        }
        if let Some(model_policy) = config.model_policy.as_ref() {
            crate::http::routines_automations::validate_model_policy(model_policy)
                .map_err(anyhow::Error::msg)?;
        }
        validate_bug_monitor_monitored_projects(self, &mut config).await?;
        config.updated_at_ms = now_ms();
        *self.bug_monitor_config.write().await = config.clone();
        self.persist_bug_monitor_config().await?;
        Ok(config)
    }

    pub async fn load_bug_monitor_log_watcher_state(&self) -> anyhow::Result<()> {
        if !self.bug_monitor_log_watcher_state_path.exists() {
            return Ok(());
        }
        check_file_permissions(&self.bug_monitor_log_watcher_state_path);
        let raw = fs::read_to_string(&self.bug_monitor_log_watcher_state_path).await?;
        let parsed =
            serde_json::from_str::<BugMonitorLogWatcherStateFile>(&raw).unwrap_or_default();
        *self.bug_monitor_log_source_states.write().await = parsed.sources;
        Ok(())
    }

    pub async fn persist_bug_monitor_log_watcher_state(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.bug_monitor_log_watcher_state_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.bug_monitor_log_source_states.read().await;
            serde_json::to_string_pretty(&BugMonitorLogWatcherStateFile {
                schema_version: 1,
                sources: guard.clone(),
            })?
        };
        write_state_file_atomically(&self.bug_monitor_log_watcher_state_path, payload).await
    }

    pub async fn get_bug_monitor_log_source_state(
        &self,
        project_id: &str,
        source_id: &str,
    ) -> Option<BugMonitorLogSourceState> {
        self.bug_monitor_log_source_states
            .read()
            .await
            .get(&bug_monitor_log_source_state_key(project_id, source_id))
            .cloned()
    }

    pub async fn put_bug_monitor_log_source_state(
        &self,
        source_state: BugMonitorLogSourceState,
    ) -> anyhow::Result<BugMonitorLogSourceState> {
        let key =
            bug_monitor_log_source_state_key(&source_state.project_id, &source_state.source_id);
        self.bug_monitor_log_source_states
            .write()
            .await
            .insert(key, source_state.clone());
        self.persist_bug_monitor_log_watcher_state().await?;
        Ok(source_state)
    }

    pub async fn update_bug_monitor_log_watcher_status(
        &self,
        update: impl FnOnce(&mut BugMonitorLogWatcherStatus),
    ) -> BugMonitorLogWatcherStatus {
        let mut guard = self.bug_monitor_log_watcher_status.write().await;
        update(&mut guard);
        guard.clone()
    }

    pub async fn load_bug_monitor_intake_keys(&self) -> anyhow::Result<()> {
        if !self.bug_monitor_intake_keys_path.exists() {
            return Ok(());
        }
        check_file_permissions(&self.bug_monitor_intake_keys_path);
        let raw = fs::read_to_string(&self.bug_monitor_intake_keys_path).await?;
        let parsed = serde_json::from_str::<
            std::collections::HashMap<String, BugMonitorProjectIntakeKey>,
        >(&raw)
        .unwrap_or_default();
        *self.bug_monitor_intake_keys.write().await = parsed;
        Ok(())
    }

    pub async fn persist_bug_monitor_intake_keys(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.bug_monitor_intake_keys_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.bug_monitor_intake_keys.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        write_state_file_atomically(&self.bug_monitor_intake_keys_path, payload).await
    }

    pub async fn list_bug_monitor_intake_keys(&self) -> Vec<BugMonitorProjectIntakeKey> {
        let mut rows = self
            .bug_monitor_intake_keys
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.project_id.cmp(&b.project_id).then(a.name.cmp(&b.name)));
        rows
    }

    pub async fn put_bug_monitor_intake_key(
        &self,
        key: BugMonitorProjectIntakeKey,
    ) -> anyhow::Result<BugMonitorProjectIntakeKey> {
        self.bug_monitor_intake_keys
            .write()
            .await
            .insert(key.key_id.clone(), key.clone());
        self.persist_bug_monitor_intake_keys().await?;
        Ok(key)
    }

    pub async fn validate_bug_monitor_intake_key(
        &self,
        raw_key: &str,
        project_id: &str,
        required_scope: &str,
    ) -> Option<BugMonitorProjectIntakeKey> {
        let key_hash = crate::sha256_hex(&[raw_key.trim()]);
        let mut matched = {
            self.bug_monitor_intake_keys
                .read()
                .await
                .values()
                .find(|row| {
                    row.enabled
                        && row.project_id == project_id
                        && row.key_hash == key_hash
                        && row.scopes.iter().any(|scope| scope == required_scope)
                })
                .cloned()
        }?;
        matched.last_used_at_ms = Some(now_ms());
        let _ = self.put_bug_monitor_intake_key(matched.clone()).await;
        Some(matched)
    }

    pub async fn load_bug_monitor_drafts(&self) -> anyhow::Result<()> {
        let path = if self.bug_monitor_drafts_path.exists() {
            self.bug_monitor_drafts_path.clone()
        } else if let Some(path) =
            config::paths::resolve_legacy_root_file_path("bug_monitor_drafts.json")
        {
            if path.exists() {
                path
            } else if config::paths::legacy_failure_reporter_path("failure_reporter_drafts.json")
                .exists()
            {
                config::paths::legacy_failure_reporter_path("failure_reporter_drafts.json")
            } else {
                return Ok(());
            }
        } else if config::paths::legacy_failure_reporter_path("failure_reporter_drafts.json")
            .exists()
        {
            config::paths::legacy_failure_reporter_path("failure_reporter_drafts.json")
        } else {
            return Ok(());
        };
        let raw = fs::read_to_string(path).await?;
        let parsed =
            serde_json::from_str::<std::collections::HashMap<String, BugMonitorDraftRecord>>(&raw)
                .unwrap_or_default();
        *self.bug_monitor_drafts.write().await = parsed;
        Ok(())
    }

    pub async fn persist_bug_monitor_drafts(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.bug_monitor_drafts_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.bug_monitor_drafts.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.bug_monitor_drafts_path, payload).await?;
        Ok(())
    }

    pub async fn load_bug_monitor_incidents(&self) -> anyhow::Result<()> {
        let path = if self.bug_monitor_incidents_path.exists() {
            self.bug_monitor_incidents_path.clone()
        } else if let Some(path) =
            config::paths::resolve_legacy_root_file_path("bug_monitor_incidents.json")
        {
            if path.exists() {
                path
            } else if config::paths::legacy_failure_reporter_path("failure_reporter_incidents.json")
                .exists()
            {
                config::paths::legacy_failure_reporter_path("failure_reporter_incidents.json")
            } else {
                return Ok(());
            }
        } else if config::paths::legacy_failure_reporter_path("failure_reporter_incidents.json")
            .exists()
        {
            config::paths::legacy_failure_reporter_path("failure_reporter_incidents.json")
        } else {
            return Ok(());
        };
        let raw = fs::read_to_string(path).await?;
        let parsed = serde_json::from_str::<
            std::collections::HashMap<String, BugMonitorIncidentRecord>,
        >(&raw)
        .unwrap_or_default();
        *self.bug_monitor_incidents.write().await = parsed;
        Ok(())
    }

    pub async fn persist_bug_monitor_incidents(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.bug_monitor_incidents_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.bug_monitor_incidents.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.bug_monitor_incidents_path, payload).await?;
        Ok(())
    }

    pub async fn load_bug_monitor_posts(&self) -> anyhow::Result<()> {
        let path = if self.bug_monitor_posts_path.exists() {
            self.bug_monitor_posts_path.clone()
        } else if let Some(path) =
            config::paths::resolve_legacy_root_file_path("bug_monitor_posts.json")
        {
            if path.exists() {
                path
            } else if config::paths::legacy_failure_reporter_path("failure_reporter_posts.json")
                .exists()
            {
                config::paths::legacy_failure_reporter_path("failure_reporter_posts.json")
            } else {
                return Ok(());
            }
        } else if config::paths::legacy_failure_reporter_path("failure_reporter_posts.json")
            .exists()
        {
            config::paths::legacy_failure_reporter_path("failure_reporter_posts.json")
        } else {
            return Ok(());
        };
        let raw = fs::read_to_string(path).await?;
        let parsed =
            serde_json::from_str::<std::collections::HashMap<String, BugMonitorPostRecord>>(&raw)
                .unwrap_or_default();
        *self.bug_monitor_posts.write().await = parsed;
        Ok(())
    }

    pub async fn persist_bug_monitor_posts(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.bug_monitor_posts_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.bug_monitor_posts.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.bug_monitor_posts_path, payload).await?;
        Ok(())
    }

    pub async fn load_external_actions(&self) -> anyhow::Result<()> {
        let Some(raw) =
            read_state_file_with_legacy(&self.external_actions_path, "external_actions.json")
                .await?
        else {
            return Ok(());
        };
        let parsed =
            serde_json::from_str::<std::collections::HashMap<String, ExternalActionRecord>>(&raw)
                .unwrap_or_default();
        *self.external_actions.write().await = parsed;
        Ok(())
    }

    pub async fn load_policy_decisions(&self) -> anyhow::Result<()> {
        if !self.policy_decisions_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.policy_decisions_path).await?;
        let parsed =
            serde_json::from_str::<std::collections::HashMap<String, PolicyDecisionRecord>>(&raw)
                .unwrap_or_default();
        *self.policy_decisions.write().await = parsed;
        Ok(())
    }

    pub async fn persist_policy_decisions(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.policy_decisions_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.policy_decisions.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.policy_decisions_path, payload).await?;
        Ok(())
    }

    pub async fn persist_external_actions(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.external_actions_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.external_actions.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.external_actions_path, payload).await?;
        Ok(())
    }

    pub async fn list_bug_monitor_incidents(&self, limit: usize) -> Vec<BugMonitorIncidentRecord> {
        let mut rows = self
            .bug_monitor_incidents
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows.truncate(limit.clamp(1, 200));
        rows
    }

    pub async fn get_bug_monitor_incident(
        &self,
        incident_id: &str,
    ) -> Option<BugMonitorIncidentRecord> {
        self.bug_monitor_incidents
            .read()
            .await
            .get(incident_id)
            .cloned()
    }

    pub async fn put_bug_monitor_incident(
        &self,
        incident: BugMonitorIncidentRecord,
    ) -> anyhow::Result<BugMonitorIncidentRecord> {
        self.bug_monitor_incidents
            .write()
            .await
            .insert(incident.incident_id.clone(), incident.clone());
        self.persist_bug_monitor_incidents().await?;
        Ok(incident)
    }

    pub async fn delete_bug_monitor_incidents(&self, ids: &[String]) -> anyhow::Result<usize> {
        let mut removed = 0usize;
        {
            let mut guard = self.bug_monitor_incidents.write().await;
            for id in ids {
                if guard.remove(id).is_some() {
                    removed += 1;
                }
            }
        }
        if removed > 0 {
            self.persist_bug_monitor_incidents().await?;
        }
        Ok(removed)
    }

    pub async fn clear_bug_monitor_incidents(&self) -> anyhow::Result<usize> {
        let removed = {
            let mut guard = self.bug_monitor_incidents.write().await;
            let count = guard.len();
            guard.clear();
            count
        };
        if removed > 0 {
            self.persist_bug_monitor_incidents().await?;
        }
        Ok(removed)
    }

    pub async fn list_bug_monitor_posts(&self, limit: usize) -> Vec<BugMonitorPostRecord> {
        let mut rows = self
            .bug_monitor_posts
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows.truncate(limit.clamp(1, 200));
        rows
    }

    pub async fn get_bug_monitor_post(&self, post_id: &str) -> Option<BugMonitorPostRecord> {
        self.bug_monitor_posts.read().await.get(post_id).cloned()
    }

    pub async fn put_bug_monitor_post(
        &self,
        post: BugMonitorPostRecord,
    ) -> anyhow::Result<BugMonitorPostRecord> {
        self.bug_monitor_posts
            .write()
            .await
            .insert(post.post_id.clone(), post.clone());
        self.persist_bug_monitor_posts().await?;
        Ok(post)
    }

    pub async fn try_claim_bug_monitor_post_idempotency(
        &self,
        post: BugMonitorPostRecord,
    ) -> anyhow::Result<(bool, BugMonitorPostRecord)> {
        let now = crate::now_ms();
        let pending_claim_ttl_ms = 10 * 60 * 1000;
        let result = {
            let mut guard = self.bug_monitor_posts.write().await;
            if let Some(existing) = guard
                .values()
                .find(|row| {
                    row.idempotency_key == post.idempotency_key
                        && (row.status == "posted"
                            || (row.status == "pending"
                                && now.saturating_sub(row.updated_at_ms) < pending_claim_ttl_ms))
                })
                .cloned()
            {
                (false, existing)
            } else {
                guard.insert(post.post_id.clone(), post.clone());
                (true, post)
            }
        };
        if result.0 {
            self.persist_bug_monitor_posts().await?;
        }
        Ok(result)
    }

    pub async fn delete_bug_monitor_posts(&self, ids: &[String]) -> anyhow::Result<usize> {
        let mut removed = 0usize;
        {
            let mut guard = self.bug_monitor_posts.write().await;
            for id in ids {
                if guard.remove(id).is_some() {
                    removed += 1;
                }
            }
        }
        if removed > 0 {
            self.persist_bug_monitor_posts().await?;
        }
        Ok(removed)
    }

    pub async fn clear_bug_monitor_posts(&self) -> anyhow::Result<usize> {
        let removed = {
            let mut guard = self.bug_monitor_posts.write().await;
            let count = guard.len();
            guard.clear();
            count
        };
        if removed > 0 {
            self.persist_bug_monitor_posts().await?;
        }
        Ok(removed)
    }

    pub async fn list_external_actions(&self, limit: usize) -> Vec<ExternalActionRecord> {
        let mut rows = self
            .external_actions
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows.truncate(limit.clamp(1, 200));
        rows
    }

    pub async fn get_external_action(&self, action_id: &str) -> Option<ExternalActionRecord> {
        self.external_actions.read().await.get(action_id).cloned()
    }

    pub async fn list_policy_decisions(&self, limit: usize) -> Vec<PolicyDecisionRecord> {
        let mut rows = self
            .policy_decisions
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
        rows.truncate(limit.clamp(1, 500));
        rows
    }

    pub async fn list_policy_decisions_for_run(
        &self,
        run_id: &str,
        limit: usize,
    ) -> Vec<PolicyDecisionRecord> {
        let mut rows = self
            .policy_decisions
            .read()
            .await
            .values()
            .filter(|decision| decision.run_id.as_deref() == Some(run_id))
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
        rows.truncate(limit.clamp(1, 500));
        rows
    }

    pub async fn get_policy_decision(&self, decision_id: &str) -> Option<PolicyDecisionRecord> {
        self.policy_decisions
            .read()
            .await
            .get(decision_id)
            .cloned()
    }

    pub async fn record_policy_decision(
        &self,
        decision: PolicyDecisionRecord,
    ) -> anyhow::Result<PolicyDecisionRecord> {
        {
            let mut guard = self.policy_decisions.write().await;
            guard.insert(decision.decision_id.clone(), decision.clone());
        }
        self.persist_policy_decisions().await?;
        if self.is_ready() {
            self.event_bus.publish(EngineEvent::new(
                "policy.decision.recorded",
                serde_json::json!({
                    "decisionID": decision.decision_id.clone(),
                    "sessionID": decision.session_id.clone(),
                    "messageID": decision.message_id.clone(),
                    "runID": decision.run_id.clone(),
                    "automationID": decision.automation_id.clone(),
                    "tool": decision.tool.clone(),
                    "decision": decision.decision,
                    "reasonCode": decision.reason_code.clone(),
                    "tenantContext": decision.tenant_context.clone(),
                    "record": decision.clone(),
                }),
            ));
        }
        Ok(decision)
    }

    pub async fn get_external_action_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Option<ExternalActionRecord> {
        let normalized = idempotency_key.trim();
        if normalized.is_empty() {
            return None;
        }
        self.external_actions
            .read()
            .await
            .values()
            .find(|action| {
                action
                    .idempotency_key
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    == Some(normalized)
            })
            .cloned()
    }

    pub async fn put_external_action(
        &self,
        action: ExternalActionRecord,
    ) -> anyhow::Result<ExternalActionRecord> {
        self.external_actions
            .write()
            .await
            .insert(action.action_id.clone(), action.clone());
        self.persist_external_actions().await?;
        Ok(action)
    }

    pub async fn record_external_action(
        &self,
        action: ExternalActionRecord,
    ) -> anyhow::Result<ExternalActionRecord> {
        let action = {
            let mut guard = self.external_actions.write().await;
            if let Some(idempotency_key) = action
                .idempotency_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                if let Some(existing) = guard
                    .values()
                    .find(|existing| {
                        existing
                            .idempotency_key
                            .as_deref()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            == Some(idempotency_key)
                    })
                    .cloned()
                {
                    return Ok(existing);
                }
            }
            guard.insert(action.action_id.clone(), action.clone());
            action
        };
        self.persist_external_actions().await?;
        if let Some(run_id) = action.routine_run_id.as_deref() {
            let artifact = RoutineRunArtifact {
                artifact_id: format!("external-action-{}", action.action_id),
                uri: format!("external-action://{}", action.action_id),
                kind: "external_action_receipt".to_string(),
                label: Some(format!("external action receipt: {}", action.operation)),
                created_at_ms: action.updated_at_ms,
                metadata: Some(json!({
                    "actionID": action.action_id,
                    "operation": action.operation,
                    "status": action.status,
                    "sourceKind": action.source_kind,
                    "sourceID": action.source_id,
                    "capabilityID": action.capability_id,
                    "target": action.target,
                })),
            };
            let _ = self
                .append_routine_run_artifact(run_id, artifact.clone())
                .await;
            if let Some(runtime) = self.runtime.get() {
                runtime.event_bus.publish(EngineEvent::new(
                    "routine.run.artifact_added",
                    json!({
                        "runID": run_id,
                        "artifact": artifact,
                    }),
                ));
            }
        }
        if let Some(context_run_id) = action.context_run_id.as_deref() {
            let payload = serde_json::to_value(&action)?;
            if let Err(error) = crate::http::context_runs::append_json_artifact_to_context_run(
                self,
                context_run_id,
                &format!("external-action-{}", action.action_id),
                "external_action_receipt",
                &format!("external-actions/{}.json", action.action_id),
                &payload,
            )
            .await
            {
                tracing::warn!(
                    "failed to append external action artifact {} to context run {}: {}",
                    action.action_id,
                    context_run_id,
                    error
                );
            }
        }
        Ok(action)
    }

    pub async fn update_bug_monitor_runtime_status(
        &self,
        update: impl FnOnce(&mut BugMonitorRuntimeStatus),
    ) -> BugMonitorRuntimeStatus {
        let mut guard = self.bug_monitor_runtime_status.write().await;
        update(&mut guard);
        guard.clone()
    }

    pub async fn list_bug_monitor_drafts(&self, limit: usize) -> Vec<BugMonitorDraftRecord> {
        let mut rows = self
            .bug_monitor_drafts
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
        rows.truncate(limit.clamp(1, 200));
        rows
    }

    pub async fn get_bug_monitor_draft(&self, draft_id: &str) -> Option<BugMonitorDraftRecord> {
        self.bug_monitor_drafts.read().await.get(draft_id).cloned()
    }

    pub async fn put_bug_monitor_draft(
        &self,
        draft: BugMonitorDraftRecord,
    ) -> anyhow::Result<BugMonitorDraftRecord> {
        self.bug_monitor_drafts
            .write()
            .await
            .insert(draft.draft_id.clone(), draft.clone());
        self.persist_bug_monitor_drafts().await?;
        Ok(draft)
    }

    pub async fn delete_bug_monitor_drafts(&self, ids: &[String]) -> anyhow::Result<usize> {
        let mut removed = 0usize;
        {
            let mut guard = self.bug_monitor_drafts.write().await;
            for id in ids {
                if guard.remove(id).is_some() {
                    removed += 1;
                }
            }
        }
        if removed > 0 {
            self.persist_bug_monitor_drafts().await?;
        }
        Ok(removed)
    }

    pub async fn clear_bug_monitor_drafts(&self) -> anyhow::Result<usize> {
        let removed = {
            let mut guard = self.bug_monitor_drafts.write().await;
            let count = guard.len();
            guard.clear();
            count
        };
        if removed > 0 {
            self.persist_bug_monitor_drafts().await?;
        }
        Ok(removed)
    }
}
