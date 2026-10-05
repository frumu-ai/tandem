fn merge_consolidation_lineage(chunks: &[MemoryChunk], request: &ScopedMemoryConsolidationRequest)
    -> MemoryResult<Option<crate::DerivedMemoryLineage>> {
    use crate::derived_lineage::{CanonicalInputReference, DerivedMemoryLineage};
    let mut sources = Vec::new();
    let mut input_refs = Vec::new();
    let mut owner_org_unit_id = request.org_unit.clone();
    let mut found = false;
    for chunk in chunks {
        let Some(lineage) = DerivedMemoryLineage::from_metadata(chunk.metadata.as_ref())? else { continue; };
        found = true;
        lineage.all_input_refs()?;
        if lineage.owner_subject.as_ref().is_some_and(|owner| request.subject.as_ref() != Some(owner)) {
            return Err(MemoryError::InvalidConfig("consolidation lineage owner mismatch".into()));
        }
        if let Some(unit) = lineage.owner_org_unit_id {
            match &owner_org_unit_id {
                Some(current) if current != &unit => return Err(MemoryError::InvalidConfig(
                    "consolidation cannot erase conflicting department floors".into())),
                None => owner_org_unit_id = Some(unit),
                _ => {}
            }
        }
        for source in lineage.sources {
            match sources.iter().find(|known: &&crate::CanonicalMemoryRestriction| known.memory_id == source.memory_id) {
                Some(known) if known != &source => return Err(MemoryError::InvalidConfig("conflicting consolidation source".into())),
                Some(_) => {},
                None => sources.push(source),
            }
        }
        for input in lineage.input_refs {
            let same = |known: &&CanonicalInputReference| match (&input,*known) {
                (CanonicalInputReference::Memory {source:a},CanonicalInputReference::Memory {source:b}) => a.memory_id == b.memory_id,
                (CanonicalInputReference::SessionMessage {session_id:a,message_id:b,..},
                    CanonicalInputReference::SessionMessage {session_id:c,message_id:d,..}) => a == c && b == d,
                _ => false,
            };
            match input_refs.iter().find(same) {
                Some(known) if known != &input => return Err(MemoryError::InvalidConfig("conflicting consolidation input".into())),
                Some(_) => {},
                None => input_refs.push(input),
            }
        }
        if sources.len() > crate::MAX_DERIVED_MEMORY_SOURCES || input_refs.len() > crate::MAX_DERIVED_MEMORY_INPUTS {
            return Err(MemoryError::InvalidConfig("consolidation lineage exceeds bounds".into()));
        }
    }
    if !found { return Ok(None); }
    let lineage = DerivedMemoryLineage::new(request.subject.clone(),owner_org_unit_id,sources,input_refs)?;
    lineage.all_input_refs()?;
    Ok(Some(lineage))
}

fn consolidation_semantic_classes(lineage: &crate::DerivedMemoryLineage)
    -> MemoryResult<Vec<tandem_data_boundary::SensitiveDataClass>> {
    use tandem_data_boundary::SensitiveDataClass as Semantic;
    use tandem_enterprise_contract::DataClass;
    let mut classes = Vec::new();
    for class in lineage.source_data_classes()? {
        let mapped = match class {
            DataClass::Public | DataClass::Internal => None,
            DataClass::CustomerData => Some(Semantic::CustomerData),
            DataClass::SourceCode => Some(Semantic::SourceCode),
            DataClass::FinancialRecord => Some(Semantic::Financial),
            DataClass::Credential => Some(Semantic::Credential),
            DataClass::Regulated => Some(Semantic::UnknownSensitive),
            DataClass::Confidential | DataClass::Restricted | DataClass::Executive => Some(Semantic::ProprietaryBusinessData),
        };
        if let Some(class) = mapped { if !classes.contains(&class) { classes.push(class); } }
    }
    Ok(classes)
}

fn plain_ordinary_consolidation_chunk(chunk: &MemoryChunk) -> bool {
    // The canonical lineage DTO names memory rows and native session messages,
    // not arbitrary legacy chunks. Only default/Internal, exactly scoped chat
    // chunks have no additional disposition to carry into the summary.
    if chunk.source_path.is_some() || chunk.source_mtime.is_some()
        || chunk.source_size.is_some() || chunk.source_hash.is_some() {
        return false;
    }
    let Some(metadata) = chunk.metadata.as_ref() else { return true; };
    let Some(object) = metadata.as_object() else { return false; };
    if object.keys().any(|key| !matches!(key.as_str(),
        "owner_subject" | "owner_org_unit_id" | "tenant_shared" | "classification")) {
        return false;
    }
    if object.get("classification").is_some()
        && crate::types::data_class_from_metadata(Some(metadata))
            != Some(tandem_enterprise_contract::DataClass::Internal) {
        return false;
    }
    for key in ["owner_subject", "owner_org_unit_id"] {
        if object.get(key).is_some_and(|value| !value.is_null()
            && !value.as_str().is_some_and(|owner| !owner.trim().is_empty() && owner.trim() == owner)) {
            return false;
        }
    }
    !object.get("tenant_shared").is_some_and(|value| !value.is_boolean())
}

impl MemoryManager {
    async fn authorize_consolidation_lineage(&self, lineage: &crate::DerivedMemoryLineage,
        scope: &MemoryReadScope, request: &ScopedMemoryConsolidationRequest,
        access_filter: Option<&crate::types::MemoryAccessFilter>) -> MemoryResult<crate::types::MemoryAccessFilter> {
        let filter = access_filter.ok_or_else(|| MemoryError::InvalidConfig(
            "derived consolidation requires governed access authority".into()))?;
        if filter.mode == crate::types::GovernedReadMode::GovernedStrict
            && filter.caller_subject != request.subject {
            return Err(MemoryError::InvalidConfig("consolidation caller subject mismatch".into()));
        }
        let inputs = lineage.all_input_refs()?;
        let native = inputs.iter().any(|input| matches!(input,crate::CanonicalInputReference::SessionMessage { .. }));
        let mut current_filter = filter.clone();
        current_filter.now_ms = Utc::now().timestamp_millis().max(0) as u64;
        let mut resolved = if let Some(resolver) = &self.derived_memory_access_resolver {
            resolver(self.store.clone(),scope.clone(),lineage.clone(),current_filter).await
        } else if native {
            None
        } else {
            crate::resolve_derived_lineage(self.store.as_ref(),scope,lineage).await.ok()
                .map(|proof| current_filter.with_resolved_derived_lineage(proof))
        }.ok_or_else(|| MemoryError::InvalidConfig("consolidation lineage authority unavailable".into()))?;
        resolved.now_ms = Utc::now().timestamp_millis().max(0) as u64;
        let decision = resolved.decision_for_derived_lineage(lineage);
        if !decision.allowed {
            return Err(MemoryError::InvalidConfig(format!("consolidation lineage denied:{}",
                decision.reason.as_deref().unwrap_or("denied"))));
        }
        let partition = crate::MemoryPartition {org_id:request.tenant_scope.org_id.clone(),
            workspace_id:request.tenant_scope.workspace_id.clone(),project_id:request.project_id.clone(),
            tier:crate::GovernedMemoryTier::Project};
        let decision = lineage.write_scope_decision(&partition,Utc::now().timestamp_millis().max(0) as u64)?;
        if !decision.allowed {
            return Err(MemoryError::InvalidConfig(format!("consolidation lineage write denied:{}",decision.reason_code)));
        }
        Ok(resolved)
    }

    async fn authorize_consolidation_contributors(&self, chunks: &[MemoryChunk],
        scope: &MemoryReadScope, request: &ScopedMemoryConsolidationRequest,
        access_filter: Option<&crate::types::MemoryAccessFilter>) -> MemoryResult<()> {
        let Some(filter) = access_filter else { return Ok(()); };
        let mut derived = false;
        let mut ordinary = false;
        let mut restricted_ordinary = false;
        for chunk in chunks {
            let mut current = filter.clone();
            current.now_ms = Utc::now().timestamp_millis().max(0) as u64;
            match crate::DerivedMemoryLineage::from_metadata(chunk.metadata.as_ref())? {
                Some(lineage) => {
                    derived = true;
                    current = self.authorize_consolidation_lineage(
                        &lineage, scope, request, Some(&current)).await?;
                }
                None => {
                    ordinary = true;
                    restricted_ordinary |= !plain_ordinary_consolidation_chunk(chunk);
                }
            }
            current.now_ms = Utc::now().timestamp_millis().max(0) as u64;
            let decision = current.decision_for_chunk(chunk);
            if !decision.allowed {
                return Err(MemoryError::InvalidConfig(format!("consolidation contributor denied:{}",
                    decision.reason.as_deref().unwrap_or("denied"))));
            }
        }
        if derived && ordinary {
            return Err(MemoryError::InvalidConfig(
                "consolidation mixed contributors lack canonical lineage".into()));
        }
        if restricted_ordinary {
            return Err(MemoryError::InvalidConfig(
                "consolidation ordinary restrictions lack canonical lineage".into()));
        }
        Ok(())
    }
}

impl MemoryManager {
    /// Consolidate visible session memory into a summary with the same trusted
    /// ownership scope. Summary creation and source cleanup commit atomically.
    pub async fn consolidate_scoped_session(
        &self,
        request: &ScopedMemoryConsolidationRequest,
        providers: &ProviderRegistry,
        config: &MemoryConsolidationConfig,
        provider_egress: &MemoryProviderEgressContext,
    ) -> MemoryResult<Option<String>> {
        self.consolidate_scoped_session_with_access_filter(request,providers,config,provider_egress,None).await
    }

    /// Authorized derived contributors retain their complete source conjunction.
    /// The compatibility wrapper remains sufficient for ordinary scoped chunks.
    /// Governed consolidation accepts all-derived contributors, or only plain
    /// default/Internal ordinary chunks. Mixed and restricted ordinary inputs
    /// fail closed until their canonical chunk dispositions can be represented.
    pub async fn consolidate_scoped_session_with_access_filter(
        &self,
        request: &ScopedMemoryConsolidationRequest,
        providers: &ProviderRegistry,
        config: &MemoryConsolidationConfig,
        provider_egress: &MemoryProviderEgressContext,
        access_filter: Option<&crate::types::MemoryAccessFilter>,
    ) -> MemoryResult<Option<String>> {
        if !config.enabled {
            return Ok(None);
        }
        if request.session_id.trim().is_empty() || request.project_id.trim().is_empty() {
            return Err(MemoryError::InvalidConfig(
                "memory consolidation requires non-empty session and project ids".to_string(),
            ));
        }

        let read_scope = MemoryReadScope {
            tenant: request.tenant_scope.clone(),
            org_unit: request.org_unit.clone(),
            subject: request.subject.clone(),
            access: crate::store::MemoryReadAccess::Scoped,
        };

        let chunks = self
            .read_chunks(
                MemoryChunkSelector::session_in_project(
                    &request.session_id,
                    &request.project_id,
                ),
                read_scope.clone(),
                None,
            )
            .await?;
        let chunks = chunks
            .into_iter()
            .filter(|chunk| consolidation_chunk_has_exact_ownership(chunk, request))
            .collect::<Vec<_>>();
        if chunks.is_empty() {
            return Ok(None);
        }
        self.authorize_consolidation_contributors(&chunks,&read_scope,request,access_filter).await?;
        let lineage = merge_consolidation_lineage(&chunks,request)?;
        let mut effective_egress = provider_egress.clone();
        if let Some(lineage) = &lineage {
            self.authorize_consolidation_lineage(lineage,&read_scope,request,access_filter).await?;
            effective_egress = effective_egress.with_additional_data_classes(consolidation_semantic_classes(lineage)?);
        }

        // Assemble text
        let mut text_parts = Vec::new();
        for chunk in &chunks {
            text_parts.push(chunk.content.clone());
        }
        let full_text = text_parts.join("\n\n---\n\n");

        // Build prompt
        let prompt = format!(
            "Please provide a concise but comprehensive summary of the following chat session. \
            Focus on the key decisions, technical details, code changes, and unresolved issues. \
            Do NOT include conversational filler, greetings, or sign-offs. \
            This summary will be used as long-term memory to recall the context of this work.\n\n\
            Session transcripts:\n\n{}",
            full_text
        );

        let provider_override = config.provider.as_deref().filter(|s| !s.is_empty());
        let model_override = config.model.as_deref().filter(|s| !s.is_empty());

        let operation_id = format!("{}:memory_consolidation", request.session_id);
        let summary_text = match complete_memory_prompt(
            providers,
            &prompt,
            provider_override,
            model_override,
            Some(&effective_egress),
            MemoryProviderEgressKind::Consolidation,
            &operation_id,
            "memory.session_consolidation",
        )
        .await
        {
            Ok(s) => s,
            Err(error @ MemoryError::TenantScopeViolation(_)) => return Err(error),
            Err(e) => {
                tracing::warn!(
                    "Memory consolidation LLM failed for session {}: {e}",
                    request.session_id
                );
                return Ok(None);
            }
        };

        if summary_text.trim().is_empty() {
            return Ok(None);
        }

        // Generate embedding for the summary
        let embedding = {
            let service = self.embedding_service.lock().await;
            service
                .embed(&summary_text)
                .await
                .map_err(|e| crate::types::MemoryError::Embedding(e.to_string()))?
        };

        // Store the summary chunk
        let source_chunk_ids = chunks
            .iter()
            .map(|chunk| chunk.id.clone())
            .collect::<Vec<_>>();
        let mut metadata = serde_json::Map::new();
        if let Some(org_unit) = request.org_unit.as_ref() {
            metadata.insert(
                crate::types::OWNER_ORG_UNIT_METADATA_KEY.to_string(),
                serde_json::Value::String(org_unit.clone()),
            );
        }
        if let Some(subject) = request.subject.as_ref() {
            metadata.insert(
                crate::types::OWNER_SUBJECT_METADATA_KEY.to_string(),
                serde_json::Value::String(subject.clone()),
            );
        } else if request.org_unit.is_none() {
            metadata.insert(
                crate::types::TENANT_SHARED_METADATA_KEY.to_string(),
                serde_json::Value::Bool(true),
            );
        }
        metadata.insert(
            "consolidation_provenance".to_string(),
            serde_json::json!({
                "session_id": request.session_id,
                "source_chunk_ids": source_chunk_ids,
                "source_count": chunks.len(),
                "tenant_context": {
                    "org_id": request.tenant_scope.org_id,
                    "workspace_id": request.tenant_scope.workspace_id,
                    "deployment_id": request.tenant_scope.deployment_id,
                }
            }),
        );
        let mut output_metadata = Some(serde_json::Value::Object(metadata));
        if let Some(lineage) = &lineage {
            if let Some(metadata) = output_metadata.as_mut() {
                metadata["classification"] = serde_json::to_value(lineage.output_data_class())?;
            }
            output_metadata = crate::metadata_with_derived_lineage(output_metadata,lineage)?;
        }

        let chunk = MemoryChunk {
            id: uuid::Uuid::new_v4().to_string(),
            content: summary_text.clone(),
            tier: MemoryTier::Project,
            session_id: None,
            project_id: Some(request.project_id.clone()),
            created_at: Utc::now(),
            source: "consolidation".to_string(),
            token_count: self.count_tokens(&summary_text) as i64,
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            tenant_scope: request.tenant_scope.clone(),
            subject: request.subject.clone(),
            metadata: output_metadata,
        };

        self.authorize_consolidation_contributors(&chunks,&read_scope,request,access_filter).await?;
        if let Some(lineage) = &lineage {
            self.authorize_consolidation_lineage(lineage,&read_scope,request,access_filter).await?;
        }

        match self
            .store
            .mutate(MemoryStoreMutationRequest::ReplaceSessionWithSummary {
                scope: read_scope,
                session_id: request.session_id.clone(),
                project_id: request.project_id.clone(),
                source_chunk_ids,
                summary_scope: Self::chunk_write_scope(&chunk),
                summary: Box::new(chunk),
                embedding,
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreMutationResult::Affected(_) => {}
            _ => return Err(Self::unexpected_store_result("consolidate session")),
        }

        tracing::info!(
            "Session {} consolidated into a scoped summary chunk",
            request.session_id
        );

        Ok(Some(summary_text))
    }
}
