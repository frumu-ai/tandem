use super::*;

impl EngineLoop {
    pub(super) async fn execute_tool_with_timeout(
        &self,
        tool: &str,
        args: Value,
        cancel: CancellationToken,
        progress: Option<SharedToolProgressSink>,
    ) -> anyhow::Result<tandem_types::ToolResult> {
        let timeout_ms = tool_exec_timeout_ms() as u64;
        match tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            self.tools
                .execute_with_cancel_and_progress(tool, args, cancel, progress),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => anyhow::bail!("TOOL_EXEC_TIMEOUT_MS_EXCEEDED({timeout_ms})"),
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
            let session = self.storage.get_session(session_id).await?;
            let workspace = session
                .workspace_root
                .or_else(|| crate::normalize_workspace_path(&session.directory))?;
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
                "Sandbox blocked MCP tool `{tool}` path `{outside}` (workspace root: `{workspace}`)"
            ));
        }
        let session = self.storage.get_session(session_id).await?;
        let workspace = session
            .workspace_root
            .or_else(|| crate::normalize_workspace_path(&session.directory))?;
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
            "Sandbox blocked `{tool}` path `{outside}` (workspace root: `{workspace}`)"
        ))
    }

    pub(super) async fn resolve_tool_execution_context(
        &self,
        session_id: &str,
    ) -> Option<(String, String, Option<String>)> {
        let session = self.storage.get_session(session_id).await?;
        let workspace_root = session
            .workspace_root
            .or_else(|| crate::normalize_workspace_path(&session.directory))?;
        let effective_cwd = if session.directory.trim().is_empty()
            || session.directory.trim() == "."
        {
            workspace_root.clone()
        } else {
            crate::normalize_workspace_path(&session.directory).unwrap_or(workspace_root.clone())
        };
        let project_id = session
            .project_id
            .clone()
            .or_else(|| crate::workspace_project_id(&workspace_root));
        Some((workspace_root, effective_cwd, project_id))
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
        active_agent: &AgentDefinition,
        provider_hint: Option<&str>,
        model_id: Option<&str>,
        cancel: CancellationToken,
        tool_outputs: &[String],
    ) -> Option<String> {
        if cancel.is_cancelled() {
            return None;
        }
        let mut messages = load_chat_history(
            self.storage.clone(),
            session_id,
            ChatHistoryProfile::Standard,
        )
        .await;
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
        let stream = self
            .providers
            .stream_for_provider(
                provider_hint,
                model_id,
                messages,
                ToolMode::None,
                None,
                cancel.clone(),
            )
            .await
            .ok()?;
        tokio::pin!(stream);
        let mut completion = String::new();
        while let Some(chunk) = stream.next().await {
            if cancel.is_cancelled() {
                return None;
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
                Err(_) => return None,
            }
        }
        let completion = truncate_text(&strip_model_control_markers(&completion), 16_000);
        if completion.trim().is_empty() {
            None
        } else {
            Some(completion)
        }
    }
}
