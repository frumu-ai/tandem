use super::*;
use tandem_memory::types::MemoryAccessFilter;
use tandem_types::{RuntimeAuthMode, Session};

#[derive(Clone)]
pub(super) struct ServerPromptContextHook {
    state: AppState,
}

pub(super) const DEFAULT_PROMPT_HOOK_CONTEXT_BUDGET_CHARS: usize = 6_000;
pub(super) const MIN_PROMPT_HOOK_CONTEXT_BUDGET_CHARS: usize = 512;
pub(super) const SOURCE_IDENTITY: &str = "identity";
pub(super) const SOURCE_MEMORY_SCOPE: &str = "memoryScope";
pub(super) const SOURCE_KB_GROUNDING: &str = "kbGrounding";
pub(super) const SOURCE_DOCS: &str = "docs";
pub(super) const SOURCE_GLOBAL_MEMORY: &str = "globalMemory";

#[derive(Debug, Clone)]
pub(super) enum PromptMemoryAccess {
    Local {
        user_id: String,
    },
    Governed {
        tenant_context: TenantContext,
        subject: String,
        access_filter: MemoryAccessFilter,
    },
    Blocked {
        reason: &'static str,
        tenant_context: Option<TenantContext>,
    },
}

impl PromptMemoryAccess {
    fn mode(&self) -> &'static str {
        match self {
            Self::Local { .. } => "local",
            Self::Governed { .. } => "governed",
            Self::Blocked { .. } => "blocked",
        }
    }
}

pub(super) struct PromptHookBudget {
    pub(super) stats: PromptContextHookStats,
}

#[derive(Debug, Clone, Default)]
pub(super) struct DocsContextBlock {
    content: String,
    included_count: usize,
    included_chars: usize,
    dropped_count: usize,
    dropped_chars: usize,
}

impl PromptHookBudget {
    pub(super) fn new() -> Self {
        let budget_chars = prompt_hook_context_budget_chars();
        Self {
            stats: PromptContextHookStats {
                budget_chars: Some(budget_chars),
                remaining_chars: Some(budget_chars),
                ..PromptContextHookStats::default()
            },
        }
    }

    fn remaining_chars(&self) -> usize {
        self.stats.remaining_chars.unwrap_or(usize::MAX)
    }

    pub(super) fn push_system_message(
        &mut self,
        messages: &mut Vec<ChatMessage>,
        source: &'static str,
        content: String,
        injected_count: usize,
        required: bool,
    ) -> bool {
        let chars = content.len();
        if !required && chars > self.remaining_chars() {
            self.stats.record_deferred(source, injected_count, chars);
            return false;
        }
        messages.push(ChatMessage {
            role: "system".to_string(),
            content,
            attachments: Vec::new(),
        });
        self.stats.record_injected(source, injected_count, chars);
        true
    }

    fn record_dropped(&mut self, source: &'static str, count: usize, chars: usize) {
        self.stats.record_dropped(source, count, chars);
    }

    fn record_deferred(&mut self, source: &'static str, count: usize, chars: usize) {
        self.stats.record_deferred(source, count, chars);
    }

    pub(super) fn finish(self) -> PromptContextHookStats {
        self.stats
    }
}

pub(super) fn prompt_hook_context_budget_chars() -> usize {
    std::env::var("TANDEM_PROMPT_HOOK_CONTEXT_BUDGET_CHARS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value >= MIN_PROMPT_HOOK_CONTEXT_BUDGET_CHARS)
        .unwrap_or(DEFAULT_PROMPT_HOOK_CONTEXT_BUDGET_CHARS)
}

pub(super) const DEFAULT_DOCS_CONTEXT_BUDGET_CHARS: usize = 2_400;
pub(super) const DEFAULT_MEMORY_CONTEXT_BUDGET_CHARS: usize = 2_200;
pub(super) const MIN_SOURCE_CONTEXT_BUDGET_CHARS: usize = 256;

/// Explicit char budget for the embedded-docs grounding block. The effective
/// budget at injection time is the minimum of this and the remaining shared
/// prompt hook budget.
pub(super) fn docs_context_budget_chars() -> usize {
    std::env::var("TANDEM_DOCS_CONTEXT_BUDGET_CHARS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value >= MIN_SOURCE_CONTEXT_BUDGET_CHARS)
        .unwrap_or(DEFAULT_DOCS_CONTEXT_BUDGET_CHARS)
}

/// Explicit char budget for the global-memory context block, matching the
/// long-standing memory block budget. The effective budget at injection time
/// is the minimum of this and the remaining shared prompt hook budget.
pub(super) fn memory_context_budget_chars() -> usize {
    std::env::var("TANDEM_MEMORY_CONTEXT_BUDGET_CHARS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value >= MIN_SOURCE_CONTEXT_BUDGET_CHARS)
        .unwrap_or(DEFAULT_MEMORY_CONTEXT_BUDGET_CHARS)
}

impl ServerPromptContextHook {
    pub(super) fn new(state: AppState) -> Self {
        Self { state }
    }

    async fn open_memory_db(&self) -> Option<MemoryDatabase> {
        if let Some(parent) = self.state.memory_db_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        MemoryDatabase::new(&self.state.memory_db_path).await.ok()
    }

    async fn open_memory_manager(&self) -> Option<tandem_memory::MemoryManager> {
        if let Some(parent) = self.state.memory_db_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        tandem_memory::MemoryManager::new(&self.state.memory_db_path)
            .await
            .ok()
    }

    fn hash_query(input: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub(super) fn build_memory_block(
        hits: &[tandem_memory::types::GlobalMemorySearchHit],
    ) -> String {
        prompt_memory_context::build_memory_block(hits)
    }

    pub(super) fn governed_memory_visible_without_source_grant(
        record: &tandem_memory::types::GlobalMemoryRecord,
    ) -> bool {
        MemorySourceAccessTarget::from_metadata(record.metadata.as_ref()).is_none()
    }

    pub(super) fn governed_memory_visible_with_access_filter(
        record: &tandem_memory::types::GlobalMemoryRecord,
        access_filter: &MemoryAccessFilter,
    ) -> bool {
        let Some(target) = MemorySourceAccessTarget::from_metadata(record.metadata.as_ref()) else {
            return true;
        };
        access_filter.allows_source_target(&target)
    }

    fn run_client_memory_subject(run_client_id: Option<&str>) -> String {
        run_client_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("default")
            .to_string()
    }

    pub(super) fn resolve_prompt_memory_access(
        runtime_auth_mode: RuntimeAuthMode,
        session: Option<&Session>,
        run_client_id: Option<&str>,
        now_ms: u64,
    ) -> PromptMemoryAccess {
        let Some(session) = session else {
            return if runtime_auth_mode == RuntimeAuthMode::LocalSingleTenant {
                PromptMemoryAccess::Local {
                    user_id: Self::run_client_memory_subject(run_client_id),
                }
            } else {
                PromptMemoryAccess::Blocked {
                    reason: "missing_session",
                    tenant_context: None,
                }
            };
        };
        let verified = session.verified_tenant_context.as_ref();
        let tenant_context = verified
            .map(|context| context.tenant_context.clone())
            .unwrap_or_else(|| session.tenant_context.clone());
        let governed = runtime_auth_mode != RuntimeAuthMode::LocalSingleTenant
            || verified.is_some()
            || !tenant_context.is_local_implicit();
        if !governed {
            return PromptMemoryAccess::Local {
                user_id: Self::run_client_memory_subject(run_client_id),
            };
        }
        let Some(verified) = verified else {
            return PromptMemoryAccess::Blocked {
                reason: "missing_verified_tenant_context",
                tenant_context: Some(tenant_context),
            };
        };
        if verified.is_expired_at(now_ms) {
            return PromptMemoryAccess::Blocked {
                reason: "expired_verified_tenant_context",
                tenant_context: Some(verified.tenant_context.clone()),
            };
        }
        if verified.tenant_context.is_local_implicit() {
            return PromptMemoryAccess::Blocked {
                reason: "local_implicit_tenant_context",
                tenant_context: Some(verified.tenant_context.clone()),
            };
        }
        let Some(strict_projection) = verified.strict_projection.clone() else {
            return PromptMemoryAccess::Blocked {
                reason: "missing_strict_projection",
                tenant_context: Some(verified.tenant_context.clone()),
            };
        };
        if strict_projection.is_expired_at(now_ms) {
            return PromptMemoryAccess::Blocked {
                reason: "expired_strict_projection",
                tenant_context: Some(verified.tenant_context.clone()),
            };
        }
        let Some(subject) = verified
            .tenant_context
            .actor_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                let human_actor = verified.human_actor.actor_id.trim();
                (!human_actor.is_empty()).then_some(human_actor)
            })
            .map(ToString::to_string)
        else {
            return PromptMemoryAccess::Blocked {
                reason: "missing_actor_id",
                tenant_context: Some(verified.tenant_context.clone()),
            };
        };
        PromptMemoryAccess::Governed {
            tenant_context: verified.tenant_context.clone(),
            subject,
            access_filter: MemoryAccessFilter::strict(strict_projection, now_ms),
        }
    }

    fn extract_docs_source_url(chunk: &tandem_memory::types::MemoryChunk) -> Option<String> {
        chunk
            .metadata
            .as_ref()
            .and_then(|meta| meta.get("source_url"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string)
    }

    fn extract_docs_relative_path(chunk: &tandem_memory::types::MemoryChunk) -> String {
        if let Some(path) = chunk
            .metadata
            .as_ref()
            .and_then(|meta| meta.get("relative_path"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            return path.to_string();
        }
        chunk
            .source
            .strip_prefix("guide_docs:")
            .unwrap_or(chunk.source.as_str())
            .to_string()
    }

    fn build_docs_memory_block_with_budget(
        hits: &[tandem_memory::types::MemorySearchResult],
        char_budget: usize,
    ) -> DocsContextBlock {
        let mut out = vec!["<docs_context>".to_string()];
        let mut used = 0usize;
        let mut result = DocsContextBlock::default();
        for hit in hits {
            let url = Self::extract_docs_source_url(&hit.chunk).unwrap_or_default();
            let path = Self::extract_docs_relative_path(&hit.chunk);
            let text = hit
                .chunk
                .content
                .split_whitespace()
                .take(70)
                .collect::<Vec<_>>()
                .join(" ");
            let line = format!(
                "- [{:.3}] {} (doc_id={}, doc_path={}, source_url={})",
                hit.similarity, text, hit.chunk.id, path, url
            );
            let next_used = used.saturating_add(line.len());
            if next_used > char_budget {
                result.dropped_count = result.dropped_count.saturating_add(1);
                result.dropped_chars = result.dropped_chars.saturating_add(line.len());
                continue;
            }
            used = next_used;
            result.included_count = result.included_count.saturating_add(1);
            result.included_chars = result.included_chars.saturating_add(line.len());
            out.push(line);
        }
        out.push("</docs_context>".to_string());
        result.content = out.join("\n");
        result.included_chars = result.content.len();
        result
    }

    async fn search_embedded_docs(
        &self,
        query: &str,
        limit: usize,
    ) -> Vec<tandem_memory::types::MemorySearchResult> {
        let Some(manager) = self.open_memory_manager().await else {
            return Vec::new();
        };
        let search_limit = (limit.saturating_mul(3)).clamp(6, 36) as i64;
        manager
            .search(
                query,
                Some(MemoryTier::Global),
                None,
                None,
                Some(search_limit),
            )
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|hit| hit.chunk.source.starts_with("guide_docs:"))
            .take(limit)
            .collect()
    }

    pub(super) async fn search_prompt_global_memory(
        db: &MemoryDatabase,
        memory_access: &PromptMemoryAccess,
        query: &str,
        project_id: Option<&str>,
    ) -> (
        Vec<tandem_memory::types::GlobalMemorySearchHit>,
        Vec<tandem_memory::types::GlobalMemorySearchHit>,
    ) {
        match memory_access {
            PromptMemoryAccess::Local { user_id } => {
                let project_hits = if let Some(project_id) = project_id {
                    db.search_global_memory(user_id, query, 8, Some(project_id), None, None)
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|hit| {
                            Self::governed_memory_visible_without_source_grant(&hit.record)
                        })
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                let global_hits = db
                    .search_global_memory(user_id, query, 8, None, None, None)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|hit| Self::governed_memory_visible_without_source_grant(&hit.record))
                    .collect::<Vec<_>>();
                (project_hits, global_hits)
            }
            PromptMemoryAccess::Governed {
                tenant_context,
                subject,
                access_filter,
            } => {
                let project_hits = if let Some(project_id) = project_id {
                    db.search_global_memory_for_tenant(
                        &tenant_context.org_id,
                        &tenant_context.workspace_id,
                        tenant_context.deployment_id.as_deref(),
                        subject,
                        query,
                        8,
                        Some(project_id),
                        None,
                        None,
                    )
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|hit| {
                        Self::governed_memory_visible_with_access_filter(&hit.record, access_filter)
                    })
                    .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                let global_hits = db
                    .search_global_memory_for_tenant(
                        &tenant_context.org_id,
                        &tenant_context.workspace_id,
                        tenant_context.deployment_id.as_deref(),
                        subject,
                        query,
                        8,
                        None,
                        None,
                        None,
                    )
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|hit| {
                        Self::governed_memory_visible_with_access_filter(&hit.record, access_filter)
                    })
                    .collect::<Vec<_>>();
                (project_hits, global_hits)
            }
            PromptMemoryAccess::Blocked { .. } => (Vec::new(), Vec::new()),
        }
    }

    fn dedupe_global_memory_hits(
        hits: Vec<tandem_memory::types::GlobalMemorySearchHit>,
    ) -> Vec<tandem_memory::types::GlobalMemorySearchHit> {
        let mut seen = std::collections::HashSet::new();
        let mut deduped = Vec::new();
        for hit in hits {
            if seen.insert(hit.record.id.clone()) {
                deduped.push(hit);
            }
        }
        deduped
    }

    pub(super) fn select_memory_hits_for_context(
        project_hits: Vec<tandem_memory::types::GlobalMemorySearchHit>,
        global_hits: Vec<tandem_memory::types::GlobalMemorySearchHit>,
    ) -> (
        Vec<tandem_memory::types::GlobalMemorySearchHit>,
        Vec<tandem_memory::types::GlobalMemorySearchHit>,
        bool,
    ) {
        let project_hits = Self::dedupe_global_memory_hits(project_hits);
        if project_hits.is_empty() {
            return (
                Self::dedupe_global_memory_hits(global_hits),
                Vec::new(),
                false,
            );
        }

        let selected_ids = project_hits
            .iter()
            .map(|hit| hit.record.id.clone())
            .collect::<std::collections::HashSet<_>>();
        let deferred_global_hits = Self::dedupe_global_memory_hits(global_hits)
            .into_iter()
            .filter(|hit| !selected_ids.contains(&hit.record.id))
            .collect::<Vec<_>>();
        (project_hits, deferred_global_hits, true)
    }

    fn should_skip_memory_injection(query: &str) -> bool {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return true;
        }
        let lower = trimmed.to_ascii_lowercase();
        let social = [
            "hi",
            "hello",
            "hey",
            "thanks",
            "thank you",
            "ok",
            "okay",
            "cool",
            "nice",
            "yo",
            "good morning",
            "good afternoon",
            "good evening",
        ];
        lower.len() <= 32 && social.contains(&lower.as_str())
    }
}

impl PromptContextHook for ServerPromptContextHook {
    fn augment_provider_messages(
        &self,
        ctx: PromptContextHookContext,
        mut messages: Vec<ChatMessage>,
    ) -> BoxFuture<'static, anyhow::Result<PromptContextHookResult>> {
        let this = self.clone();
        Box::pin(async move {
            // Startup can invoke prompt plumbing before RuntimeState is installed.
            // Never panic from context hooks; fail-open and continue without augmentation.
            if !this.state.is_ready() {
                return Ok(PromptContextHookResult::new(
                    messages,
                    PromptContextHookStats::default(),
                ));
            }
            let run = this.state.run_registry.get(&ctx.session_id).await;
            let Some(run) = run else {
                return Ok(PromptContextHookResult::new(
                    messages,
                    PromptContextHookStats::default(),
                ));
            };
            let mut budget = PromptHookBudget::new();
            let config = this.state.config.get_effective_value().await;
            if let Some(identity_block) =
                prompt_context_blocks::resolve_identity_block(&config, run.agent_profile.as_deref())
            {
                budget.push_system_message(&mut messages, SOURCE_IDENTITY, identity_block, 1, true);
            }
            let session = this.state.storage.get_session(&ctx.session_id).await;
            let project_id = session
                .as_ref()
                .and_then(|session| session.project_id.as_deref())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);
            if let Some(session) = session.as_ref() {
                budget.push_system_message(
                    &mut messages,
                    SOURCE_MEMORY_SCOPE,
                    prompt_context_blocks::build_memory_scope_block(
                        &ctx.session_id,
                        session.project_id.as_deref(),
                        session.workspace_root.as_deref(),
                    ),
                    1,
                    true,
                );
            }
            let run_id = run.run_id;
            let run_client_id = run.client_id.clone();
            let query = messages
                .iter()
                .rev()
                .find(|m| m.role == "user")
                .map(|m| m.content.clone())
                .unwrap_or_default();
            if query.trim().is_empty() {
                return Ok(PromptContextHookResult::new(messages, budget.finish()));
            }
            if Self::should_skip_memory_injection(&query) {
                return Ok(PromptContextHookResult::new(messages, budget.finish()));
            }
            if let Some(policy) = this
                .state
                .engine_loop
                .get_session_kb_grounding_policy(&ctx.session_id)
                .await
            {
                if policy.required {
                    let kb_block = prompt_context_blocks::build_kb_grounding_block(&policy);
                    let kb_chars = kb_block.len();
                    let injected = budget.push_system_message(
                        &mut messages,
                        SOURCE_KB_GROUNDING,
                        kb_block.clone(),
                        1,
                        true,
                    );
                    this.state.event_bus.publish(EngineEvent::new(
                        "kb.grounding.context.injected",
                        json!({
                            "runID": run_id,
                            "sessionID": ctx.session_id,
                            "messageID": ctx.message_id,
                            "iteration": ctx.iteration,
                            "strict": policy.strict,
                            "serverNames": policy.server_names,
                            "toolPatterns": policy.tool_patterns,
                            "budgetChars": budget.stats.budget_chars,
                            "remainingBudgetChars": budget.stats.remaining_chars,
                            "charSize": kb_chars,
                            "injected": injected,
                            "tokenSizeApprox": kb_block.split_whitespace().count(),
                        }),
                    ));
                }
            }

            let docs_hits = this.search_embedded_docs(&query, 6).await;
            if !docs_hits.is_empty() {
                let docs_budget = docs_context_budget_chars().min(budget.remaining_chars());
                let docs_block = Self::build_docs_memory_block_with_budget(&docs_hits, docs_budget);
                if docs_block.dropped_count > 0 {
                    budget.record_dropped(
                        SOURCE_DOCS,
                        docs_block.dropped_count,
                        docs_block.dropped_chars,
                    );
                }
                let injected = docs_block.included_count > 0
                    && budget.push_system_message(
                        &mut messages,
                        SOURCE_DOCS,
                        docs_block.content.clone(),
                        docs_block.included_count,
                        false,
                    );
                this.state.event_bus.publish(EngineEvent::new(
                    "memory.docs.context.injected",
                    json!({
                        "runID": run_id,
                        "sessionID": ctx.session_id,
                        "messageID": ctx.message_id,
                        "iteration": ctx.iteration,
                        "count": docs_hits.len(),
                        "injected": injected,
                        "injectedCount": if injected { docs_block.included_count } else { 0 },
                        "droppedCount": docs_block.dropped_count,
                        "deferredCount": if injected { 0 } else { docs_block.included_count },
                        "budgetChars": budget.stats.budget_chars,
                        "remainingBudgetChars": budget.stats.remaining_chars,
                        "sourceBudgetChars": docs_budget,
                        "charSize": docs_block.content.len(),
                        "tokenSizeApprox": docs_block.content.split_whitespace().count(),
                        "sourcePrefix": "guide_docs:"
                    }),
                ));
            }

            let started = now_ms();
            let runtime_auth_mode = crate::config::env::resolve_runtime_auth_mode();
            let memory_access = Self::resolve_prompt_memory_access(
                runtime_auth_mode,
                session.as_ref(),
                run_client_id.as_deref(),
                started,
            );
            if let PromptMemoryAccess::Blocked {
                reason,
                tenant_context,
            } = &memory_access
            {
                this.state.event_bus.publish(EngineEvent::new(
                    "memory.context.blocked",
                    json!({
                        "runID": run_id,
                        "sessionID": ctx.session_id,
                        "messageID": ctx.message_id,
                        "providerID": ctx.provider_id,
                        "modelID": ctx.model_id,
                        "iteration": ctx.iteration,
                        "reason": reason,
                        "source": SOURCE_GLOBAL_MEMORY,
                        "policyMode": memory_access.mode(),
                        "runtimeAuthMode": runtime_auth_mode.as_str(),
                        "tenantOrgID": tenant_context.as_ref().map(|tenant| tenant.org_id.clone()),
                        "tenantWorkspaceID": tenant_context.as_ref().map(|tenant| tenant.workspace_id.clone()),
                        "tenantDeploymentID": tenant_context.as_ref().and_then(|tenant| tenant.deployment_id.clone()),
                    }),
                ));
                return Ok(PromptContextHookResult::new(messages, budget.finish()));
            }
            let Some(db) = this.open_memory_db().await else {
                return Ok(PromptContextHookResult::new(messages, budget.finish()));
            };
            let (project_hits, global_hits) = Self::search_prompt_global_memory(
                &db,
                &memory_access,
                &query,
                project_id.as_deref(),
            )
            .await;
            let (hits, deferred_global_hits, project_scope_used) =
                Self::select_memory_hits_for_context(project_hits, global_hits);
            let latency_ms = now_ms().saturating_sub(started);
            let scores = hits.iter().map(|h| h.score).collect::<Vec<_>>();
            this.state.event_bus.publish(EngineEvent::new(
                "memory.search.performed",
                json!({
                    "runID": run_id,
                    "sessionID": ctx.session_id,
                    "messageID": ctx.message_id,
                    "providerID": ctx.provider_id,
                    "modelID": ctx.model_id,
                    "iteration": ctx.iteration,
                    "queryHash": Self::hash_query(&query),
                    "resultCount": hits.len(),
                    "projectScopeUsed": project_scope_used,
                    "currentProjectID": project_id.clone(),
                    "globalFallbackDeferredCount": deferred_global_hits.len(),
                    "scoreMin": scores.iter().copied().reduce(f64::min),
                    "scoreMax": scores.iter().copied().reduce(f64::max),
                    "scores": scores,
                    "latencyMs": latency_ms,
                    "policyMode": memory_access.mode(),
                    "sources": hits.iter().map(|h| h.record.source_type.clone()).collect::<Vec<_>>(),
                }),
            ));

            if hits.is_empty() {
                return Ok(PromptContextHookResult::new(messages, budget.finish()));
            }

            let memory_budget = memory_context_budget_chars().min(budget.remaining_chars());
            let memory_block =
                prompt_memory_context::build_memory_block_with_budget(&hits, memory_budget);
            if memory_block.dropped_count > 0 {
                budget.record_dropped(
                    SOURCE_GLOBAL_MEMORY,
                    memory_block.dropped_count,
                    memory_block.dropped_chars,
                );
            }
            let injected = memory_block.included_count > 0
                && budget.push_system_message(
                    &mut messages,
                    SOURCE_GLOBAL_MEMORY,
                    memory_block.content.clone(),
                    memory_block.included_count,
                    false,
                );
            this.state.event_bus.publish(EngineEvent::new(
                "memory.context.injected",
                json!({
                    "runID": run_id,
                    "sessionID": ctx.session_id,
                    "messageID": ctx.message_id,
                    "iteration": ctx.iteration,
                    "count": hits.len(),
                    "injected": injected,
                    "injectedCount": if injected { memory_block.included_count } else { 0 },
                    "droppedCount": memory_block.dropped_count,
                    "deferredCount": if injected { 0 } else { memory_block.included_count },
                    "deferredGlobalFallbackCount": deferred_global_hits.len(),
                    "projectScopeUsed": project_scope_used,
                    "currentProjectID": project_id.clone(),
                    "budgetChars": budget.stats.budget_chars,
                    "remainingBudgetChars": budget.stats.remaining_chars,
                    "sourceBudgetChars": memory_budget,
                    "charSize": memory_block.content.len(),
                    "tokenSizeApprox": memory_block.content.split_whitespace().count(),
                }),
            ));
            Ok(PromptContextHookResult::new(messages, budget.finish()))
        })
    }
}
