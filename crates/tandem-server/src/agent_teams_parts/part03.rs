impl AgentTeamRuntime {
    async fn instance_id_for_session(&self, session_id: &str) -> Option<String> {
        self.instances
            .read()
            .await
            .values()
            .find(|instance| instance.session_id == session_id)
            .map(|instance| instance.instance_id.clone())
    }

    async fn apply_budget_delta(
        &self,
        state: &AppState,
        instance_id: &str,
        delta_tokens: u64,
        delta_steps: u32,
        delta_tool_calls: u32,
    ) -> bool {
        let policy = self.policy.read().await.clone().unwrap_or(SpawnPolicy {
            enabled: false,
            require_justification: false,
            max_agents: None,
            max_concurrent: None,
            child_budget_percent_of_parent_remaining: None,
            mission_total_budget: None,
            cost_per_1k_tokens_usd: None,
            spawn_edges: HashMap::new(),
            required_skills: HashMap::new(),
            role_defaults: HashMap::new(),
            skill_sources: Default::default(),
        });
        let mut budgets = self.budgets.write().await;
        let Some(usage) = budgets.get_mut(instance_id) else {
            return false;
        };
        if usage.exhausted {
            return true;
        }
        let prev_cost_used_usd = usage.cost_used_usd;
        usage.tokens_used = usage.tokens_used.saturating_add(delta_tokens);
        usage.steps_used = usage.steps_used.saturating_add(delta_steps);
        usage.tool_calls_used = usage.tool_calls_used.saturating_add(delta_tool_calls);
        if let Some(rate) = policy.cost_per_1k_tokens_usd {
            usage.cost_used_usd += (delta_tokens as f64 / 1000.0) * rate;
        }
        let elapsed_ms = usage
            .started_at
            .map(|started| started.elapsed().as_millis() as u64)
            .unwrap_or(0);

        let mut exhausted_reason: Option<&'static str> = None;
        let mut snapshot: Option<AgentInstance> = None;
        {
            let mut instances = self.instances.write().await;
            if let Some(instance) = instances.get_mut(instance_id) {
                instance.metadata = Some(merge_metadata_usage(
                    instance.metadata.take(),
                    usage.tokens_used,
                    usage.steps_used,
                    usage.tool_calls_used,
                    usage.cost_used_usd,
                    elapsed_ms,
                ));
                if let Some(limit) = instance.budget.max_tokens {
                    if usage.tokens_used >= limit {
                        exhausted_reason = Some("max_tokens");
                    }
                }
                if exhausted_reason.is_none() {
                    if let Some(limit) = instance.budget.max_steps {
                        if usage.steps_used >= limit {
                            exhausted_reason = Some("max_steps");
                        }
                    }
                }
                if exhausted_reason.is_none() {
                    if let Some(limit) = instance.budget.max_tool_calls {
                        if usage.tool_calls_used >= limit {
                            exhausted_reason = Some("max_tool_calls");
                        }
                    }
                }
                if exhausted_reason.is_none() {
                    if let Some(limit) = instance.budget.max_duration_ms {
                        if elapsed_ms >= limit {
                            exhausted_reason = Some("max_duration_ms");
                        }
                    }
                }
                if exhausted_reason.is_none() {
                    if let Some(limit) = instance.budget.max_cost_usd {
                        if usage.cost_used_usd >= limit {
                            exhausted_reason = Some("max_cost_usd");
                        }
                    }
                }
                snapshot = Some(instance.clone());
            }
        }
        let Some(instance) = snapshot else {
            return false;
        };
        emit_budget_usage(
            state,
            &instance,
            usage.tokens_used,
            usage.steps_used,
            usage.tool_calls_used,
            usage.cost_used_usd,
            elapsed_ms,
        );
        let mission_exhausted = self
            .apply_mission_budget_delta(
                state,
                &instance.mission_id,
                delta_tokens,
                u64::from(delta_steps),
                u64::from(delta_tool_calls),
                usage.cost_used_usd - prev_cost_used_usd,
                &policy,
            )
            .await;
        if mission_exhausted {
            usage.exhausted = true;
            let _ = self
                .cancel_mission(state, &instance.mission_id, "mission budget exhausted")
                .await;
            return true;
        }
        if let Some(reason) = exhausted_reason {
            usage.exhausted = true;
            emit_budget_exhausted(
                state,
                &instance,
                reason,
                usage.tokens_used,
                usage.steps_used,
                usage.tool_calls_used,
                usage.cost_used_usd,
                elapsed_ms,
            );
            return true;
        }
        false
    }

    async fn apply_exact_token_usage(
        &self,
        state: &AppState,
        instance_id: &str,
        total_tokens: u64,
        cost_used_usd: f64,
    ) -> bool {
        let policy = self.policy.read().await.clone().unwrap_or(SpawnPolicy {
            enabled: false,
            require_justification: false,
            max_agents: None,
            max_concurrent: None,
            child_budget_percent_of_parent_remaining: None,
            mission_total_budget: None,
            cost_per_1k_tokens_usd: None,
            spawn_edges: HashMap::new(),
            required_skills: HashMap::new(),
            role_defaults: HashMap::new(),
            skill_sources: Default::default(),
        });
        let mut budgets = self.budgets.write().await;
        let Some(usage) = budgets.get_mut(instance_id) else {
            return false;
        };
        if usage.exhausted {
            return true;
        }
        let prev_tokens = usage.tokens_used;
        let prev_cost_used_usd = usage.cost_used_usd;
        usage.tokens_used = usage.tokens_used.max(total_tokens);
        if cost_used_usd > 0.0 {
            usage.cost_used_usd = usage.cost_used_usd.max(cost_used_usd);
        } else if let Some(rate) = policy.cost_per_1k_tokens_usd {
            let delta = usage.tokens_used.saturating_sub(prev_tokens);
            usage.cost_used_usd += (delta as f64 / 1000.0) * rate;
        }
        let elapsed_ms = usage
            .started_at
            .map(|started| started.elapsed().as_millis() as u64)
            .unwrap_or(0);
        let mut exhausted_reason: Option<&'static str> = None;
        let mut snapshot: Option<AgentInstance> = None;
        {
            let mut instances = self.instances.write().await;
            if let Some(instance) = instances.get_mut(instance_id) {
                instance.metadata = Some(merge_metadata_usage(
                    instance.metadata.take(),
                    usage.tokens_used,
                    usage.steps_used,
                    usage.tool_calls_used,
                    usage.cost_used_usd,
                    elapsed_ms,
                ));
                if let Some(limit) = instance.budget.max_tokens {
                    if usage.tokens_used >= limit {
                        exhausted_reason = Some("max_tokens");
                    }
                }
                if exhausted_reason.is_none() {
                    if let Some(limit) = instance.budget.max_cost_usd {
                        if usage.cost_used_usd >= limit {
                            exhausted_reason = Some("max_cost_usd");
                        }
                    }
                }
                snapshot = Some(instance.clone());
            }
        }
        let Some(instance) = snapshot else {
            return false;
        };
        emit_budget_usage(
            state,
            &instance,
            usage.tokens_used,
            usage.steps_used,
            usage.tool_calls_used,
            usage.cost_used_usd,
            elapsed_ms,
        );
        let mission_exhausted = self
            .apply_mission_budget_delta(
                state,
                &instance.mission_id,
                usage.tokens_used.saturating_sub(prev_tokens),
                0,
                0,
                usage.cost_used_usd - prev_cost_used_usd,
                &policy,
            )
            .await;
        if mission_exhausted {
            usage.exhausted = true;
            let _ = self
                .cancel_mission(state, &instance.mission_id, "mission budget exhausted")
                .await;
            return true;
        }
        if let Some(reason) = exhausted_reason {
            usage.exhausted = true;
            emit_budget_exhausted(
                state,
                &instance,
                reason,
                usage.tokens_used,
                usage.steps_used,
                usage.tool_calls_used,
                usage.cost_used_usd,
                elapsed_ms,
            );
            return true;
        }
        false
    }

    async fn append_audit(&self, action: &str, instance: &AgentInstance) -> anyhow::Result<()> {
        let path = self.audit_path.read().await.clone();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let row = json!({
            "action": action,
            "missionID": instance.mission_id,
            "instanceID": instance.instance_id,
            "parentInstanceID": instance.parent_instance_id,
            "role": instance.role,
            "templateID": instance.template_id,
            "sessionID": instance.session_id,
            "skillHash": instance.skill_hash,
            "workspaceRoot": instance_workspace_root(instance),
            "workspaceRepoRoot": instance_workspace_repo_root(instance),
            "managedWorktree": instance_managed_worktree(instance),
            "timestampMs": crate::now_ms(),
        });
        let mut existing = if path.exists() {
            fs::read_to_string(&path).await.unwrap_or_default()
        } else {
            String::new()
        };
        existing.push_str(&serde_json::to_string(&row)?);
        existing.push('\n');
        fs::write(path, existing).await?;
        Ok(())
    }

    async fn append_approval_audit(
        &self,
        action: &str,
        approval_id: &str,
        instance: Option<&AgentInstance>,
        reason: &str,
    ) -> anyhow::Result<()> {
        let path = self.audit_path.read().await.clone();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let row = json!({
            "action": action,
            "approvalID": approval_id,
            "reason": reason,
            "missionID": instance.map(|v| v.mission_id.clone()),
            "instanceID": instance.map(|v| v.instance_id.clone()),
            "parentInstanceID": instance.and_then(|v| v.parent_instance_id.clone()),
            "role": instance.map(|v| v.role.clone()),
            "templateID": instance.map(|v| v.template_id.clone()),
            "sessionID": instance.map(|v| v.session_id.clone()),
            "skillHash": instance.map(|v| v.skill_hash.clone()),
            "workspaceRoot": instance.and_then(instance_workspace_root),
            "workspaceRepoRoot": instance.and_then(instance_workspace_repo_root),
            "managedWorktree": instance.and_then(instance_managed_worktree),
            "timestampMs": crate::now_ms(),
        });
        let mut existing = if path.exists() {
            fs::read_to_string(&path).await.unwrap_or_default()
        } else {
            String::new()
        };
        existing.push_str(&serde_json::to_string(&row)?);
        existing.push('\n');
        fs::write(path, existing).await?;
        Ok(())
    }

    async fn apply_mission_budget_delta(
        &self,
        state: &AppState,
        mission_id: &str,
        delta_tokens: u64,
        delta_steps: u64,
        delta_tool_calls: u64,
        delta_cost_used_usd: f64,
        policy: &SpawnPolicy,
    ) -> bool {
        let mut budgets = self.mission_budgets.write().await;
        let row = budgets.entry(mission_id.to_string()).or_default();
        row.tokens_used = row.tokens_used.saturating_add(delta_tokens);
        row.steps_used = row.steps_used.saturating_add(delta_steps);
        row.tool_calls_used = row.tool_calls_used.saturating_add(delta_tool_calls);
        row.cost_used_usd += delta_cost_used_usd.max(0.0);
        if row.exhausted {
            return true;
        }
        let Some(limit) = policy.mission_total_budget.as_ref() else {
            return false;
        };
        let mut exhausted_by: Option<&'static str> = None;
        if let Some(max) = limit.max_tokens {
            if row.tokens_used >= max {
                exhausted_by = Some("mission_max_tokens");
            }
        }
        if exhausted_by.is_none() {
            if let Some(max) = limit.max_steps {
                if row.steps_used >= u64::from(max) {
                    exhausted_by = Some("mission_max_steps");
                }
            }
        }
        if exhausted_by.is_none() {
            if let Some(max) = limit.max_tool_calls {
                if row.tool_calls_used >= u64::from(max) {
                    exhausted_by = Some("mission_max_tool_calls");
                }
            }
        }
        if exhausted_by.is_none() {
            if let Some(max) = limit.max_cost_usd {
                if row.cost_used_usd >= max {
                    exhausted_by = Some("mission_max_cost_usd");
                }
            }
        }
        if let Some(exhausted_by) = exhausted_by {
            row.exhausted = true;
            emit_mission_budget_exhausted(
                state,
                mission_id,
                exhausted_by,
                row.tokens_used,
                row.steps_used,
                row.tool_calls_used,
                row.cost_used_usd,
            );
            return true;
        }
        false
    }

    pub async fn set_for_test(
        &self,
        workspace_root: Option<String>,
        policy: Option<SpawnPolicy>,
        templates: Vec<AgentTemplate>,
    ) {
        *self.policy.write().await = policy;
        let mut by_id = HashMap::new();
        for template in templates {
            by_id.insert(template.template_id.clone(), template);
        }
        *self.templates.write().await = by_id;
        self.instances.write().await.clear();
        self.budgets.write().await.clear();
        self.mission_budgets.write().await.clear();
        self.spawn_approvals.write().await.clear();
        *self.loaded_workspace.write().await = workspace_root;
    }
}

fn resolve_budget(
    policy: &SpawnPolicy,
    parent_instance: Option<AgentInstance>,
    parent_usage: Option<InstanceBudgetState>,
    template: &AgentTemplate,
    override_budget: Option<BudgetLimit>,
    role: &AgentRole,
) -> BudgetLimit {
    let role_default = policy.role_defaults.get(role).cloned().unwrap_or_default();
    let mut chosen = merge_budget(
        merge_budget(role_default, template.default_budget.clone()),
        override_budget.unwrap_or_default(),
    );

    if let Some(parent) = parent_instance {
        let usage = parent_usage.unwrap_or_default();
        if let Some(pct) = policy.child_budget_percent_of_parent_remaining {
            if pct > 0 {
                chosen.max_tokens = cap_budget_remaining_u64(
                    chosen.max_tokens,
                    parent.budget.max_tokens,
                    usage.tokens_used,
                    pct,
                );
                chosen.max_steps = cap_budget_remaining_u32(
                    chosen.max_steps,
                    parent.budget.max_steps,
                    usage.steps_used,
                    pct,
                );
                chosen.max_tool_calls = cap_budget_remaining_u32(
                    chosen.max_tool_calls,
                    parent.budget.max_tool_calls,
                    usage.tool_calls_used,
                    pct,
                );
                chosen.max_duration_ms = cap_budget_remaining_u64(
                    chosen.max_duration_ms,
                    parent.budget.max_duration_ms,
                    usage
                        .started_at
                        .map(|started| started.elapsed().as_millis() as u64)
                        .unwrap_or(0),
                    pct,
                );
                chosen.max_cost_usd = cap_budget_remaining_f64(
                    chosen.max_cost_usd,
                    parent.budget.max_cost_usd,
                    usage.cost_used_usd,
                    pct,
                );
            }
        }
    }
    chosen
}

fn merge_budget(base: BudgetLimit, overlay: BudgetLimit) -> BudgetLimit {
    BudgetLimit {
        max_tokens: overlay.max_tokens.or(base.max_tokens),
        max_steps: overlay.max_steps.or(base.max_steps),
        max_tool_calls: overlay.max_tool_calls.or(base.max_tool_calls),
        max_duration_ms: overlay.max_duration_ms.or(base.max_duration_ms),
        max_cost_usd: overlay.max_cost_usd.or(base.max_cost_usd),
    }
}

fn cap_budget_remaining_u64(
    child: Option<u64>,
    parent_limit: Option<u64>,
    parent_used: u64,
    pct: u8,
) -> Option<u64> {
    match (child, parent_limit) {
        (Some(child), Some(parent_limit)) => {
            let remaining = parent_limit.saturating_sub(parent_used);
            Some(child.min(remaining.saturating_mul(pct as u64) / 100))
        }
        (None, Some(parent_limit)) => {
            let remaining = parent_limit.saturating_sub(parent_used);
            Some(remaining.saturating_mul(pct as u64) / 100)
        }
        (Some(child), None) => Some(child),
        (None, None) => None,
    }
}

fn cap_budget_remaining_u32(
    child: Option<u32>,
    parent_limit: Option<u32>,
    parent_used: u32,
    pct: u8,
) -> Option<u32> {
    match (child, parent_limit) {
        (Some(child), Some(parent_limit)) => {
            let remaining = parent_limit.saturating_sub(parent_used);
            Some(child.min(remaining.saturating_mul(pct as u32) / 100))
        }
        (None, Some(parent_limit)) => {
            let remaining = parent_limit.saturating_sub(parent_used);
            Some(remaining.saturating_mul(pct as u32) / 100)
        }
        (Some(child), None) => Some(child),
        (None, None) => None,
    }
}

fn cap_budget_remaining_f64(
    child: Option<f64>,
    parent_limit: Option<f64>,
    parent_used: f64,
    pct: u8,
) -> Option<f64> {
    match (child, parent_limit) {
        (Some(child), Some(parent_limit)) => {
            let remaining = (parent_limit - parent_used).max(0.0);
            Some(child.min(remaining * f64::from(pct) / 100.0))
        }
        (None, Some(parent_limit)) => {
            let remaining = (parent_limit - parent_used).max(0.0);
            Some(remaining * f64::from(pct) / 100.0)
        }
        (Some(child), None) => Some(child),
        (None, None) => None,
    }
}

async fn compute_skill_hash(
    workspace_root: &str,
    template: &AgentTemplate,
    policy: &SpawnPolicy,
) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut rows = Vec::new();
    let skill_service = SkillService::for_workspace(Some(PathBuf::from(workspace_root)));
    for skill in &template.skills {
        if let Some(path) = skill.path.as_deref() {
            validate_skill_source(skill.id.as_deref(), Some(path), policy)?;
            let skill_path = Path::new(workspace_root).join(path);
            let raw = fs::read_to_string(&skill_path)
                .await
                .map_err(|_| format!("missing required skill path `{}`", skill_path.display()))?;
            let digest = hash_hex(raw.as_bytes());
            validate_pinned_hash(skill.id.as_deref(), Some(path), &digest, policy)?;
            rows.push(format!("path:{}:{}", path, digest));
        } else if let Some(id) = skill.id.as_deref() {
            validate_skill_source(Some(id), None, policy)?;
            let loaded = skill_service
                .load_skill(id)
                .map_err(|err| format!("failed loading skill `{id}`: {err}"))?;
            let Some(loaded) = loaded else {
                return Err(format!("missing required skill id `{id}`"));
            };
            let digest = hash_hex(loaded.content.as_bytes());
            validate_pinned_hash(Some(id), None, &digest, policy)?;
            rows.push(format!("id:{}:{}", id, digest));
        }
    }
    rows.sort();
    let mut hasher = Sha256::new();
    for row in rows {
        hasher.update(row.as_bytes());
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    Ok(format!("sha256:{}", hash_hex(digest.as_slice())))
}

fn validate_skill_source(
    id: Option<&str>,
    path: Option<&str>,
    policy: &SpawnPolicy,
) -> Result<(), String> {
    use tandem_orchestrator::SkillSourceMode;
    match policy.skill_sources.mode {
        SkillSourceMode::Any => Ok(()),
        SkillSourceMode::ProjectOnly => {
            if id.is_some() {
                return Err("skill source denied: project_only forbids skill IDs".to_string());
            }
            let Some(path) = path else {
                return Err("skill source denied: project_only requires skill path".to_string());
            };
            let p = PathBuf::from(path);
            if p.is_absolute() {
                return Err("skill source denied: absolute skill paths are forbidden".to_string());
            }
            Ok(())
        }
        SkillSourceMode::Allowlist => {
            if let Some(id) = id {
                if policy.skill_sources.allowlist_ids.iter().any(|v| v == id) {
                    return Ok(());
                }
            }
            if let Some(path) = path {
                if policy
                    .skill_sources
                    .allowlist_paths
                    .iter()
                    .any(|v| v == path)
                {
                    return Ok(());
                }
            }
            Err("skill source denied: not present in allowlist".to_string())
        }
    }
}

fn validate_pinned_hash(
    id: Option<&str>,
    path: Option<&str>,
    digest: &str,
    policy: &SpawnPolicy,
) -> Result<(), String> {
    let by_id = id.and_then(|id| policy.skill_sources.pinned_hashes.get(&format!("id:{id}")));
    let by_path = path.and_then(|path| {
        policy
            .skill_sources
            .pinned_hashes
            .get(&format!("path:{path}"))
    });
    let expected = by_id.or(by_path);
    if let Some(expected) = expected {
        let normalized = expected.strip_prefix("sha256:").unwrap_or(expected);
        if normalized != digest {
            return Err("pinned hash mismatch for skill reference".to_string());
        }
    }
    Ok(())
}

fn hash_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{:02x}", byte);
    }
    out
}

fn estimate_tokens(text: &str) -> u64 {
    let chars = text.chars().count() as u64;
    (chars / 4).max(1)
}

fn extract_session_id(event: &EngineEvent) -> Option<String> {
    event
        .properties
        .get("sessionID")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
        .or_else(|| {
            event
                .properties
                .get("part")
                .and_then(|v| v.get("sessionID"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string())
        })
}

#[cfg(test)]
mod fintech_policy_tests {
    use super::*;
    use serde_json::json;
    use tandem_types::{AuthorityChain, HumanActor, RequestPrincipal, TenantContext};

    fn fintech_test_automation(metadata: Value) -> crate::AutomationV2Spec {
        crate::AutomationV2Spec {
            automation_id: "automation-fintech".to_string(),
            name: "Fintech Compliance Brief".to_string(),
            description: None,
            status: crate::AutomationV2Status::Active,
            schedule: crate::AutomationV2Schedule {
                schedule_type: crate::AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
            },
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            agents: Vec::new(),
            flow: crate::AutomationFlowSpec { nodes: Vec::new() },
            execution: crate::AutomationExecutionPolicy {
                profile: Some(crate::automation_v2::execution_profile::ExecutionProfile::Strict),
                max_parallel_agents: Some(1),
                max_total_runtime_ms: None,
                max_total_tool_calls: None,
                max_total_tokens: None,
                max_total_cost_usd: None,
            },
            output_targets: Vec::new(),
            created_at_ms: 1,
            updated_at_ms: 1,
            creator_id: "test".to_string(),
            workspace_root: Some(".".to_string()),
            metadata: Some(metadata),
            next_fire_at_ms: None,
            last_fired_at_ms: None,
            scope_policy: None,
            watch_conditions: Vec::new(),
            handoff_config: None,
        }
    }

    fn fintech_test_run(automation: crate::AutomationV2Spec) -> crate::AutomationV2RunRecord {
        crate::AutomationV2RunRecord {
            run_id: "automation-v2-run-fintech".to_string(),
            automation_id: automation.automation_id.clone(),
            tenant_context: TenantContext::local_implicit(),
            trigger_type: "manual".to_string(),
            status: crate::AutomationRunStatus::Running,
            created_at_ms: 1,
            updated_at_ms: 1,
            started_at_ms: Some(1),
            finished_at_ms: None,
            active_session_ids: vec!["session-fintech".to_string()],
            latest_session_id: Some("session-fintech".to_string()),
            active_instance_ids: Vec::new(),
            checkpoint: crate::AutomationRunCheckpoint {
                completed_nodes: Vec::new(),
                pending_nodes: Vec::new(),
                node_outputs: HashMap::new(),
                node_attempts: HashMap::new(),
                node_attempt_verdicts: HashMap::new(),
                blocked_nodes: Vec::new(),
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: Vec::new(),
                last_failure: None,
            },
            runtime_context: None,
            automation_snapshot: Some(automation),
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
            effective_execution_profile:
                crate::automation_v2::execution_profile::ExecutionProfile::Strict,
            requested_execution_profile: None,
        }
    }

    async fn fintech_policy_state(metadata: Value) -> AppState {
        let mut state = AppState::new_starting("test".to_string(), true);
        state.policy_decisions_path = std::env::temp_dir().join(format!(
            "tandem-policy-decisions-{}.json",
            Uuid::new_v4()
        ));
        let automation = fintech_test_automation(metadata);
        let run = fintech_test_run(automation);
        state
            .automation_v2_runs
            .write()
            .await
            .insert(run.run_id.clone(), run);
        state.automation_v2_session_runs.write().await.insert(
            "session-fintech".to_string(),
            "automation-v2-run-fintech".to_string(),
        );
        state
    }

    async fn add_fintech_approval_receipt(
        state: &AppState,
        tool: &str,
        args: &Value,
        category: &str,
        tenant_override: Option<Value>,
        expires_at_ms: Option<u64>,
    ) {
        let mut runs = state.automation_v2_runs.write().await;
        let run = runs
            .get_mut("automation-v2-run-fintech")
            .expect("fintech run");
        let tenant = tenant_override.unwrap_or_else(|| {
            json!({
                "org_id": run.tenant_context.org_id,
                "workspace_id": run.tenant_context.workspace_id,
            })
        });
        run.checkpoint
            .gate_history
            .push(crate::AutomationGateDecisionRecord {
                node_id: "approve_protected_action".to_string(),
                decision: "approve".to_string(),
                reason: Some("approved for test".to_string()),
                decided_at_ms: crate::now_ms(),
                decided_by: None,
                metadata: Some(json!({
                    "fintech_protected_action": {
                        "category": category,
                        "tool": tool,
                        "action_hash": tandem_core::fintech_protected_action_hash(tool, args),
                        "tenant": tenant,
                        "expires_at_ms": expires_at_ms.unwrap_or_else(|| crate::now_ms() + 60_000),
                    }
                })),
            });
    }

    fn verified_context_for_tenant(
        tenant_context: TenantContext,
        issued_at_ms: u64,
        expires_at_ms: u64,
    ) -> VerifiedTenantContext {
        let actor_id = tenant_context
            .actor_id
            .clone()
            .unwrap_or_else(|| "actor-1".to_string());
        VerifiedTenantContext {
            tenant_context,
            human_actor: HumanActor::tandem_user(actor_id.clone()),
            authority_chain: AuthorityChain::from_request(RequestPrincipal::authenticated_user(
                actor_id,
                "tandem-web",
            )),
            roles: Vec::new(),
            org_units: Vec::new(),
            capabilities: Vec::new(),
            policy_version: None,
            strict_projection: None,
            issuer: "tandem-web".to_string(),
            audience: "tandem-runtime".to_string(),
            issued_at_ms,
            expires_at_ms,
            assertion_id: "assertion-test".to_string(),
            assertion_key_id: None,
        }
    }

    #[tokio::test]
    async fn fintech_strict_blocks_protected_action_tools() {
        let state = fintech_policy_state(json!({"runtime_profile": "fintech_strict"})).await;
        let hook = ServerToolPolicyHook::new(state.clone());
        let decision = hook
            .evaluate_tool(ToolPolicyContext {
                session_id: "session-fintech".to_string(),
                message_id: "message-1".to_string(),
                tenant_context: None,
                verified_tenant_context: None,
                tool: "mcp.bank.release_funds".to_string(),
                args: json!({}),
            })
            .await
            .expect("policy decision");
        assert!(!decision.allowed);
        assert!(decision
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("money_movement"));
        assert!(decision
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("fail-closed"));
        assert!(decision
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("approval gates are not treated as authorization"));
        let decision_id = decision
            .policy_decision_id
            .as_deref()
            .expect("policy decision id");
        let stored = state
            .get_policy_decision(decision_id)
            .await
            .expect("stored policy decision");
        assert_eq!(stored.decision, PolicyDecisionEffect::ApprovalRequired);
        assert_eq!(stored.reason_code, "approval_required_unverified");
        assert_eq!(stored.tool.as_deref(), Some("mcp.bank.release_funds"));
        assert_eq!(
            stored.risk_tier.as_deref(),
            Some("money_movement_contract")
        );
    }

    #[tokio::test]
    async fn fintech_strict_allows_matching_approved_protected_action_receipt() {
        let state = fintech_policy_state(json!({"runtime_profile": "fintech_strict"})).await;
        let args = json!({"account_id": "acct-1", "amount": 10});
        add_fintech_approval_receipt(
            &state,
            "mcp.bank.release_funds",
            &args,
            "money_movement",
            None,
            None,
        )
        .await;
        let hook = ServerToolPolicyHook::new(state.clone());

        let decision = hook
            .evaluate_tool(ToolPolicyContext {
                session_id: "session-fintech".to_string(),
                message_id: "message-1".to_string(),
                tenant_context: None,
                verified_tenant_context: None,
                tool: "mcp.bank.release_funds".to_string(),
                args,
            })
            .await
            .expect("policy decision");

        assert!(decision.allowed, "{:?}", decision.reason);
        let decision_id = decision
            .policy_decision_id
            .as_deref()
            .expect("policy decision id");
        let stored = state
            .get_policy_decision(decision_id)
            .await
            .expect("stored policy decision");
        assert_eq!(stored.decision, PolicyDecisionEffect::Allow);
        assert_eq!(stored.reason_code, "matching_approval_receipt");
        assert_eq!(stored.approval_id.as_deref(), Some("approve_protected_action"));
        assert_eq!(
            stored.risk_tier.as_deref(),
            Some("money_movement_contract")
        );
    }

    #[tokio::test]
    async fn fintech_strict_rejects_session_tenant_mismatch() {
        let state = fintech_policy_state(json!({"runtime_profile": "fintech_strict"})).await;
        let hook = ServerToolPolicyHook::new(state);

        let decision = hook
            .evaluate_tool(ToolPolicyContext {
                session_id: "session-fintech".to_string(),
                message_id: "message-1".to_string(),
                tenant_context: Some(TenantContext::explicit(
                    "other-org",
                    "local-workspace",
                    Some("actor-1".to_string()),
                )),
                verified_tenant_context: None,
                tool: "mcp.bank.release_funds".to_string(),
                args: json!({"account_id": "acct-1", "amount": 10}),
            })
            .await
            .expect("policy decision");

        assert!(!decision.allowed);
        assert!(decision
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("does not match automation run tenant"));
    }

    #[tokio::test]
    async fn fintech_strict_enterprise_mode_rejects_missing_tenant_context_for_protected_tools() {
        let state = fintech_policy_state(json!({"runtime_profile": "fintech_strict"})).await;

        let decision = evaluate_fintech_strict_tool_policy(
            &state,
            "session-fintech",
            "message-1",
            None,
            None,
            RuntimeAuthMode::EnterpriseRequired,
            "mcp.bank.release_funds",
            &json!({"account_id": "acct-1", "amount": 10}),
        )
        .await
        .expect("strict policy should decide");

        assert!(!decision.allowed);
        assert!(decision
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("requires verified tenant context"));
    }

    #[tokio::test]
    async fn fintech_strict_enterprise_mode_rejects_local_implicit_context_for_protected_tools() {
        let state = fintech_policy_state(json!({"runtime_profile": "fintech_strict"})).await;

        let local_context = TenantContext::local_implicit();
        let decision = evaluate_fintech_strict_tool_policy(
            &state,
            "session-fintech",
            "message-1",
            Some(&local_context),
            None,
            RuntimeAuthMode::EnterpriseRequired,
            "mcp.bank.release_funds",
            &json!({"account_id": "acct-1", "amount": 10}),
        )
        .await
        .expect("strict policy should decide");

        assert!(!decision.allowed);
        assert!(decision
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("rejects local implicit tenant context"));
    }

    #[tokio::test]
    async fn fintech_strict_enterprise_mode_rejects_expired_verified_context_for_protected_tools() {
        let state = fintech_policy_state(json!({"runtime_profile": "fintech_strict"})).await;
        let tenant_context = TenantContext::explicit("local", "local", Some("actor-1".to_string()));
        let verified = verified_context_for_tenant(
            tenant_context.clone(),
            1,
            crate::now_ms().saturating_sub(1),
        );

        let decision = evaluate_fintech_strict_tool_policy(
            &state,
            "session-fintech",
            "message-1",
            Some(&tenant_context),
            Some(&verified),
            RuntimeAuthMode::EnterpriseRequired,
            "mcp.bank.release_funds",
            &json!({"account_id": "acct-1", "amount": 10}),
        )
        .await
        .expect("strict policy should decide");

        assert!(!decision.allowed);
        assert!(decision
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("expired tenant context assertions"));
    }

    #[tokio::test]
    async fn fintech_strict_rejects_mismatched_approval_receipts() {
        let state = fintech_policy_state(json!({"runtime_profile": "fintech_strict"})).await;
        add_fintech_approval_receipt(
            &state,
            "mcp.bank.release_funds",
            &json!({"account_id": "acct-1", "amount": 10}),
            "money_movement",
            None,
            None,
        )
        .await;
        let hook = ServerToolPolicyHook::new(state);

        let decision = hook
            .evaluate_tool(ToolPolicyContext {
                session_id: "session-fintech".to_string(),
                message_id: "message-1".to_string(),
                tenant_context: None,
                verified_tenant_context: None,
                tool: "mcp.bank.release_funds".to_string(),
                args: json!({"account_id": "acct-1", "amount": 11}),
            })
            .await
            .expect("policy decision");

        assert!(!decision.allowed);
        assert!(decision
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("fail-closed"));
    }

    #[tokio::test]
    async fn fintech_strict_rejects_cross_tenant_or_expired_approval_receipts() {
        let state = fintech_policy_state(json!({"runtime_profile": "fintech_strict"})).await;
        let args = json!({"account_id": "acct-1", "amount": 10});
        add_fintech_approval_receipt(
            &state,
            "mcp.bank.release_funds",
            &args,
            "money_movement",
            Some(json!({"org_id": "other-org", "workspace_id": "local-workspace"})),
            None,
        )
        .await;
        add_fintech_approval_receipt(
            &state,
            "mcp.bank.release_funds",
            &args,
            "money_movement",
            None,
            Some(crate::now_ms().saturating_sub(1)),
        )
        .await;
        let hook = ServerToolPolicyHook::new(state);

        let decision = hook
            .evaluate_tool(ToolPolicyContext {
                session_id: "session-fintech".to_string(),
                message_id: "message-1".to_string(),
                tenant_context: None,
                verified_tenant_context: None,
                tool: "mcp.bank.release_funds".to_string(),
                args,
            })
            .await
            .expect("policy decision");

        assert!(!decision.allowed);
    }

    #[tokio::test]
    async fn fintech_strict_allows_research_tools() {
        let state = fintech_policy_state(json!({"fintech": {"strict": true}})).await;
        let hook = ServerToolPolicyHook::new(state);
        let decision = hook
            .evaluate_tool(ToolPolicyContext {
                session_id: "session-fintech".to_string(),
                message_id: "message-1".to_string(),
                tenant_context: None,
                verified_tenant_context: None,
                tool: "mcp.regulator.fetch_bulletin".to_string(),
                args: json!({"url": "https://regulator.example/rule-1"}),
            })
            .await
            .expect("policy decision");
        assert!(decision.allowed, "{:?}", decision.reason);
    }

    #[tokio::test]
    async fn non_fintech_automation_is_not_gated_by_fintech_policy() {
        let state = fintech_policy_state(json!({"runtime_profile": "default"})).await;
        let hook = ServerToolPolicyHook::new(state);
        let decision = hook
            .evaluate_tool(ToolPolicyContext {
                session_id: "session-fintech".to_string(),
                message_id: "message-1".to_string(),
                tenant_context: None,
                verified_tenant_context: None,
                tool: "mcp.bank.release_funds".to_string(),
                args: json!({}),
            })
            .await
            .expect("policy decision");
        assert!(decision.allowed, "{:?}", decision.reason);
    }

    fn phase_tool_test_automation() -> crate::AutomationV2Spec {
        let node = crate::AutomationFlowNode {
            node_id: "phase-research".to_string(),
            agent_id: "phase-agent".to_string(),
            objective: "Collect approved evidence".to_string(),
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            tool_policy: None,
            mcp_policy: None,
            retry_policy: None,
            timeout_ms: None,
            max_tool_calls: None,
            stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
            gate: None,
            metadata: Some(json!({ "phase": "research" })),
        };
        crate::AutomationV2Spec {
            automation_id: "automation-phase-tools".to_string(),
            name: "Phase Tool Authority".to_string(),
            description: None,
            status: crate::AutomationV2Status::Active,
            schedule: crate::AutomationV2Schedule {
                schedule_type: crate::AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
            },
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            agents: Vec::new(),
            flow: crate::AutomationFlowSpec { nodes: vec![node] },
            execution: crate::AutomationExecutionPolicy {
                profile: Some(crate::automation_v2::execution_profile::ExecutionProfile::Strict),
                max_parallel_agents: Some(1),
                max_total_runtime_ms: None,
                max_total_tool_calls: None,
                max_total_tokens: None,
                max_total_cost_usd: None,
            },
            output_targets: Vec::new(),
            created_at_ms: 1,
            updated_at_ms: 1,
            creator_id: "test".to_string(),
            workspace_root: Some(".".to_string()),
            metadata: None,
            next_fire_at_ms: None,
            last_fired_at_ms: None,
            scope_policy: None,
            watch_conditions: Vec::new(),
            handoff_config: None,
        }
    }

    fn phase_tool_test_run(automation: crate::AutomationV2Spec) -> crate::AutomationV2RunRecord {
        let tenant = TenantContext::explicit(
            "acme-org",
            "acme-workspace",
            Some("phase-actor".to_string()),
        );
        crate::AutomationV2RunRecord {
            run_id: "automation-v2-run-phase-tools".to_string(),
            automation_id: automation.automation_id.clone(),
            tenant_context: tenant,
            trigger_type: "manual".to_string(),
            status: crate::AutomationRunStatus::Running,
            created_at_ms: 1,
            updated_at_ms: 1,
            started_at_ms: Some(1),
            finished_at_ms: None,
            active_session_ids: vec!["session-phase-tools".to_string()],
            latest_session_id: Some("session-phase-tools".to_string()),
            active_instance_ids: Vec::new(),
            checkpoint: crate::AutomationRunCheckpoint {
                completed_nodes: Vec::new(),
                pending_nodes: vec!["phase-research".to_string()],
                node_outputs: HashMap::new(),
                node_attempts: HashMap::new(),
                node_attempt_verdicts: HashMap::new(),
                blocked_nodes: Vec::new(),
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: Vec::new(),
                last_failure: None,
            },
            runtime_context: None,
            automation_snapshot: Some(automation),
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
            effective_execution_profile:
                crate::automation_v2::execution_profile::ExecutionProfile::Strict,
            requested_execution_profile: None,
        }
    }

    async fn phase_tool_policy_state() -> AppState {
        let state = crate::test_support::test_state().await;
        let automation = phase_tool_test_automation();
        let run = phase_tool_test_run(automation);
        state
            .automation_v2_runs
            .write()
            .await
            .insert(run.run_id.clone(), run);
        state
            .add_automation_v2_node_session(
                "automation-v2-run-phase-tools",
                "phase-research",
                "session-phase-tools",
            )
            .await;
        state
    }

    #[tokio::test]
    async fn phase_tool_policy_denies_forbidden_tool_and_records_evidence() {
        let state = phase_tool_policy_state().await;
        state
            .engine_loop
            .set_session_allowed_tools("session-phase-tools", vec!["read".to_string()])
            .await;
        let hook = ServerToolPolicyHook::new(state.clone());

        let decision = hook
            .evaluate_tool(ToolPolicyContext {
                session_id: "session-phase-tools".to_string(),
                message_id: "message-phase-tools".to_string(),
                tenant_context: Some(TenantContext::explicit(
                    "acme-org",
                    "acme-workspace",
                    Some("phase-actor".to_string()),
                )),
                verified_tenant_context: None,
                tool: "write".to_string(),
                args: json!({ "path": "out.md" }),
            })
            .await
            .expect("phase tool policy decision");

        assert!(!decision.allowed);
        assert!(decision
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("workflow phase `research`"));
        let decision_id = decision
            .policy_decision_id
            .as_deref()
            .expect("policy decision id");
        let stored = state
            .get_policy_decision(decision_id)
            .await
            .expect("stored policy decision");
        assert_eq!(stored.policy_id.as_deref(), Some("workflow_phase_tool_authority"));
        assert_eq!(stored.reason_code, "phase_tool_not_allowed");
        assert_eq!(stored.decision, PolicyDecisionEffect::Deny);
        assert_eq!(stored.run_id.as_deref(), Some("automation-v2-run-phase-tools"));
        assert_eq!(stored.node_id.as_deref(), Some("phase-research"));
        assert_eq!(stored.metadata["phase_tool_authority"]["phase"], "research");

        let audit = tokio::fs::read_to_string(&state.protected_audit_path)
            .await
            .expect("protected audit file");
        assert!(audit.contains("automation.phase_tool.denied"));
        assert!(audit.contains("phase_tool_not_allowed"));
        assert!(audit.contains("phase-research"));
    }

    #[tokio::test]
    async fn phase_tool_policy_allows_tool_in_phase_allowlist() {
        let state = phase_tool_policy_state().await;
        state
            .engine_loop
            .set_session_allowed_tools(
                "session-phase-tools",
                vec!["read".to_string(), "write".to_string()],
            )
            .await;
        let hook = ServerToolPolicyHook::new(state);

        let decision = hook
            .evaluate_tool(ToolPolicyContext {
                session_id: "session-phase-tools".to_string(),
                message_id: "message-phase-tools".to_string(),
                tenant_context: Some(TenantContext::explicit(
                    "acme-org",
                    "acme-workspace",
                    Some("phase-actor".to_string()),
                )),
                verified_tenant_context: None,
                tool: "write".to_string(),
                args: json!({ "path": "out.md" }),
            })
            .await
            .expect("phase tool policy decision");

        assert!(decision.allowed, "{:?}", decision.reason);
    }

    #[tokio::test]
    async fn session_allowlist_denial_bypasses_hook_for_non_automation_scope() {
        let state = crate::test_support::test_state().await;
        state
            .engine_loop
            .set_session_allowed_tools("session-generic-allowlist", vec!["read".to_string()])
            .await;
        let hook = ServerToolPolicyHook::new(state.clone());

        let decision = hook
            .evaluate_tool(ToolPolicyContext {
                session_id: "session-generic-allowlist".to_string(),
                message_id: "message-generic-allowlist".to_string(),
                tenant_context: None,
                verified_tenant_context: None,
                tool: "write".to_string(),
                args: json!({ "path": "out.md" }),
            })
            .await
            .expect("policy decision");

        assert!(decision.allowed, "core allowlist should handle the denial");
        assert!(decision.reason.is_none());
        assert!(decision.policy_decision_id.is_none());
        assert!(state.policy_decisions.read().await.is_empty());
    }
}
