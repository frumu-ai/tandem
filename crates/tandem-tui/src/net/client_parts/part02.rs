impl EngineClient {
    pub fn new(base_url: String) -> Self {
        Self::new_with_token(base_url, None)
    }

    pub fn new_with_token(base_url: String, api_token: Option<String>) -> Self {
        let mut headers = HeaderMap::new();
        if let Some(token) = api_token
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            if let Ok(value) = HeaderValue::from_str(token) {
                headers.insert("x-tandem-token", value);
            }
        }
        let client = Client::builder()
            .default_headers(headers)
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            base_url,
            client,
            api_key: None,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn check_health(&self) -> Result<bool> {
        let url = format!("{}/global/health", self.base_url);
        let resp = self.client.get(&url).send().await?;
        Ok(resp.status().is_success())
    }

    pub async fn get_engine_status(&self) -> Result<EngineStatus> {
        let url = format!("{}/global/health", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let status = resp.json::<EngineStatus>().await?;
        Ok(status)
    }

    pub async fn get_browser_status(&self) -> Result<BrowserStatusResponse> {
        let url = format!("{}/browser/status", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let status = resp.json::<BrowserStatusResponse>().await?;
        Ok(status)
    }

    pub async fn acquire_lease(
        &self,
        client_id: &str,
        client_type: &str,
        ttl_ms: Option<u64>,
    ) -> Result<EngineLease> {
        let url = format!("{}/global/lease/acquire", self.base_url);
        let payload = serde_json::json!({
            "client_id": client_id,
            "client_type": client_type,
            "ttl_ms": ttl_ms.unwrap_or(60_000),
        });
        let resp = self.client.post(&url).json(&payload).send().await?;
        let lease = resp.json::<EngineLease>().await?;
        Ok(lease)
    }

    pub async fn renew_lease(&self, lease_id: &str) -> Result<bool> {
        let url = format!("{}/global/lease/renew", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "lease_id": lease_id }))
            .send()
            .await?;
        let body = resp.json::<serde_json::Value>().await?;
        Ok(body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn release_lease(&self, lease_id: &str) -> Result<bool> {
        let url = format!("{}/global/lease/release", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "lease_id": lease_id }))
            .send()
            .await?;
        let body = resp.json::<serde_json::Value>().await?;
        Ok(body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn list_sessions(&self) -> Result<Vec<Session>> {
        let workspace = std::env::current_dir()
            .ok()
            .and_then(|p| normalize_workspace_path(&p));
        self.list_sessions_scoped(SessionScope::Workspace, workspace)
            .await
    }

    pub async fn list_sessions_scoped(
        &self,
        scope: SessionScope,
        workspace: Option<String>,
    ) -> Result<Vec<Session>> {
        let url = format!("{}/api/session", self.base_url);
        let scope_value = match scope {
            SessionScope::Workspace => "workspace",
            SessionScope::Global => "global",
        };
        let mut req = self.client.get(&url).query(&[("scope", scope_value)]);
        if matches!(scope, SessionScope::Workspace) {
            if let Some(workspace) = workspace {
                req = req.query(&[("workspace", workspace)]);
            }
        }
        let resp = req.send().await?;
        let sessions = resp.json::<Vec<Session>>().await?;
        Ok(sessions)
    }

    pub async fn create_session(&self, title: Option<String>) -> Result<Session> {
        let url = format!("{}/api/session", self.base_url);
        let req = CreateSessionRequest {
            parent_id: None,
            title,
            directory: std::env::current_dir()
                .ok()
                .and_then(|p| normalize_workspace_path(&p)),
            workspace_root: std::env::current_dir()
                .ok()
                .and_then(|p| normalize_workspace_path(&p)),
            project_id: None,
            model: None,
            provider: None,
            permission: Some(default_tui_permission_rules()),
        };

        let resp = match send_with_engine_retry(|| self.client.post(&url).json(&req)).await? {
            EngineRetryOutcome::Response(resp) => resp,
            EngineRetryOutcome::ErrorStatus(status, body) => {
                bail!("{}: {}", status, body);
            }
        };
        let session = resp.json::<Session>().await?;
        Ok(session)
    }

    pub async fn get_session(&self, session_id: &str) -> Result<Session> {
        let url = format!("{}/session/{}", self.base_url, session_id);
        let resp = self.client.get(&url).send().await?;
        let session = resp.json::<Session>().await?;
        Ok(session)
    }

    pub async fn get_session_messages(&self, session_id: &str) -> Result<Vec<WireSessionMessage>> {
        let url = format!("{}/session/{}/message", self.base_url, session_id);
        let resp = self.client.get(&url).send().await?;
        let messages = resp.json::<Vec<WireSessionMessage>>().await?;
        Ok(messages)
    }

    pub async fn update_session(
        &self,
        session_id: &str,
        req: UpdateSessionRequest,
    ) -> Result<Session> {
        let url = format!("{}/session/{}", self.base_url, session_id);
        let resp = self.client.patch(&url).json(&req).send().await?;
        let session = resp.json::<Session>().await?;
        Ok(session)
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        let url = format!("{}/session/{}", self.base_url, session_id);
        self.client.delete(&url).send().await?;
        Ok(())
    }

    pub async fn list_providers(&self) -> Result<ProviderCatalog> {
        let url = format!("{}/provider", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let catalog = resp.json::<ProviderCatalog>().await?;
        Ok(catalog)
    }

    pub async fn config_providers(&self) -> Result<ConfigProvidersResponse> {
        let url = format!("{}/config/providers", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let config = resp.json::<ConfigProvidersResponse>().await?;
        Ok(config)
    }

    pub async fn set_auth(&self, provider_id: &str, api_key: &str) -> Result<()> {
        let url = format!("{}/auth/{}", self.base_url, provider_id);
        self.client
            .put(&url)
            .json(&serde_json::json!({ "apiKey": api_key }))
            .send()
            .await?;
        Ok(())
    }

    pub async fn delete_auth(&self, provider_id: &str) -> Result<()> {
        let url = format!("{}/auth/{}", self.base_url, provider_id);
        self.client.delete(&url).send().await?;
        Ok(())
    }

    pub async fn list_permissions(&self) -> Result<PermissionSnapshot> {
        let url = format!("{}/permission", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let snapshot = resp.json::<PermissionSnapshot>().await?;
        Ok(snapshot)
    }

    pub async fn reply_permission(&self, id: &str, reply: &str) -> Result<bool> {
        let url = format!("{}/permission/{}/reply", self.base_url, id);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "reply": reply }))
            .send()
            .await?;
        let body = resp.json::<serde_json::Value>().await?;
        Ok(body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn list_questions(&self) -> Result<Vec<QuestionRequest>> {
        let url = format!("{}/question", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let snapshot = resp.json::<Vec<QuestionRequest>>().await?;
        Ok(snapshot)
    }

    pub async fn reply_question(&self, id: &str, answers: Vec<Vec<String>>) -> Result<bool> {
        let url = format!("{}/question/{}/reply", self.base_url, id);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "answers": answers }))
            .send()
            .await?;
        let body = resp.json::<serde_json::Value>().await?;
        Ok(body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn reject_question(&self, id: &str) -> Result<bool> {
        let url = format!("{}/question/{}/reject", self.base_url, id);
        let resp = self.client.post(&url).send().await?;
        let body = resp.json::<serde_json::Value>().await?;
        Ok(body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn send_prompt(
        &self,
        session_id: &str,
        message: &str,
        agent: Option<&str>,
        model: Option<ModelSpec>,
    ) -> Result<Vec<WireSessionMessage>> {
        let result = self
            .send_prompt_with_stream(session_id, message, agent, model, |_| {})
            .await?;
        Ok(result.messages)
    }

    pub async fn send_prompt_with_stream<F>(
        &self,
        session_id: &str,
        message: &str,
        agent: Option<&str>,
        model: Option<ModelSpec>,
        mut on_delta: F,
    ) -> Result<PromptRunResult>
    where
        F: FnMut(String),
    {
        self.send_prompt_with_stream_events(session_id, message, agent, None, model, |event| {
            if let Some(delta) = extract_delta_text(&event.payload) {
                if !delta.is_empty() {
                    on_delta(delta);
                }
            }
        })
        .await
    }

    pub async fn send_prompt_with_stream_events<F>(
        &self,
        session_id: &str,
        message: &str,
        agent: Option<&str>,
        agent_id: Option<&str>,
        model: Option<ModelSpec>,
        mut on_event: F,
    ) -> Result<PromptRunResult>
    where
        F: FnMut(StreamEventEnvelope),
    {
        let append_url = format!(
            "{}/session/{}/message?mode=append",
            self.base_url, session_id
        );
        let prompt_url = format!("{}/session/{}/prompt_sync", self.base_url, session_id);
        let req = SendMessageRequest {
            parts: vec![MessagePartInput::Text {
                text: message.to_string(),
            }],
            model,
            agent: agent.map(String::from),
        };
        match send_with_engine_retry(|| self.client.post(&append_url).json(&req)).await? {
            EngineRetryOutcome::Response(_) => {}
            EngineRetryOutcome::ErrorStatus(status, body) => {
                bail!("append failed {}: {}", status, body);
            }
        }
        let resp = match send_with_engine_retry(|| {
            let mut prompt_req = self
                .client
                .post(&prompt_url)
                .header("Accept", "text/event-stream");
            if let Some(agent_id) = agent_id {
                prompt_req = prompt_req.header("x-tandem-agent-id", agent_id);
            }
            prompt_req.json(&req)
        })
        .await?
        {
            EngineRetryOutcome::Response(resp) => resp,
            EngineRetryOutcome::ErrorStatus(status, body)
                if status == reqwest::StatusCode::CONFLICT =>
            {
                let run_id = serde_json::from_str::<PromptConflictResponse>(&body)
                    .ok()
                    .and_then(|payload| {
                        if payload.code.as_deref() == Some("SESSION_RUN_CONFLICT") {
                            payload.active_run.and_then(|run| run.run_id)
                        } else {
                            None
                        }
                    });
                if let Some(run_id) = run_id {
                    bail!(
                        "409 Conflict: session has active run `{}`. Queue follow-up or cancel first.",
                        run_id
                    );
                }
                bail!("409 Conflict: {}", body);
            }
            EngineRetryOutcome::ErrorStatus(status, body) => {
                bail!("{}: {}", status, body);
            }
        };
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await?;
            bail!("{}: {}", status, body);
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if content_type.starts_with("text/event-stream") {
            let mut stream = resp.bytes_stream();
            let mut streamed = false;
            let mut buffer = String::new();
            while let Some(chunk) =
                tokio::time::timeout(Duration::from_secs(90), stream.next()).await?
            {
                let chunk = chunk?;
                let text = String::from_utf8_lossy(&chunk);
                buffer.push_str(&text);
                while let Some(payload) = parse_sse_payload(&mut buffer) {
                    if let Some(event) = parse_stream_event_envelope(payload) {
                        if extract_delta_text(&event.payload)
                            .map(|d| !d.trim().is_empty())
                            .unwrap_or(false)
                        {
                            streamed = true;
                        }
                        on_event(event);
                    }
                }
            }
            let final_url = format!("{}/session/{}/message", self.base_url, session_id);
            let final_resp = self.client.get(&final_url).send().await?;
            let final_status = final_resp.status();
            let final_body = final_resp.text().await?;
            if !final_status.is_success() {
                bail!("{}: {}", final_status, final_body);
            }
            let messages: Vec<WireSessionMessage> = serde_json::from_str(&final_body)
                .map_err(|err| anyhow!("Invalid response body: {} | body: {}", err, final_body))?;
            return Ok(PromptRunResult { messages, streamed });
        }
        let body = resp.text().await?;
        let messages: Vec<WireSessionMessage> = serde_json::from_str(&body)
            .map_err(|err| anyhow!("Invalid response body: {} | body: {}", err, body))?;
        Ok(PromptRunResult {
            messages,
            streamed: false,
        })
    }

    pub async fn abort_session(&self, session_id: &str) -> Result<()> {
        let url = format!("{}/session/{}/cancel", self.base_url, session_id);
        self.client.post(&url).send().await?;
        Ok(())
    }

    pub async fn cancel_run_by_id(&self, session_id: &str, run_id: &str) -> Result<bool> {
        let url = format!(
            "{}/session/{}/run/{}/cancel",
            self.base_url, session_id, run_id
        );
        let resp = self.client.post(&url).send().await?;
        let payload = resp.json::<serde_json::Value>().await?;
        Ok(payload
            .get("cancelled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false))
    }

    pub async fn get_config(&self) -> Result<serde_json::Value> {
        let url = format!("{}/config", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let config = resp.json::<serde_json::Value>().await?;
        Ok(config)
    }

    pub async fn patch_config(&self, patch: serde_json::Value) -> Result<serde_json::Value> {
        let url = format!("{}/config", self.base_url);
        let resp = self.client.patch(&url).json(&patch).send().await?;
        let config = resp.json::<serde_json::Value>().await?;
        Ok(config)
    }

    pub async fn attach_session_to_workspace(
        &self,
        session_id: &str,
        target_workspace: &str,
        reason_tag: &str,
    ) -> Result<Session> {
        let url = format!("{}/api/session/{}/attach", self.base_url, session_id);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({
                "target_workspace": target_workspace,
                "reason_tag": reason_tag
            }))
            .send()
            .await?;
        let session = resp.json::<Session>().await?;
        Ok(session)
    }

    pub async fn routines_list(&self) -> Result<Vec<RoutineSpec>> {
        let url = format!("{}/routines", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<RoutineListResponse>().await?;
        Ok(payload.routines)
    }

    pub async fn routines_create(&self, request: RoutineCreateRequest) -> Result<RoutineSpec> {
        let url = format!("{}/routines", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<RoutineRecordResponse>().await?;
        Ok(payload.routine)
    }

    pub async fn routines_patch(
        &self,
        routine_id: &str,
        request: RoutinePatchRequest,
    ) -> Result<RoutineSpec> {
        let url = format!("{}/routines/{}", self.base_url, routine_id);
        let resp = self.client.patch(&url).json(&request).send().await?;
        let payload = resp.json::<RoutineRecordResponse>().await?;
        Ok(payload.routine)
    }

    pub async fn routines_delete(&self, routine_id: &str) -> Result<bool> {
        let url = format!("{}/routines/{}", self.base_url, routine_id);
        let resp = self.client.delete(&url).send().await?;
        let payload = resp.json::<RoutineDeleteResponse>().await?;
        Ok(payload.deleted)
    }

    pub async fn routines_run_now(
        &self,
        routine_id: &str,
        request: RoutineRunNowRequest,
    ) -> Result<RoutineRunNowResponse> {
        let url = format!("{}/routines/{}/run_now", self.base_url, routine_id);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<RoutineRunNowResponse>().await?;
        Ok(payload)
    }

    pub async fn routines_history(
        &self,
        routine_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<RoutineHistoryEvent>> {
        let url = format!("{}/routines/{}/history", self.base_url, routine_id);
        let mut req = self.client.get(&url);
        if let Some(limit) = limit {
            req = req.query(&[("limit", limit)]);
        }
        let resp = req.send().await?;
        let payload = resp.json::<RoutineHistoryResponse>().await?;
        Ok(payload.events)
    }

    pub async fn packs_list(&self) -> Result<Vec<PackInstallRecord>> {
        let url = format!("{}/packs", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<PacksListResponse>().await?;
        Ok(payload.packs)
    }

    pub async fn packs_get(&self, selector: &str) -> Result<PackInstallRecord> {
        let url = format!("{}/packs/{}", self.base_url, selector);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<PackRecordEnvelope>().await?;
        Ok(payload.pack.installed)
    }

    pub async fn packs_install(&self, request: serde_json::Value) -> Result<PackInstallRecord> {
        let url = format!("{}/packs/install", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<PackInstallResponse>().await?;
        Ok(payload.installed)
    }

    pub async fn packs_uninstall(&self, request: serde_json::Value) -> Result<PackInstallRecord> {
        let url = format!("{}/packs/uninstall", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<PackUninstallResponse>().await?;
        Ok(payload.removed)
    }

    pub async fn packs_export(&self, request: serde_json::Value) -> Result<PackExportInfo> {
        let url = format!("{}/packs/export", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<PackExportResponse>().await?;
        Ok(payload.exported)
    }

    pub async fn packs_detect(&self, request: serde_json::Value) -> Result<PackDetectionResponse> {
        let url = format!("{}/packs/detect", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<PackDetectionResponse>().await?;
        Ok(payload)
    }

    pub async fn packs_updates(&self, selector: &str) -> Result<PackUpdatesResponse> {
        let url = format!("{}/packs/{}/updates", self.base_url, selector);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<PackUpdatesResponse>().await?;
        Ok(payload)
    }

    pub async fn packs_update(
        &self,
        selector: &str,
        request: serde_json::Value,
    ) -> Result<PackUpdateResult> {
        let url = format!("{}/packs/{}/update", self.base_url, selector);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<PackUpdateResult>().await?;
        Ok(payload)
    }

    pub async fn presets_index(&self) -> Result<PresetIndex> {
        let url = format!("{}/presets/index", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<PresetsIndexResponse>().await?;
        Ok(payload.index)
    }

    pub async fn presets_compose_preview(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = format!("{}/presets/compose/preview", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<serde_json::Value>().await?;
        Ok(payload)
    }

    pub async fn presets_capability_summary(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = format!("{}/presets/capability_summary", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<serde_json::Value>().await?;
        Ok(payload)
    }

    pub async fn presets_fork(&self, request: serde_json::Value) -> Result<serde_json::Value> {
        let url = format!("{}/presets/fork", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<serde_json::Value>().await?;
        Ok(payload)
    }

    pub async fn presets_override_put(
        &self,
        kind: &str,
        id: &str,
        content: &str,
    ) -> Result<serde_json::Value> {
        let url = format!("{}/presets/overrides/{}/{}", self.base_url, kind, id);
        let body = serde_json::json!({ "content": content });
        let resp = self.client.put(&url).json(&body).send().await?;
        let payload = resp.json::<serde_json::Value>().await?;
        Ok(payload)
    }

    pub async fn capabilities_bindings_get(&self) -> Result<CapabilityBindingsFile> {
        let url = format!("{}/capabilities/bindings", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<CapabilityBindingsEnvelope>().await?;
        Ok(payload.bindings)
    }

    pub async fn capabilities_bindings_put(&self, request: CapabilityBindingsFile) -> Result<bool> {
        let url = format!("{}/capabilities/bindings", self.base_url);
        let resp = self.client.put(&url).json(&request).send().await?;
        let payload = resp.json::<serde_json::Value>().await?;
        Ok(payload.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn capabilities_discovery(&self) -> Result<CapabilityDiscoveryResponse> {
        let url = format!("{}/capabilities/discovery", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<CapabilityDiscoveryResponse>().await?;
        Ok(payload)
    }

    pub async fn capabilities_resolve(
        &self,
        request: CapabilityResolveRequest,
    ) -> Result<CapabilityResolutionResponse> {
        let url = format!("{}/capabilities/resolve", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<CapabilityResolutionResponse>().await?;
        Ok(payload)
    }

    pub async fn context_runs_list(&self) -> Result<Vec<ContextRunState>> {
        let url = format!("{}/context/runs", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<ContextRunListResponse>().await?;
        Ok(payload.runs)
    }

    pub async fn context_run_create(
        &self,
        run_id: Option<String>,
        objective: String,
        run_type: Option<String>,
        workspace: Option<ContextWorkspaceLease>,
    ) -> Result<ContextRunState> {
        let url = format!("{}/context/runs", self.base_url);
        let body = serde_json::json!({
            "run_id": run_id,
            "objective": objective,
            "run_type": run_type,
            "workspace": workspace,
        });
        let resp = self.client.post(&url).json(&body).send().await?;
        let payload = resp.json::<ContextRunRecordResponse>().await?;
        Ok(payload.run)
    }

    pub async fn context_run_get(&self, run_id: &str) -> Result<ContextRunDetailResponse> {
        let url = format!("{}/context/runs/{}", self.base_url, run_id);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<ContextRunDetailResponse>().await?;
        Ok(payload)
    }

    pub async fn context_run_put(&self, run: &ContextRunState) -> Result<ContextRunState> {
        let url = format!("{}/context/runs/{}", self.base_url, run.run_id);
        let resp = self.client.put(&url).json(run).send().await?;
        let payload = resp.json::<ContextRunRecordResponse>().await?;
        Ok(payload.run)
    }

    pub async fn context_run_events(
        &self,
        run_id: &str,
        since_seq: Option<u64>,
        tail: Option<usize>,
    ) -> Result<Vec<ContextRunEventRecord>> {
        let url = format!("{}/context/runs/{}/events", self.base_url, run_id);
        let mut req = self.client.get(&url);
        if let Some(since_seq) = since_seq {
            req = req.query(&[("since_seq", since_seq)]);
        }
        if let Some(tail) = tail {
            req = req.query(&[("tail", tail)]);
        }
        let resp = req.send().await?;
        let payload = resp.json::<ContextRunEventsResponse>().await?;
        Ok(payload.events)
    }

    pub async fn context_run_append_event(
        &self,
        run_id: &str,
        event_type: &str,
        status: ContextRunStatus,
        step_id: Option<String>,
        payload: serde_json::Value,
    ) -> Result<ContextRunEventRecord> {
        let url = format!("{}/context/runs/{}/events", self.base_url, run_id);
        let body = serde_json::json!({
            "type": event_type,
            "status": status,
            "step_id": step_id,
            "payload": payload,
        });
        let resp = self.client.post(&url).json(&body).send().await?;
        let parsed = resp.json::<ContextRunEventRecordResponse>().await?;
        Ok(parsed.event)
    }

    pub async fn context_run_blackboard(&self, run_id: &str) -> Result<ContextBlackboardState> {
        let url = format!("{}/context/runs/{}/blackboard", self.base_url, run_id);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<ContextBlackboardResponse>().await?;
        Ok(payload.blackboard)
    }

    pub async fn context_run_rollback_history(
        &self,
        run_id: &str,
    ) -> Result<ContextRunRollbackHistoryResponse> {
        let url = format!(
            "{}/context/runs/{}/checkpoints/mutations/rollback-history",
            self.base_url, run_id
        );
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<ContextRunRollbackHistoryResponse>().await?;
        Ok(payload)
    }

    pub async fn context_run_rollback_preview(
        &self,
        run_id: &str,
    ) -> Result<ContextRunRollbackPreviewResponse> {
        let url = format!(
            "{}/context/runs/{}/checkpoints/mutations/rollback-preview",
            self.base_url, run_id
        );
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<ContextRunRollbackPreviewResponse>().await?;
        Ok(payload)
    }

    pub async fn context_run_rollback_execute(
        &self,
        run_id: &str,
        event_ids: Vec<String>,
        policy_ack: Option<String>,
    ) -> Result<ContextRunRollbackExecuteResponse> {
        let url = format!(
            "{}/context/runs/{}/checkpoints/mutations/rollback-execute",
            self.base_url, run_id
        );
        let body = serde_json::json!({
            "confirm": "rollback",
            "policy_ack": policy_ack,
            "event_ids": event_ids,
        });
        let resp = self.client.post(&url).json(&body).send().await?;
        let payload = resp.json::<ContextRunRollbackExecuteResponse>().await?;
        Ok(payload)
    }

    pub async fn context_run_replay(
        &self,
        run_id: &str,
        upto_seq: Option<u64>,
        from_checkpoint: Option<bool>,
    ) -> Result<ContextRunReplayResponse> {
        let url = format!("{}/context/runs/{}/replay", self.base_url, run_id);
        let mut req = self.client.get(&url);
        if let Some(upto_seq) = upto_seq {
            req = req.query(&[("upto_seq", upto_seq)]);
        }
        if let Some(from_checkpoint) = from_checkpoint {
            req = req.query(&[("from_checkpoint", from_checkpoint)]);
        }
        let resp = req.send().await?;
        let payload = resp.json::<ContextRunReplayResponse>().await?;
        Ok(payload)
    }

    pub async fn context_run_driver_next(
        &self,
        run_id: &str,
        dry_run: bool,
    ) -> Result<ContextDriverNextResponse> {
        let url = format!("{}/context/runs/{}/driver/next", self.base_url, run_id);
        let body = serde_json::json!({ "dry_run": dry_run });
        let resp = self.client.post(&url).json(&body).send().await?;
        let payload = resp.json::<ContextDriverNextResponse>().await?;
        Ok(payload)
    }

    pub async fn context_run_sync_todos(
        &self,
        run_id: &str,
        todos: Vec<ContextTodoSyncItem>,
        replace: bool,
        source_session_id: Option<String>,
        source_run_id: Option<String>,
    ) -> Result<ContextRunState> {
        let url = format!("{}/context/runs/{}/todos/sync", self.base_url, run_id);
        let body = serde_json::json!({
            "replace": replace,
            "source_session_id": source_session_id,
            "source_run_id": source_run_id,
            "todos": todos,
        });
        let resp = self.client.post(&url).json(&body).send().await?;
        let payload = resp.json::<ContextRunRecordResponse>().await?;
        Ok(payload.run)
    }

    pub async fn mission_list(&self) -> Result<Vec<MissionState>> {
        let url = format!("{}/mission", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<MissionListResponse>().await?;
        Ok(payload.missions)
    }

    pub async fn mission_create(&self, request: MissionCreateRequest) -> Result<MissionState> {
        let url = format!("{}/mission", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<MissionRecordResponse>().await?;
        Ok(payload.mission)
    }

    pub async fn mission_get(&self, mission_id: &str) -> Result<MissionState> {
        let url = format!("{}/mission/{}", self.base_url, mission_id);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<MissionRecordResponse>().await?;
        Ok(payload.mission)
    }

    pub async fn mission_apply_event(
        &self,
        mission_id: &str,
        event: serde_json::Value,
    ) -> Result<MissionApplyEventResult> {
        let url = format!("{}/mission/{}/event", self.base_url, mission_id);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "event": event }))
            .send()
            .await?;
        let payload = resp.json::<MissionApplyEventResult>().await?;
        Ok(payload)
    }

    pub async fn agent_team_missions(&self) -> Result<Vec<AgentTeamMissionSummary>> {
        let url = format!("{}/agent-team/missions", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<AgentTeamMissionsResponse>().await?;
        Ok(payload.missions)
    }

    pub async fn agent_team_instances(
        &self,
        mission_id: Option<&str>,
    ) -> Result<Vec<AgentTeamInstance>> {
        let url = format!("{}/agent-team/instances", self.base_url);
        let req = if let Some(mission_id) = mission_id {
            self.client.get(&url).query(&[("missionID", mission_id)])
        } else {
            self.client.get(&url)
        };
        let resp = req.send().await?;
        let payload = resp.json::<AgentTeamInstancesResponse>().await?;
        Ok(payload.instances)
    }

    pub async fn agent_team_approvals(&self) -> Result<AgentTeamApprovalsResponse> {
        let url = format!("{}/agent-team/approvals", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<AgentTeamApprovalsResponse>().await?;
        Ok(payload)
    }

    pub async fn agent_team_approve_spawn(&self, approval_id: &str, reason: &str) -> Result<bool> {
        let url = format!(
            "{}/agent-team/approvals/spawn/{}/approve",
            self.base_url, approval_id
        );
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "reason": reason }))
            .send()
            .await?;
        Ok(resp.status().is_success())
    }

    pub async fn agent_team_deny_spawn(&self, approval_id: &str, reason: &str) -> Result<bool> {
        let url = format!(
            "{}/agent-team/approvals/spawn/{}/deny",
            self.base_url, approval_id
        );
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "reason": reason }))
            .send()
            .await?;
        Ok(resp.status().is_success())
    }
}

