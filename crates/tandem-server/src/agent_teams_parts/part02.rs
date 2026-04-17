
fn merge_metadata_usage(
    metadata: Option<Value>,
    tokens_used: u64,
    steps_used: u32,
    tool_calls_used: u32,
    cost_used_usd: f64,
    elapsed_ms: u64,
) -> Value {
    let mut base = metadata
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    base.insert(
        "budgetUsage".to_string(),
        json!({
            "tokensUsed": tokens_used,
            "stepsUsed": steps_used,
            "toolCallsUsed": tool_calls_used,
            "costUsedUsd": cost_used_usd,
            "elapsedMs": elapsed_ms
        }),
    );
    Value::Object(base)
}

fn instance_workspace_root(instance: &AgentInstance) -> Option<Value> {
    instance
        .metadata
        .as_ref()
        .and_then(|row| row.get("workspaceRoot"))
        .cloned()
}

fn instance_workspace_repo_root(instance: &AgentInstance) -> Option<Value> {
    instance
        .metadata
        .as_ref()
        .and_then(|row| row.get("workspaceRepoRoot"))
        .cloned()
}

fn instance_managed_worktree(instance: &AgentInstance) -> Option<Value> {
    instance
        .metadata
        .as_ref()
        .and_then(|row| row.get("managedWorktree"))
        .cloned()
}

async fn prepare_agent_instance_workspace(
    state: &AppState,
    workspace_root: &str,
    mission_id: Option<&str>,
    instance_id: &str,
    template_id: &str,
) -> Option<crate::runtime::worktrees::ManagedWorktreeEnsureResult> {
    let repo_root = crate::runtime::worktrees::resolve_git_repo_root(workspace_root)?;
    crate::runtime::worktrees::ensure_managed_worktree(
        state,
        crate::runtime::worktrees::ManagedWorktreeEnsureInput {
            repo_root,
            task_id: mission_id.map(ToString::to_string),
            owner_run_id: Some(instance_id.to_string()),
            lease_id: None,
            branch_hint: Some(template_id.to_string()),
            base: "HEAD".to_string(),
            cleanup_branch: true,
        },
    )
    .await
    .ok()
}

async fn cleanup_instance_managed_worktree(state: &AppState, instance: &AgentInstance) {
    let Some(metadata) = instance.metadata.as_ref() else {
        return;
    };
    let Some(worktree) = metadata.get("managedWorktree").and_then(Value::as_object) else {
        return;
    };
    let Some(path) = worktree.get("path").and_then(Value::as_str) else {
        return;
    };
    let Some(branch) = worktree.get("branch").and_then(Value::as_str) else {
        return;
    };
    let Some(repo_root) = worktree.get("repoRoot").and_then(Value::as_str) else {
        return;
    };
    let record = crate::ManagedWorktreeRecord {
        key: crate::runtime::worktrees::managed_worktree_key(
            repo_root,
            instance.mission_id.as_str().into(),
            Some(instance.instance_id.as_str()),
            None,
            path,
            branch,
        ),
        repo_root: repo_root.to_string(),
        path: path.to_string(),
        branch: branch.to_string(),
        base: "HEAD".to_string(),
        managed: true,
        task_id: Some(instance.mission_id.clone()),
        owner_run_id: Some(instance.instance_id.clone()),
        lease_id: None,
        cleanup_branch: worktree
            .get("cleanupBranch")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    let _ = crate::runtime::worktrees::delete_managed_worktree(state, &record).await;
}

fn normalize_tool_name(name: &str) -> String {
    match name.trim().to_lowercase().replace('-', "_").as_str() {
        "todowrite" | "update_todo_list" | "update_todos" => "todo_write".to_string(),
        other => other.to_string(),
    }
}

async fn evaluate_capability_deny(
    state: &AppState,
    instance: &AgentInstance,
    tool: &str,
    args: &Value,
    caps: &tandem_orchestrator::CapabilitySpec,
    session_id: &str,
    message_id: &str,
) -> Option<String> {
    let deny_patterns = caps
        .tool_denylist
        .iter()
        .map(|name| normalize_tool_name(name))
        .collect::<Vec<_>>();
    if !deny_patterns.is_empty() && any_policy_matches(&deny_patterns, tool) {
        return Some(format!("tool `{tool}` denied by agent capability policy"));
    }
    let allow_patterns = caps
        .tool_allowlist
        .iter()
        .map(|name| normalize_tool_name(name))
        .collect::<Vec<_>>();
    if !allow_patterns.is_empty() && !any_policy_matches(&allow_patterns, tool) {
        return Some(format!("tool `{tool}` not in agent allowlist"));
    }

    let browser_execution_tool = matches!(
        tool,
        "browser_open"
            | "browser_navigate"
            | "browser_snapshot"
            | "browser_click"
            | "browser_type"
            | "browser_press"
            | "browser_wait"
            | "browser_extract"
            | "browser_screenshot"
            | "browser_close"
    );

    if matches!(
        tool,
        "websearch" | "webfetch" | "webfetch_html" | "browser_open" | "browser_navigate"
    ) || browser_execution_tool
    {
        if !caps.net_scopes.enabled {
            return Some("network disabled for this agent instance".to_string());
        }
        if !caps.net_scopes.allow_hosts.is_empty() {
            if tool == "websearch" {
                return Some(
                    "websearch blocked: host allowlist cannot be verified for search tool"
                        .to_string(),
                );
            }
            if let Some(host) = extract_url_host(args) {
                let allowed = caps.net_scopes.allow_hosts.iter().any(|h| {
                    let allowed = h.trim().to_ascii_lowercase();
                    !allowed.is_empty()
                        && (host == allowed || host.ends_with(&format!(".{allowed}")))
                });
                if !allowed {
                    return Some(format!("network host `{host}` not in allow_hosts"));
                }
            }
        }
    }

    if tool == "bash" {
        let cmd = args
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if cmd.contains("git push") {
            if !caps.git_caps.push {
                return Some("git push disabled for this agent instance".to_string());
            }
            if caps.git_caps.push_requires_approval {
                let action = state.permissions.evaluate("git_push", "git_push").await;
                match action {
                    tandem_core::PermissionAction::Allow => {}
                    tandem_core::PermissionAction::Deny => {
                        return Some("git push denied by policy rule".to_string());
                    }
                    tandem_core::PermissionAction::Ask => {
                        let pending = state
                            .permissions
                            .ask_for_session_with_context(
                                Some(session_id),
                                "git_push",
                                args.clone(),
                                Some(tandem_core::PermissionArgsContext {
                                    args_source: "agent_team.git_push".to_string(),
                                    args_integrity: "runtime-checked".to_string(),
                                    query: Some(format!(
                                        "instanceID={} messageID={}",
                                        instance.instance_id, message_id
                                    )),
                                }),
                            )
                            .await;
                        return Some(format!(
                            "git push requires explicit user approval (approvalID={})",
                            pending.id
                        ));
                    }
                }
            }
        }
        if cmd.contains("git commit") && !caps.git_caps.commit {
            return Some("git commit disabled for this agent instance".to_string());
        }
    }

    let access_kind = tool_fs_access_kind(tool);
    if let Some(kind) = access_kind {
        let Some(session) = state.storage.get_session(session_id).await else {
            return Some("session not found for capability evaluation".to_string());
        };
        let Some(root) = session.workspace_root.clone() else {
            return Some("workspace root missing for capability evaluation".to_string());
        };
        let requested = extract_tool_candidate_paths(tool, args);
        if !requested.is_empty() {
            let allowed_scopes = if kind == "read" {
                &caps.fs_scopes.read
            } else {
                &caps.fs_scopes.write
            };
            if allowed_scopes.is_empty() {
                return Some(format!("fs {kind} access blocked: no scopes configured"));
            }
            for candidate in requested {
                if !is_path_allowed_by_scopes(&root, &candidate, allowed_scopes) {
                    return Some(format!("fs {kind} access denied for path `{}`", candidate));
                }
            }
        }
    }

    denied_secrets_reason(tool, caps, args)
}

fn denied_secrets_reason(
    tool: &str,
    caps: &tandem_orchestrator::CapabilitySpec,
    args: &Value,
) -> Option<String> {
    if tool == "auth" {
        if caps.secrets_scopes.is_empty() {
            return Some("secrets are disabled for this agent instance".to_string());
        }
        let alias = args
            .get("id")
            .or_else(|| args.get("provider"))
            .or_else(|| args.get("providerID"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if !alias.is_empty() && !caps.secrets_scopes.iter().any(|allowed| allowed == alias) {
            return Some(format!(
                "secret alias `{alias}` is not in agent secretsScopes allowlist"
            ));
        }
    }
    None
}

fn tool_fs_access_kind(tool: &str) -> Option<&'static str> {
    match tool {
        "read" | "glob" | "grep" | "codesearch" | "lsp" => Some("read"),
        "write" | "edit" | "apply_patch" => Some("write"),
        _ => None,
    }
}

fn extract_tool_candidate_paths(tool: &str, args: &Value) -> Vec<String> {
    let Some(obj) = args.as_object() else {
        return Vec::new();
    };
    let keys: &[&str] = match tool {
        "read" | "write" | "edit" | "grep" | "codesearch" => &["path", "filePath", "cwd"],
        "glob" => &["pattern"],
        "lsp" => &["filePath", "path"],
        "bash" => &["cwd"],
        "apply_patch" => &["path"],
        _ => &["path", "cwd"],
    };
    keys.iter()
        .filter_map(|key| obj.get(*key))
        .filter_map(|value| value.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|raw| strip_glob_tokens(raw).to_string())
        .collect()
}

fn strip_glob_tokens(path: &str) -> &str {
    let mut end = path.len();
    for (idx, ch) in path.char_indices() {
        if ch == '*' || ch == '?' || ch == '[' {
            end = idx;
            break;
        }
    }
    &path[..end]
}

fn is_path_allowed_by_scopes(root: &str, candidate: &str, scopes: &[String]) -> bool {
    let root_path = PathBuf::from(root);
    let candidate_path = resolve_path(&root_path, candidate);
    scopes.iter().any(|scope| {
        let scope_path = resolve_path(&root_path, scope);
        candidate_path.starts_with(scope_path)
    })
}

fn resolve_path(root: &Path, raw: &str) -> PathBuf {
    let raw = raw.trim();
    if raw.is_empty() {
        return root.to_path_buf();
    }
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn extract_url_host(args: &Value) -> Option<String> {
    let url = args
        .get("url")
        .or_else(|| args.get("uri"))
        .or_else(|| args.get("link"))
        .and_then(|v| v.as_str())?;
    let raw = url.trim();
    let (_, after_scheme) = raw.split_once("://")?;
    let host_port = after_scheme.split('/').next().unwrap_or_default();
    let host = host_port.split('@').next_back().unwrap_or_default();
    let host = host
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

pub fn emit_spawn_requested(state: &AppState, req: &SpawnRequest) {
    emit_spawn_requested_with_context(state, req, &SpawnEventContext::default());
}

pub fn emit_spawn_denied(state: &AppState, req: &SpawnRequest, decision: &SpawnDecision) {
    emit_spawn_denied_with_context(state, req, decision, &SpawnEventContext::default());
}

pub fn emit_spawn_approved(state: &AppState, req: &SpawnRequest, instance: &AgentInstance) {
    emit_spawn_approved_with_context(state, req, instance, &SpawnEventContext::default());
}

#[derive(Default)]
pub struct SpawnEventContext<'a> {
    pub session_id: Option<&'a str>,
    pub message_id: Option<&'a str>,
    pub run_id: Option<&'a str>,
}

pub fn emit_spawn_requested_with_context(
    state: &AppState,
    req: &SpawnRequest,
    ctx: &SpawnEventContext<'_>,
) {
    state.event_bus.publish(EngineEvent::new(
        "agent_team.spawn.requested",
        json!({
            "sessionID": ctx.session_id,
            "messageID": ctx.message_id,
            "runID": ctx.run_id,
            "missionID": req.mission_id,
            "instanceID": Value::Null,
            "parentInstanceID": req.parent_instance_id,
            "source": req.source,
            "requestedRole": req.role,
            "templateID": req.template_id,
            "justification": req.justification,
            "timestampMs": crate::now_ms(),
        }),
    ));
}

pub fn emit_spawn_denied_with_context(
    state: &AppState,
    req: &SpawnRequest,
    decision: &SpawnDecision,
    ctx: &SpawnEventContext<'_>,
) {
    state.event_bus.publish(EngineEvent::new(
        "agent_team.spawn.denied",
        json!({
            "sessionID": ctx.session_id,
            "messageID": ctx.message_id,
            "runID": ctx.run_id,
            "missionID": req.mission_id,
            "instanceID": Value::Null,
            "parentInstanceID": req.parent_instance_id,
            "source": req.source,
            "requestedRole": req.role,
            "templateID": req.template_id,
            "code": decision.code,
            "error": decision.reason,
            "timestampMs": crate::now_ms(),
        }),
    ));
}

pub fn emit_spawn_approved_with_context(
    state: &AppState,
    req: &SpawnRequest,
    instance: &AgentInstance,
    ctx: &SpawnEventContext<'_>,
) {
    state.event_bus.publish(EngineEvent::new(
        "agent_team.spawn.approved",
        json!({
            "sessionID": ctx.session_id.unwrap_or(&instance.session_id),
            "messageID": ctx.message_id,
            "runID": ctx.run_id.or(instance.run_id.as_deref()),
            "missionID": instance.mission_id,
            "instanceID": instance.instance_id,
            "parentInstanceID": instance.parent_instance_id,
            "source": req.source,
            "requestedRole": req.role,
            "templateID": instance.template_id,
            "skillHash": instance.skill_hash,
            "workspaceRoot": instance_workspace_root(instance),
            "workspaceRepoRoot": instance_workspace_repo_root(instance),
            "managedWorktree": instance_managed_worktree(instance),
            "timestampMs": crate::now_ms(),
        }),
    ));
    state.event_bus.publish(EngineEvent::new(
        "agent_team.instance.started",
        json!({
            "sessionID": ctx.session_id.unwrap_or(&instance.session_id),
            "messageID": ctx.message_id,
            "runID": ctx.run_id.or(instance.run_id.as_deref()),
            "missionID": instance.mission_id,
            "instanceID": instance.instance_id,
            "parentInstanceID": instance.parent_instance_id,
            "role": instance.role,
            "status": instance.status,
            "budgetLimit": instance.budget,
            "skillHash": instance.skill_hash,
            "workspaceRoot": instance_workspace_root(instance),
            "workspaceRepoRoot": instance_workspace_repo_root(instance),
            "managedWorktree": instance_managed_worktree(instance),
            "timestampMs": crate::now_ms(),
        }),
    ));
}

pub fn emit_budget_usage(
    state: &AppState,
    instance: &AgentInstance,
    tokens_used: u64,
    steps_used: u32,
    tool_calls_used: u32,
    cost_used_usd: f64,
    elapsed_ms: u64,
) {
    state.event_bus.publish(EngineEvent::new(
        "agent_team.budget.usage",
        json!({
            "sessionID": instance.session_id,
            "messageID": Value::Null,
            "runID": instance.run_id,
            "missionID": instance.mission_id,
            "instanceID": instance.instance_id,
            "tokensUsed": tokens_used,
            "stepsUsed": steps_used,
            "toolCallsUsed": tool_calls_used,
            "costUsedUsd": cost_used_usd,
            "elapsedMs": elapsed_ms,
            "timestampMs": crate::now_ms(),
        }),
    ));
}

pub fn emit_budget_exhausted(
    state: &AppState,
    instance: &AgentInstance,
    exhausted_by: &str,
    tokens_used: u64,
    steps_used: u32,
    tool_calls_used: u32,
    cost_used_usd: f64,
    elapsed_ms: u64,
) {
    state.event_bus.publish(EngineEvent::new(
        "agent_team.budget.exhausted",
        json!({
            "sessionID": instance.session_id,
            "messageID": Value::Null,
            "runID": instance.run_id,
            "missionID": instance.mission_id,
            "instanceID": instance.instance_id,
            "exhaustedBy": exhausted_by,
            "tokensUsed": tokens_used,
            "stepsUsed": steps_used,
            "toolCallsUsed": tool_calls_used,
            "costUsedUsd": cost_used_usd,
            "elapsedMs": elapsed_ms,
            "timestampMs": crate::now_ms(),
        }),
    ));
}

pub fn emit_instance_cancelled(state: &AppState, instance: &AgentInstance, reason: &str) {
    state.event_bus.publish(EngineEvent::new(
        "agent_team.instance.cancelled",
        json!({
            "sessionID": instance.session_id,
            "messageID": Value::Null,
            "runID": instance.run_id,
            "missionID": instance.mission_id,
            "instanceID": instance.instance_id,
            "parentInstanceID": instance.parent_instance_id,
            "role": instance.role,
            "status": instance.status,
            "reason": reason,
            "workspaceRoot": instance_workspace_root(instance),
            "workspaceRepoRoot": instance_workspace_repo_root(instance),
            "managedWorktree": instance_managed_worktree(instance),
            "timestampMs": crate::now_ms(),
        }),
    ));
}

pub fn emit_instance_completed(state: &AppState, instance: &AgentInstance) {
    state.event_bus.publish(EngineEvent::new(
        "agent_team.instance.completed",
        json!({
            "sessionID": instance.session_id,
            "messageID": Value::Null,
            "runID": instance.run_id,
            "missionID": instance.mission_id,
            "instanceID": instance.instance_id,
            "parentInstanceID": instance.parent_instance_id,
            "role": instance.role,
            "status": instance.status,
            "workspaceRoot": instance_workspace_root(instance),
            "workspaceRepoRoot": instance_workspace_repo_root(instance),
            "managedWorktree": instance_managed_worktree(instance),
            "timestampMs": crate::now_ms(),
        }),
    ));
}

pub fn emit_instance_failed(state: &AppState, instance: &AgentInstance) {
    state.event_bus.publish(EngineEvent::new(
        "agent_team.instance.failed",
        json!({
            "sessionID": instance.session_id,
            "messageID": Value::Null,
            "runID": instance.run_id,
            "missionID": instance.mission_id,
            "instanceID": instance.instance_id,
            "parentInstanceID": instance.parent_instance_id,
            "role": instance.role,
            "status": instance.status,
            "workspaceRoot": instance_workspace_root(instance),
            "workspaceRepoRoot": instance_workspace_repo_root(instance),
            "managedWorktree": instance_managed_worktree(instance),
            "timestampMs": crate::now_ms(),
        }),
    ));
}

pub fn emit_mission_budget_exhausted(
    state: &AppState,
    mission_id: &str,
    exhausted_by: &str,
    tokens_used: u64,
    steps_used: u64,
    tool_calls_used: u64,
    cost_used_usd: f64,
) {
    state.event_bus.publish(EngineEvent::new(
        "agent_team.mission.budget.exhausted",
        json!({
            "sessionID": Value::Null,
            "messageID": Value::Null,
            "runID": Value::Null,
            "missionID": mission_id,
            "instanceID": Value::Null,
            "exhaustedBy": exhausted_by,
            "tokensUsed": tokens_used,
            "stepsUsed": steps_used,
            "toolCallsUsed": tool_calls_used,
            "costUsedUsd": cost_used_usd,
            "timestampMs": crate::now_ms(),
        }),
    ));
}
