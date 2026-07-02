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

/// Move an unparseable incident-monitor state file aside instead of silently
/// discarding it. Loads run at startup with their errors ignored, so without
/// this a corrupt file would be replaced by an empty default on the next
/// persist — losing publish receipts / idempotency keys and re-filing duplicate
/// external issues. Quarantining preserves the original bytes for recovery and
/// logs loudly, while the caller continues with empty in-memory state.
async fn quarantine_corrupt_incident_monitor_state(
    path: &std::path::Path,
    kind: &str,
    error: &serde_json::Error,
) {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("incident_monitor_state.json");
    let quarantined = path.with_file_name(format!("{file_name}.corrupt-{}", now_ms()));
    match fs::rename(path, &quarantined).await {
        Ok(()) => tracing::error!(
            path = %path.display(),
            quarantined = %quarantined.display(),
            error = %error,
            "incident monitor {kind} state failed to parse; quarantined the corrupt file and continued with empty state"
        ),
        Err(rename_error) => tracing::error!(
            path = %path.display(),
            error = %error,
            rename_error = %rename_error,
            "incident monitor {kind} state failed to parse and could not be quarantined; leaving the file in place to avoid overwriting recoverable data"
        ),
    }
}

/// Resolve which file to read incident-monitor state from, preferring the
/// canonical path but falling back to legacy locations AND legacy file names
/// (`failure_reporter_*` / `bug_monitor_*`) written by pre-rename deployments.
/// Returns `(path, is_legacy)`; a legacy hit is migrated to the canonical path
/// on read so upgrades don't silently come up empty and lose receipts /
/// idempotency history (TAN-542).
fn resolve_incident_monitor_state_read_path(
    canonical: &std::path::Path,
    file_stem: &str,
) -> Option<(std::path::PathBuf, bool)> {
    if canonical.exists() {
        return Some((canonical.to_path_buf(), false));
    }
    let candidate_names = [
        format!("incident_monitor_{file_stem}.json"),
        format!("failure_reporter_{file_stem}.json"),
        format!("bug_monitor_{file_stem}.json"),
    ];
    for name in &candidate_names {
        if let Some(path) = config::paths::resolve_legacy_root_file_path(name) {
            if path.exists() {
                return Some((path, true));
            }
        }
        let legacy = config::paths::legacy_incident_monitor_path(name);
        if legacy.exists() {
            return Some((legacy, true));
        }
    }
    None
}

fn policy_decision_scope_level(decision: &PolicyDecisionRecord) -> EnterprisePolicyScopeLevel {
    if policy_decision_workflow_phase(decision).is_some() {
        EnterprisePolicyScopeLevel::Phase
    } else if decision.resource.is_some() {
        EnterprisePolicyScopeLevel::Resource
    } else if decision.automation_id.is_some()
        || decision.node_id.is_some()
        || decision.run_id.is_some()
    {
        EnterprisePolicyScopeLevel::Workflow
    } else if policy_decision_org_unit_id(decision).is_some() {
        EnterprisePolicyScopeLevel::OrgUnit
    } else {
        EnterprisePolicyScopeLevel::Tenant
    }
}

fn policy_decision_metadata_string(
    decision: &PolicyDecisionRecord,
    pointers: &[&str],
) -> Option<String> {
    pointers.iter().find_map(|pointer| {
        decision
            .metadata
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn policy_decision_org_unit_id(decision: &PolicyDecisionRecord) -> Option<String> {
    policy_decision_metadata_string(
        decision,
        &[
            "/enterprise_scope/owning_org_unit_id",
            "/enterprise_scope/owningOrgUnitId",
            "/resource_access/owning_org_unit_id",
            "/resourceAccess/owningOrgUnitId",
            "/automation_webhook/owning_org_unit_id",
            "/automationWebhook/owningOrgUnitId",
            "/org_unit_id",
            "/orgUnitId",
        ],
    )
}

fn policy_decision_workflow_id(decision: &PolicyDecisionRecord) -> Option<String> {
    decision
        .automation_id
        .clone()
        .or_else(|| decision.run_id.clone())
}

fn policy_decision_workflow_phase(decision: &PolicyDecisionRecord) -> Option<String> {
    policy_decision_metadata_string(
        decision,
        &[
            "/phase_tool_authority/phase",
            "/workflow_phase",
            "/workflowPhase",
            "/builder/phase",
            "/runtime/phase",
        ],
    )
}

fn policy_decision_permission(decision: &PolicyDecisionRecord) -> Option<AccessPermission> {
    [
        "/authority/permission",
        "/permission",
        "/context_assertion/permission",
        "/memory_promotion/permission",
    ]
    .iter()
    .find_map(|pointer| {
        decision
            .metadata
            .pointer(pointer)
            .and_then(|value| serde_json::from_value::<AccessPermission>(value.clone()).ok())
    })
    .or_else(|| decision.tool.as_ref().map(|_| AccessPermission::Execute))
    .or_else(|| decision.resource.as_ref().map(|_| AccessPermission::Read))
}

fn policy_decision_input_base(decision: &PolicyDecisionRecord) -> EnterprisePolicyInput {
    let mut input = EnterprisePolicyInput::new(decision.tenant_context.clone());
    if let Some(org_unit_id) = policy_decision_org_unit_id(decision) {
        input = input.with_org_unit_id(org_unit_id);
    }
    if let Some(resource) = decision.resource.clone() {
        input = input.with_resource(resource);
    }
    if let Some(workflow_id) = policy_decision_workflow_id(decision) {
        input = input.with_workflow_id(workflow_id);
    }
    if let Some(workflow_phase) = policy_decision_workflow_phase(decision) {
        input = input.with_workflow_phase(workflow_phase);
    }
    if let Some(permission) = policy_decision_permission(decision) {
        input = input.with_permission(permission);
    }
    if let Some(tool) = decision.tool.clone() {
        input = input.with_tool(tool);
    }
    input
}

fn policy_decision_inputs(decision: &PolicyDecisionRecord) -> Vec<EnterprisePolicyInput> {
    let input = policy_decision_input_base(decision);
    if decision.data_classes.is_empty() {
        return vec![input];
    }
    decision
        .data_classes
        .iter()
        .copied()
        .map(|data_class| input.clone().with_data_class(data_class))
        .collect()
}

fn enterprise_policy_effect_priority(effect: EnterprisePolicyEffect) -> u8 {
    match effect {
        EnterprisePolicyEffect::Allow => 0,
        EnterprisePolicyEffect::ApprovalRequired => 1,
        EnterprisePolicyEffect::Deny => 2,
    }
}

fn effective_policy_snapshot_priority(
    snapshot: &tandem_enterprise_contract::EffectivePolicySnapshot,
) -> (u8, u8, u64, u64, String) {
    let Some(source) = snapshot.decision_source.as_ref() else {
        return (
            enterprise_policy_effect_priority(snapshot.effect),
            0,
            0,
            snapshot.resolved_at_ms,
            String::new(),
        );
    };
    (
        enterprise_policy_effect_priority(snapshot.effect),
        source.scope_level.inheritance_rank(),
        source.version,
        snapshot.resolved_at_ms,
        source.rule_id.clone(),
    )
}

fn select_effective_policy_snapshot(
    snapshots: Vec<tandem_enterprise_contract::EffectivePolicySnapshot>,
) -> Option<tandem_enterprise_contract::EffectivePolicySnapshot> {
    snapshots
        .into_iter()
        .max_by_key(effective_policy_snapshot_priority)
}

fn authority_decision_from_policy_record(
    mut decision: AuthorityDecision,
    record: &PolicyDecisionRecord,
) -> AuthorityDecision {
    decision.effect = match record.decision {
        PolicyDecisionEffect::Allow => AuthorityEffect::Allow,
        PolicyDecisionEffect::Deny | PolicyDecisionEffect::ApprovalRequired => AuthorityEffect::Deny,
    };
    decision.reason_code = record.reason_code.clone();
    decision.reason = record.reason.clone();
    decision.grant_id = record.grant_id.clone();
    decision
}

fn gate_outcome_from_policy_record(
    mut outcome: GateOutcome,
    record: &PolicyDecisionRecord,
) -> GateOutcome {
    outcome.effect = record.decision;
    outcome.reason_code = record.reason_code.clone();
    outcome.reason = record.reason.clone();
    match record.decision {
        PolicyDecisionEffect::Allow | PolicyDecisionEffect::Deny => {
            outcome.reviewer_eligibility = tandem_types::ReviewerEligibility::None;
            outcome.approval_ttl_ms = 0;
        }
        PolicyDecisionEffect::ApprovalRequired => {
            if matches!(
                outcome.reviewer_eligibility,
                tandem_types::ReviewerEligibility::None
            ) {
                outcome.reviewer_eligibility = tandem_types::ReviewerEligibility::ElevatedReviewer;
            }
            if outcome.approval_ttl_ms == 0 {
                outcome.approval_ttl_ms = tandem_types::ELEVATED_APPROVAL_TTL_MS;
            }
        }
    }
    outcome
}

fn runtime_policy_rule_for_decision(decision: &PolicyDecisionRecord) -> EnterprisePolicyRule {
    let policy_id = decision
        .policy_id
        .clone()
        .unwrap_or_else(|| "runtime_policy_decision".to_string());
    let mut rule = EnterprisePolicyRule::new(
        format!("{policy_id}:{}", decision.decision_id),
        policy_id,
        policy_decision_scope_level(decision),
        decision.decision.enterprise_effect(),
    )
    .with_tenant_context(decision.tenant_context.clone())
    .with_reason(decision.reason_code.clone(), decision.reason.clone())
    .with_updated_at_ms(decision.created_at_ms)
    .with_overridable(true);

    if let Some(org_unit_id) = policy_decision_org_unit_id(decision) {
        rule = rule.with_org_unit_id(org_unit_id);
    }
    if let Some(resource) = decision.resource.clone() {
        rule = rule.with_resource(resource);
    }
    if let Some(workflow_id) = policy_decision_workflow_id(decision) {
        rule = rule.with_workflow_id(workflow_id);
    }
    if let Some(workflow_phase) = policy_decision_workflow_phase(decision) {
        rule = rule.with_workflow_phase(workflow_phase);
    }
    if let Some(permission) = policy_decision_permission(decision) {
        rule = rule.with_permissions(vec![permission]);
    }
    if !decision.data_classes.is_empty() {
        rule = rule.with_data_classes(decision.data_classes.clone());
    }
    if let Some(tool) = decision.tool.clone() {
        rule = rule.with_tool_patterns(vec![tool]);
    }
    if let Some(approval_id) = decision.approval_id.clone() {
        rule = rule.with_approval_id(approval_id);
    }
    rule
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

    pub async fn load_incident_monitor_config(&self) -> anyhow::Result<()> {
        let Some((path, is_legacy)) = resolve_incident_monitor_state_read_path(
            &self.incident_monitor_config_path,
            "config",
        ) else {
            return Ok(());
        };
        check_file_permissions(&path);
        let raw = fs::read_to_string(&path).await?;
        let (parsed, migrate) = match serde_json::from_str::<IncidentMonitorConfig>(&raw) {
            Ok(parsed) => (parsed, is_legacy),
            Err(error) => {
                quarantine_corrupt_incident_monitor_state(&path, "config", &error).await;
                (config::env::resolve_incident_monitor_env_config(), false)
            }
        };
        *self.incident_monitor_config.write().await = parsed;
        if migrate {
            self.migrate_legacy_incident_monitor_state(&path, "config", || async {
                self.persist_incident_monitor_config().await
            })
            .await;
        }
        Ok(())
    }

    /// Persist migrated legacy state to the canonical path (one-time on read)
    /// and log a deprecation warning. Failures are logged, not fatal — the
    /// in-memory state is already loaded and the legacy file is left intact.
    async fn migrate_legacy_incident_monitor_state<F, Fut>(
        &self,
        legacy_path: &std::path::Path,
        kind: &str,
        persist: F,
    ) where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<()>>,
    {
        match persist().await {
            Ok(()) => tracing::warn!(
                legacy_path = %legacy_path.display(),
                "migrated legacy incident monitor {kind} state to the canonical path; the legacy file name is deprecated and will stop being read in a future release"
            ),
            Err(error) => tracing::error!(
                legacy_path = %legacy_path.display(),
                error = %error,
                "failed to migrate legacy incident monitor {kind} state to the canonical path; will retry on next load"
            ),
        }
    }

    pub async fn persist_incident_monitor_config(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.incident_monitor_config_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.incident_monitor_config.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        write_state_file_atomically(&self.incident_monitor_config_path, payload).await
    }

    pub async fn incident_monitor_config(&self) -> IncidentMonitorConfig {
        self.incident_monitor_config.read().await.clone()
    }

    pub async fn put_incident_monitor_config(
        &self,
        mut config: IncidentMonitorConfig,
    ) -> anyhow::Result<IncidentMonitorConfig> {
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
        validate_incident_monitor_monitored_projects(self, &mut config).await?;
        config.updated_at_ms = now_ms();
        *self.incident_monitor_config.write().await = config.clone();
        self.persist_incident_monitor_config().await?;
        Ok(config)
    }

    pub async fn load_incident_monitor_log_watcher_state(&self) -> anyhow::Result<()> {
        if !self.incident_monitor_log_watcher_state_path.exists() {
            return Ok(());
        }
        check_file_permissions(&self.incident_monitor_log_watcher_state_path);
        let raw = fs::read_to_string(&self.incident_monitor_log_watcher_state_path).await?;
        let parsed =
            serde_json::from_str::<IncidentMonitorLogWatcherStateFile>(&raw).unwrap_or_default();
        *self.incident_monitor_log_source_states.write().await = parsed.sources;
        Ok(())
    }

    pub async fn persist_incident_monitor_log_watcher_state(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.incident_monitor_log_watcher_state_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.incident_monitor_log_source_states.read().await;
            serde_json::to_string_pretty(&IncidentMonitorLogWatcherStateFile {
                schema_version: 1,
                sources: guard.clone(),
            })?
        };
        write_state_file_atomically(&self.incident_monitor_log_watcher_state_path, payload).await
    }

    pub async fn get_incident_monitor_log_source_state(
        &self,
        project_id: &str,
        source_id: &str,
    ) -> Option<IncidentMonitorLogSourceState> {
        self.incident_monitor_log_source_states
            .read()
            .await
            .get(&incident_monitor_log_source_state_key(
                project_id, source_id,
            ))
            .cloned()
    }

    pub async fn put_incident_monitor_log_source_state(
        &self,
        source_state: IncidentMonitorLogSourceState,
    ) -> anyhow::Result<IncidentMonitorLogSourceState> {
        let key = incident_monitor_log_source_state_key(
            &source_state.project_id,
            &source_state.source_id,
        );
        self.incident_monitor_log_source_states
            .write()
            .await
            .insert(key, source_state.clone());
        self.persist_incident_monitor_log_watcher_state().await?;
        Ok(source_state)
    }

    pub async fn update_incident_monitor_log_watcher_status(
        &self,
        update: impl FnOnce(&mut IncidentMonitorLogWatcherStatus),
    ) -> IncidentMonitorLogWatcherStatus {
        let mut guard = self.incident_monitor_log_watcher_status.write().await;
        update(&mut guard);
        guard.clone()
    }

    pub async fn load_incident_monitor_intake_keys(&self) -> anyhow::Result<()> {
        if !self.incident_monitor_intake_keys_path.exists() {
            return Ok(());
        }
        check_file_permissions(&self.incident_monitor_intake_keys_path);
        let raw = fs::read_to_string(&self.incident_monitor_intake_keys_path).await?;
        let parsed = serde_json::from_str::<
            std::collections::HashMap<String, IncidentMonitorProjectIntakeKey>,
        >(&raw)
        .unwrap_or_default();
        *self.incident_monitor_intake_keys.write().await = parsed;
        Ok(())
    }

    pub async fn persist_incident_monitor_intake_keys(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.incident_monitor_intake_keys_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.incident_monitor_intake_keys.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        write_state_file_atomically(&self.incident_monitor_intake_keys_path, payload).await
    }

    pub async fn list_incident_monitor_intake_keys(&self) -> Vec<IncidentMonitorProjectIntakeKey> {
        let mut rows = self
            .incident_monitor_intake_keys
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.project_id.cmp(&b.project_id).then(a.name.cmp(&b.name)));
        rows
    }

    pub async fn put_incident_monitor_intake_key(
        &self,
        key: IncidentMonitorProjectIntakeKey,
    ) -> anyhow::Result<IncidentMonitorProjectIntakeKey> {
        self.incident_monitor_intake_keys
            .write()
            .await
            .insert(key.key_id.clone(), key.clone());
        self.persist_incident_monitor_intake_keys().await?;
        Ok(key)
    }

    pub async fn validate_incident_monitor_intake_key(
        &self,
        raw_key: &str,
        project_id: &str,
        required_scope: &str,
    ) -> Option<IncidentMonitorProjectIntakeKey> {
        let key_hash = crate::sha256_hex(&[raw_key.trim()]);
        let mut matched = {
            self.incident_monitor_intake_keys
                .read()
                .await
                .values()
                .find(|row| {
                    row.enabled
                        && row.project_id == project_id
                        && crate::constant_time_str_eq(&row.key_hash, &key_hash)
                        && row.scopes.iter().any(|scope| scope == required_scope)
                })
                .cloned()
        }?;
        matched.last_used_at_ms = Some(now_ms());
        let _ = self.put_incident_monitor_intake_key(matched.clone()).await;
        Some(matched)
    }

    pub async fn load_incident_monitor_drafts(&self) -> anyhow::Result<()> {
        let Some((path, is_legacy)) = resolve_incident_monitor_state_read_path(
            &self.incident_monitor_drafts_path,
            "drafts",
        ) else {
            return Ok(());
        };
        let raw = fs::read_to_string(&path).await?;
        let (parsed, migrate) = match serde_json::from_str::<
            std::collections::HashMap<String, IncidentMonitorDraftRecord>,
        >(&raw)
        {
            Ok(parsed) => (parsed, is_legacy),
            Err(error) => {
                quarantine_corrupt_incident_monitor_state(&path, "drafts", &error).await;
                (std::collections::HashMap::new(), false)
            }
        };
        *self.incident_monitor_drafts.write().await = parsed;
        if migrate {
            self.migrate_legacy_incident_monitor_state(&path, "drafts", || async {
                self.persist_incident_monitor_drafts().await
            })
            .await;
        }
        Ok(())
    }

    pub async fn persist_incident_monitor_drafts(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.incident_monitor_drafts_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.incident_monitor_drafts.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        write_state_file_atomically(&self.incident_monitor_drafts_path, payload).await
    }

    pub async fn load_incident_monitor_incidents(&self) -> anyhow::Result<()> {
        let Some((path, is_legacy)) = resolve_incident_monitor_state_read_path(
            &self.incident_monitor_incidents_path,
            "incidents",
        ) else {
            return Ok(());
        };
        let raw = fs::read_to_string(&path).await?;
        let (parsed, migrate) = match serde_json::from_str::<
            std::collections::HashMap<String, IncidentMonitorIncidentRecord>,
        >(&raw)
        {
            Ok(parsed) => (parsed, is_legacy),
            Err(error) => {
                quarantine_corrupt_incident_monitor_state(&path, "incidents", &error).await;
                (std::collections::HashMap::new(), false)
            }
        };
        *self.incident_monitor_incidents.write().await = parsed;
        if migrate {
            self.migrate_legacy_incident_monitor_state(&path, "incidents", || async {
                self.persist_incident_monitor_incidents().await
            })
            .await;
        }
        Ok(())
    }

    pub async fn persist_incident_monitor_incidents(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.incident_monitor_incidents_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.incident_monitor_incidents.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        write_state_file_atomically(&self.incident_monitor_incidents_path, payload).await
    }

    pub async fn load_incident_monitor_posts(&self) -> anyhow::Result<()> {
        let Some((path, is_legacy)) = resolve_incident_monitor_state_read_path(
            &self.incident_monitor_posts_path,
            "posts",
        ) else {
            return Ok(());
        };
        let raw = fs::read_to_string(&path).await?;
        let (parsed, migrate) = match serde_json::from_str::<
            std::collections::HashMap<String, IncidentMonitorPostRecord>,
        >(&raw)
        {
            Ok(parsed) => (parsed, is_legacy),
            Err(error) => {
                quarantine_corrupt_incident_monitor_state(&path, "posts", &error).await;
                (std::collections::HashMap::new(), false)
            }
        };
        *self.incident_monitor_posts.write().await = parsed;
        if migrate {
            self.migrate_legacy_incident_monitor_state(&path, "posts", || async {
                self.persist_incident_monitor_posts().await
            })
            .await;
        }
        Ok(())
    }

    pub async fn persist_incident_monitor_posts(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.incident_monitor_posts_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.incident_monitor_posts.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        write_state_file_atomically(&self.incident_monitor_posts_path, payload).await
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

    pub async fn list_incident_monitor_incidents(
        &self,
        limit: usize,
    ) -> Vec<IncidentMonitorIncidentRecord> {
        let mut rows = self
            .incident_monitor_incidents
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows.truncate(limit.clamp(1, 200));
        rows
    }

    pub async fn get_incident_monitor_incident(
        &self,
        incident_id: &str,
    ) -> Option<IncidentMonitorIncidentRecord> {
        self.incident_monitor_incidents
            .read()
            .await
            .get(incident_id)
            .cloned()
    }

    pub async fn put_incident_monitor_incident(
        &self,
        incident: IncidentMonitorIncidentRecord,
    ) -> anyhow::Result<IncidentMonitorIncidentRecord> {
        self.incident_monitor_incidents
            .write()
            .await
            .insert(incident.incident_id.clone(), incident.clone());
        self.persist_incident_monitor_incidents().await?;
        Ok(incident)
    }

    pub async fn delete_incident_monitor_incidents(&self, ids: &[String]) -> anyhow::Result<usize> {
        let mut removed = 0usize;
        {
            let mut guard = self.incident_monitor_incidents.write().await;
            for id in ids {
                if guard.remove(id).is_some() {
                    removed += 1;
                }
            }
        }
        if removed > 0 {
            self.persist_incident_monitor_incidents().await?;
        }
        Ok(removed)
    }

    pub async fn clear_incident_monitor_incidents(&self) -> anyhow::Result<usize> {
        let removed = {
            let mut guard = self.incident_monitor_incidents.write().await;
            let count = guard.len();
            guard.clear();
            count
        };
        if removed > 0 {
            self.persist_incident_monitor_incidents().await?;
        }
        Ok(removed)
    }

    pub async fn list_incident_monitor_posts(
        &self,
        limit: usize,
    ) -> Vec<IncidentMonitorPostRecord> {
        let mut rows = self
            .incident_monitor_posts
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows.truncate(limit.clamp(1, 200));
        rows
    }

    pub async fn list_incident_monitor_posts_by_destination(
        &self,
        limit: usize,
        destination_id: &str,
    ) -> Vec<IncidentMonitorPostRecord> {
        let mut rows = self
            .incident_monitor_posts
            .read()
            .await
            .values()
            .filter(|row| {
                row.destination_id
                    .as_deref()
                    .unwrap_or(INCIDENT_MONITOR_LEGACY_GITHUB_DESTINATION_ID)
                    == destination_id
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows.truncate(limit.clamp(1, 200));
        rows
    }

    pub async fn get_incident_monitor_post(
        &self,
        post_id: &str,
    ) -> Option<IncidentMonitorPostRecord> {
        self.incident_monitor_posts
            .read()
            .await
            .get(post_id)
            .cloned()
    }

    /// TAN-546: stamp a publish receipt with the tenant/workspace of its draft
    /// so tenant-scoped assessment reports can filter receipts the same way they
    /// filter incidents and audit events. Only fills gaps — an adapter that
    /// already set the fields wins, and single-tenant drafts leave them None.
    async fn stamp_incident_monitor_post_tenant(&self, post: &mut IncidentMonitorPostRecord) {
        if post.tenant_id.is_some() || post.workspace_id.is_some() {
            return;
        }
        if let Some(draft) = self.get_incident_monitor_draft(&post.draft_id).await {
            post.tenant_id = draft.tenant_id.clone();
            post.workspace_id = draft.workspace_id.clone();
        }
    }

    pub async fn put_incident_monitor_post(
        &self,
        mut post: IncidentMonitorPostRecord,
    ) -> anyhow::Result<IncidentMonitorPostRecord> {
        self.stamp_incident_monitor_post_tenant(&mut post).await;
        self.incident_monitor_posts
            .write()
            .await
            .insert(post.post_id.clone(), post.clone());
        self.persist_incident_monitor_posts().await?;
        Ok(post)
    }

    pub async fn try_claim_incident_monitor_post_idempotency(
        &self,
        mut post: IncidentMonitorPostRecord,
    ) -> anyhow::Result<(bool, IncidentMonitorPostRecord)> {
        self.stamp_incident_monitor_post_tenant(&mut post).await;
        let now = crate::now_ms();
        let pending_claim_ttl_ms = 10 * 60 * 1000;
        let result = {
            let mut guard = self.incident_monitor_posts.write().await;
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
            self.persist_incident_monitor_posts().await?;
        }
        Ok(result)
    }

    pub async fn delete_incident_monitor_posts(&self, ids: &[String]) -> anyhow::Result<usize> {
        let mut removed = 0usize;
        {
            let mut guard = self.incident_monitor_posts.write().await;
            for id in ids {
                if guard.remove(id).is_some() {
                    removed += 1;
                }
            }
        }
        if removed > 0 {
            self.persist_incident_monitor_posts().await?;
        }
        Ok(removed)
    }

    pub async fn clear_incident_monitor_posts(&self) -> anyhow::Result<usize> {
        let removed = {
            let mut guard = self.incident_monitor_posts.write().await;
            let count = guard.len();
            guard.clear();
            count
        };
        if removed > 0 {
            self.persist_incident_monitor_posts().await?;
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
        self.policy_decisions.read().await.get(decision_id).cloned()
    }

    pub async fn record_policy_decision(
        &self,
        decision: PolicyDecisionRecord,
    ) -> anyhow::Result<PolicyDecisionRecord> {
        let decision = if decision.effective_policy_snapshot().is_some() {
            decision
        } else {
            let snapshot = self.resolve_effective_policy_snapshot(&decision).await;
            decision.apply_effective_policy_snapshot(snapshot)
        };
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

    async fn resolve_effective_policy_snapshot(
        &self,
        decision: &PolicyDecisionRecord,
    ) -> tandem_enterprise_contract::EffectivePolicySnapshot {
        if let Err(error) = self.load_enterprise_policy_rules_if_needed().await {
            tracing::warn!("failed to load enterprise policy rules for resolver: {error:?}");
        }
        let mut rules = self
            .enterprise
            .policy_rules
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rules.push(runtime_policy_rule_for_decision(decision));
        let resolver = EnterprisePolicyResolver::new(rules);
        let snapshots = policy_decision_inputs(decision)
            .iter()
            .map(|input| resolver.resolve(input, decision.created_at_ms))
            .collect::<Vec<_>>();
        select_effective_policy_snapshot(snapshots).unwrap_or_else(|| {
            resolver.resolve(
                &policy_decision_input_base(decision),
                decision.created_at_ms,
            )
        })
    }

    async fn load_enterprise_policy_rules_if_needed(&self) -> anyhow::Result<()> {
        if !self.enterprise.policy_rules.read().await.is_empty() {
            return Ok(());
        }
        if !self.enterprise.policy_rules_path.exists() {
            return Ok(());
        }
        check_file_permissions(&self.enterprise.policy_rules_path);
        let bytes = fs::read(&self.enterprise.policy_rules_path).await?;
        let registry: std::collections::HashMap<String, EnterprisePolicyRule> =
            serde_json::from_slice(&bytes)?;
        *self.enterprise.policy_rules.write().await = registry;
        Ok(())
    }

    fn intra_tenant_context_matches(
        candidate: &TenantContext,
        tenant_context: &TenantContext,
    ) -> bool {
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
            .enterprise
            .org_units
            .read()
            .await
            .values()
            .filter(|unit| Self::intra_tenant_context_matches(&unit.tenant_context, tenant_context))
            .cloned()
            .collect::<Vec<_>>();
        let memberships = self
            .enterprise
            .org_unit_memberships
            .read()
            .await
            .values()
            .filter(|membership| {
                Self::intra_tenant_context_matches(&membership.tenant_context, tenant_context)
            })
            .cloned()
            .collect::<Vec<_>>();
        let unit_access_grants = self
            .enterprise
            .org_unit_access_grants
            .read()
            .await
            .values()
            .filter(|grant| {
                Self::intra_tenant_context_matches(&grant.tenant_context, tenant_context)
            })
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
        let recorded = self
            .record_intra_tenant_authority_decision(tenant_context, request, &decision, now_ms)
            .await;
        let resolved_decision = recorded
            .as_ref()
            .map(|record| authority_decision_from_policy_record(decision.clone(), record))
            .unwrap_or(decision);
        let decision_id = recorded.map(|record| record.decision_id);
        (resolved_decision, decision_id)
    }

    async fn record_intra_tenant_authority_decision(
        &self,
        tenant_context: &TenantContext,
        request: &AuthorityAccessRequest,
        decision: &AuthorityDecision,
        now_ms: u64,
    ) -> Option<PolicyDecisionRecord> {
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
            Ok(record) => Some(record),
            Err(error) => {
                tracing::warn!("failed to record intra-tenant authority decision: {error:?}");
                None
            }
        };
        let audit_decision = recorded
            .as_ref()
            .map(|record| authority_decision_from_policy_record(decision.clone(), record))
            .unwrap_or_else(|| decision.clone());

        if audit_decision.is_deny() {
            if let Err(error) = crate::audit::append_protected_audit_event(
                self,
                "authority.access.denied",
                tenant_context,
                actor_id,
                serde_json::json!({
                    "decision_id": recorded.as_ref().map(|record| record.decision_id.as_str()),
                    "principal": request.principal,
                    "resource": request.resource,
                    "permission": request.permission,
                    "data_class": request.data_class,
                    "reason_code": audit_decision.reason_code,
                    "grant_id": audit_decision.grant_id,
                    "source_principal": audit_decision.source_principal,
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
        let recorded = self
            .record_action_gate_decision(tenant_context, request, &outcome, tool, actor_id, now_ms)
            .await;
        let resolved_outcome = recorded
            .as_ref()
            .map(|record| gate_outcome_from_policy_record(outcome.clone(), record))
            .unwrap_or(outcome);
        let decision_id = recorded.map(|record| record.decision_id);
        (resolved_outcome, decision_id)
    }

    async fn record_action_gate_decision(
        &self,
        tenant_context: &TenantContext,
        request: &GateRequest,
        outcome: &GateOutcome,
        tool: Option<String>,
        actor_id: Option<String>,
        now_ms: u64,
    ) -> Option<PolicyDecisionRecord> {
        let decision_id = format!("policy_decision_{}", uuid::Uuid::new_v4().simple());
        let data_classes = request
            .data_class
            .map(|class| vec![class])
            .unwrap_or_default();
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
            Ok(record) => Some(record),
            Err(error) => {
                tracing::warn!("failed to record approval gate decision: {error:?}");
                None
            }
        };
        let audit_outcome = recorded
            .as_ref()
            .map(|record| gate_outcome_from_policy_record(outcome.clone(), record))
            .unwrap_or_else(|| outcome.clone());

        // Approval-required and deny outcomes are consequential gate events and
        // must leave durable, tenant-attributed audit evidence.
        if !audit_outcome.is_allowed() {
            let event_type = if audit_outcome.is_denied() {
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
                    "decision_id": recorded.as_ref().map(|record| record.decision_id.as_str()),
                    "risk_tier": request.risk_tier.map(|tier| tier.as_str()),
                    "data_class": request.data_class,
                    "external_customer_facing": request.external_customer_facing,
                    "effect": audit_outcome.effect,
                    "reviewer_eligibility": audit_outcome.reviewer_eligibility.as_str(),
                    "approval_ttl_ms": audit_outcome.approval_ttl_ms,
                    "reason_code": audit_outcome.reason_code,
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
        let reliability_path =
            crate::stateful_runtime::stateful_reliability_path_from_runtime_events_path(
                &self.runtime_events_path,
            );
        let reliability_scope = self.stateful_scope_for_external_action(&action).await;
        if let Err(error) = crate::stateful_runtime::record_external_action_reliability_bridge(
            &reliability_path,
            reliability_scope,
            &action,
        )
        .await
        {
            tracing::warn!(
                "failed to mirror external action {} into stateful reliability store: {}",
                action.action_id,
                error
            );
        }
        Ok(action)
    }

    async fn stateful_scope_for_external_action(
        &self,
        action: &ExternalActionRecord,
    ) -> crate::stateful_runtime::StatefulRuntimeScope {
        if let Some(tenant_context) = action
            .metadata
            .as_ref()
            .and_then(|metadata| {
                metadata
                    .get("tenant_context")
                    .or_else(|| metadata.get("tenantContext"))
            })
            .cloned()
            .and_then(|value| serde_json::from_value::<TenantContext>(value).ok())
        {
            return crate::stateful_runtime::StatefulRuntimeScope::from_tenant_context(
                tenant_context,
            );
        }

        if let Some(run_id) = external_action_metadata_string(action, "automationRunID")
            .or_else(|| external_action_metadata_string(action, "automation_run_id"))
            .or_else(|| {
                action
                    .context_run_id
                    .as_deref()
                    .and_then(|value| value.strip_prefix("automation-v2-"))
                    .map(str::to_string)
            })
        {
            let runs = self.automation_v2_runs.read().await;
            if let Some(run) = runs.get(&run_id) {
                return crate::stateful_runtime::stateful_run_from_automation_v2(run).scope;
            }
        }

        if let Some(run_id) = external_action_metadata_string(action, "workflowRunID")
            .or_else(|| external_action_metadata_string(action, "workflow_run_id"))
            .or_else(|| {
                action
                    .context_run_id
                    .as_deref()
                    .and_then(|value| value.strip_prefix("workflow-run-"))
                    .map(str::to_string)
            })
        {
            let runs = self.workflow_runs.read().await;
            if let Some(run) = runs.get(&run_id) {
                return crate::stateful_runtime::stateful_run_from_workflow(run).scope;
            }
        }

        crate::stateful_runtime::StatefulRuntimeScope::local_implicit()
    }

    pub async fn update_incident_monitor_runtime_status(
        &self,
        update: impl FnOnce(&mut IncidentMonitorRuntimeStatus),
    ) -> IncidentMonitorRuntimeStatus {
        let mut guard = self.incident_monitor_runtime_status.write().await;
        update(&mut guard);
        guard.clone()
    }

    pub async fn list_incident_monitor_drafts(
        &self,
        limit: usize,
    ) -> Vec<IncidentMonitorDraftRecord> {
        let mut rows = self
            .incident_monitor_drafts
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
        rows.truncate(limit.clamp(1, 200));
        rows
    }

    pub async fn get_incident_monitor_draft(
        &self,
        draft_id: &str,
    ) -> Option<IncidentMonitorDraftRecord> {
        self.incident_monitor_drafts
            .read()
            .await
            .get(draft_id)
            .cloned()
    }

    pub async fn put_incident_monitor_draft(
        &self,
        draft: IncidentMonitorDraftRecord,
    ) -> anyhow::Result<IncidentMonitorDraftRecord> {
        self.incident_monitor_drafts
            .write()
            .await
            .insert(draft.draft_id.clone(), draft.clone());
        self.persist_incident_monitor_drafts().await?;
        Ok(draft)
    }

    pub async fn delete_incident_monitor_drafts(&self, ids: &[String]) -> anyhow::Result<usize> {
        let mut removed = 0usize;
        {
            let mut guard = self.incident_monitor_drafts.write().await;
            for id in ids {
                if guard.remove(id).is_some() {
                    removed += 1;
                }
            }
        }
        if removed > 0 {
            self.persist_incident_monitor_drafts().await?;
        }
        Ok(removed)
    }

    pub async fn clear_incident_monitor_drafts(&self) -> anyhow::Result<usize> {
        let removed = {
            let mut guard = self.incident_monitor_drafts.write().await;
            let count = guard.len();
            guard.clear();
            count
        };
        if removed > 0 {
            self.persist_incident_monitor_drafts().await?;
        }
        Ok(removed)
    }
}

fn external_action_metadata_string(action: &ExternalActionRecord, key: &str) -> Option<String> {
    action
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| match value {
            Value::String(value) => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            Value::Bool(value) => Some(value.to_string()),
            _ => None,
        })
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
