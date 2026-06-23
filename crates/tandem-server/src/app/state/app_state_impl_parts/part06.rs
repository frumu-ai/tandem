// Continuation of the AppState impl split from part01.rs for the file-size gate.
// A second `impl AppState` block (Rust permits multiple); included via mod.rs.

fn automation_v2_definition_is_context_recovered(automation: &AutomationV2Spec) -> bool {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("recovered_from"))
        .and_then(Value::as_str)
        == Some("context_run")
}

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
            if run.trigger_type == "recovered_context_run"
                && !Self::automation_run_is_terminal(&run.status)
            {
                continue;
            }
            let Some(snapshot) = run.automation_snapshot.clone() else {
                continue;
            };
            let snapshot_is_context_recovered =
                automation_v2_definition_is_context_recovered(&snapshot);
            let should_replace = match guard.get(&run.automation_id) {
                Some(existing)
                    if snapshot_is_context_recovered
                        && !automation_v2_definition_is_context_recovered(existing) =>
                {
                    false
                }
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

    pub async fn list_bug_monitor_posts_by_destination(
        &self,
        limit: usize,
        destination_id: &str,
    ) -> Vec<BugMonitorPostRecord> {
        let mut rows = self
            .bug_monitor_posts
            .read()
            .await
            .values()
            .filter(|row| {
                row.destination_id
                    .as_deref()
                    .unwrap_or(BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID)
                    == destination_id
            })
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

    pub async fn list_policy_decisions(
        &self,
        tenant_context: &TenantContext,
        limit: usize,
    ) -> Vec<PolicyDecisionRecord> {
        let mut rows = self
            .policy_decisions
            .read()
            .await
            .values()
            .filter(|decision| {
                decision.tenant_context.org_id == tenant_context.org_id
                    && decision.tenant_context.workspace_id == tenant_context.workspace_id
                    && decision.tenant_context.deployment_id == tenant_context.deployment_id
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
        rows.truncate(limit.clamp(1, 500));
        rows
    }

    pub async fn list_policy_decisions_for_run(
        &self,
        tenant_context: &TenantContext,
        run_id: &str,
        limit: usize,
    ) -> Vec<PolicyDecisionRecord> {
        let mut rows = self
            .policy_decisions
            .read()
            .await
            .values()
            .filter(|decision| {
                decision.run_id.as_deref() == Some(run_id)
                    && decision.tenant_context.org_id == tenant_context.org_id
                    && decision.tenant_context.workspace_id == tenant_context.workspace_id
                    && decision.tenant_context.deployment_id == tenant_context.deployment_id
            })
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

    fn intra_tenant_context_matches(candidate: &TenantContext, tenant_context: &TenantContext) -> bool {
        candidate.org_id == tenant_context.org_id
            && candidate.workspace_id == tenant_context.workspace_id
            && candidate.deployment_id == tenant_context.deployment_id
    }

    /// Build the intra-tenant authority graph (CT-18 / TAN-89) for
    /// `tenant_context` from stored enterprise org units, memberships, and
    /// access grants, plus any `direct_grants` carried by the caller's verified
    /// context strict projection.
    pub async fn build_intra_tenant_authority_graph(
        &self,
        tenant_context: &TenantContext,
        direct_grants: Vec<ScopedGrant>,
    ) -> IntraTenantAuthorityGraph {
        let units = self
            .enterprise.org_units
            .read()
            .await
            .values()
            .filter(|unit| Self::intra_tenant_context_matches(&unit.tenant_context, tenant_context))
            .cloned()
            .collect::<Vec<_>>();
        let memberships = self
            .enterprise.org_unit_memberships
            .read()
            .await
            .values()
            .filter(|membership| {
                Self::intra_tenant_context_matches(&membership.tenant_context, tenant_context)
            })
            .cloned()
            .collect::<Vec<_>>();
        let unit_access_grants = self
            .enterprise.org_unit_access_grants
            .read()
            .await
            .values()
            .filter(|grant| Self::intra_tenant_context_matches(&grant.tenant_context, tenant_context))
            .cloned()
            .collect::<Vec<_>>();

        let mut graph = IntraTenantAuthorityGraph::new(tenant_context.clone());
        graph.extend_units(units);
        graph.extend_memberships(memberships);
        graph.extend_unit_access_grants(unit_access_grants);
        graph.extend_direct_grants(direct_grants);
        graph
    }

    /// Evaluate an intra-tenant access request and record the decision.
    ///
    /// Every decision is persisted as a [`PolicyDecisionRecord`]; a denial also
    /// writes a tenant-attributed protected audit event so the denial leaves
    /// durable evidence. Returns the decision plus the id of the recorded
    /// policy decision (when recording succeeded).
    pub async fn enforce_intra_tenant_access(
        &self,
        tenant_context: &TenantContext,
        request: &AuthorityAccessRequest,
        direct_grants: Vec<ScopedGrant>,
        now_ms: u64,
    ) -> (AuthorityDecision, Option<String>) {
        let graph = self
            .build_intra_tenant_authority_graph(tenant_context, direct_grants)
            .await;
        let decision = graph.evaluate(request, now_ms);
        let decision_id = self
            .record_intra_tenant_authority_decision(tenant_context, request, &decision, now_ms)
            .await;
        (decision, decision_id)
    }

    async fn record_intra_tenant_authority_decision(
        &self,
        tenant_context: &TenantContext,
        request: &AuthorityAccessRequest,
        decision: &AuthorityDecision,
        now_ms: u64,
    ) -> Option<String> {
        let effect = match decision.effect {
            AuthorityEffect::Allow => PolicyDecisionEffect::Allow,
            AuthorityEffect::Deny => PolicyDecisionEffect::Deny,
        };
        let actor_id = request
            .principal
            .tenant_actor_id
            .clone()
            .or_else(|| tenant_context.actor_id.clone())
            .or_else(|| Some(request.principal.id.clone()));
        let decision_id = format!("policy_decision_{}", uuid::Uuid::new_v4().simple());
        let metadata = serde_json::json!({
            "authority": {
                "principal": request.principal,
                "permission": request.permission,
                "source_principal": decision.source_principal,
            }
        });
        let record = PolicyDecisionRecord {
            decision_id: decision_id.clone(),
            tenant_context: tenant_context.clone(),
            actor_id: actor_id.clone(),
            session_id: None,
            message_id: None,
            run_id: None,
            automation_id: None,
            node_id: None,
            tool: None,
            resource: Some(request.resource.clone()),
            data_classes: vec![request.data_class],
            risk_tier: None,
            decision: effect,
            reason_code: decision.reason_code.clone(),
            reason: decision.reason.clone(),
            policy_id: Some("intra_tenant_authority".to_string()),
            grant_id: decision.grant_id.clone(),
            approval_id: None,
            audit_event_id: None,
            created_at_ms: now_ms,
            metadata,
        };
        let recorded = match self.record_policy_decision(record).await {
            Ok(record) => Some(record.decision_id),
            Err(error) => {
                tracing::warn!("failed to record intra-tenant authority decision: {error:?}");
                None
            }
        };

        if decision.is_deny() {
            if let Err(error) = crate::audit::append_protected_audit_event(
                self,
                "authority.access.denied",
                tenant_context,
                actor_id,
                serde_json::json!({
                    "decision_id": recorded,
                    "principal": request.principal,
                    "resource": request.resource,
                    "permission": request.permission,
                    "data_class": request.data_class,
                    "reason_code": decision.reason_code,
                    "grant_id": decision.grant_id,
                    "source_principal": decision.source_principal,
                }),
            )
            .await
            {
                tracing::warn!("failed to append intra-tenant authority audit: {error:?}");
            }
        }

        recorded
    }

    /// Resolve an action against the strict approval gate matrix (CT-20 /
    /// TAN-91) and record the resulting gate decision.
    ///
    /// Every gate decision is persisted as a [`PolicyDecisionRecord`]; gates
    /// that require approval or deny outright also append a tenant-attributed
    /// protected audit event. Returns the resolved outcome plus the recorded
    /// policy decision id (when recording succeeded). Allowing execution off
    /// the back of an approval still requires checking the approval has not
    /// expired (see `tandem_types::gate_matrix::approval_authorizes_execution`).
    pub async fn enforce_action_gate(
        &self,
        tenant_context: &TenantContext,
        request: &GateRequest,
        tool: Option<String>,
        actor_id: Option<String>,
        now_ms: u64,
    ) -> (GateOutcome, Option<String>) {
        let outcome = ApprovalGateMatrix::strict_default().resolve(request);
        let decision_id = self
            .record_action_gate_decision(tenant_context, request, &outcome, tool, actor_id, now_ms)
            .await;
        (outcome, decision_id)
    }

    async fn record_action_gate_decision(
        &self,
        tenant_context: &TenantContext,
        request: &GateRequest,
        outcome: &GateOutcome,
        tool: Option<String>,
        actor_id: Option<String>,
        now_ms: u64,
    ) -> Option<String> {
        let decision_id = format!("policy_decision_{}", uuid::Uuid::new_v4().simple());
        let data_classes = request.data_class.map(|class| vec![class]).unwrap_or_default();
        let metadata = serde_json::json!({
            "gate": {
                "reviewer_eligibility": outcome.reviewer_eligibility.as_str(),
                "approval_ttl_ms": outcome.approval_ttl_ms,
                "external_customer_facing": request.external_customer_facing,
            }
        });
        let record = PolicyDecisionRecord {
            decision_id: decision_id.clone(),
            tenant_context: tenant_context.clone(),
            actor_id: actor_id.clone(),
            session_id: None,
            message_id: None,
            run_id: None,
            automation_id: None,
            node_id: None,
            tool,
            resource: None,
            data_classes,
            risk_tier: request.risk_tier.map(|tier| tier.as_str().to_string()),
            decision: outcome.effect,
            reason_code: outcome.reason_code.clone(),
            reason: outcome.reason.clone(),
            policy_id: Some("approval_gate_matrix".to_string()),
            grant_id: None,
            approval_id: None,
            audit_event_id: None,
            created_at_ms: now_ms,
            metadata,
        };
        let recorded = match self.record_policy_decision(record).await {
            Ok(record) => Some(record.decision_id),
            Err(error) => {
                tracing::warn!("failed to record approval gate decision: {error:?}");
                None
            }
        };

        // Approval-required and deny outcomes are consequential gate events and
        // must leave durable, tenant-attributed audit evidence.
        if !outcome.is_allowed() {
            let event_type = if outcome.is_denied() {
                "approval.gate.denied"
            } else {
                "approval.gate.approval_required"
            };
            if let Err(error) = crate::audit::append_protected_audit_event(
                self,
                event_type,
                tenant_context,
                actor_id,
                serde_json::json!({
                    "decision_id": recorded,
                    "risk_tier": request.risk_tier.map(|tier| tier.as_str()),
                    "data_class": request.data_class,
                    "external_customer_facing": request.external_customer_facing,
                    "effect": outcome.effect,
                    "reviewer_eligibility": outcome.reviewer_eligibility.as_str(),
                    "approval_ttl_ms": outcome.approval_ttl_ms,
                    "reason_code": outcome.reason_code,
                }),
            )
            .await
            {
                tracing::warn!("failed to append approval gate audit: {error:?}");
            }
        }

        recorded
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
