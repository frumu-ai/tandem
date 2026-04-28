impl AppState {
    pub async fn submit_bug_monitor_draft(
        &self,
        mut submission: BugMonitorSubmission,
    ) -> anyhow::Result<BugMonitorDraftRecord> {
        fn normalize_optional(value: Option<String>) -> Option<String> {
            value
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        }

        fn compute_fingerprint(parts: &[&str]) -> String {
            use std::hash::{Hash, Hasher};

            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for part in parts {
                part.hash(&mut hasher);
            }
            format!("{:016x}", hasher.finish())
        }

        submission.repo = normalize_optional(submission.repo);
        submission.title = normalize_optional(submission.title);
        submission.detail = normalize_optional(submission.detail);
        submission.source = normalize_optional(submission.source);
        submission.run_id = normalize_optional(submission.run_id);
        submission.session_id = normalize_optional(submission.session_id);
        submission.correlation_id = normalize_optional(submission.correlation_id);
        submission.file_name = normalize_optional(submission.file_name);
        submission.process = normalize_optional(submission.process);
        submission.component = normalize_optional(submission.component);
        submission.event = normalize_optional(submission.event);
        submission.level = normalize_optional(submission.level);
        submission.fingerprint = normalize_optional(submission.fingerprint);
        submission.confidence = normalize_optional(submission.confidence);
        submission.risk_level = normalize_optional(submission.risk_level);
        submission.expected_destination = normalize_optional(submission.expected_destination);
        submission.excerpt = submission
            .excerpt
            .into_iter()
            .map(|line| line.trim_end().to_string())
            .filter(|line| !line.is_empty())
            .take(50)
            .collect();
        submission.evidence_refs = submission
            .evidence_refs
            .into_iter()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .take(50)
            .collect();
        if submission.source.is_none() {
            submission.source = Some("manual".to_string());
        }
        if submission.event.is_none() {
            submission.event = Some("manual.report".to_string());
        }
        if submission.confidence.is_none() {
            submission.confidence = Some("medium".to_string());
        }
        if submission.risk_level.is_none() {
            submission.risk_level = Some("medium".to_string());
        }
        if submission.expected_destination.is_none() {
            submission.expected_destination = Some("bug_monitor_issue_draft".to_string());
        }

        let config = self.bug_monitor_config().await;
        let repo = submission
            .repo
            .clone()
            .or(config.repo.clone())
            .ok_or_else(|| anyhow::anyhow!("Bug Monitor repo is not configured"))?;
        if !is_valid_owner_repo_slug(&repo) {
            anyhow::bail!("Bug Monitor repo must be in owner/repo format");
        }

        let title = submission.title.clone().unwrap_or_else(|| {
            if let Some(event) = submission.event.as_ref() {
                format!("Failure detected in {event}")
            } else if let Some(component) = submission.component.as_ref() {
                format!("Failure detected in {component}")
            } else if let Some(process) = submission.process.as_ref() {
                format!("Failure detected in {process}")
            } else if let Some(source) = submission.source.as_ref() {
                format!("Failure report from {source}")
            } else {
                "Failure report".to_string()
            }
        });

        let mut detail_lines = Vec::new();
        if let Some(source) = submission.source.as_ref() {
            detail_lines.push(format!("source: {source}"));
        }
        if let Some(file_name) = submission.file_name.as_ref() {
            detail_lines.push(format!("file: {file_name}"));
        }
        if let Some(level) = submission.level.as_ref() {
            detail_lines.push(format!("level: {level}"));
        }
        if let Some(process) = submission.process.as_ref() {
            detail_lines.push(format!("process: {process}"));
        }
        if let Some(component) = submission.component.as_ref() {
            detail_lines.push(format!("component: {component}"));
        }
        if let Some(event) = submission.event.as_ref() {
            detail_lines.push(format!("event: {event}"));
        }
        if let Some(confidence) = submission.confidence.as_ref() {
            detail_lines.push(format!("confidence: {confidence}"));
        }
        if let Some(risk_level) = submission.risk_level.as_ref() {
            detail_lines.push(format!("risk_level: {risk_level}"));
        }
        if let Some(expected_destination) = submission.expected_destination.as_ref() {
            detail_lines.push(format!("expected_destination: {expected_destination}"));
        }
        if let Some(run_id) = submission.run_id.as_ref() {
            detail_lines.push(format!("run_id: {run_id}"));
        }
        if let Some(session_id) = submission.session_id.as_ref() {
            detail_lines.push(format!("session_id: {session_id}"));
        }
        if let Some(correlation_id) = submission.correlation_id.as_ref() {
            detail_lines.push(format!("correlation_id: {correlation_id}"));
        }
        if let Some(detail) = submission.detail.as_ref() {
            detail_lines.push(String::new());
            detail_lines.push(detail.clone());
        }
        if !submission.excerpt.is_empty() {
            if !detail_lines.is_empty() {
                detail_lines.push(String::new());
            }
            detail_lines.push("excerpt:".to_string());
            detail_lines.extend(submission.excerpt.iter().map(|line| format!("  {line}")));
        }
        if !submission.evidence_refs.is_empty() {
            if !detail_lines.is_empty() {
                detail_lines.push(String::new());
            }
            detail_lines.push("evidence_refs:".to_string());
            detail_lines.extend(
                submission
                    .evidence_refs
                    .iter()
                    .map(|line| format!("  {line}")),
            );
        }
        let detail = if detail_lines.is_empty() {
            None
        } else {
            Some(detail_lines.join("\n"))
        };

        let fingerprint = submission.fingerprint.clone().unwrap_or_else(|| {
            compute_fingerprint(&[
                repo.as_str(),
                title.as_str(),
                detail.as_deref().unwrap_or(""),
                submission.source.as_deref().unwrap_or(""),
                submission.run_id.as_deref().unwrap_or(""),
                submission.session_id.as_deref().unwrap_or(""),
                submission.correlation_id.as_deref().unwrap_or(""),
            ])
        });
        submission.fingerprint = Some(fingerprint.clone());
        let quality_gate =
            crate::bug_monitor::service::evaluate_bug_monitor_submission_quality(&submission);
        if !quality_gate.passed {
            anyhow::bail!(
                "Bug Monitor signal quality gate blocked draft creation: {}",
                quality_gate
                    .blocked_reason
                    .clone()
                    .unwrap_or_else(|| "signal did not pass quality gates".to_string())
            );
        }

        let mut drafts = self.bug_monitor_drafts.write().await;
        if let Some(existing_id) = drafts
            .values()
            .find(|row| row.repo == repo && row.fingerprint == fingerprint)
            .map(|row| row.draft_id.clone())
        {
            let Some(existing) = drafts.get_mut(&existing_id) else {
                anyhow::bail!("Bug Monitor draft index changed while deduping");
            };
            let mut changed = false;
            if existing.confidence.is_none() && submission.confidence.is_some() {
                existing.confidence = submission.confidence.clone();
                changed = true;
            }
            if existing.risk_level.is_none() && submission.risk_level.is_some() {
                existing.risk_level = submission.risk_level.clone();
                changed = true;
            }
            if existing.expected_destination.is_none() && submission.expected_destination.is_some()
            {
                existing.expected_destination = submission.expected_destination.clone();
                changed = true;
            }
            existing.quality_gate = Some(quality_gate.clone());
            changed = true;
            for evidence_ref in &submission.evidence_refs {
                if !existing.evidence_refs.iter().any(|row| row == evidence_ref) {
                    existing.evidence_refs.push(evidence_ref.clone());
                    changed = true;
                }
            }
            let existing = existing.clone();
            drop(drafts);
            if changed {
                self.persist_bug_monitor_drafts().await?;
            }
            return Ok(existing);
        }

        let draft = BugMonitorDraftRecord {
            draft_id: format!("failure-draft-{}", uuid::Uuid::new_v4().simple()),
            fingerprint,
            repo,
            status: if config.require_approval_for_new_issues {
                "approval_required".to_string()
            } else {
                "draft_ready".to_string()
            },
            created_at_ms: now_ms(),
            triage_run_id: None,
            issue_number: None,
            title: Some(title),
            detail,
            github_status: None,
            github_issue_url: None,
            github_comment_url: None,
            github_posted_at_ms: None,
            matched_issue_number: None,
            matched_issue_state: None,
            evidence_digest: None,
            confidence: submission.confidence.clone(),
            risk_level: submission.risk_level.clone(),
            expected_destination: submission.expected_destination.clone(),
            evidence_refs: submission.evidence_refs.clone(),
            quality_gate: Some(quality_gate),
            last_post_error: None,
        };
        drafts.insert(draft.draft_id.clone(), draft.clone());
        drop(drafts);
        self.persist_bug_monitor_drafts().await?;
        Ok(draft)
    }

    pub async fn update_bug_monitor_draft_status(
        &self,
        draft_id: &str,
        next_status: &str,
        reason: Option<&str>,
    ) -> anyhow::Result<BugMonitorDraftRecord> {
        let normalized_status = next_status.trim().to_ascii_lowercase();
        if normalized_status != "draft_ready" && normalized_status != "denied" {
            anyhow::bail!("unsupported Bug Monitor draft status");
        }

        let mut drafts = self.bug_monitor_drafts.write().await;
        let Some(draft) = drafts.get_mut(draft_id) else {
            anyhow::bail!("Bug Monitor draft not found");
        };
        if !draft.status.eq_ignore_ascii_case("approval_required") {
            anyhow::bail!("Bug Monitor draft is not waiting for approval");
        }
        draft.status = normalized_status.clone();
        if let Some(reason) = reason
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            let next_detail = if let Some(detail) = draft.detail.as_ref() {
                format!("{detail}\n\noperator_note: {reason}")
            } else {
                format!("operator_note: {reason}")
            };
            draft.detail = Some(next_detail);
        }
        let updated = draft.clone();
        drop(drafts);
        self.persist_bug_monitor_drafts().await?;

        let event_name = if normalized_status == "draft_ready" {
            "bug_monitor.draft.approved"
        } else {
            "bug_monitor.draft.denied"
        };
        self.event_bus.publish(EngineEvent::new(
            event_name,
            serde_json::json!({
                "draft_id": updated.draft_id,
                "repo": updated.repo,
                "status": updated.status,
                "reason": reason,
            }),
        ));
        Ok(updated)
    }

    pub async fn bug_monitor_status(&self) -> BugMonitorStatus {
        let required_capabilities = vec![
            "github.list_issues".to_string(),
            "github.get_issue".to_string(),
            "github.create_issue".to_string(),
            "github.comment_on_issue".to_string(),
        ];
        let config = self.bug_monitor_config().await;
        let drafts = self.bug_monitor_drafts.read().await;
        let incidents = self.bug_monitor_incidents.read().await;
        let posts = self.bug_monitor_posts.read().await;
        let total_incidents = incidents.len();
        let pending_incidents = incidents
            .values()
            .filter(|row| {
                matches!(
                    row.status.as_str(),
                    "queued"
                        | "draft_created"
                        | "triage_queued"
                        | "analysis_queued"
                        | "triage_pending"
                        | "issue_draft_pending"
                )
            })
            .count();
        let pending_drafts = drafts
            .values()
            .filter(|row| row.status.eq_ignore_ascii_case("approval_required"))
            .count();
        let pending_posts = posts
            .values()
            .filter(|row| matches!(row.status.as_str(), "queued" | "failed"))
            .count();
        let last_activity_at_ms = drafts
            .values()
            .map(|row| row.created_at_ms)
            .chain(posts.values().map(|row| row.updated_at_ms))
            .max();
        drop(drafts);
        drop(incidents);
        drop(posts);
        let mut runtime = self.bug_monitor_runtime_status.read().await.clone();
        runtime.paused = config.paused;
        runtime.total_incidents = total_incidents;
        runtime.pending_incidents = pending_incidents;
        runtime.pending_posts = pending_posts;

        let mut status = BugMonitorStatus {
            config: config.clone(),
            runtime,
            pending_drafts,
            pending_posts,
            last_activity_at_ms,
            ..BugMonitorStatus::default()
        };
        let repo_valid = config
            .repo
            .as_ref()
            .map(|repo| is_valid_owner_repo_slug(repo))
            .unwrap_or(false);
        let mut servers = self.mcp.list().await;
        let mut selected_server = config
            .mcp_server
            .as_ref()
            .and_then(|name| servers.get(name))
            .cloned();
        if config.enabled {
            if let Some(server_name) = config.mcp_server.as_deref() {
                if selected_server
                    .as_ref()
                    .is_some_and(|server| server.enabled && !server.connected)
                    && self.mcp.connect(server_name).await
                {
                    servers = self.mcp.list().await;
                    selected_server = servers.get(server_name).cloned();
                }
            }
        }
        let provider_catalog = self.providers.list().await;
        let selected_model = config
            .model_policy
            .as_ref()
            .and_then(|policy| policy.get("default_model"))
            .and_then(crate::app::routines::parse_model_spec);
        let selected_model_ready = selected_model
            .as_ref()
            .map(|spec| crate::app::routines::provider_catalog_has_model(&provider_catalog, spec))
            .unwrap_or(false);
        let selected_server_tools = if let Some(server_name) = config.mcp_server.as_ref() {
            self.mcp.server_tools(server_name).await
        } else {
            Vec::new()
        };
        let discovered_tools = self
            .capability_resolver
            .discover_from_runtime(selected_server_tools, Vec::new())
            .await;
        status.discovered_mcp_tools = discovered_tools
            .iter()
            .map(|row| row.tool_name.clone())
            .collect();
        let discovered_providers = discovered_tools
            .iter()
            .map(|row| row.provider.to_ascii_lowercase())
            .collect::<std::collections::HashSet<_>>();
        let provider_preference = match config.provider_preference {
            BugMonitorProviderPreference::OfficialGithub => {
                vec![
                    "mcp".to_string(),
                    "composio".to_string(),
                    "arcade".to_string(),
                ]
            }
            BugMonitorProviderPreference::Composio => {
                vec![
                    "composio".to_string(),
                    "mcp".to_string(),
                    "arcade".to_string(),
                ]
            }
            BugMonitorProviderPreference::Arcade => {
                vec![
                    "arcade".to_string(),
                    "mcp".to_string(),
                    "composio".to_string(),
                ]
            }
            BugMonitorProviderPreference::Auto => {
                vec![
                    "mcp".to_string(),
                    "composio".to_string(),
                    "arcade".to_string(),
                ]
            }
        };
        let capability_resolution = self
            .capability_resolver
            .resolve(
                crate::capability_resolver::CapabilityResolveInput {
                    workflow_id: Some("bug_monitor".to_string()),
                    required_capabilities: required_capabilities.clone(),
                    optional_capabilities: Vec::new(),
                    provider_preference,
                    available_tools: discovered_tools,
                },
                Vec::new(),
            )
            .await
            .ok();
        let bindings_file = self.capability_resolver.list_bindings().await.ok();
        if let Some(bindings) = bindings_file.as_ref() {
            status.binding_source_version = bindings.builtin_version.clone();
            status.bindings_last_merged_at_ms = bindings.last_merged_at_ms;
            status.selected_server_binding_candidates = bindings
                .bindings
                .iter()
                .filter(|binding| required_capabilities.contains(&binding.capability_id))
                .filter(|binding| {
                    discovered_providers.is_empty()
                        || discovered_providers.contains(&binding.provider.to_ascii_lowercase())
                })
                .map(|binding| {
                    let binding_key = format!(
                        "{}::{}",
                        binding.capability_id,
                        binding.tool_name.to_ascii_lowercase()
                    );
                    let matched = capability_resolution
                        .as_ref()
                        .map(|resolution| {
                            resolution.resolved.iter().any(|row| {
                                row.capability_id == binding.capability_id
                                    && format!(
                                        "{}::{}",
                                        row.capability_id,
                                        row.tool_name.to_ascii_lowercase()
                                    ) == binding_key
                            })
                        })
                        .unwrap_or(false);
                    BugMonitorBindingCandidate {
                        capability_id: binding.capability_id.clone(),
                        binding_tool_name: binding.tool_name.clone(),
                        aliases: binding.tool_name_aliases.clone(),
                        matched,
                    }
                })
                .collect();
            status.selected_server_binding_candidates.sort_by(|a, b| {
                a.capability_id
                    .cmp(&b.capability_id)
                    .then_with(|| a.binding_tool_name.cmp(&b.binding_tool_name))
            });
        }
        let capability_ready = |capability_id: &str| -> bool {
            capability_resolution
                .as_ref()
                .map(|resolved| {
                    resolved
                        .resolved
                        .iter()
                        .any(|row| row.capability_id == capability_id)
                })
                .unwrap_or(false)
        };
        if let Some(resolution) = capability_resolution.as_ref() {
            status.missing_required_capabilities = resolution.missing_required.clone();
            status.resolved_capabilities = resolution
                .resolved
                .iter()
                .map(|row| BugMonitorCapabilityMatch {
                    capability_id: row.capability_id.clone(),
                    provider: row.provider.clone(),
                    tool_name: row.tool_name.clone(),
                    binding_index: row.binding_index,
                })
                .collect();
        } else {
            status.missing_required_capabilities = required_capabilities.clone();
        }
        status.required_capabilities = BugMonitorCapabilityReadiness {
            github_list_issues: capability_ready("github.list_issues"),
            github_get_issue: capability_ready("github.get_issue"),
            github_create_issue: capability_ready("github.create_issue"),
            github_comment_on_issue: capability_ready("github.comment_on_issue"),
        };
        status.selected_model = selected_model;
        status.readiness = BugMonitorReadiness {
            config_valid: repo_valid
                && selected_server.is_some()
                && status.required_capabilities.github_list_issues
                && status.required_capabilities.github_get_issue
                && status.required_capabilities.github_create_issue
                && status.required_capabilities.github_comment_on_issue
                && selected_model_ready,
            repo_valid,
            mcp_server_present: selected_server.is_some(),
            mcp_connected: selected_server
                .as_ref()
                .map(|row| row.connected)
                .unwrap_or(false),
            github_read_ready: status.required_capabilities.github_list_issues
                && status.required_capabilities.github_get_issue,
            github_write_ready: status.required_capabilities.github_create_issue
                && status.required_capabilities.github_comment_on_issue,
            selected_model_ready,
            ingest_ready: config.enabled && !config.paused && repo_valid,
            publish_ready: config.enabled
                && !config.paused
                && repo_valid
                && selected_server
                    .as_ref()
                    .map(|row| row.connected)
                    .unwrap_or(false)
                && status.required_capabilities.github_list_issues
                && status.required_capabilities.github_get_issue
                && status.required_capabilities.github_create_issue
                && status.required_capabilities.github_comment_on_issue
                && selected_model_ready,
            runtime_ready: config.enabled
                && !config.paused
                && repo_valid
                && selected_server
                    .as_ref()
                    .map(|row| row.connected)
                    .unwrap_or(false)
                && status.required_capabilities.github_list_issues
                && status.required_capabilities.github_get_issue
                && status.required_capabilities.github_create_issue
                && status.required_capabilities.github_comment_on_issue
                && selected_model_ready,
        };
        if config.enabled {
            if config.paused {
                status.last_error = Some("Bug monitor monitoring is paused.".to_string());
            } else if !repo_valid {
                status.last_error = Some("Target repo is missing or invalid.".to_string());
            } else if selected_server.is_none() {
                status.last_error = Some("Selected MCP server is missing.".to_string());
            } else if !status.readiness.mcp_connected {
                status.last_error = Some("Selected MCP server is disconnected.".to_string());
            } else if !selected_model_ready {
                status.last_error = Some(
                    "Selected provider/model is unavailable. Bug monitor is fail-closed."
                        .to_string(),
                );
            } else if !status.readiness.github_read_ready || !status.readiness.github_write_ready {
                let missing = if status.missing_required_capabilities.is_empty() {
                    "unknown".to_string()
                } else {
                    status.missing_required_capabilities.join(", ")
                };
                status.last_error = Some(format!(
                    "Selected MCP server is missing required GitHub capabilities: {missing}"
                ));
            }
        }
        status.runtime.monitoring_active = status.readiness.ingest_ready;
        status
    }

    pub async fn load_workflow_runs(&self) -> anyhow::Result<()> {
        if !self.workflow_runs_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.workflow_runs_path).await?;
        let parsed =
            serde_json::from_str::<std::collections::HashMap<String, WorkflowRunRecord>>(&raw)
                .unwrap_or_default();
        *self.workflow_runs.write().await = parsed;
        Ok(())
    }

    pub async fn persist_workflow_runs(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.workflow_runs_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.workflow_runs.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.workflow_runs_path, payload).await?;
        Ok(())
    }

    pub async fn load_workflow_hook_overrides(&self) -> anyhow::Result<()> {
        if !self.workflow_hook_overrides_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.workflow_hook_overrides_path).await?;
        let parsed = serde_json::from_str::<std::collections::HashMap<String, bool>>(&raw)
            .unwrap_or_default();
        *self.workflow_hook_overrides.write().await = parsed;
        Ok(())
    }

    pub async fn persist_workflow_hook_overrides(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.workflow_hook_overrides_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.workflow_hook_overrides.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.workflow_hook_overrides_path, payload).await?;
        Ok(())
    }

    pub async fn reload_workflows(&self) -> anyhow::Result<Vec<WorkflowValidationMessage>> {
        let mut sources = Vec::new();
        sources.push(WorkflowLoadSource {
            root: config::paths::resolve_builtin_workflows_dir(),
            kind: WorkflowSourceKind::BuiltIn,
            pack_id: None,
        });

        let workspace_root = self.workspace_index.snapshot().await.root;
        sources.push(WorkflowLoadSource {
            root: PathBuf::from(workspace_root).join(".tandem"),
            kind: WorkflowSourceKind::Workspace,
            pack_id: None,
        });

        if let Ok(packs) = self.pack_manager.list().await {
            for pack in packs {
                sources.push(WorkflowLoadSource {
                    root: PathBuf::from(pack.install_path),
                    kind: WorkflowSourceKind::Pack,
                    pack_id: Some(pack.pack_id),
                });
            }
        }

        let mut registry = load_workflow_registry(&sources)?;
        let overrides = self.workflow_hook_overrides.read().await.clone();
        for hook in &mut registry.hooks {
            if let Some(enabled) = overrides.get(&hook.binding_id) {
                hook.enabled = *enabled;
            }
        }
        for workflow in registry.workflows.values_mut() {
            workflow.hooks = registry
                .hooks
                .iter()
                .filter(|hook| hook.workflow_id == workflow.workflow_id)
                .cloned()
                .collect();
        }
        let messages = validate_workflow_registry(&registry);
        *self.workflows.write().await = registry;
        Ok(messages)
    }

    pub async fn workflow_registry(&self) -> WorkflowRegistry {
        self.workflows.read().await.clone()
    }

    pub async fn list_workflows(&self) -> Vec<WorkflowSpec> {
        let mut rows = self
            .workflows
            .read()
            .await
            .workflows
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.workflow_id.cmp(&b.workflow_id));
        rows
    }

    pub async fn get_workflow(&self, workflow_id: &str) -> Option<WorkflowSpec> {
        self.workflows
            .read()
            .await
            .workflows
            .get(workflow_id)
            .cloned()
    }

    pub async fn list_workflow_hooks(&self, workflow_id: Option<&str>) -> Vec<WorkflowHookBinding> {
        let mut rows = self
            .workflows
            .read()
            .await
            .hooks
            .iter()
            .filter(|hook| workflow_id.map(|id| hook.workflow_id == id).unwrap_or(true))
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.binding_id.cmp(&b.binding_id));
        rows
    }

    pub async fn set_workflow_hook_enabled(
        &self,
        binding_id: &str,
        enabled: bool,
    ) -> anyhow::Result<Option<WorkflowHookBinding>> {
        self.workflow_hook_overrides
            .write()
            .await
            .insert(binding_id.to_string(), enabled);
        self.persist_workflow_hook_overrides().await?;
        let _ = self.reload_workflows().await?;
        Ok(self
            .workflows
            .read()
            .await
            .hooks
            .iter()
            .find(|hook| hook.binding_id == binding_id)
            .cloned())
    }

    pub async fn put_workflow_run(&self, run: WorkflowRunRecord) -> anyhow::Result<()> {
        self.workflow_runs
            .write()
            .await
            .insert(run.run_id.clone(), run);
        self.persist_workflow_runs().await
    }

    pub async fn update_workflow_run(
        &self,
        run_id: &str,
        update: impl FnOnce(&mut WorkflowRunRecord),
    ) -> Option<WorkflowRunRecord> {
        let mut guard = self.workflow_runs.write().await;
        let row = guard.get_mut(run_id)?;
        update(row);
        row.updated_at_ms = now_ms();
        if matches!(
            row.status,
            WorkflowRunStatus::Completed | WorkflowRunStatus::Failed
        ) {
            row.finished_at_ms.get_or_insert_with(now_ms);
        }
        let out = row.clone();
        drop(guard);
        let _ = self.persist_workflow_runs().await;
        Some(out)
    }

    pub async fn list_workflow_runs(
        &self,
        workflow_id: Option<&str>,
        limit: usize,
    ) -> Vec<WorkflowRunRecord> {
        let mut rows = self
            .workflow_runs
            .read()
            .await
            .values()
            .filter(|row| workflow_id.map(|id| row.workflow_id == id).unwrap_or(true))
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
        rows.truncate(limit.clamp(1, 500));
        rows
    }

    pub async fn get_workflow_run(&self, run_id: &str) -> Option<WorkflowRunRecord> {
        self.workflow_runs.read().await.get(run_id).cloned()
    }

    pub async fn put_automation_v2(
        &self,
        mut automation: AutomationV2Spec,
    ) -> anyhow::Result<AutomationV2Spec> {
        if automation.automation_id.trim().is_empty() {
            anyhow::bail!("automation_id is required");
        }
        for agent in &mut automation.agents {
            if agent.display_name.trim().is_empty() {
                agent.display_name = auto_generated_agent_name(&agent.agent_id);
            }
            agent.tool_policy.allowlist =
                config::channels::normalize_allowed_tools(agent.tool_policy.allowlist.clone());
            agent.tool_policy.denylist =
                config::channels::normalize_allowed_tools(agent.tool_policy.denylist.clone());
            agent.mcp_policy.allowed_servers =
                normalize_non_empty_list(agent.mcp_policy.allowed_servers.clone());
            agent.mcp_policy.allowed_tools = agent
                .mcp_policy
                .allowed_tools
                .take()
                .map(normalize_allowed_tools);
        }
        let now = now_ms();
        if automation.created_at_ms == 0 {
            automation.created_at_ms = now;
        }
        automation.updated_at_ms = now;
        if automation.next_fire_at_ms.is_none() {
            automation.next_fire_at_ms =
                automation_schedule_next_fire_at_ms(&automation.schedule, now);
        }
        migrate_bundled_studio_research_split_automation(&mut automation);
        canonicalize_automation_output_paths(&mut automation);
        repair_automation_output_contracts(&mut automation);
        let _guard = self.automations_v2_persistence.lock().await;
        self.automations_v2
            .write()
            .await
            .insert(automation.automation_id.clone(), automation.clone());
        self.persist_automations_v2_locked().await?;
        let _ = self
            .sync_automation_governance_from_spec(&automation, None)
            .await;
        self.verify_automation_v2_persisted_locked(&automation.automation_id, true)
            .await?;
        Ok(automation)
    }

    pub async fn get_automation_v2(&self, automation_id: &str) -> Option<AutomationV2Spec> {
        self.automations_v2.read().await.get(automation_id).cloned()
    }

    pub fn automation_v2_runtime_context(
        &self,
        run: &AutomationV2RunRecord,
    ) -> Option<AutomationRuntimeContextMaterialization> {
        run.runtime_context.clone().or_else(|| {
            run.automation_snapshot.as_ref().and_then(|automation| {
                automation
                    .runtime_context_materialization()
                    .or_else(|| automation.approved_plan_runtime_context_materialization())
            })
        })
    }

    fn merge_automation_runtime_context_materializations(
        base: Option<AutomationRuntimeContextMaterialization>,
        extra: Option<AutomationRuntimeContextMaterialization>,
    ) -> Option<AutomationRuntimeContextMaterialization> {
        let mut partitions = std::collections::BTreeMap::<
            String,
            tandem_plan_compiler::api::ProjectedRoutineContextPartition,
        >::new();
        let mut merge_partition =
            |partition: tandem_plan_compiler::api::ProjectedRoutineContextPartition| {
                let entry = partitions
                    .entry(partition.routine_id.clone())
                    .or_insert_with(|| {
                        tandem_plan_compiler::api::ProjectedRoutineContextPartition {
                            routine_id: partition.routine_id.clone(),
                            visible_context_objects: Vec::new(),
                            step_context_bindings: Vec::new(),
                        }
                    });

                let mut seen_context_object_ids = entry
                    .visible_context_objects
                    .iter()
                    .map(|context_object| context_object.context_object_id.clone())
                    .collect::<std::collections::HashSet<_>>();
                for context_object in partition.visible_context_objects {
                    if seen_context_object_ids.insert(context_object.context_object_id.clone()) {
                        entry.visible_context_objects.push(context_object);
                    }
                }
                entry
                    .visible_context_objects
                    .sort_by(|left, right| left.context_object_id.cmp(&right.context_object_id));

                let mut seen_step_ids = entry
                    .step_context_bindings
                    .iter()
                    .map(|binding| binding.step_id.clone())
                    .collect::<std::collections::HashSet<_>>();
                for binding in partition.step_context_bindings {
                    if seen_step_ids.insert(binding.step_id.clone()) {
                        entry.step_context_bindings.push(binding);
                    }
                }
                entry
                    .step_context_bindings
                    .sort_by(|left, right| left.step_id.cmp(&right.step_id));
            };

        if let Some(base) = base {
            for partition in base.routines {
                merge_partition(partition);
            }
        }
        if let Some(extra) = extra {
            for partition in extra.routines {
                merge_partition(partition);
            }
        }
        if partitions.is_empty() {
            None
        } else {
            Some(AutomationRuntimeContextMaterialization {
                routines: partitions.into_values().collect(),
            })
        }
    }

    async fn automation_v2_shared_context_runtime_context(
        &self,
        automation: &AutomationV2Spec,
    ) -> anyhow::Result<Option<AutomationRuntimeContextMaterialization>> {
        let pack_ids = crate::http::context_packs::shared_context_pack_ids_from_metadata(
            automation.metadata.as_ref(),
        );
        if pack_ids.is_empty() {
            return Ok(None);
        }

        let mut contexts = Vec::new();
        for pack_id in pack_ids {
            let Some(pack) = self.get_context_pack(&pack_id).await else {
                anyhow::bail!("shared workflow context not found: {pack_id}");
            };
            if pack.state != crate::http::context_packs::ContextPackState::Published {
                anyhow::bail!("shared workflow context is not published: {pack_id}");
            }
            let pack_context = pack
                .manifest
                .runtime_context
                .clone()
                .and_then(|value| {
                    serde_json::from_value::<AutomationRuntimeContextMaterialization>(value).ok()
                })
                .or_else(|| {
                    pack.manifest
                        .plan_package
                        .as_ref()
                        .and_then(|value| {
                            serde_json::from_value::<tandem_plan_compiler::api::PlanPackage>(
                                value.clone(),
                            )
                            .ok()
                        })
                        .map(|plan_package| {
                            tandem_plan_compiler::api::project_plan_context_materialization(
                                &plan_package,
                            )
                        })
                });
            let Some(pack_context) = pack_context else {
                anyhow::bail!("shared workflow context lacks runtime context: {pack_id}");
            };
            contexts.push(pack_context);
        }

        let mut merged: Option<AutomationRuntimeContextMaterialization> = None;
        for context in contexts {
            merged = Self::merge_automation_runtime_context_materializations(merged, Some(context));
        }
        Ok(merged)
    }

    async fn automation_v2_effective_runtime_context(
        &self,
        automation: &AutomationV2Spec,
        base_runtime_context: Option<AutomationRuntimeContextMaterialization>,
    ) -> anyhow::Result<Option<AutomationRuntimeContextMaterialization>> {
        let shared_context = self
            .automation_v2_shared_context_runtime_context(automation)
            .await?;
        Ok(Self::merge_automation_runtime_context_materializations(
            base_runtime_context,
            shared_context,
        ))
    }

    pub(crate) fn automation_v2_approved_plan_materialization(
        &self,
        run: &AutomationV2RunRecord,
    ) -> Option<tandem_plan_compiler::api::ApprovedPlanMaterialization> {
        run.automation_snapshot
            .as_ref()
            .and_then(AutomationV2Spec::approved_plan_materialization)
    }

    pub async fn put_workflow_plan(&self, plan: WorkflowPlan) {
        self.workflow_plans
            .write()
            .await
            .insert(plan.plan_id.clone(), plan);
    }

    pub async fn get_workflow_plan(&self, plan_id: &str) -> Option<WorkflowPlan> {
        self.workflow_plans.read().await.get(plan_id).cloned()
    }

    pub async fn put_workflow_plan_draft(&self, draft: WorkflowPlanDraftRecord) {
        self.workflow_plan_drafts
            .write()
            .await
            .insert(draft.current_plan.plan_id.clone(), draft.clone());
        self.put_workflow_plan(draft.current_plan).await;
    }

    pub async fn get_workflow_plan_draft(&self, plan_id: &str) -> Option<WorkflowPlanDraftRecord> {
        self.workflow_plan_drafts.read().await.get(plan_id).cloned()
    }

    pub async fn load_workflow_planner_sessions(&self) -> anyhow::Result<()> {
        if !self.workflow_planner_sessions_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.workflow_planner_sessions_path).await?;
        let parsed = serde_json::from_str::<
            std::collections::HashMap<
                String,
                crate::http::workflow_planner::WorkflowPlannerSessionRecord,
            >,
        >(&raw)
        .unwrap_or_default();
        self.replace_workflow_planner_sessions(parsed).await?;
        Ok(())
    }

    pub async fn persist_workflow_planner_sessions(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.workflow_planner_sessions_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.workflow_planner_sessions.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.workflow_planner_sessions_path, payload).await?;
        Ok(())
    }

    async fn replace_workflow_planner_sessions(
        &self,
        sessions: std::collections::HashMap<
            String,
            crate::http::workflow_planner::WorkflowPlannerSessionRecord,
        >,
    ) -> anyhow::Result<()> {
        {
            let mut sessions_guard = self.workflow_planner_sessions.write().await;
            *sessions_guard = sessions.clone();
        }
        {
            let mut plans = self.workflow_plans.write().await;
            let mut drafts = self.workflow_plan_drafts.write().await;
            plans.clear();
            drafts.clear();
            for session in sessions.values() {
                if let Some(draft) = session.draft.as_ref() {
                    plans.insert(
                        draft.current_plan.plan_id.clone(),
                        draft.current_plan.clone(),
                    );
                    drafts.insert(draft.current_plan.plan_id.clone(), draft.clone());
                }
            }
        }
        Ok(())
    }

    async fn sync_workflow_planner_session_cache(
        &self,
        session: &crate::http::workflow_planner::WorkflowPlannerSessionRecord,
    ) {
        if let Some(draft) = session.draft.as_ref() {
            self.workflow_plans.write().await.insert(
                draft.current_plan.plan_id.clone(),
                draft.current_plan.clone(),
            );
            self.workflow_plan_drafts
                .write()
                .await
                .insert(draft.current_plan.plan_id.clone(), draft.clone());
        }
    }

    pub async fn put_workflow_planner_session(
        &self,
        mut session: crate::http::workflow_planner::WorkflowPlannerSessionRecord,
    ) -> anyhow::Result<crate::http::workflow_planner::WorkflowPlannerSessionRecord> {
        if session.session_id.trim().is_empty() {
            anyhow::bail!("session_id is required");
        }
        if session.project_slug.trim().is_empty() {
            anyhow::bail!("project_slug is required");
        }
        let now = now_ms();
        if session.created_at_ms == 0 {
            session.created_at_ms = now;
        }
        session.updated_at_ms = now;
        {
            self.workflow_planner_sessions
                .write()
                .await
                .insert(session.session_id.clone(), session.clone());
        }
        self.sync_workflow_planner_session_cache(&session).await;
        self.persist_workflow_planner_sessions().await?;
        Ok(session)
    }

    pub async fn get_workflow_planner_session(
        &self,
        session_id: &str,
    ) -> Option<crate::http::workflow_planner::WorkflowPlannerSessionRecord> {
        self.workflow_planner_sessions
            .read()
            .await
            .get(session_id)
            .cloned()
    }

    pub async fn list_workflow_planner_sessions(
        &self,
        project_slug: Option<&str>,
    ) -> Vec<crate::http::workflow_planner::WorkflowPlannerSessionRecord> {
        let mut rows = self
            .workflow_planner_sessions
            .read()
            .await
            .values()
            .filter(|session| {
                project_slug
                    .map(|slug| session.project_slug == slug)
                    .unwrap_or(true)
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows
    }

    pub async fn delete_workflow_planner_session(
        &self,
        session_id: &str,
    ) -> Option<crate::http::workflow_planner::WorkflowPlannerSessionRecord> {
        let removed = self
            .workflow_planner_sessions
            .write()
            .await
            .remove(session_id);
        if let Some(session) = removed.as_ref() {
            if let Some(draft) = session.draft.as_ref() {
                self.workflow_plan_drafts
                    .write()
                    .await
                    .remove(&draft.current_plan.plan_id);
                self.workflow_plans
                    .write()
                    .await
                    .remove(&draft.current_plan.plan_id);
            }
        }
        let _ = self.persist_workflow_planner_sessions().await;
        removed
    }

    pub async fn load_workflow_learning_candidates(&self) -> anyhow::Result<()> {
        if !self.workflow_learning_candidates_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.workflow_learning_candidates_path).await?;
        let parsed = serde_json::from_str::<
            std::collections::HashMap<String, WorkflowLearningCandidate>,
        >(&raw)
        .unwrap_or_default();
        *self.workflow_learning_candidates.write().await = parsed;
        Ok(())
    }

    pub async fn persist_workflow_learning_candidates(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.workflow_learning_candidates_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.workflow_learning_candidates.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.workflow_learning_candidates_path, payload).await?;
        Ok(())
    }

    pub async fn get_workflow_learning_candidate(
        &self,
        candidate_id: &str,
    ) -> Option<WorkflowLearningCandidate> {
        self.workflow_learning_candidates
            .read()
            .await
            .get(candidate_id)
            .cloned()
    }

    pub async fn list_workflow_learning_candidates(
        &self,
        workflow_id: Option<&str>,
        status: Option<WorkflowLearningCandidateStatus>,
        kind: Option<WorkflowLearningCandidateKind>,
    ) -> Vec<WorkflowLearningCandidate> {
        let mut rows = self
            .workflow_learning_candidates
            .read()
            .await
            .values()
            .filter(|candidate| {
                workflow_id
                    .map(|value| candidate.workflow_id == value)
                    .unwrap_or(true)
                    && status
                        .map(|value| candidate.status == value)
                        .unwrap_or(true)
                    && kind.map(|value| candidate.kind == value).unwrap_or(true)
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows
    }

    pub async fn put_workflow_learning_candidate(
        &self,
        mut candidate: WorkflowLearningCandidate,
    ) -> anyhow::Result<WorkflowLearningCandidate> {
        if candidate.candidate_id.trim().is_empty() {
            anyhow::bail!("candidate_id is required");
        }
        let now = now_ms();
        if candidate.created_at_ms == 0 {
            candidate.created_at_ms = now;
        }
        candidate.updated_at_ms = now;
        self.workflow_learning_candidates
            .write()
            .await
            .insert(candidate.candidate_id.clone(), candidate.clone());
        self.persist_workflow_learning_candidates().await?;
        Ok(candidate)
    }

    pub async fn upsert_workflow_learning_candidate(
        &self,
        mut candidate: WorkflowLearningCandidate,
    ) -> anyhow::Result<WorkflowLearningCandidate> {
        let now = now_ms();
        if candidate.candidate_id.trim().is_empty() {
            candidate.candidate_id = format!("wflearn-{}", uuid::Uuid::new_v4());
        }
        if candidate.created_at_ms == 0 {
            candidate.created_at_ms = now;
        }
        candidate.updated_at_ms = now;

        let stored = {
            let mut guard = self.workflow_learning_candidates.write().await;
            if let Some(existing) = guard.values_mut().find(|row| {
                row.workflow_id == candidate.workflow_id
                    && row.kind == candidate.kind
                    && row.fingerprint == candidate.fingerprint
            }) {
                existing.summary = candidate.summary.clone();
                existing.confidence = existing.confidence.max(candidate.confidence);
                existing.updated_at_ms = now;
                if existing.node_id.is_none() {
                    existing.node_id = candidate.node_id.clone();
                }
                if existing.node_kind.is_none() {
                    existing.node_kind = candidate.node_kind.clone();
                }
                if existing.validator_family.is_none() {
                    existing.validator_family = candidate.validator_family.clone();
                }
                if existing.proposed_memory_payload.is_none() {
                    existing.proposed_memory_payload = candidate.proposed_memory_payload.clone();
                }
                if existing.proposed_revision_prompt.is_none() {
                    existing.proposed_revision_prompt = candidate.proposed_revision_prompt.clone();
                }
                if existing.source_memory_id.is_none() {
                    existing.source_memory_id = candidate.source_memory_id.clone();
                }
                if existing.promoted_memory_id.is_none() {
                    existing.promoted_memory_id = candidate.promoted_memory_id.clone();
                }
                if existing.baseline_before.is_none() {
                    existing.baseline_before = candidate.baseline_before.clone();
                }
                if candidate.latest_observed_metrics.is_some() {
                    existing.latest_observed_metrics = candidate.latest_observed_metrics.clone();
                }
                if candidate.last_revision_session_id.is_some() {
                    existing.last_revision_session_id = candidate.last_revision_session_id.clone();
                }
                existing.needs_plan_bundle |= candidate.needs_plan_bundle;
                for artifact_ref in candidate.artifact_refs {
                    if !existing
                        .artifact_refs
                        .iter()
                        .any(|value| value == &artifact_ref)
                    {
                        existing.artifact_refs.push(artifact_ref);
                    }
                }
                for run_id in candidate.run_ids {
                    if !existing.run_ids.iter().any(|value| value == &run_id) {
                        existing.run_ids.push(run_id);
                    }
                }
                for evidence_ref in candidate.evidence_refs {
                    if !existing.evidence_refs.contains(&evidence_ref) {
                        existing.evidence_refs.push(evidence_ref);
                    }
                }
                existing.clone()
            } else {
                guard.insert(candidate.candidate_id.clone(), candidate.clone());
                candidate
            }
        };
        self.persist_workflow_learning_candidates().await?;
        Ok(stored)
    }

    pub async fn update_workflow_learning_candidate(
        &self,
        candidate_id: &str,
        update: impl FnOnce(&mut WorkflowLearningCandidate),
    ) -> Option<WorkflowLearningCandidate> {
        let updated = {
            let mut guard = self.workflow_learning_candidates.write().await;
            let candidate = guard.get_mut(candidate_id)?;
            update(candidate);
            candidate.updated_at_ms = now_ms();
            candidate.clone()
        };
        let _ = self.persist_workflow_learning_candidates().await;
        Some(updated)
    }

    pub async fn workflow_learning_metrics_for_workflow(
        &self,
        workflow_id: &str,
    ) -> WorkflowLearningMetricsSnapshot {
        let runs = self.list_automation_v2_runs(Some(workflow_id), 50).await;
        crate::app::state::automation::workflow_learning_metrics_snapshot(&runs)
    }

    pub async fn workflow_learning_context_for_automation_node(
        &self,
        automation: &AutomationV2Spec,
        node: &AutomationFlowNode,
    ) -> (Vec<String>, Option<String>) {
        let project_id = crate::app::state::automation::workflow_learning_project_id(automation);
        let node_kind = node
            .stage_kind
            .as_ref()
            .map(|kind| format!("{kind:?}").to_ascii_lowercase());
        let validator_family = node
            .output_contract
            .as_ref()
            .and_then(|contract| contract.validator.as_ref())
            .map(|validator| format!("{validator:?}").to_ascii_lowercase());
        let candidates = self
            .workflow_learning_candidates
            .read()
            .await
            .values()
            .filter(|candidate| {
                matches!(
                    candidate.status,
                    WorkflowLearningCandidateStatus::Approved
                        | WorkflowLearningCandidateStatus::Applied
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut ordered = Vec::new();
        let mut push_unique = |candidate: WorkflowLearningCandidate| {
            if ordered.iter().any(|existing: &WorkflowLearningCandidate| {
                existing.candidate_id == candidate.candidate_id
            }) {
                return;
            }
            ordered.push(candidate);
        };
        for candidate in candidates
            .iter()
            .filter(|candidate| candidate.workflow_id == automation.automation_id)
        {
            push_unique(candidate.clone());
        }
        for candidate in candidates.iter().filter(|candidate| {
            candidate.workflow_id == automation.automation_id
                && (candidate.node_kind.as_deref() == node_kind.as_deref()
                    || candidate.validator_family.as_deref() == validator_family.as_deref())
        }) {
            push_unique(candidate.clone());
        }
        for candidate in candidates.iter().filter(|candidate| {
            candidate.project_id == project_id && candidate.workflow_id != automation.automation_id
        }) {
            push_unique(candidate.clone());
        }
        ordered.truncate(6);
        let candidate_ids = ordered
            .iter()
            .map(|candidate| candidate.candidate_id.clone())
            .collect::<Vec<_>>();
        let context =
            crate::app::state::automation::workflow_learning_context_for_candidates(&ordered);
        (candidate_ids, context)
    }

    pub async fn record_automation_v2_run_learning_usage(
        &self,
        run_id: &str,
        candidate_ids: &[String],
    ) -> Option<AutomationV2RunRecord> {
        if candidate_ids.is_empty() {
            return self.get_automation_v2_run(run_id).await;
        }
        let updated = {
            let mut guard = self.automation_v2_runs.write().await;
            let run = guard.get_mut(run_id)?;
            let summary = run
                .learning_summary
                .get_or_insert_with(WorkflowLearningRunSummary::default);
            for candidate_id in candidate_ids {
                if !summary
                    .approved_learning_ids_considered
                    .iter()
                    .any(|value| value == candidate_id)
                {
                    summary
                        .approved_learning_ids_considered
                        .push(candidate_id.clone());
                }
                if !summary
                    .injected_learning_ids
                    .iter()
                    .any(|value| value == candidate_id)
                {
                    summary.injected_learning_ids.push(candidate_id.clone());
                }
            }
            run.updated_at_ms = now_ms();
            run.clone()
        };
        let _ = self.persist_automation_v2_runs().await;
        let _ = self.persist_automation_v2_run_status_json(&updated).await;
        Some(updated)
    }

    async fn finalize_terminal_automation_v2_run_learning(
        &self,
        run: &AutomationV2RunRecord,
    ) -> anyhow::Result<()> {
        const WORKFLOW_LEARNING_POST_APPLY_MIN_SAMPLE_SIZE: usize = 3;
        let automation = if let Some(snapshot) = run.automation_snapshot.clone() {
            snapshot
        } else if let Some(current) = self.get_automation_v2(&run.automation_id).await {
            current
        } else {
            return Ok(());
        };
        let recent_runs = self
            .list_automation_v2_runs(Some(&run.automation_id), 50)
            .await;
        let metrics =
            crate::app::state::automation::workflow_learning_metrics_snapshot(&recent_runs);
        let existing_candidates = self
            .list_workflow_learning_candidates(Some(&run.automation_id), None, None)
            .await;
        let generated =
            crate::app::state::automation::workflow_learning_candidates_for_terminal_run(
                &automation,
                run,
                &recent_runs,
                &existing_candidates,
            );
        let mut generated_candidate_ids = Vec::new();
        for candidate in generated {
            let stored = self.upsert_workflow_learning_candidate(candidate).await?;
            generated_candidate_ids.push(stored.candidate_id);
        }
        let candidate_ids = self
            .list_workflow_learning_candidates(Some(&run.automation_id), None, None)
            .await
            .into_iter()
            .filter(|candidate| {
                matches!(
                    candidate.status,
                    WorkflowLearningCandidateStatus::Approved
                        | WorkflowLearningCandidateStatus::Applied
                ) && candidate.baseline_before.is_some()
            })
            .map(|candidate| candidate.candidate_id)
            .collect::<Vec<_>>();
        for candidate_id in candidate_ids {
            let _ = self
                .update_workflow_learning_candidate(&candidate_id, |candidate| {
                    candidate.latest_observed_metrics = Some(metrics.clone());
                    if candidate.status == WorkflowLearningCandidateStatus::Applied {
                        if let Some(baseline) = candidate.baseline_before.as_ref() {
                            let post_change_sample_size =
                                metrics.sample_size.saturating_sub(baseline.sample_size);
                            if post_change_sample_size
                                < WORKFLOW_LEARNING_POST_APPLY_MIN_SAMPLE_SIZE
                            {
                                return;
                            }
                            if metrics.completion_rate + f64::EPSILON < baseline.completion_rate
                                || metrics.validation_pass_rate + f64::EPSILON
                                    < baseline.validation_pass_rate
                            {
                                candidate.status = WorkflowLearningCandidateStatus::Regressed;
                            }
                        }
                    }
                })
                .await;
        }
        let updated_run = {
            let mut guard = self.automation_v2_runs.write().await;
            let Some(stored_run) = guard.get_mut(&run.run_id) else {
                return Ok(());
            };
            let summary = stored_run
                .learning_summary
                .get_or_insert_with(WorkflowLearningRunSummary::default);
            for candidate_id in generated_candidate_ids {
                if !summary
                    .generated_candidate_ids
                    .iter()
                    .any(|value| value == &candidate_id)
                {
                    summary.generated_candidate_ids.push(candidate_id);
                }
            }
            summary.post_run_metrics = Some(metrics);
            stored_run.clone()
        };
        self.persist_automation_v2_runs().await?;
        self.persist_automation_v2_run_status_json(&updated_run)
            .await?;
        Ok(())
    }

    pub async fn load_context_packs(&self) -> anyhow::Result<()> {
        if !self.context_packs_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.context_packs_path).await?;
        let parsed = serde_json::from_str::<
            std::collections::HashMap<String, crate::http::context_packs::ContextPackRecord>,
        >(&raw)
        .unwrap_or_default();
        {
            let mut guard = self.context_packs.write().await;
            *guard = parsed;
        }
        Ok(())
    }

    pub async fn persist_context_packs(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.context_packs_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.context_packs.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.context_packs_path, payload).await?;
        Ok(())
    }

    pub(crate) async fn put_context_pack(
        &self,
        mut pack: crate::http::context_packs::ContextPackRecord,
    ) -> anyhow::Result<crate::http::context_packs::ContextPackRecord> {
        if pack.pack_id.trim().is_empty() {
            anyhow::bail!("pack_id is required");
        }
        if pack.title.trim().is_empty() {
            anyhow::bail!("title is required");
        }
        if pack.workspace_root.trim().is_empty() {
            anyhow::bail!("workspace_root is required");
        }
        let now = now_ms();
        if pack.created_at_ms == 0 {
            pack.created_at_ms = now;
        }
        pack.updated_at_ms = now;
        {
            self.context_packs
                .write()
                .await
                .insert(pack.pack_id.clone(), pack.clone());
        }
        self.persist_context_packs().await?;
        Ok(pack)
    }

    pub(crate) async fn get_context_pack(
        &self,
        pack_id: &str,
    ) -> Option<crate::http::context_packs::ContextPackRecord> {
        self.context_packs.read().await.get(pack_id).cloned()
    }

    pub(crate) async fn list_context_packs(
        &self,
        project_key: Option<&str>,
        workspace_root: Option<&str>,
    ) -> Vec<crate::http::context_packs::ContextPackRecord> {
        let mut rows = self
            .context_packs
            .read()
            .await
            .values()
            .filter(|pack| {
                let project_ok =
                    crate::http::context_packs::context_pack_allows_project(pack, project_key);
                let workspace_ok = workspace_root
                    .map(|root| pack.workspace_root == root)
                    .unwrap_or(true);
                project_ok && workspace_ok
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows
    }

    pub(crate) async fn update_context_pack(
        &self,
        pack_id: &str,
        update: impl FnOnce(&mut crate::http::context_packs::ContextPackRecord) -> anyhow::Result<()>,
    ) -> anyhow::Result<crate::http::context_packs::ContextPackRecord> {
        let mut guard = self.context_packs.write().await;
        let Some(pack) = guard.get_mut(pack_id) else {
            anyhow::bail!("shared workflow context not found");
        };
        update(pack)?;
        pack.updated_at_ms = now_ms();
        let next = pack.clone();
        drop(guard);
        self.persist_context_packs().await?;
        Ok(next)
    }

    pub(crate) async fn revoke_context_pack(
        &self,
        pack_id: &str,
        revoked_actor_metadata: Option<Value>,
    ) -> anyhow::Result<crate::http::context_packs::ContextPackRecord> {
        self.update_context_pack(pack_id, move |pack| {
            pack.state = crate::http::context_packs::ContextPackState::Revoked;
            pack.revoked_at_ms = Some(now_ms());
            pack.revoked_actor_metadata = revoked_actor_metadata;
            Ok(())
        })
        .await
    }

    pub(crate) async fn supersede_context_pack(
        &self,
        pack_id: &str,
        superseded_by_pack_id: String,
        superseded_actor_metadata: Option<Value>,
    ) -> anyhow::Result<crate::http::context_packs::ContextPackRecord> {
        self.update_context_pack(pack_id, move |pack| {
            pack.state = crate::http::context_packs::ContextPackState::Superseded;
            pack.superseded_by_pack_id = Some(superseded_by_pack_id);
            pack.superseded_at_ms = Some(now_ms());
            pack.superseded_actor_metadata = superseded_actor_metadata;
            Ok(())
        })
        .await
    }

    pub(crate) async fn bind_context_pack(
        &self,
        pack_id: &str,
        binding: crate::http::context_packs::ContextPackBindingRecord,
    ) -> anyhow::Result<crate::http::context_packs::ContextPackRecord> {
        self.update_context_pack(pack_id, move |pack| {
            pack.bindings
                .retain(|row| row.binding_id != binding.binding_id);
            pack.bindings.push(binding);
            Ok(())
        })
        .await
    }

    pub async fn put_optimization_campaign(
        &self,
        mut campaign: OptimizationCampaignRecord,
    ) -> anyhow::Result<OptimizationCampaignRecord> {
        if campaign.optimization_id.trim().is_empty() {
            anyhow::bail!("optimization_id is required");
        }
        if campaign.source_workflow_id.trim().is_empty() {
            anyhow::bail!("source_workflow_id is required");
        }
        if campaign.name.trim().is_empty() {
            anyhow::bail!("name is required");
        }
        let now = now_ms();
        if campaign.created_at_ms == 0 {
            campaign.created_at_ms = now;
        }
        campaign.updated_at_ms = now;
        campaign.source_workflow_snapshot_hash =
            optimization_snapshot_hash(&campaign.source_workflow_snapshot);
        campaign.baseline_snapshot_hash = optimization_snapshot_hash(&campaign.baseline_snapshot);
        self.optimization_campaigns
            .write()
            .await
            .insert(campaign.optimization_id.clone(), campaign.clone());
        self.persist_optimization_campaigns().await?;
        Ok(campaign)
    }

    pub async fn get_optimization_campaign(
        &self,
        optimization_id: &str,
    ) -> Option<OptimizationCampaignRecord> {
        self.optimization_campaigns
            .read()
            .await
            .get(optimization_id)
            .cloned()
    }

    pub async fn list_optimization_campaigns(&self) -> Vec<OptimizationCampaignRecord> {
        let mut rows = self
            .optimization_campaigns
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows
    }

    pub async fn put_optimization_experiment(
        &self,
        mut experiment: OptimizationExperimentRecord,
    ) -> anyhow::Result<OptimizationExperimentRecord> {
        if experiment.experiment_id.trim().is_empty() {
            anyhow::bail!("experiment_id is required");
        }
        if experiment.optimization_id.trim().is_empty() {
            anyhow::bail!("optimization_id is required");
        }
        let now = now_ms();
        if experiment.created_at_ms == 0 {
            experiment.created_at_ms = now;
        }
        experiment.updated_at_ms = now;
        experiment.candidate_snapshot_hash =
            optimization_snapshot_hash(&experiment.candidate_snapshot);
        self.optimization_experiments
            .write()
            .await
            .insert(experiment.experiment_id.clone(), experiment.clone());
        self.persist_optimization_experiments().await?;
        Ok(experiment)
    }

    pub async fn get_optimization_experiment(
        &self,
        optimization_id: &str,
        experiment_id: &str,
    ) -> Option<OptimizationExperimentRecord> {
        self.optimization_experiments
            .read()
            .await
            .get(experiment_id)
            .filter(|row| row.optimization_id == optimization_id)
            .cloned()
    }

    pub async fn list_optimization_experiments(
        &self,
        optimization_id: &str,
    ) -> Vec<OptimizationExperimentRecord> {
        let mut rows = self
            .optimization_experiments
            .read()
            .await
            .values()
            .filter(|row| row.optimization_id == optimization_id)
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows
    }

    pub async fn count_optimization_experiments(&self, optimization_id: &str) -> usize {
        self.optimization_experiments
            .read()
            .await
            .values()
            .filter(|row| row.optimization_id == optimization_id)
            .count()
    }

    fn automation_run_is_terminal(status: &crate::AutomationRunStatus) -> bool {
        matches!(
            status,
            crate::AutomationRunStatus::Completed
                | crate::AutomationRunStatus::Blocked
                | crate::AutomationRunStatus::Failed
                | crate::AutomationRunStatus::Cancelled
        )
    }

    fn optimization_consecutive_failure_count(
        experiments: &[OptimizationExperimentRecord],
    ) -> usize {
        let mut ordered = experiments.to_vec();
        ordered.sort_by(|a, b| a.created_at_ms.cmp(&b.created_at_ms));
        ordered
            .iter()
            .rev()
            .take_while(|experiment| experiment.status == OptimizationExperimentStatus::Failed)
            .count()
    }

    fn optimization_mutation_field_path(field: OptimizationMutableField) -> &'static str {
        match field {
            OptimizationMutableField::Objective => "objective",
            OptimizationMutableField::OutputContractSummaryGuidance => {
                "output_contract.summary_guidance"
            }
            OptimizationMutableField::TimeoutMs => "timeout_ms",
            OptimizationMutableField::RetryPolicyMaxAttempts => "retry_policy.max_attempts",
            OptimizationMutableField::RetryPolicyRetries => "retry_policy.retries",
        }
    }

    fn optimization_node_field_value(
        node: &crate::AutomationFlowNode,
        field: OptimizationMutableField,
    ) -> Result<Value, String> {
        match field {
            OptimizationMutableField::Objective => Ok(Value::String(node.objective.clone())),
            OptimizationMutableField::OutputContractSummaryGuidance => node
                .output_contract
                .as_ref()
                .and_then(|contract| contract.summary_guidance.clone())
                .map(Value::String)
                .ok_or_else(|| {
                    format!(
                        "node `{}` is missing output_contract.summary_guidance",
                        node.node_id
                    )
                }),
            OptimizationMutableField::TimeoutMs => node
                .timeout_ms
                .map(|value| json!(value))
                .ok_or_else(|| format!("node `{}` is missing timeout_ms", node.node_id)),
            OptimizationMutableField::RetryPolicyMaxAttempts => node
                .retry_policy
                .as_ref()
                .and_then(Value::as_object)
                .and_then(|policy| policy.get("max_attempts"))
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "node `{}` is missing retry_policy.max_attempts",
                        node.node_id
                    )
                }),
            OptimizationMutableField::RetryPolicyRetries => node
                .retry_policy
                .as_ref()
                .and_then(Value::as_object)
                .and_then(|policy| policy.get("retries"))
                .cloned()
                .ok_or_else(|| format!("node `{}` is missing retry_policy.retries", node.node_id)),
        }
    }

    fn set_optimization_node_field_value(
        node: &mut crate::AutomationFlowNode,
        field: OptimizationMutableField,
        value: &Value,
    ) -> Result<(), String> {
        match field {
            OptimizationMutableField::Objective => {
                node.objective = value
                    .as_str()
                    .ok_or_else(|| "objective apply value must be a string".to_string())?
                    .to_string();
            }
            OptimizationMutableField::OutputContractSummaryGuidance => {
                let guidance = value
                    .as_str()
                    .ok_or_else(|| {
                        "output_contract.summary_guidance apply value must be a string".to_string()
                    })?
                    .to_string();
                let contract = node.output_contract.as_mut().ok_or_else(|| {
                    format!(
                        "node `{}` is missing output_contract for apply",
                        node.node_id
                    )
                })?;
                contract.summary_guidance = Some(guidance);
            }
            OptimizationMutableField::TimeoutMs => {
                node.timeout_ms = Some(
                    value
                        .as_u64()
                        .ok_or_else(|| "timeout_ms apply value must be an integer".to_string())?,
                );
            }
            OptimizationMutableField::RetryPolicyMaxAttempts => {
                let next = value.as_i64().ok_or_else(|| {
                    "retry_policy.max_attempts apply value must be an integer".to_string()
                })?;
                let policy = node.retry_policy.get_or_insert_with(|| json!({}));
                let object = policy.as_object_mut().ok_or_else(|| {
                    format!("node `{}` retry_policy must be a JSON object", node.node_id)
                })?;
                object.insert("max_attempts".to_string(), json!(next));
            }
            OptimizationMutableField::RetryPolicyRetries => {
                let next = value.as_i64().ok_or_else(|| {
                    "retry_policy.retries apply value must be an integer".to_string()
                })?;
                let policy = node.retry_policy.get_or_insert_with(|| json!({}));
                let object = policy.as_object_mut().ok_or_else(|| {
                    format!("node `{}` retry_policy must be a JSON object", node.node_id)
                })?;
                object.insert("retries".to_string(), json!(next));
            }
        }
        Ok(())
    }

    fn append_optimization_apply_metadata(
        metadata: Option<Value>,
        record: Value,
    ) -> Result<Option<Value>, String> {
        let mut root = match metadata {
            Some(Value::Object(map)) => map,
            Some(_) => return Err("automation metadata must be a JSON object".to_string()),
            None => serde_json::Map::new(),
        };
        let history = root
            .entry("optimization_apply_history".to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        let Some(entries) = history.as_array_mut() else {
            return Err("optimization_apply_history metadata must be an array".to_string());
        };
        entries.push(record.clone());
        root.insert("last_optimization_apply".to_string(), record);
        Ok(Some(Value::Object(root)))
    }
}
