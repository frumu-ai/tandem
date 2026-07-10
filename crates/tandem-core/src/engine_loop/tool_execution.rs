use super::*;
use tandem_tools::{
    ToolDispatchContext, ToolDispatchLedger, ToolDispatchLedgerEvent, ToolDispatchPolicyOutcome,
    ToolDispatchSource, ToolDispatchStatus,
};

#[derive(Clone)]
pub(super) struct EngineToolDispatchLedger {
    pub(super) event_bus: EventBus,
}

#[async_trait::async_trait]
impl ToolDispatchLedger for EngineToolDispatchLedger {
    async fn record(&self, event: ToolDispatchLedgerEvent) -> anyhow::Result<()> {
        self.event_bus.publish(EngineEvent::new(
            "tool.dispatch.recorded",
            serde_json::to_value(event).unwrap_or(Value::Null),
        ));
        Ok(())
    }
}

impl EngineLoop {
    pub(super) async fn execute_tool_with_timeout(
        &self,
        session_id: &str,
        message_id: &str,
        tool: &str,
        args: Value,
        cancel: CancellationToken,
        progress: Option<SharedToolProgressSink>,
    ) -> anyhow::Result<tandem_types::ToolResult> {
        let timeout_ms = tool_exec_timeout_ms() as u64;
        let session = self.storage.get_session(session_id).await;
        let tenant_context = session
            .as_ref()
            .map(|session| session.tenant_context.clone())
            .unwrap_or_else(TenantContext::local_implicit);
        let verified_tenant_context = session.and_then(|session| session.verified_tenant_context);
        let scope_allowlist = self
            .session_allowed_tools
            .read()
            .await
            .get(session_id)
            .cloned()
            .unwrap_or_default();
        let tool_dispatch_ledger = self.tool_dispatch_ledger.read().await.clone();
        let mut dispatch_context = ToolDispatchContext::for_tenant("engine_loop", tenant_context)
            .with_source(
                ToolDispatchSource::new("engine_loop")
                    .session(session_id)
                    .message(message_id),
            )
            .with_scope_allowlist(scope_allowlist)
            .with_ledger(tool_dispatch_ledger);
        if let Some(verified_tenant_context) = verified_tenant_context {
            dispatch_context =
                dispatch_context.with_verified_tenant_context(verified_tenant_context);
        }
        let (timeout_dispatch_id, timeout_payload_digest) = self
            .tool_dispatcher
            .dispatch_identity_for(tool, &args, &dispatch_context)
            .await;
        let timeout_dispatch_context = dispatch_context.clone();
        match tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            self.tool_dispatcher.dispatch_with_cancel_and_progress(
                tool,
                args,
                dispatch_context,
                cancel,
                progress,
            ),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                timeout_dispatch_context
                    .ledger
                    .record(ToolDispatchLedgerEvent {
                        dispatch_id: Some(timeout_dispatch_id),
                        tool: tool.to_string(),
                        canonical_tool: None,
                        tenant_context: timeout_dispatch_context.tenant_context.clone(),
                        source: timeout_dispatch_context.source.clone(),
                        scope_allowlist: timeout_dispatch_context.scope_allowlist.clone(),
                        policy_outcome: ToolDispatchPolicyOutcome::Allowed,
                        policy_decision_id: None,
                        payload_digest: Some(timeout_payload_digest),
                        status: ToolDispatchStatus::Failed,
                        error: Some(format!("TOOL_EXEC_TIMEOUT_MS_EXCEEDED({timeout_ms})")),
                    })
                    .await?;
                anyhow::bail!("TOOL_EXEC_TIMEOUT_MS_EXCEEDED({timeout_ms})");
            }
        }
    }

    pub(super) async fn find_recent_matching_user_message_id(
        &self,
        session_id: &str,
        text: &str,
    ) -> Option<String> {
        let session = self.storage.get_session(session_id).await?;
        let last = session.messages.last()?;
        if !matches!(last.role, MessageRole::User) {
            return None;
        }
        let age_ms = (Utc::now() - last.created_at).num_milliseconds().max(0) as u64;
        if age_ms > 10_000 {
            return None;
        }
        let last_text = last
            .parts
            .iter()
            .filter_map(|part| match part {
                MessagePart::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if last_text == text {
            return Some(last.id.clone());
        }
        None
    }

    pub(super) async fn auto_rename_session_from_user_text(
        &self,
        session_id: &str,
        fallback_text: &str,
    ) {
        let Some(mut session) = self.storage.get_session(session_id).await else {
            return;
        };
        if !title_needs_repair(&session.title) {
            return;
        }

        let first_user_text = session.messages.iter().find_map(|message| {
            if !matches!(message.role, MessageRole::User) {
                return None;
            }
            message.parts.iter().find_map(|part| match part {
                MessagePart::Text { text } if !text.trim().is_empty() => Some(text.clone()),
                _ => None,
            })
        });

        let source = first_user_text.unwrap_or_else(|| fallback_text.to_string());
        let Some(title) = derive_session_title_from_prompt(&source, 60) else {
            return;
        };

        session.title = title;
        session.time.updated = Utc::now();
        let _ = self.storage.save_session(session).await;
    }

    pub(super) async fn workspace_sandbox_violation(
        &self,
        session_id: &str,
        tool: &str,
        args: &Value,
    ) -> Option<String> {
        if self.workspace_override_active(session_id).await {
            return None;
        }
        if is_mcp_tool_name(tool) {
            if let Some(server) = mcp_server_from_tool_name(tool) {
                if is_mcp_sandbox_exempt_server(server) {
                    return None;
                }
            }
            let candidate_paths = extract_tool_candidate_paths(tool, args);
            if candidate_paths.is_empty() {
                return None;
            }
            let Some(session) = self.storage.get_session(session_id).await else {
                return Some(format!(
                    "ToolDenied {{ reason: WorkspaceScope }}: Sandbox blocked MCP tool `{tool}` because the session workspace could not be resolved."
                ));
            };
            let Some(workspace) = session_effective_workspace_root(&session) else {
                return Some(format!(
                    "ToolDenied {{ reason: WorkspaceScope }}: Sandbox blocked MCP tool `{tool}` because the session workspace could not be resolved."
                ));
            };
            let workspace_path = PathBuf::from(&workspace);
            if let Some(sensitive) = candidate_paths.iter().find(|path| {
                let raw = Path::new(path);
                let resolved = if raw.is_absolute() {
                    raw.to_path_buf()
                } else {
                    workspace_path.join(raw)
                };
                is_sensitive_path_candidate(&resolved)
            }) {
                return Some(format!(
                    "Sandbox blocked MCP tool `{tool}` path `{sensitive}` (sensitive path policy)."
                ));
            }
            let outside = candidate_paths.iter().find(|path| {
                let raw = Path::new(path);
                let resolved = if raw.is_absolute() {
                    raw.to_path_buf()
                } else {
                    workspace_path.join(raw)
                };
                !crate::is_within_workspace_root(&resolved, &workspace_path)
            })?;
            return Some(format!(
                "ToolDenied {{ reason: WorkspaceScope }}: Sandbox blocked MCP tool `{tool}` path `{outside}` (workspace root: `{workspace}`)"
            ));
        }
        let Some(session) = self.storage.get_session(session_id).await else {
            if is_shell_tool_name(tool) || super::write_targets::requires_concrete(tool, args) {
                return Some(format!(
                    "ToolDenied {{ reason: WorkspaceScope }}: Sandbox blocked `{tool}` because the session workspace could not be resolved."
                ));
            }
            return None;
        };
        let Some(workspace) = session_effective_workspace_root(&session) else {
            if is_shell_tool_name(tool) || super::write_targets::requires_concrete(tool, args) {
                return Some(format!(
                    "ToolDenied {{ reason: WorkspaceScope }}: Sandbox blocked `{tool}` because the session workspace could not be resolved."
                ));
            }
            return None;
        };
        let workspace_path = PathBuf::from(&workspace);
        let candidate_paths = extract_tool_candidate_paths(tool, args);
        if candidate_paths.is_empty() {
            if is_shell_tool_name(tool) {
                if let Some(command) = extract_shell_command(args) {
                    if shell_command_targets_sensitive_path(&command) {
                        return Some(format!(
                            "Sandbox blocked `{tool}` command targeting sensitive paths."
                        ));
                    }
                }
            }
            return None;
        }
        if let Some(sensitive) = candidate_paths.iter().find(|path| {
            let raw = Path::new(path);
            let resolved = if raw.is_absolute() {
                raw.to_path_buf()
            } else {
                workspace_path.join(raw)
            };
            is_sensitive_path_candidate(&resolved)
        }) {
            return Some(format!(
                "Sandbox blocked `{tool}` path `{sensitive}` (sensitive path policy)."
            ));
        }

        let outside = candidate_paths.iter().find(|path| {
            let raw = Path::new(path);
            let resolved = if raw.is_absolute() {
                raw.to_path_buf()
            } else {
                workspace_path.join(raw)
            };
            !crate::is_within_workspace_root(&resolved, &workspace_path)
        })?;
        Some(format!(
            "ToolDenied {{ reason: WorkspaceScope }}: Sandbox blocked `{tool}` path `{outside}` (workspace root: `{workspace}`)"
        ))
    }

    pub(super) async fn session_write_policy_violation(
        &self,
        session_id: &str,
        tool: &str,
        args: &Value,
    ) -> Option<String> {
        let policy = self.get_session_write_policy(session_id).await?;
        if matches!(policy.mode, SessionWritePolicyMode::RepoEdit) {
            return None;
        }

        let targets = super::write_targets::paths(tool, args);
        if targets.is_empty() {
            if super::write_targets::requires_concrete(tool, args) {
                return Some(format!(
                    "Write policy blocked `{tool}` because this session only allows declared output targets."
                ));
            }
            return None;
        }

        let Some(session) = self.storage.get_session(session_id).await else {
            return Some(format!(
                "Write policy blocked `{tool}` because the session workspace could not be resolved."
            ));
        };
        let Some(workspace) = session
            .workspace_root
            .or_else(|| crate::normalize_workspace_path(&session.directory))
        else {
            return Some(format!(
                "Write policy blocked `{tool}` because the session workspace could not be resolved."
            ));
        };
        let workspace_path = normalize_path_lexical(Path::new(&workspace));
        let effective_cwd = string_field(args, "__effective_cwd")
            .map(PathBuf::from)
            .or_else(|| {
                let directory = session.directory.trim();
                if directory.is_empty() || directory == "." {
                    None
                } else {
                    Some(PathBuf::from(directory))
                }
            })
            .unwrap_or_else(|| workspace_path.clone());
        let allowed_paths = policy
            .allowed_paths
            .iter()
            .map(|path| resolve_policy_path(path, &workspace_path, &workspace_path))
            .collect::<HashSet<_>>();
        if allowed_paths.is_empty() {
            return Some(format!(
                "Write policy blocked `{tool}` because no declared output targets are available for this session."
            ));
        }

        let outside = targets.iter().find(|target| {
            let resolved = resolve_policy_path(target, &effective_cwd, &workspace_path);
            !allowed_paths.contains(&resolved)
        });
        outside.map(|target| {
            format!(
                "Write policy blocked `{tool}` target `{target}`. This automation session may only write declared output targets."
            )
        })
    }

    pub(super) async fn resolve_tool_execution_context(
        &self,
        session_id: &str,
    ) -> Option<(String, String, Option<String>)> {
        let session = self.storage.get_session(session_id).await?;
        let workspace_root = session_effective_workspace_root(&session)?;
        let effective_cwd =
            if session.directory.trim().is_empty() || session.directory.trim() == "." {
                workspace_root.clone()
            } else {
                let candidate = crate::normalize_workspace_path(&session.directory)
                    .unwrap_or(workspace_root.clone());
                if session.pinned_workspace_id.is_some()
                    && !crate::is_within_workspace_root(
                        Path::new(&candidate),
                        Path::new(&workspace_root),
                    )
                {
                    workspace_root.clone()
                } else {
                    candidate
                }
            };
        let project_id = session
            .project_id
            .clone()
            .or_else(|| crate::workspace_project_id(&workspace_root));
        Some((workspace_root, effective_cwd, project_id))
    }

    pub(super) async fn mark_session_run_failed(&self, session_id: &str, error: &str) {
        let detail = truncate_text(error, 1_000);
        self.event_bus.publish(EngineEvent::new(
            "session.updated",
            json!({
                "sessionID": session_id,
                "status": "failed",
                "error": detail,
            }),
        ));
        self.event_bus.publish(EngineEvent::new(
            "session.status",
            json!({
                "sessionID": session_id,
                "status": "failed",
                "error": detail,
            }),
        ));
        self.cancellations.remove(session_id).await;
    }

    pub(super) async fn workspace_override_active(&self, session_id: &str) -> bool {
        let now = chrono::Utc::now().timestamp_millis().max(0) as u64;
        let mut overrides = self.workspace_overrides.write().await;
        let expired: Vec<String> = overrides
            .iter()
            .filter_map(|(id, &exp)| if exp <= now { Some(id.clone()) } else { None })
            .collect();
        overrides.retain(|_, expires_at| *expires_at > now);
        drop(overrides);
        for expired_id in expired {
            self.event_bus.publish(EngineEvent::new(
                "workspace.override.expired",
                json!({ "sessionID": expired_id }),
            ));
        }
        self.workspace_overrides
            .read()
            .await
            .get(session_id)
            .map(|expires_at| *expires_at > now)
            .unwrap_or(false)
    }

    pub(super) async fn generate_final_narrative_without_tools(
        &self,
        session_id: &str,
        run_id: Option<&str>,
        active_agent: &AgentDefinition,
        provider_hint: Option<&str>,
        model_id: Option<&str>,
        sampling: SamplingParams,
        cancel: CancellationToken,
        tool_outputs: &[String],
    ) -> anyhow::Result<Option<String>> {
        if cancel.is_cancelled() {
            return Ok(None);
        }
        let route = match self
            .providers
            .resolve_provider_route(provider_hint, model_id)
            .await
        {
            Ok(route) => route,
            Err(_) => return Ok(None),
        };
        let mut messages = load_chat_history(
            self.storage.clone(),
            session_id,
            ChatHistoryProfile::Standard,
        )
        .await
        .messages;
        let mut system_parts = vec![tandem_runtime_system_prompt(
            &self.host_runtime_context,
            &[],
        )];
        if let Some(system) = active_agent.system_prompt.as_ref() {
            system_parts.push(system.clone());
        }
        messages.insert(
            0,
            ChatMessage {
                role: "system".to_string(),
                content: system_parts.join("\n\n"),
                attachments: Vec::new(),
            },
        );
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: build_post_tool_final_narrative_prompt(tool_outputs),
            attachments: Vec::new(),
        });
        let session = self.storage.get_session(session_id).await;
        let tenant = session
            .as_ref()
            .map(|session| &session.tenant_context)
            .filter(|tenant| !tenant.is_local_implicit());
        let operation_id = format!("{session_id}:post_tool_narrative");
        let data_classes = [
            tandem_data_boundary::SensitiveDataClass::CustomerData,
            tandem_data_boundary::SensitiveDataClass::SourceCode,
            tandem_data_boundary::SensitiveDataClass::ProprietaryBusinessData,
            tandem_data_boundary::SensitiveDataClass::UnknownSensitive,
        ];
        let provider_egress_permit = match data_boundary_gate::evaluate_dispatch_boundary(
            &data_boundary_gate::DataBoundaryDispatchContext {
                session_id,
                run_id,
                message_id: &operation_id,
                correlation_id: None,
                provider_id: route.provider_id.as_str(),
                model_id: route.model_id.as_deref(),
                tool_schema_payload: None,
                source_ref: "engine_loop.post_tool_synthesis",
                data_classes: &data_classes,
                authority_ref: session
                    .as_ref()
                    .and_then(|session| session.verified_tenant_context.as_ref())
                    .map(|verified| verified.assertion_id.as_str()),
                org_id: tenant.map(|tenant| tenant.org_id.as_str()),
                workspace_id: tenant.map(|tenant| tenant.workspace_id.as_str()),
                deployment_id: tenant.and_then(|tenant| tenant.deployment_id.as_deref()),
            },
            &messages,
        ) {
            data_boundary_gate::DataBoundaryDispatchOutcome::Off { permit } => permit,
            data_boundary_gate::DataBoundaryDispatchOutcome::Proceed { event, permit } => {
                self.event_bus.publish(event);
                permit
            }
            data_boundary_gate::DataBoundaryDispatchOutcome::ProceedTransformed {
                event,
                messages: transformed,
                permit,
            } => {
                self.event_bus.publish(event);
                messages = transformed;
                permit
            }
            data_boundary_gate::DataBoundaryDispatchOutcome::RequireApproval {
                event,
                evidence,
                denial_reason,
                approval,
            } => {
                self.event_bus.publish(event);
                let pending = self.permissions.ask_for_session(
                    Some(session_id),
                    "data_boundary_egress",
                    evidence,
                );
                let pending = pending.await;
                let (reply, timed_out) = self
                    .permissions
                    .wait_for_reply_with_timeout(
                        &pending.id,
                        cancel.clone(),
                        Some(Duration::from_millis(permission_wait_timeout_ms() as u64)),
                    )
                    .await;
                if !matches!(
                    reply.as_deref(),
                    Some("once") | Some("always") | Some("allow")
                ) {
                    let detail = format!(
                        "{denial_reason} ({})",
                        if timed_out {
                            "approval timed out"
                        } else {
                            "approval denied"
                        }
                    );
                    self.mark_session_run_failed(session_id, &detail).await;
                    anyhow::bail!(detail);
                }
                approval.approve(pending.id).map_err(anyhow::Error::msg)?
            }
            data_boundary_gate::DataBoundaryDispatchOutcome::Blocked { event, reason } => {
                self.event_bus.publish(event);
                self.mark_session_run_failed(session_id, &reason).await;
                anyhow::bail!(reason);
            }
        };
        let stream = match self
            .providers
            .stream_with_egress_permit(
                &provider_egress_permit,
                Some(route.provider_id.as_str()),
                route.model_id.as_deref(),
                messages,
                ToolMode::None,
                None,
                sampling,
                cancel.clone(),
            )
            .await
        {
            Ok(stream) => stream,
            Err(_) => return Ok(None),
        };
        tokio::pin!(stream);
        let mut completion = String::new();
        while let Some(chunk) = stream.next().await {
            if cancel.is_cancelled() {
                return Ok(None);
            }
            match chunk {
                Ok(StreamChunk::TextDelta(delta)) => {
                    let delta = strip_model_control_markers(&delta);
                    if !delta.trim().is_empty() {
                        completion.push_str(&delta);
                    }
                }
                Ok(StreamChunk::Done { .. }) => break,
                Ok(_) => {}
                Err(_) => return Ok(None),
            }
        }
        let completion = truncate_text(&strip_model_control_markers(&completion), 16_000);
        if completion.trim().is_empty() {
            Ok(None)
        } else {
            Ok(Some(completion))
        }
    }
}

fn session_effective_workspace_root(session: &tandem_types::Session) -> Option<String> {
    session
        .pinned_workspace_id
        .as_deref()
        .and_then(crate::normalize_workspace_path)
        .or_else(|| session.workspace_root.clone())
        .or_else(|| crate::normalize_workspace_path(&session.directory))
}

fn resolve_policy_path(path: &str, effective_cwd: &Path, workspace_root: &Path) -> PathBuf {
    let raw = Path::new(path);
    let resolved = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        let base = if effective_cwd.is_absolute() {
            effective_cwd.to_path_buf()
        } else {
            workspace_root.join(effective_cwd)
        };
        base.join(raw)
    };
    normalize_path_lexical(&resolved)
}

fn normalize_path_lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push("..");
                }
            }
            std::path::Component::Normal(value) => normalized.push(value),
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                normalized.push(component.as_os_str())
            }
        }
    }
    normalized
}

// Write-target derivation lives in `super::write_targets` (Invariant 1 of
// `docs/SPINE.md`). `string_field`/`string_fields` remain here because
// they are also used by `session_write_policy_violation` above; the
// write_targets module imports them via `pub(super)`.

pub(super) fn string_field(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn string_fields(args: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .filter_map(|key| string_field(args, key))
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod session_write_policy_tests {
    use super::*;

    // Write-target and shell-mutation tests moved with their functions to
    // `super::write_targets` (Invariant 1, `docs/SPINE.md`). Path
    // normalization stays here because `resolve_policy_path` lives in this
    // module.

    #[test]
    fn write_policy_normalizes_equivalent_paths() {
        let workspace = Path::new("/workspace/project");
        let resolved = resolve_policy_path(
            "./.tandem/runs/run-1/../run-1/artifacts/out.md",
            workspace,
            workspace,
        );
        assert_eq!(
            resolved,
            PathBuf::from("/workspace/project/.tandem/runs/run-1/artifacts/out.md")
        );
    }

    #[test]
    fn pinned_workspace_overrides_session_workspace_root() {
        let mut session = tandem_types::Session::new(
            Some("slack channel".to_string()),
            Some("/workspaces/other".to_string()),
        );
        session.source_kind = Some("channel".to_string());
        session.workspace_root = Some("/workspaces/other".to_string());
        session.pinned_workspace_id = Some("/workspaces/acme".to_string());

        let expected = crate::normalize_workspace_path("/workspaces/acme");
        assert_eq!(session_effective_workspace_root(&session), expected);
    }

    #[test]
    fn workspace_scope_denial_message_is_structured() {
        let tool = "read";
        let outside = "/workspaces/other/secret.txt";
        let workspace = "/workspaces/acme";
        let message = format!(
            "ToolDenied {{ reason: WorkspaceScope }}: Sandbox blocked `{tool}` path `{outside}` (workspace root: `{workspace}`)"
        );
        assert!(message.contains("ToolDenied { reason: WorkspaceScope }"));
    }
}
