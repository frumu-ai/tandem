struct GovernedDistillationWriter {
    state: AppState,
    tenant_context: TenantContext,
    verified_tenant_context: Option<VerifiedTenantContext>,
    partition: tandem_memory::MemoryPartition,
    capability: MemoryCapabilityToken,
    run_id: String,
    workflow_id: Option<String>,
    artifact_refs: Vec<String>,
    subject: String,
    lineage: DerivedMemoryLineage,
}

impl GovernedDistillationWriter {
    async fn source_binding(&self, session_id: &str) -> MemoryResult<WorkflowLearningCandidateSourceBinding> {
        workflow_learning_distillation_source_binding(
            &self.state, &self.tenant_context, self.verified_tenant_context.as_ref(),
            self.workflow_id.as_deref(), session_id, &self.subject,
        ).await.map_err(|status| tandem_memory::types::MemoryError::InvalidConfig(
            format!("distillation_source_binding_denied: {status}"),
        ))
    }

    async fn ensure_current_lineage(&self) -> MemoryResult<MemoryAccessFilter> {
        let write_scope = self.lineage.write_scope_decision(&self.partition, crate::now_ms())?;
        if !write_scope.allowed {
            return Err(tandem_memory::types::MemoryError::InvalidConfig(write_scope.reason_code));
        }
        if self.state.enterprise.hosted_policy.authorize(self.verified_tenant_context.as_ref()).is_err() {
            return Err(tandem_memory::types::MemoryError::InvalidConfig("distillation_authority_stale".into()));
        }
        let store = open_global_memory_store_for_state(&self.state).await.ok_or_else(||
            tandem_memory::types::MemoryError::InvalidConfig("global memory db unavailable".into()))?;
        let scope = distillation_read_scope(&self.tenant_context, self.verified_tenant_context.as_ref(), &self.subject)
            .map_err(|_| tandem_memory::types::MemoryError::InvalidConfig("distillation_scope_denied".into()))?;
        let filter = with_verified_memory_decrypt_principal(self.verified_tenant_context.as_ref(),
            crate::memory::derived_lineage::resolved_filter_for_lineage(
                &self.state, &self.tenant_context, store.as_ref(), &scope, &self.lineage,
                distillation_access_filter(self.verified_tenant_context.as_ref(), &self.subject),
            ),
        ).await.ok_or_else(|| tandem_memory::types::MemoryError::InvalidConfig("distillation_source_changed".into()))?;
        Ok(filter)
    }

    async fn upsert_memory_fact_candidate(
        &self,
        session_id: &str,
        fact: &DistilledFact,
        memory_id: Option<String>,
        fingerprint: &str,
    ) -> MemoryResult<String> {
        let workflow_id = self
            .workflow_id
            .clone()
            .unwrap_or_else(|| format!("session:{}", session_id.trim()));
        let candidate = WorkflowLearningCandidate {
            candidate_id: format!("wflearn-{}", Uuid::new_v4()),
            workflow_id,
            project_id: self.partition.project_id.clone(),
            source_run_id: self.run_id.clone(),
            source_binding: Some(self.source_binding(session_id).await?),
            kind: WorkflowLearningCandidateKind::MemoryFact,
            status: WorkflowLearningCandidateStatus::Proposed,
            confidence: fact.importance_score,
            summary: fact.content.clone(),
            fingerprint: fingerprint.to_string(),
            node_id: None,
            node_kind: None,
            validator_family: None,
            evidence_refs: vec![json!({
                "session_id": session_id,
                "run_id": self.run_id,
                "distillation_id": fact.distillation_id,
                "fact_id": fact.id,
                "fact_category": fact.category,
            })],
            artifact_refs: self.artifact_refs.clone(),
            proposed_memory_payload: Some(json!({
                "content": fact.content,
                "kind": "fact",
                "classification": distillation_classification(&self.lineage),
                "private": self.lineage.owner_subject.is_some(),
                "metadata": metadata_with_derived_lineage(None, &self.lineage)?,
            })),
            proposed_revision_prompt: None,
            source_memory_id: memory_id,
            promoted_memory_id: None,
            needs_plan_bundle: false,
            baseline_before: None,
            latest_observed_metrics: None,
            last_revision_session_id: None,
            run_ids: vec![self.run_id.clone()],
            created_at_ms: crate::now_ms(),
            updated_at_ms: crate::now_ms(),
        };
        // Binding preparation awaited canonical storage. Refresh the complete
        // source proof before taking either candidate/publication writer lock.
        let filter = self.ensure_current_lineage().await?;
        let lineage = self.lineage.clone();
        let partition = self.partition.clone();
        let capability_expires_at_ms = self.capability.expires_at;
        let authority = derived_memory_commit_authority_with_lineage(
            &self.state, &self.tenant_context, self.verified_tenant_context.as_ref(),
            self.lineage.clone(), filter, None,
            move |now| now < capability_expires_at_ms
                && lineage.write_scope_decision(&partition, now).is_ok_and(|decision| decision.allowed),
        );
        self.state
            .upsert_workflow_learning_candidate_with_commit_authority(
                candidate, authority,
            )
            .await
            .map(|candidate| candidate.candidate_id)
            .map_err(|error| tandem_memory::types::MemoryError::InvalidConfig(error.to_string()))
    }

    async fn store_fact(
        &self,
        session_id: &str,
        fact: &DistilledFact,
    ) -> MemoryResult<tandem_memory::DistillationMemoryWrite> {
        self.ensure_current_lineage().await?;
        let content_hash = hash_text(&fact.content);
        let fact_category = fact.category.to_string();
        let fingerprint = hash_text(&format!(
            "{}:{}:{}:{}:{}",
            self.partition.project_id,
            self.workflow_id.as_deref().unwrap_or(session_id),
            fact.category,
            fact.content,
            self.lineage.digest()?
        ));
        let store = open_global_memory_store_for_state(&self.state)
            .await
            .ok_or_else(|| {
                tandem_memory::types::MemoryError::InvalidConfig(
                    "global memory db unavailable".to_string(),
                )
            })?;
        let mut scope = tandem_memory::MemoryReadScope::tenant(MemoryTenantScope {
            org_id: self.tenant_context.org_id.clone(),
            workspace_id: self.tenant_context.workspace_id.clone(),
            deployment_id: self.tenant_context.deployment_id.clone(),
        });
        scope.subject = Some(self.subject.clone());
        scope.org_unit = crate::memory::subject::active_org_unit(
            self.verified_tenant_context.as_ref(),
        );
        let existing = match with_verified_memory_decrypt_principal(
            self.verified_tenant_context.as_ref(),
            store.query(tandem_memory::MemoryStoreQueryRequest::ListGlobalRecords {
                scope: scope.clone(),
                user_id: self.subject.clone(),
                query: None,
                project_tag: Some(self.partition.project_id.clone()),
                channel_tag: None,
                limit: 200,
                offset: 0,
            }),
        )
            .await
            .map_err(|error| tandem_memory::types::MemoryError::InvalidConfig(error.to_string()))?
        {
            tandem_memory::MemoryStoreQueryResult::GlobalRecords(records) => records,
            _ => {
                return Err(tandem_memory::types::MemoryError::InvalidConfig(
                    "memory store returned an unexpected global-record list result".to_string(),
                ));
            }
        }
        .into_iter()
            .find(|record| {
                record.content_hash == content_hash
                    && tandem_memory::types::owner_subject_from_metadata(record.metadata.as_ref()) == self.lineage.owner_subject
                    && tandem_memory::types::owner_org_unit_id_from_metadata(record.metadata.as_ref()) == self.lineage.owner_org_unit_id
                    && DerivedMemoryLineage::from_metadata(record.metadata.as_ref()).ok().flatten().as_ref() == Some(&self.lineage)
                    && record
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("origin"))
                        .and_then(Value::as_str)
                        == Some("session_distillation")
                    && record
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("fact_category"))
                        .and_then(Value::as_str)
                        == Some(fact_category.as_str())
                    && record
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("workflow_id"))
                        .and_then(Value::as_str)
                        == self.workflow_id.as_deref()
            });

        if let Some(existing) = existing {
            let filter = self.ensure_current_lineage().await?;
            let mut next_metadata = existing.metadata.clone().unwrap_or_else(|| json!({}));
            if let Some(object) = next_metadata.as_object_mut() {
                object.insert("fingerprint".to_string(), json!(fingerprint));
                object.insert("artifact_refs".to_string(), json!(self.artifact_refs));
                object.insert("session_id".to_string(), json!(session_id));
                object.insert("workflow_id".to_string(), json!(self.workflow_id));
                object.insert("last_distilled_at_ms".to_string(), json!(crate::now_ms()));
            }
            // Stamp the active department on the dedupe/update path too (TAN-646),
            // so a repeated fact matching a pre-TAN-646 (unstamped) row gets its
            // owner_org_unit_id set rather than staying tenant-wide. An existing
            // department is preserved (first-collector wins); the update re-derives
            // the column from this metadata.
            next_metadata = memory_metadata_with_owner_org_unit(
                Some(next_metadata),
                self.lineage.owner_org_unit_id.as_deref(),
            )
            .unwrap_or_else(|| json!({}));
            let target_reference = tandem_memory::CanonicalMemoryRestriction::from_global_record(
                &existing, &scope.tenant,
            )?.source_reference();
            let policy_metadata = next_metadata.clone();
            let partition = self.partition.clone();
            let lineage = self.lineage.clone();
            let capability_expires_at_ms = self.capability.expires_at;
            let require_scope_metadata = crate::memory::policy_status::current_memory_context_policy_status().strict_required;
            let mutation = tandem_memory::MemoryStoreMutationRequest::UpdateGlobalRecordContext {
                scope, id: existing.id.clone(), visibility: existing.visibility.clone(),
                demoted: existing.demoted, metadata: Some(next_metadata), provenance: existing.provenance.clone(),
            };
            let verified = self.verified_tenant_context.clone();
            let authority = derived_memory_commit_authority_with_lineage(
                &self.state, &self.tenant_context, self.verified_tenant_context.as_ref(),
                self.lineage.clone(), filter, Some(existing.clone()),
                move |now| now < capability_expires_at_ms
                    && lineage.write_scope_decision(&partition, now).is_ok_and(|decision| decision.allowed)
                    && tandem_memory::memory_write_scope_decision_for_context_with_enterprise_mode(
                        &partition, Some(&policy_metadata), None, require_scope_metadata, now,
                    ).is_ok_and(|decision| decision.allowed),
            );
            let changed = commit_derived_memory_with_current_policy(
                &self.state, &self.tenant_context, self.verified_tenant_context.as_ref(),
                async move { with_verified_memory_decrypt_principal(
                    verified.as_ref(), store.mutate_with_commit_authority_if_unchanged(
                        mutation, target_reference, authority,
                    ),
                ).await },
            ).await.map_err(|status| tandem_memory::types::MemoryError::InvalidConfig(
                format!("distillation_commit_denied: {status}"),
            ))?
                .map_err(|error| {
                    tandem_memory::types::MemoryError::InvalidConfig(error.to_string())
                })?;
            if !matches!(changed, tandem_memory::MemoryStoreMutationResult::Changed(true)) {
                return Err(tandem_memory::types::MemoryError::InvalidConfig("distillation_dedupe_source_disappeared".into()));
            }
            self.ensure_current_lineage().await?;
            let candidate_id = self
                .upsert_memory_fact_candidate(
                    session_id,
                    fact,
                    Some(existing.id.clone()),
                    &fingerprint,
                )
                .await?;
            return Ok(tandem_memory::DistillationMemoryWrite {
                stored: false,
                deduped: true,
                memory_id: Some(existing.id),
                candidate_id: Some(candidate_id),
            });
        }

        let request = MemoryPutRequest {
            private: self.lineage.owner_subject.is_some(),
            run_id: self.run_id.clone(),
            partition: self.partition.clone(),
            kind: tandem_memory::MemoryContentKind::Fact,
            content: fact.content.clone(),
            artifact_refs: self.artifact_refs.clone(),
            classification: distillation_classification(&self.lineage),
            authority_job_context: None,
            metadata: memory_metadata_with_owner_org_unit(metadata_with_derived_lineage(Some(json!({
                "origin": "session_distillation",
                "fact_category": fact.category,
                "session_id": session_id,
                "run_id": self.run_id,
                "workflow_id": self.workflow_id,
                "artifact_refs": self.artifact_refs,
                "fingerprint": fingerprint,
                "distillation_id": fact.distillation_id,
                "fact_id": fact.id,
                "source_message_ids": fact.source_message_ids,
                "extraction_confidence": fact.importance_score,
                "tenant_shared": self.lineage.owner_org_unit_id.is_none(),
                "source_data_classes": distillation_source_data_classes(&self.lineage),
            })), &self.lineage)?, self.lineage.owner_org_unit_id.as_deref()),
        };
        let response = memory_put_impl_with_verified(
            &self.state,
            &self.tenant_context,
            self.verified_tenant_context.as_ref(),
            request,
            Some(self.capability.clone()),
        )
        .await
        .map_err(|status| {
            tandem_memory::types::MemoryError::InvalidConfig(format!(
                "memory_put failed with status {status}"
            ))
        })?;
        self.ensure_current_lineage().await?;
        let candidate_id = self
            .upsert_memory_fact_candidate(session_id, fact, Some(response.id.clone()), &fingerprint)
            .await?;
        Ok(tandem_memory::DistillationMemoryWrite {
            stored: response.stored,
            deduped: !response.stored,
            memory_id: Some(response.id),
            candidate_id: Some(candidate_id),
        })
    }
}

#[async_trait]
impl tandem_memory::DistillationMemoryWriter for GovernedDistillationWriter {
    async fn store_user_fact(
        &self,
        session_id: &str,
        fact: &DistilledFact,
    ) -> MemoryResult<tandem_memory::DistillationMemoryWrite> {
        self.store_fact(session_id, fact).await
    }

    async fn store_agent_fact(
        &self,
        session_id: &str,
        fact: &DistilledFact,
    ) -> MemoryResult<tandem_memory::DistillationMemoryWrite> {
        self.store_fact(session_id, fact).await
    }
}
