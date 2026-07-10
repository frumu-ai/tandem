use crate::types::{
    KnowledgeCoverageRecord, KnowledgeItemRecord, KnowledgePromotionRequest,
    KnowledgePromotionResult, KnowledgeSpaceRecord,
};
use std::collections::HashSet;
use tandem_orchestrator::{
    build_knowledge_coverage_key, normalize_knowledge_segment, KnowledgeBinding, KnowledgePackItem,
    KnowledgePreflightRequest, KnowledgePreflightResult, KnowledgeReuseDecision,
    KnowledgeReuseMode, KnowledgeScope, KnowledgeTrustLevel,
};

impl MemoryManager {
    pub async fn upsert_knowledge_space(&self, space: &KnowledgeSpaceRecord) -> MemoryResult<()> {
        self.upsert_knowledge_space_for_tenant(space, &MemoryTenantScope::local())
            .await
    }

    pub async fn upsert_knowledge_space_for_tenant(
        &self,
        space: &KnowledgeSpaceRecord,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<()> {
        validate_memory_envelope_for_write(tenant_scope, space.metadata.as_ref())?;
        match self
            .store
            .write(MemoryStoreWriteRequest::KnowledgeSpace {
                scope: MemoryWriteScope::tenant(tenant_scope.clone()),
                record: space.clone(),
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreWriteResult::Stored => Ok(()),
            _ => Err(Self::unexpected_store_result("upsert knowledge space")),
        }
    }

    pub async fn get_knowledge_space(
        &self,
        id: &str,
    ) -> MemoryResult<Option<KnowledgeSpaceRecord>> {
        self.get_knowledge_space_for_tenant(id, &MemoryTenantScope::local())
            .await
    }

    pub async fn get_knowledge_space_for_tenant(
        &self,
        id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<KnowledgeSpaceRecord>> {
        match self
            .store
            .read(MemoryStoreReadRequest::KnowledgeSpace {
                scope: Self::read_scope(tenant_scope),
                id: id.to_string(),
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreReadResult::KnowledgeSpace(space) => Ok(space),
            _ => Err(Self::unexpected_store_result("read knowledge space")),
        }
    }

    pub async fn list_knowledge_spaces(
        &self,
        project_id: Option<&str>,
    ) -> MemoryResult<Vec<KnowledgeSpaceRecord>> {
        self.list_knowledge_spaces_for_tenant(project_id, &MemoryTenantScope::local())
            .await
    }

    pub async fn list_knowledge_spaces_for_tenant(
        &self,
        project_id: Option<&str>,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Vec<KnowledgeSpaceRecord>> {
        match self
            .store
            .query(MemoryStoreQueryRequest::KnowledgeSpaces {
                scope: Self::read_scope(tenant_scope),
                project_id: project_id.map(ToString::to_string),
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreQueryResult::KnowledgeSpaces(spaces) => Ok(spaces),
            _ => Err(Self::unexpected_store_result("list knowledge spaces")),
        }
    }

    pub async fn upsert_knowledge_item(&self, item: &KnowledgeItemRecord) -> MemoryResult<()> {
        self.upsert_knowledge_item_for_tenant(item, &MemoryTenantScope::local())
            .await
    }

    pub async fn upsert_knowledge_item_for_tenant(
        &self,
        item: &KnowledgeItemRecord,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<()> {
        validate_memory_envelope_for_write(tenant_scope, item.metadata.as_ref())?;
        match self
            .store
            .write(MemoryStoreWriteRequest::KnowledgeItem {
                scope: MemoryWriteScope::tenant(tenant_scope.clone()),
                record: item.clone(),
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreWriteResult::Stored => Ok(()),
            _ => Err(Self::unexpected_store_result("upsert knowledge item")),
        }
    }

    pub async fn get_knowledge_item(&self, id: &str) -> MemoryResult<Option<KnowledgeItemRecord>> {
        self.get_knowledge_item_for_tenant(id, &MemoryTenantScope::local())
            .await
    }

    pub async fn get_knowledge_item_for_tenant(
        &self,
        id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<KnowledgeItemRecord>> {
        match self
            .store
            .read(MemoryStoreReadRequest::KnowledgeItem {
                scope: Self::read_scope(tenant_scope),
                id: id.to_string(),
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreReadResult::KnowledgeItem(item) => Ok(item),
            _ => Err(Self::unexpected_store_result("read knowledge item")),
        }
    }

    pub async fn list_knowledge_items(
        &self,
        space_id: &str,
        coverage_key: Option<&str>,
    ) -> MemoryResult<Vec<KnowledgeItemRecord>> {
        self.list_knowledge_items_for_tenant(space_id, coverage_key, &MemoryTenantScope::local())
            .await
    }

    pub async fn list_knowledge_items_for_tenant(
        &self,
        space_id: &str,
        coverage_key: Option<&str>,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Vec<KnowledgeItemRecord>> {
        match self
            .store
            .query(MemoryStoreQueryRequest::KnowledgeItems {
                scope: Self::read_scope(tenant_scope),
                space_id: space_id.to_string(),
                coverage_key: coverage_key.map(ToString::to_string),
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreQueryResult::KnowledgeItems(items) => Ok(items),
            _ => Err(Self::unexpected_store_result("list knowledge items")),
        }
    }

    pub async fn upsert_knowledge_coverage(
        &self,
        coverage: &KnowledgeCoverageRecord,
    ) -> MemoryResult<()> {
        self.upsert_knowledge_coverage_for_tenant(coverage, &MemoryTenantScope::local())
            .await
    }

    pub async fn upsert_knowledge_coverage_for_tenant(
        &self,
        coverage: &KnowledgeCoverageRecord,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<()> {
        validate_memory_envelope_for_write(tenant_scope, coverage.metadata.as_ref())?;
        match self
            .store
            .write(MemoryStoreWriteRequest::KnowledgeCoverage {
                scope: MemoryWriteScope::tenant(tenant_scope.clone()),
                record: coverage.clone(),
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreWriteResult::Stored => Ok(()),
            _ => Err(Self::unexpected_store_result("upsert knowledge coverage")),
        }
    }

    pub async fn get_knowledge_coverage(
        &self,
        coverage_key: &str,
        space_id: &str,
    ) -> MemoryResult<Option<KnowledgeCoverageRecord>> {
        self.get_knowledge_coverage_for_tenant(coverage_key, space_id, &MemoryTenantScope::local())
            .await
    }

    pub async fn get_knowledge_coverage_for_tenant(
        &self,
        coverage_key: &str,
        space_id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<KnowledgeCoverageRecord>> {
        match self
            .store
            .read(MemoryStoreReadRequest::KnowledgeCoverage {
                scope: Self::read_scope(tenant_scope),
                coverage_key: coverage_key.to_string(),
                space_id: space_id.to_string(),
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreReadResult::KnowledgeCoverage(coverage) => Ok(coverage),
            _ => Err(Self::unexpected_store_result("read knowledge coverage")),
        }
    }

    pub async fn promote_knowledge_item(
        &self,
        request: &KnowledgePromotionRequest,
    ) -> MemoryResult<Option<KnowledgePromotionResult>> {
        self.promote_knowledge_item_for_tenant(request, &MemoryTenantScope::local())
            .await
    }

    pub async fn promote_knowledge_item_for_tenant(
        &self,
        request: &KnowledgePromotionRequest,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<KnowledgePromotionResult>> {
        match self
            .store
            .mutate(MemoryStoreMutationRequest::PromoteKnowledgeItem {
                scope: Self::read_scope(tenant_scope),
                request: request.clone(),
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreMutationResult::Promotion(result) => Ok(result),
            _ => Err(Self::unexpected_store_result("promote knowledge item")),
        }
    }

    fn space_matches_ref(
        space: &KnowledgeSpaceRecord,
        space_ref: &tandem_orchestrator::KnowledgeSpaceRef,
        project_id: &str,
    ) -> bool {
        if space.scope != space_ref.scope {
            return false;
        }
        match space_ref.scope {
            KnowledgeScope::Project | KnowledgeScope::Run => {
                let target_project = space_ref.project_id.as_deref().unwrap_or(project_id);
                if space.project_id.as_deref() != Some(target_project) {
                    return false;
                }
            }
            KnowledgeScope::Global => {}
        }
        if let Some(namespace) = space_ref.namespace.as_deref() {
            if space.namespace.as_deref() != Some(namespace) {
                return false;
            }
        }
        true
    }

    fn select_preflight_namespace(
        binding: &KnowledgeBinding,
        spaces: &[KnowledgeSpaceRecord],
    ) -> Option<String> {
        if let Some(namespace) = binding.namespace.clone() {
            return Some(namespace);
        }
        if binding.read_spaces.len() == 1 {
            if let Some(namespace) = binding.read_spaces[0].namespace.clone() {
                return Some(namespace);
            }
        }
        if spaces.len() == 1 {
            return spaces[0].namespace.clone();
        }
        let mut unique = HashSet::new();
        for space in spaces {
            if let Some(namespace) = space.namespace.as_ref() {
                unique.insert(namespace);
            }
        }
        if unique.len() == 1 {
            unique.into_iter().next().map(|value| value.to_string())
        } else {
            None
        }
    }

    fn binding_uses_explicit_spaces(binding: &KnowledgeBinding) -> bool {
        !binding.read_spaces.is_empty() || !binding.promote_spaces.is_empty()
    }

    fn namespace_matches(space_namespace: Option<&str>, binding_namespace: Option<&str>) -> bool {
        match (space_namespace, binding_namespace) {
            (None, None) => true,
            (Some(space), Some(binding)) => {
                normalize_knowledge_segment(space) == normalize_knowledge_segment(binding)
            }
            _ => false,
        }
    }

    fn is_fresh_enough(
        freshness_expires_at_ms: Option<u64>,
        freshness_policy_ms: Option<u64>,
        coverage_last_promoted_at_ms: Option<u64>,
        item_created_at_ms: u64,
        now_ms: u64,
    ) -> bool {
        if let Some(expires_at_ms) = freshness_expires_at_ms {
            return expires_at_ms > now_ms;
        }
        let Some(policy_ms) = freshness_policy_ms else {
            return true;
        };
        let basis_ms = coverage_last_promoted_at_ms.unwrap_or(item_created_at_ms);
        now_ms.saturating_sub(basis_ms) <= policy_ms
    }

    async fn resolve_preflight_spaces(
        &self,
        request: &KnowledgePreflightRequest,
        _coverage_key: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Vec<KnowledgeSpaceRecord>> {
        let binding = &request.binding;
        let mut spaces = Vec::new();
        let mut seen_space_ids = HashSet::new();

        let push_space = |space: KnowledgeSpaceRecord,
                          spaces: &mut Vec<KnowledgeSpaceRecord>,
                          seen_space_ids: &mut HashSet<String>| {
            if seen_space_ids.insert(space.id.clone()) {
                spaces.push(space);
            }
        };

        if Self::binding_uses_explicit_spaces(binding) {
            for space_ref in binding
                .read_spaces
                .iter()
                .chain(binding.promote_spaces.iter())
            {
                if let Some(space_id) = space_ref.space_id.as_deref() {
                    if let Some(space) = self
                        .get_knowledge_space_for_tenant(space_id, tenant_scope)
                        .await?
                    {
                        push_space(space, &mut spaces, &mut seen_space_ids);
                    }
                    continue;
                }

                match space_ref.scope {
                    KnowledgeScope::Run => {}
                    KnowledgeScope::Project => {
                        let candidate_project_id = space_ref
                            .project_id
                            .as_deref()
                            .unwrap_or(&request.project_id);
                        let project_spaces = self
                            .list_knowledge_spaces_for_tenant(
                                Some(candidate_project_id),
                                tenant_scope,
                            )
                            .await?;
                        for space in project_spaces.into_iter().filter(|space| {
                            Self::space_matches_ref(space, space_ref, &request.project_id)
                        }) {
                            push_space(space, &mut spaces, &mut seen_space_ids);
                        }
                    }
                    KnowledgeScope::Global => {
                        let global_spaces = self
                            .list_knowledge_spaces_for_tenant(None, tenant_scope)
                            .await?;
                        for space in global_spaces.into_iter().filter(|space| {
                            Self::space_matches_ref(space, space_ref, &request.project_id)
                        }) {
                            push_space(space, &mut spaces, &mut seen_space_ids);
                        }
                    }
                }
            }
            return Ok(spaces);
        }

        if request.project_id.trim().is_empty() {
            return Ok(spaces);
        }

        let project_spaces = self
            .list_knowledge_spaces_for_tenant(Some(&request.project_id), tenant_scope)
            .await?;
        let requested_namespace = if binding.namespace.is_some() {
            binding.namespace.clone()
        } else {
            Self::select_preflight_namespace(binding, &project_spaces)
        };
        let Some(requested_namespace) = requested_namespace else {
            return Ok(spaces);
        };

        for space in project_spaces.into_iter().filter(|space| {
            space.scope == KnowledgeScope::Project
                && Self::namespace_matches(
                    space.namespace.as_deref(),
                    Some(requested_namespace.as_str()),
                )
        }) {
            push_space(space, &mut spaces, &mut seen_space_ids);
        }
        Ok(spaces)
    }

    pub async fn preflight_knowledge(
        &self,
        request: &KnowledgePreflightRequest,
    ) -> MemoryResult<KnowledgePreflightResult> {
        self.preflight_knowledge_for_tenant(request, &MemoryTenantScope::local())
            .await
    }

    pub async fn preflight_knowledge_for_tenant(
        &self,
        request: &KnowledgePreflightRequest,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<KnowledgePreflightResult> {
        let binding = &request.binding;
        let project_spaces = if request.project_id.trim().is_empty() {
            Vec::new()
        } else {
            self.list_knowledge_spaces_for_tenant(Some(&request.project_id), tenant_scope)
                .await?
        };
        let namespace = binding
            .namespace
            .clone()
            .or_else(|| Self::select_preflight_namespace(binding, &project_spaces));
        let coverage_key = build_knowledge_coverage_key(
            &request.project_id,
            namespace.as_deref(),
            &request.task_family,
            &request.subject,
        );

        if !binding.enabled || binding.reuse_mode == KnowledgeReuseMode::Disabled {
            return Ok(KnowledgePreflightResult {
                project_id: request.project_id.clone(),
                namespace,
                task_family: request.task_family.clone(),
                subject: request.subject.clone(),
                coverage_key,
                decision: KnowledgeReuseDecision::Disabled,
                reuse_reason: None,
                skip_reason: Some("knowledge reuse is disabled for this binding".to_string()),
                freshness_reason: None,
                items: Vec::new(),
            });
        }

        let spaces = self
            .resolve_preflight_spaces(request, &coverage_key, tenant_scope)
            .await?;
        if spaces.is_empty() {
            return Ok(KnowledgePreflightResult {
                project_id: request.project_id.clone(),
                namespace,
                task_family: request.task_family.clone(),
                subject: request.subject.clone(),
                coverage_key,
                decision: KnowledgeReuseDecision::NoPriorKnowledge,
                reuse_reason: None,
                skip_reason: Some("no reusable knowledge spaces were found".to_string()),
                freshness_reason: None,
                items: Vec::new(),
            });
        }

        let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
        let mut fresh_items = Vec::new();
        let mut stale_items = Vec::new();
        let mut freshest_reason = None;

        for space in &spaces {
            let items = self
                .list_knowledge_items_for_tenant(&space.id, Some(&coverage_key), tenant_scope)
                .await?;
            let coverage = self
                .get_knowledge_coverage_for_tenant(&coverage_key, &space.id, tenant_scope)
                .await?;
            for item in items {
                if !item.status.is_active() {
                    continue;
                }
                let Some(trust_level) = item.status.as_trust_level() else {
                    continue;
                };
                if !trust_level.meets_floor(binding.trust_floor) {
                    continue;
                }
                let freshness_expires_at_ms = item.freshness_expires_at_ms.or_else(|| {
                    coverage
                        .as_ref()
                        .and_then(|coverage| coverage.freshness_expires_at_ms)
                });
                let pack_item = KnowledgePackItem {
                    item_id: item.id.clone(),
                    space_id: space.id.clone(),
                    coverage_key: item.coverage_key.clone(),
                    title: item.title.clone(),
                    summary: item.summary.clone(),
                    trust_level,
                    status: item.status.to_string(),
                    artifact_refs: item.artifact_refs.clone(),
                    source_memory_ids: item.source_memory_ids.clone(),
                    freshness_expires_at_ms,
                };
                if Self::is_fresh_enough(
                    freshness_expires_at_ms,
                    binding.freshness_ms,
                    coverage
                        .as_ref()
                        .and_then(|coverage| coverage.last_promoted_at_ms),
                    item.created_at_ms,
                    now_ms,
                ) {
                    fresh_items.push(pack_item);
                } else {
                    freshest_reason = Some(match freshness_expires_at_ms {
                        Some(expires_at_ms) => format!(
                            "coverage `{}` in space `{}` expired at {}",
                            coverage_key, space.id, expires_at_ms
                        ),
                        None => format!(
                            "coverage `{}` in space `{}` lacks freshness metadata",
                            coverage_key, space.id
                        ),
                    });
                    stale_items.push(pack_item);
                }
            }
        }

        fresh_items.sort_by(|left, right| {
            right
                .trust_level
                .rank()
                .cmp(&left.trust_level.rank())
                .then_with(|| {
                    right
                        .freshness_expires_at_ms
                        .unwrap_or(0)
                        .cmp(&left.freshness_expires_at_ms.unwrap_or(0))
                })
                .then_with(|| left.title.cmp(&right.title))
        });
        stale_items.sort_by(|left, right| {
            right
                .trust_level
                .rank()
                .cmp(&left.trust_level.rank())
                .then_with(|| left.title.cmp(&right.title))
        });

        if let Some(freshest_trust_level) = fresh_items.first().map(|item| item.trust_level) {
            let selected = fresh_items
                .into_iter()
                .take(MAX_KNOWLEDGE_PACK_ITEMS)
                .collect::<Vec<_>>();
            let decision = match freshest_trust_level {
                KnowledgeTrustLevel::ApprovedDefault => {
                    KnowledgeReuseDecision::ReuseApprovedDefault
                }
                _ => KnowledgeReuseDecision::ReusePromoted,
            };
            let selected_count = selected.len();
            return Ok(KnowledgePreflightResult {
                project_id: request.project_id.clone(),
                namespace,
                task_family: request.task_family.clone(),
                subject: request.subject.clone(),
                coverage_key,
                decision,
                reuse_reason: Some(format!(
                    "reusing {} promoted knowledge item(s) from {} space(s)",
                    selected_count,
                    spaces.len()
                )),
                skip_reason: None,
                freshness_reason: None,
                items: selected,
            });
        }

        if !stale_items.is_empty() {
            let selected = stale_items
                .into_iter()
                .take(MAX_KNOWLEDGE_PACK_ITEMS)
                .collect::<Vec<_>>();
            return Ok(KnowledgePreflightResult {
                project_id: request.project_id.clone(),
                namespace,
                task_family: request.task_family.clone(),
                subject: request.subject.clone(),
                coverage_key,
                decision: KnowledgeReuseDecision::RefreshRequired,
                reuse_reason: None,
                skip_reason: Some(
                    "prior knowledge exists but is not fresh enough to reuse".to_string(),
                ),
                freshness_reason: freshest_reason.or_else(|| {
                    Some("matching knowledge exists but freshness policy rejected it".to_string())
                }),
                items: selected,
            });
        }

        Ok(KnowledgePreflightResult {
            project_id: request.project_id.clone(),
            namespace,
            task_family: request.task_family.clone(),
            subject: request.subject.clone(),
            coverage_key,
            decision: KnowledgeReuseDecision::NoPriorKnowledge,
            reuse_reason: None,
            skip_reason: Some("no active promoted knowledge matched this coverage key".to_string()),
            freshness_reason: None,
            items: Vec::new(),
        })
    }
}
