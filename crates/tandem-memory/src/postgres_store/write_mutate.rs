use pgvector::Vector;

use super::*;
use crate::types::{
    owner_org_unit_id_from_metadata, owner_subject_from_metadata, tenant_shared_from_metadata,
    CleanupLogEntry, ClearFileIndexResult, GlobalMemoryWriteResult, MemoryLayer, MemoryNode,
    SourceObjectLifecycleRecord, SourceObjectLifecycleState,
};

fn deployment(scope: &crate::types::MemoryTenantScope) -> &str {
    scope.deployment_id.as_deref().unwrap_or("")
}

fn selector_tier(selector: &MemoryChunkSelector) -> String {
    serde_json::to_value(selector.tier)
        .ok()
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_else(|| "session".to_string())
}

impl PostgresMemoryStore {
    async fn upsert_entity<T: serde::Serialize>(
        &self,
        scope: &MemoryWriteScope,
        entity_type: &str,
        key1: &str,
        key2: &str,
        value: &T,
    ) -> MemoryStoreResult<()> {
        let client = self.client().await?;
        client
            .execute(
                "INSERT INTO tandem_memory_entities
                    (tenant_org_id,tenant_workspace_id,tenant_deployment_id,entity_type,key1,key2,data)
                 VALUES ($1,$2,$3,$4,$5,$6,$7)
                 ON CONFLICT (tenant_org_id,tenant_workspace_id,tenant_deployment_id,entity_type,key1,key2)
                 DO UPDATE SET data=EXCLUDED.data, updated_at=now()",
                &[
                    &scope.tenant.org_id,
                    &scope.tenant.workspace_id,
                    &deployment(&scope.tenant),
                    &entity_type,
                    &key1,
                    &key2,
                    &json_value(value)?,
                ],
            )
            .await
            .map_err(|error| store_error("write PostgreSQL memory entity", error, true))?;
        Ok(())
    }

    pub(super) async fn write_impl(
        &self,
        request: MemoryStoreWriteRequest,
    ) -> MemoryStoreResult<MemoryStoreWriteResult> {
        match request {
            MemoryStoreWriteRequest::Chunk {
                scope,
                chunk,
                embedding,
            } => {
                if scope.tenant != chunk.tenant_scope
                    || scope.subject != chunk.subject
                    || scope.org_unit != owner_org_unit_id_from_metadata(chunk.metadata.as_ref())
                {
                    return Err(MemoryStoreError::new(
                        MemoryStoreErrorKind::ScopeViolation,
                        "chunk ownership does not match the PostgreSQL write scope",
                    ));
                }
                if embedding.len() != self.embedding_dimension {
                    return Err(MemoryStoreError::invalid(format!(
                        "embedding dimension mismatch: expected {}, got {}",
                        self.embedding_dimension,
                        embedding.len()
                    )));
                }
                let (vector, ciphertext, envelope, policy_id, audit_id) = match self
                    .search_surface_mode
                {
                    PostgresSearchSurfaceMode::PlaintextPgvector => {
                        (Some(Vector::from(embedding)), None, None, None, None)
                    }
                    PostgresSearchSurfaceMode::EncryptedRerank => {
                        let (ciphertext, envelope, policy_id, audit_id) = self.encrypt_embedding(
                            &embedding,
                            &scope.tenant,
                            scope.org_unit.clone(),
                            scope.subject.clone(),
                            &chunk.id,
                        )?;
                        (
                            None,
                            Some(ciphertext),
                            envelope.map(|value| json_value(&value)).transpose()?,
                            Some(policy_id),
                            Some(audit_id),
                        )
                    }
                    PostgresSearchSurfaceMode::Disabled => (None, None, None, None, None),
                };
                let (data, data_ciphertext, data_envelope, data_policy_id, data_audit_id) = self
                    .encode_payload(
                        &chunk,
                        &scope.tenant,
                        scope.org_unit.clone(),
                        scope.subject.clone(),
                        &chunk.id,
                    )?;
                let tenant_shared = tenant_shared_from_metadata(chunk.metadata.as_ref());
                let client = self.client().await?;
                let changed = client
                    .execute(
                        "INSERT INTO tandem_memory_chunks
                       (id,tenant_org_id,tenant_workspace_id,tenant_deployment_id,
                        owner_org_unit_id,owner_subject,tenant_shared,tier,project_id,session_id,source_path,
                        created_at,data,data_ciphertext,data_envelope,data_policy_decision_id,data_audit_id,
                        embedding,embedding_ciphertext,embedding_envelope,search_policy_decision_id,search_audit_id)
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22)
                     ON CONFLICT (id) DO UPDATE SET data=EXCLUDED.data,
                       data_ciphertext=EXCLUDED.data_ciphertext,data_envelope=EXCLUDED.data_envelope,
                       data_policy_decision_id=EXCLUDED.data_policy_decision_id,
                       data_audit_id=EXCLUDED.data_audit_id,
                       embedding=EXCLUDED.embedding,
                       embedding_ciphertext=EXCLUDED.embedding_ciphertext,
                       embedding_envelope=EXCLUDED.embedding_envelope,
                       search_policy_decision_id=EXCLUDED.search_policy_decision_id,
                       search_audit_id=EXCLUDED.search_audit_id,
                       tenant_shared=EXCLUDED.tenant_shared,
                       created_at=EXCLUDED.created_at
                     WHERE tandem_memory_chunks.tenant_org_id=EXCLUDED.tenant_org_id
                       AND tandem_memory_chunks.tenant_workspace_id=EXCLUDED.tenant_workspace_id
                       AND tandem_memory_chunks.tenant_deployment_id=EXCLUDED.tenant_deployment_id
                       AND tandem_memory_chunks.owner_org_unit_id IS NOT DISTINCT FROM EXCLUDED.owner_org_unit_id
                       AND tandem_memory_chunks.owner_subject IS NOT DISTINCT FROM EXCLUDED.owner_subject
                       AND tandem_memory_chunks.tier=EXCLUDED.tier
                       AND tandem_memory_chunks.project_id IS NOT DISTINCT FROM EXCLUDED.project_id
                       AND tandem_memory_chunks.session_id IS NOT DISTINCT FROM EXCLUDED.session_id",
                        &[
                            &chunk.id,
                            &scope.tenant.org_id,
                            &scope.tenant.workspace_id,
                            &deployment(&scope.tenant),
                            &scope.org_unit,
                            &scope.subject,
                            &tenant_shared,
                            &selector_tier(&MemoryChunkSelector {
                                tier: chunk.tier,
                                project_id: chunk.project_id.clone(),
                                session_id: chunk.session_id.clone(),
                            }),
                            &chunk.project_id,
                            &chunk.session_id,
                            &chunk.source_path,
                            &chunk.created_at,
                            &data,
                            &data_ciphertext,
                            &data_envelope,
                            &data_policy_id,
                            &data_audit_id,
                            &vector,
                            &ciphertext,
                            &envelope,
                            &policy_id,
                            &audit_id,
                        ],
                    )
                    .await
                    .map_err(|error| store_error("write PostgreSQL memory chunk", error, true))?;
                if changed == 0 {
                    return Err(MemoryStoreError::new(
                        MemoryStoreErrorKind::Conflict,
                        "chunk id already exists in another PostgreSQL ownership scope",
                    ));
                }
                Ok(MemoryStoreWriteResult::Stored)
            }
            MemoryStoreWriteRequest::GlobalRecord { scope, record } => {
                let tenant = tenant_scope_from_global_record(&record);
                let owner_org = owner_org_unit_id_from_metadata(record.metadata.as_ref());
                let owner_subject = owner_subject_from_metadata(record.metadata.as_ref());
                if tenant != scope.tenant
                    || owner_org != scope.org_unit
                    || owner_subject != scope.subject
                {
                    return Err(MemoryStoreError::new(
                        MemoryStoreErrorKind::ScopeViolation,
                        "global record ownership does not match the PostgreSQL write scope",
                    ));
                }
                let client = self.client().await?;
                let existing = client
                    .query_opt(
                        "SELECT id FROM tandem_memory_global_records WHERE tenant_org_id=$1
                     AND tenant_workspace_id=$2 AND tenant_deployment_id=$3 AND user_id=$4
                     AND source_type=$5 AND content_hash=$6 AND run_id=$7
                     AND COALESCE(session_id,'')=COALESCE($8,'')
                     AND COALESCE(message_id,'')=COALESCE($9,'')
                     AND COALESCE(tool_name,'')=COALESCE($10,'')
                     AND COALESCE(owner_org_unit_id,'')=COALESCE($11,'')
                     AND private=$12 AND COALESCE(owner_subject,'')=COALESCE($13,'') LIMIT 1",
                        &[
                            &tenant.org_id,
                            &tenant.workspace_id,
                            &deployment(&tenant),
                            &record.user_id,
                            &record.source_type,
                            &record.content_hash,
                            &record.run_id,
                            &record.session_id,
                            &record.message_id,
                            &record.tool_name,
                            &owner_org,
                            &owner_subject.is_some(),
                            &owner_subject,
                        ],
                    )
                    .await
                    .map_err(|error| store_error("dedupe PostgreSQL global memory", error, true))?;
                if let Some(row) = existing {
                    return Ok(MemoryStoreWriteResult::GlobalRecord(
                        GlobalMemoryWriteResult {
                            id: row.get(0),
                            stored: false,
                            deduped: true,
                        },
                    ));
                }
                let (data, data_ciphertext, data_envelope, data_policy_id, data_audit_id) = self
                    .encode_payload(
                        &record,
                        &tenant,
                        owner_org.clone(),
                        owner_subject.clone(),
                        &record.id,
                    )?;
                let search_content =
                    if self.search_surface_mode == PostgresSearchSurfaceMode::PlaintextPgvector {
                        record.content.as_str()
                    } else {
                        ""
                    };
                client.execute(
                    "INSERT INTO tandem_memory_global_records
                     (id,tenant_org_id,tenant_workspace_id,tenant_deployment_id,owner_org_unit_id,
                      owner_subject,private,user_id,source_type,content_hash,run_id,session_id,message_id,
                      tool_name,project_tag,channel_tag,demoted,expires_at_ms,created_at_ms,search_content,
                      data,data_ciphertext,data_envelope,data_policy_decision_id,data_audit_id)
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25)",
                    &[&record.id,&tenant.org_id,&tenant.workspace_id,&deployment(&tenant),&owner_org,
                      &owner_subject,&owner_subject.is_some(),&record.user_id,&record.source_type,
                      &record.content_hash,&record.run_id,&record.session_id,&record.message_id,
                      &record.tool_name,&record.project_tag,&record.channel_tag,&record.demoted,
                      &record.expires_at_ms.map(|value| value as i64),&(record.created_at_ms as i64),
                      &search_content,&data,&data_ciphertext,&data_envelope,&data_policy_id,&data_audit_id]
                ).await.map_err(|error| store_error("write PostgreSQL global memory", error, false))?;
                Ok(MemoryStoreWriteResult::GlobalRecord(
                    GlobalMemoryWriteResult {
                        id: record.id,
                        stored: true,
                        deduped: false,
                    },
                ))
            }
            MemoryStoreWriteRequest::ProjectConfig {
                scope,
                project_id,
                config,
            } => {
                self.upsert_entity(&scope, "project_config", &project_id, "", &config)
                    .await?;
                Ok(MemoryStoreWriteResult::Stored)
            }
            MemoryStoreWriteRequest::KnowledgeSpace { scope, record } => {
                self.upsert_entity(&scope, "knowledge_space", &record.id, "", &record)
                    .await?;
                Ok(MemoryStoreWriteResult::Stored)
            }
            MemoryStoreWriteRequest::KnowledgeItem { scope, record } => {
                self.upsert_entity(&scope, "knowledge_item", &record.id, "", &record)
                    .await?;
                Ok(MemoryStoreWriteResult::Stored)
            }
            MemoryStoreWriteRequest::KnowledgeCoverage { scope, record } => {
                self.upsert_entity(
                    &scope,
                    "knowledge_coverage",
                    &record.space_id,
                    &record.coverage_key,
                    &record,
                )
                .await?;
                Ok(MemoryStoreWriteResult::Stored)
            }
            MemoryStoreWriteRequest::ImportIndexEntry {
                scope,
                selector,
                path,
                entry,
            } => {
                let key = selector
                    .project_id
                    .or(selector.session_id)
                    .unwrap_or_default();
                self.upsert_entity(&scope, "import_index", &key, &path, &entry)
                    .await?;
                Ok(MemoryStoreWriteResult::Stored)
            }
            MemoryStoreWriteRequest::ProjectIndexStatus {
                scope,
                project_id,
                total_files,
                processed_files,
                indexed_files,
                skipped_files,
                errors,
            } => {
                self.upsert_entity(
                    &scope,
                    "project_index_status",
                    &project_id,
                    "",
                    &serde_json::json!({
                        "total_files": total_files, "processed_files": processed_files,
                        "indexed_files": indexed_files, "skipped_files": skipped_files,
                        "errors": errors, "updated_at_ms": chrono::Utc::now().timestamp_millis()
                    }),
                )
                .await?;
                Ok(MemoryStoreWriteResult::Stored)
            }
            MemoryStoreWriteRequest::SourceObjectLifecycle { scope, record } => {
                if scope.tenant != record.tenant_scope {
                    return Err(MemoryStoreError::new(
                        MemoryStoreErrorKind::ScopeViolation,
                        "source lifecycle tenant mismatch",
                    ));
                }
                self.upsert_entity(
                    &scope,
                    "source_lifecycle",
                    &record.source_binding_id,
                    &record.source_object_id,
                    &record,
                )
                .await?;
                Ok(MemoryStoreWriteResult::Stored)
            }
            MemoryStoreWriteRequest::ContextNode {
                scope,
                uri,
                parent_uri,
                node_type,
                metadata,
            } => {
                let now = chrono::Utc::now();
                let node = MemoryNode {
                    id: uuid::Uuid::new_v4().to_string(),
                    uri: uri.clone(),
                    parent_uri,
                    node_type,
                    created_at: now,
                    updated_at: now,
                    metadata,
                };
                self.upsert_entity(&scope, "context_node_uri", &uri, "", &node)
                    .await?;
                self.upsert_entity(&scope, "context_node_id", &node.id, "", &node)
                    .await?;
                Ok(MemoryStoreWriteResult::ContextNodeCreated(node.id))
            }
            MemoryStoreWriteRequest::ContextLayer {
                scope,
                node_id,
                layer_type,
                content,
                token_count,
                source_chunk_id,
            } => {
                let layer = MemoryLayer {
                    id: uuid::Uuid::new_v4().to_string(),
                    node_id: node_id.clone(),
                    layer_type,
                    content,
                    token_count,
                    embedding_id: None,
                    created_at: chrono::Utc::now(),
                    source_chunk_id,
                };
                self.upsert_entity(
                    &scope,
                    "context_layer",
                    &node_id,
                    &serde_json::to_string(&layer_type).unwrap_or_default(),
                    &layer,
                )
                .await?;
                Ok(MemoryStoreWriteResult::ContextLayerCreated(layer.id))
            }
            MemoryStoreWriteRequest::CleanupLog { scope, entry } => {
                let record = CleanupLogEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    cleanup_type: entry.cleanup_type,
                    tier: entry.tier,
                    project_id: entry.project_id,
                    session_id: entry.session_id,
                    chunks_deleted: entry.chunks_deleted,
                    bytes_reclaimed: entry.bytes_reclaimed,
                    created_at: chrono::Utc::now(),
                };
                self.upsert_entity(&scope, "cleanup_log", &record.id, "", &record)
                    .await?;
                Ok(MemoryStoreWriteResult::Stored)
            }
        }
    }

    pub(super) async fn mutate_impl(
        &self,
        request: MemoryStoreMutationRequest,
    ) -> MemoryStoreResult<MemoryStoreMutationResult> {
        let client = self.client().await?;
        match request {
            MemoryStoreMutationRequest::DeleteChunk {
                scope,
                selector,
                chunk_id,
            } => {
                let changed = client
                    .execute(
                        "DELETE FROM tandem_memory_chunks WHERE id=$1 AND tenant_org_id=$2
                     AND tenant_workspace_id=$3 AND tenant_deployment_id=$4 AND tier=$5
                     AND ($6::text IS NULL OR project_id=$6) AND ($7::text IS NULL OR session_id=$7)
                     AND ($8::boolean OR owner_subject IS NULL OR owner_subject=$9)
                     AND ($10::text IS NULL OR owner_org_unit_id=$10 OR tenant_shared=true)",
                        &[
                            &chunk_id,
                            &scope.tenant.org_id,
                            &scope.tenant.workspace_id,
                            &deployment(&scope.tenant),
                            &selector_tier(&selector),
                            &selector.project_id,
                            &selector.session_id,
                            &(scope.access == MemoryReadAccess::TrustedUnrestricted),
                            &scope.subject,
                            &scope.org_unit,
                        ],
                    )
                    .await
                    .map_err(|error| store_error("delete PostgreSQL chunk", error, true))?;
                Ok(MemoryStoreMutationResult::Affected(changed))
            }
            MemoryStoreMutationRequest::ClearSession { scope, session_id } => {
                let changed = client.execute("DELETE FROM tandem_memory_chunks WHERE tenant_org_id=$1 AND tenant_workspace_id=$2 AND tenant_deployment_id=$3 AND session_id=$4",
                    &[&scope.tenant.org_id,&scope.tenant.workspace_id,&deployment(&scope.tenant),&session_id]).await.map_err(|error| store_error("clear PostgreSQL session", error, true))?;
                Ok(MemoryStoreMutationResult::Affected(changed))
            }
            MemoryStoreMutationRequest::ReplaceSessionWithSummary {
                scope,
                session_id,
                project_id,
                source_chunk_ids,
                summary_scope,
                summary,
                embedding,
            } => {
                if session_id.trim().is_empty()
                    || project_id.trim().is_empty()
                    || source_chunk_ids.is_empty()
                    || summary.tier != crate::types::MemoryTier::Project
                    || summary.project_id.as_deref() != Some(project_id.as_str())
                {
                    return Err(MemoryStoreError::invalid(
                        "session consolidation requires source chunks and a matching project summary",
                    ));
                }
                if summary_scope.tenant != summary.tenant_scope
                    || summary_scope.subject != summary.subject
                    || summary_scope.org_unit
                        != owner_org_unit_id_from_metadata(summary.metadata.as_ref())
                    || summary_scope.tenant != scope.tenant
                    || summary_scope.subject != scope.subject
                    || summary_scope.org_unit != scope.org_unit
                {
                    return Err(MemoryStoreError::new(
                        MemoryStoreErrorKind::ScopeViolation,
                        "session summary ownership must exactly match the source scope",
                    ));
                }
                if embedding.len() != self.embedding_dimension {
                    return Err(MemoryStoreError::invalid(format!(
                        "embedding dimension mismatch: expected {}, got {}",
                        self.embedding_dimension,
                        embedding.len()
                    )));
                }
                let (vector, ciphertext, envelope, policy_id, audit_id) = match self
                    .search_surface_mode
                {
                    PostgresSearchSurfaceMode::PlaintextPgvector => {
                        (Some(Vector::from(embedding)), None, None, None, None)
                    }
                    PostgresSearchSurfaceMode::EncryptedRerank => {
                        let (ciphertext, envelope, policy_id, audit_id) = self.encrypt_embedding(
                            &embedding,
                            &summary_scope.tenant,
                            summary_scope.org_unit.clone(),
                            summary_scope.subject.clone(),
                            &summary.id,
                        )?;
                        (
                            None,
                            Some(ciphertext),
                            envelope.map(|value| json_value(&value)).transpose()?,
                            Some(policy_id),
                            Some(audit_id),
                        )
                    }
                    PostgresSearchSurfaceMode::Disabled => (None, None, None, None, None),
                };
                let (data, data_ciphertext, data_envelope, data_policy, data_audit) = self
                    .encode_payload(
                        &summary,
                        &summary_scope.tenant,
                        summary_scope.org_unit.clone(),
                        summary_scope.subject.clone(),
                        &summary.id,
                    )?;
                let tenant_shared = tenant_shared_from_metadata(summary.metadata.as_ref());
                let mut client = self.client().await?;
                let transaction = client
                    .transaction()
                    .await
                    .map_err(|error| store_error("start PostgreSQL consolidation", error, true))?;
                transaction.execute(
                    "INSERT INTO tandem_memory_chunks
                     (id,tenant_org_id,tenant_workspace_id,tenant_deployment_id,owner_org_unit_id,
                      owner_subject,tenant_shared,tier,project_id,session_id,source_path,created_at,
                      data,data_ciphertext,data_envelope,data_policy_decision_id,data_audit_id,
                      embedding,embedding_ciphertext,embedding_envelope,search_policy_decision_id,search_audit_id)
                     VALUES ($1,$2,$3,$4,$5,$6,$7,'project',$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21)",
                    &[&summary.id,&summary_scope.tenant.org_id,&summary_scope.tenant.workspace_id,
                      &deployment(&summary_scope.tenant),&summary_scope.org_unit,&summary_scope.subject,
                      &tenant_shared,&summary.project_id,&summary.session_id,&summary.source_path,&summary.created_at,
                      &data,&data_ciphertext,&data_envelope,&data_policy,&data_audit,
                      &vector,&ciphertext,&envelope,&policy_id,&audit_id]
                ).await.map_err(|error| store_error("write PostgreSQL consolidation summary", error, false))?;
                let mut deleted = 0_u64;
                for chunk_id in source_chunk_ids {
                    let visible = transaction
                        .query_opt(
                            "SELECT 1 FROM tandem_memory_chunks WHERE id=$1 AND tier='session'
                         AND session_id=$2 AND project_id=$3 AND tenant_org_id=$4
                         AND tenant_workspace_id=$5 AND tenant_deployment_id=$6
                         AND (($7::text IS NULL AND owner_subject IS NULL) OR owner_subject=$7)
                         AND owner_org_unit_id IS NOT DISTINCT FROM $8",
                            &[
                                &chunk_id,
                                &session_id,
                                &project_id,
                                &scope.tenant.org_id,
                                &scope.tenant.workspace_id,
                                &deployment(&scope.tenant),
                                &scope.subject,
                                &scope.org_unit,
                            ],
                        )
                        .await
                        .map_err(|error| {
                            store_error("validate PostgreSQL consolidation source", error, true)
                        })?;
                    if visible.is_none() {
                        return Err(MemoryStoreError::new(
                            MemoryStoreErrorKind::Conflict,
                            "session changed or a source chunk is outside the consolidation scope",
                        ));
                    }
                    deleted += transaction
                        .execute("DELETE FROM tandem_memory_chunks WHERE id=$1", &[&chunk_id])
                        .await
                        .map_err(|error| {
                            store_error("delete PostgreSQL consolidation source", error, false)
                        })?;
                }
                transaction
                    .commit()
                    .await
                    .map_err(|error| store_error("commit PostgreSQL consolidation", error, true))?;
                Ok(MemoryStoreMutationResult::Affected(deleted))
            }
            MemoryStoreMutationRequest::ClearProject { scope, project_id } => {
                let changed = client.execute("DELETE FROM tandem_memory_chunks WHERE tenant_org_id=$1 AND tenant_workspace_id=$2 AND tenant_deployment_id=$3 AND project_id=$4",
                    &[&scope.tenant.org_id,&scope.tenant.workspace_id,&deployment(&scope.tenant),&project_id]).await.map_err(|error| store_error("clear PostgreSQL project", error, true))?;
                Ok(MemoryStoreMutationResult::Affected(changed))
            }
            MemoryStoreMutationRequest::ClearProjectFileIndex {
                scope,
                project_id,
                vacuum,
            } => {
                let rows = client.query("DELETE FROM tandem_memory_chunks WHERE tenant_org_id=$1 AND tenant_workspace_id=$2 AND tenant_deployment_id=$3 AND project_id=$4 AND source_path IS NOT NULL RETURNING COALESCE(octet_length(data::text),octet_length(data_ciphertext))::bigint",
                    &[&scope.tenant.org_id,&scope.tenant.workspace_id,&deployment(&scope.tenant),&project_id]).await.map_err(|error| store_error("clear PostgreSQL file index", error, true))?;
                client.execute("DELETE FROM tandem_memory_entities WHERE tenant_org_id=$1 AND tenant_workspace_id=$2 AND tenant_deployment_id=$3 AND entity_type='import_index' AND key1=$4",
                    &[&scope.tenant.org_id,&scope.tenant.workspace_id,&deployment(&scope.tenant),&project_id]).await.map_err(|error| store_error("clear PostgreSQL import index", error, true))?;
                Ok(MemoryStoreMutationResult::ClearFileIndex(
                    ClearFileIndexResult {
                        chunks_deleted: rows.len() as i64,
                        bytes_estimated: rows.iter().map(|row| row.get::<_, i64>(0)).sum(),
                        did_vacuum: vacuum,
                    },
                ))
            }
            MemoryStoreMutationRequest::DeleteGlobalRecord { scope, id } => {
                let changed = client.execute("DELETE FROM tandem_memory_global_records WHERE id=$1 AND tenant_org_id=$2 AND tenant_workspace_id=$3 AND tenant_deployment_id=$4 AND ($5::boolean OR private=false OR owner_subject=$6) AND ($7::text IS NULL OR owner_org_unit_id=$7)",
                    &[&id,&scope.tenant.org_id,&scope.tenant.workspace_id,&deployment(&scope.tenant),&(scope.access == MemoryReadAccess::TrustedUnrestricted),&scope.subject,&scope.org_unit]).await.map_err(|error| store_error("delete PostgreSQL global memory", error, true))?;
                Ok(MemoryStoreMutationResult::Changed(changed > 0))
            }
            MemoryStoreMutationRequest::UpdateGlobalRecordContext {
                scope,
                id,
                visibility,
                demoted,
                metadata,
                provenance,
            } => {
                let row = client.query_opt("SELECT data,data_ciphertext,data_envelope,data_policy_decision_id,data_audit_id,owner_org_unit_id,owner_subject FROM tandem_memory_global_records WHERE id=$1 AND tenant_org_id=$2 AND tenant_workspace_id=$3 AND tenant_deployment_id=$4 AND ($5::boolean OR private=false OR owner_subject=$6) AND ($7::text IS NULL OR owner_org_unit_id=$7)",
                    &[&id,&scope.tenant.org_id,&scope.tenant.workspace_id,&deployment(&scope.tenant),&(scope.access == MemoryReadAccess::TrustedUnrestricted),&scope.subject,&scope.org_unit]).await.map_err(|error| store_error("read PostgreSQL global memory update", error, true))?;
                let Some(row) = row else {
                    return Ok(MemoryStoreMutationResult::Changed(false));
                };
                let mut record: crate::types::GlobalMemoryRecord = self.decode_payload(
                    row.get(0),
                    row.get(1),
                    row.get(2),
                    &scope.tenant,
                    row.get(5),
                    row.get(6),
                    row.get(3),
                    row.get(4),
                )?;
                record.visibility = visibility;
                record.demoted = demoted;
                record.metadata = metadata;
                record.provenance = provenance;
                record.updated_at_ms = chrono::Utc::now().timestamp_millis() as u64;
                let next_org = owner_org_unit_id_from_metadata(record.metadata.as_ref());
                let next_subject = owner_subject_from_metadata(record.metadata.as_ref());
                let (data, cipher, envelope, policy, audit) = self.encode_payload(
                    &record,
                    &scope.tenant,
                    next_org.clone(),
                    next_subject.clone(),
                    &id,
                )?;
                client.execute("UPDATE tandem_memory_global_records SET data=$2,data_ciphertext=$3,data_envelope=$4,data_policy_decision_id=$5,data_audit_id=$6,demoted=$7,owner_org_unit_id=$8,owner_subject=$9,private=$10 WHERE id=$1",
                    &[&id,&data,&cipher,&envelope,&policy,&audit,&record.demoted,&next_org,&next_subject,&next_subject.is_some()]).await.map_err(|error| store_error("update PostgreSQL global memory", error, true))?;
                Ok(MemoryStoreMutationResult::Changed(true))
            }
            MemoryStoreMutationRequest::PromoteKnowledgeItem { scope, request } => {
                let read_scope = MemoryReadScope {
                    tenant: scope.tenant.clone(),
                    org_unit: scope.org_unit.clone(),
                    subject: scope.subject.clone(),
                    access: scope.access,
                };
                let Some(mut item) = self
                    .entity::<crate::types::KnowledgeItemRecord>(
                        &read_scope,
                        "knowledge_item",
                        &request.item_id,
                        "",
                    )
                    .await?
                else {
                    return Ok(MemoryStoreMutationResult::Promotion(None));
                };
                let previous_status = item.status;
                let previous_trust_level = item.trust_level;
                item.status = request.target_status;
                item.updated_at_ms = request.promoted_at_ms;
                item.freshness_expires_at_ms = request.freshness_expires_at_ms;
                if let Some(trust) = request.target_status.as_trust_level() {
                    item.trust_level = trust;
                }
                let mut coverage = self
                    .entity::<crate::types::KnowledgeCoverageRecord>(
                        &read_scope,
                        "knowledge_coverage",
                        &item.space_id,
                        &item.coverage_key,
                    )
                    .await?
                    .unwrap_or(crate::types::KnowledgeCoverageRecord {
                        coverage_key: item.coverage_key.clone(),
                        space_id: item.space_id.clone(),
                        latest_item_id: None,
                        latest_dedupe_key: None,
                        last_seen_at_ms: request.promoted_at_ms,
                        last_promoted_at_ms: None,
                        freshness_expires_at_ms: None,
                        metadata: None,
                    });
                coverage.latest_item_id = Some(item.id.clone());
                coverage.latest_dedupe_key = Some(item.dedupe_key.clone());
                coverage.last_promoted_at_ms = Some(request.promoted_at_ms);
                let write_scope = MemoryWriteScope {
                    tenant: scope.tenant,
                    org_unit: scope.org_unit,
                    subject: scope.subject,
                };
                self.upsert_entity(&write_scope, "knowledge_item", &item.id, "", &item)
                    .await?;
                self.upsert_entity(
                    &write_scope,
                    "knowledge_coverage",
                    &coverage.space_id,
                    &coverage.coverage_key,
                    &coverage,
                )
                .await?;
                Ok(MemoryStoreMutationResult::Promotion(Some(
                    crate::types::KnowledgePromotionResult {
                        previous_status,
                        previous_trust_level,
                        promoted: request.target_status.is_active(),
                        item,
                        coverage,
                    },
                )))
            }
            MemoryStoreMutationRequest::DeleteImportIndexEntry {
                scope,
                selector,
                path,
            } => {
                let key = selector
                    .project_id
                    .or(selector.session_id)
                    .unwrap_or_default();
                let changed = client.execute("DELETE FROM tandem_memory_entities WHERE tenant_org_id=$1 AND tenant_workspace_id=$2 AND tenant_deployment_id=$3 AND entity_type='import_index' AND key1=$4 AND key2=$5", &[&scope.tenant.org_id,&scope.tenant.workspace_id,&deployment(&scope.tenant),&key,&path]).await.map_err(|error| store_error("delete PostgreSQL import entry", error, true))?;
                Ok(MemoryStoreMutationResult::Changed(changed > 0))
            }
            MemoryStoreMutationRequest::DeleteChunksBySourcePath {
                scope,
                selector,
                source_path,
            } => {
                let rows = client.query("DELETE FROM tandem_memory_chunks WHERE tenant_org_id=$1 AND tenant_workspace_id=$2 AND tenant_deployment_id=$3 AND tier=$4 AND ($5::text IS NULL OR project_id=$5) AND ($6::text IS NULL OR session_id=$6) AND source_path=$7 RETURNING COALESCE(octet_length(data::text),octet_length(data_ciphertext))::bigint", &[&scope.tenant.org_id,&scope.tenant.workspace_id,&deployment(&scope.tenant),&selector_tier(&selector),&selector.project_id,&selector.session_id,&source_path]).await.map_err(|error| store_error("delete PostgreSQL source chunks", error, true))?;
                Ok(MemoryStoreMutationResult::SourcePathDelete(
                    MemorySourcePathDeleteResult {
                        chunks_deleted: rows.len() as i64,
                        bytes_reclaimed: rows.iter().map(|row| row.get::<_, i64>(0)).sum(),
                    },
                ))
            }
            MemoryStoreMutationRequest::UpdateChunkMetadataBySourcePath {
                scope,
                selector,
                source_path,
                metadata,
            } => {
                let rows = client.query("SELECT id,data,data_ciphertext,data_envelope,data_policy_decision_id,data_audit_id,owner_org_unit_id,owner_subject FROM tandem_memory_chunks WHERE tenant_org_id=$1 AND tenant_workspace_id=$2 AND tenant_deployment_id=$3 AND tier=$4 AND ($5::text IS NULL OR project_id=$5) AND ($6::text IS NULL OR session_id=$6) AND source_path=$7", &[&scope.tenant.org_id,&scope.tenant.workspace_id,&deployment(&scope.tenant),&selector_tier(&selector),&selector.project_id,&selector.session_id,&source_path]).await.map_err(|error| store_error("read PostgreSQL source chunks", error, true))?;
                for row in &rows {
                    let org: Option<String> = row.get(6);
                    let mut chunk: crate::types::MemoryChunk = self.decode_payload(
                        row.get(1),
                        row.get(2),
                        row.get(3),
                        &scope.tenant,
                        org.clone(),
                        row.get(7),
                        row.get(4),
                        row.get(5),
                    )?;
                    chunk.metadata = Some(metadata.clone());
                    let (data, cipher, envelope, policy, audit) = self.encode_payload(
                        &chunk,
                        &scope.tenant,
                        org,
                        chunk.subject.clone(),
                        &chunk.id,
                    )?;
                    client
                        .execute(
                            "UPDATE tandem_memory_chunks SET data=$2,data_ciphertext=$3,data_envelope=$4,data_policy_decision_id=$5,data_audit_id=$6 WHERE id=$1",
                            &[&chunk.id,&data,&cipher,&envelope,&policy,&audit],
                        )
                        .await
                        .map_err(|error| {
                            store_error("update PostgreSQL source chunk", error, true)
                        })?;
                }
                Ok(MemoryStoreMutationResult::Affected(rows.len() as u64))
            }
            MemoryStoreMutationRequest::TombstoneSourceObjectLifecycle {
                scope,
                source_binding_id,
                native_object_id,
                tombstoned_at_ms,
            } => {
                let read_scope = MemoryReadScope {
                    tenant: scope.tenant.clone(),
                    org_unit: scope.org_unit.clone(),
                    subject: scope.subject.clone(),
                    access: scope.access,
                };
                let values = self
                    .query_entity_values::<SourceObjectLifecycleRecord>(
                        &read_scope,
                        "source_lifecycle",
                        &source_binding_id,
                    )
                    .await?;
                let mut count = 0;
                let write_scope = MemoryWriteScope {
                    tenant: scope.tenant,
                    org_unit: scope.org_unit,
                    subject: scope.subject,
                };
                for mut value in values
                    .into_iter()
                    .filter(|value| value.native_object_id == native_object_id)
                {
                    value.state = SourceObjectLifecycleState::Deleted;
                    value.tombstoned_at_ms = Some(tombstoned_at_ms);
                    value.last_seen_at_ms = tombstoned_at_ms;
                    self.upsert_entity(
                        &write_scope,
                        "source_lifecycle",
                        &value.source_binding_id,
                        &value.source_object_id,
                        &value,
                    )
                    .await?;
                    count += 1;
                }
                Ok(MemoryStoreMutationResult::Affected(count))
            }
            MemoryStoreMutationRequest::SetSourceObjectLifecycleState {
                scope,
                source_binding_id,
                source_object_id,
                state,
                changed_at_ms,
            } => {
                let read_scope = MemoryReadScope {
                    tenant: scope.tenant.clone(),
                    org_unit: scope.org_unit.clone(),
                    subject: scope.subject.clone(),
                    access: scope.access,
                };
                let Some(mut value) = self
                    .entity::<SourceObjectLifecycleRecord>(
                        &read_scope,
                        "source_lifecycle",
                        &source_binding_id,
                        &source_object_id,
                    )
                    .await?
                else {
                    return Ok(MemoryStoreMutationResult::Changed(false));
                };
                value.state = state;
                value.last_seen_at_ms = changed_at_ms;
                let write_scope = MemoryWriteScope {
                    tenant: scope.tenant,
                    org_unit: scope.org_unit,
                    subject: scope.subject,
                };
                self.upsert_entity(
                    &write_scope,
                    "source_lifecycle",
                    &source_binding_id,
                    &source_object_id,
                    &value,
                )
                .await?;
                Ok(MemoryStoreMutationResult::Changed(true))
            }
            MemoryStoreMutationRequest::RunHygiene {
                scope,
                retention_days,
            } => {
                let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
                let changed=client.execute("DELETE FROM tandem_memory_chunks WHERE tenant_org_id=$1 AND tenant_workspace_id=$2 AND tenant_deployment_id=$3 AND created_at<$4", &[&scope.tenant.org_id,&scope.tenant.workspace_id,&deployment(&scope.tenant),&cutoff]).await.map_err(|error| store_error("run PostgreSQL hygiene", error, true))?;
                Ok(MemoryStoreMutationResult::Affected(changed))
            }
            MemoryStoreMutationRequest::RunHygieneAllTenants { retention_days } => {
                let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
                let changed = client
                    .execute(
                        "DELETE FROM tandem_memory_chunks WHERE created_at<$1",
                        &[&cutoff],
                    )
                    .await
                    .map_err(|error| store_error("run PostgreSQL global hygiene", error, true))?;
                Ok(MemoryStoreMutationResult::Affected(changed))
            }
            MemoryStoreMutationRequest::EnforceProjectChunkCap {
                scope,
                project_id,
                max_chunks,
            } => {
                let changed=client.execute("DELETE FROM tandem_memory_chunks WHERE id IN (SELECT id FROM tandem_memory_chunks WHERE tenant_org_id=$1 AND tenant_workspace_id=$2 AND tenant_deployment_id=$3 AND project_id=$4 ORDER BY created_at DESC OFFSET $5)", &[&scope.tenant.org_id,&scope.tenant.workspace_id,&deployment(&scope.tenant),&project_id,&max_chunks.max(0)]).await.map_err(|error| store_error("enforce PostgreSQL project cap", error, true))?;
                Ok(MemoryStoreMutationResult::Affected(changed))
            }
            MemoryStoreMutationRequest::Vacuum => {
                client.batch_execute("VACUUM (ANALYZE) tandem_memory_chunks; VACUUM (ANALYZE) tandem_memory_global_records").await.map_err(|error| store_error("vacuum PostgreSQL memory", error, true))?;
                Ok(MemoryStoreMutationResult::Completed)
            }
        }
    }

    pub(super) async fn batch_impl(
        &self,
        request: MemoryStoreBatchRequest,
    ) -> MemoryStoreResult<MemoryStoreBatchResult> {
        if request.mode == MemoryStoreBatchMode::Atomic {
            if request.operations.iter().any(|operation| {
                !matches!(
                    operation,
                    MemoryStoreBatchOperation::Write(MemoryStoreWriteRequest::Chunk { .. })
                        | MemoryStoreBatchOperation::Write(
                            MemoryStoreWriteRequest::GlobalRecord { .. }
                        )
                        | MemoryStoreBatchOperation::Mutation(
                            MemoryStoreMutationRequest::DeleteGlobalRecord { .. }
                        )
                        | MemoryStoreBatchOperation::Mutation(
                            MemoryStoreMutationRequest::UpdateGlobalRecordContext { .. }
                        )
                )
            }) {
                return Err(MemoryStoreError::unsupported(
                    "atomic PostgreSQL batches currently support chunk writes and global-record CRUD",
                ));
            }
            let mut client = self.client().await?;
            let transaction = client
                .transaction()
                .await
                .map_err(|error| store_error("start PostgreSQL memory batch", error, true))?;
            let mut items = Vec::with_capacity(request.operations.len());
            for (index, operation) in request.operations.into_iter().enumerate() {
                let value = match operation {
                    MemoryStoreBatchOperation::Write(MemoryStoreWriteRequest::Chunk {
                        scope,
                        chunk,
                        embedding,
                    }) => {
                        if scope.tenant != chunk.tenant_scope
                            || scope.subject != chunk.subject
                            || scope.org_unit
                                != owner_org_unit_id_from_metadata(chunk.metadata.as_ref())
                        {
                            return Err(MemoryStoreError::new(
                                MemoryStoreErrorKind::ScopeViolation,
                                "chunk ownership does not match the PostgreSQL write scope",
                            ));
                        }
                        if embedding.len() != self.embedding_dimension {
                            return Err(MemoryStoreError::invalid(format!(
                                "embedding dimension mismatch: expected {}, got {}",
                                self.embedding_dimension,
                                embedding.len()
                            )));
                        }
                        let (vector, ciphertext, envelope, policy_id, audit_id) = match self
                            .search_surface_mode
                        {
                            PostgresSearchSurfaceMode::PlaintextPgvector => {
                                (Some(Vector::from(embedding)), None, None, None, None)
                            }
                            PostgresSearchSurfaceMode::EncryptedRerank => {
                                let (ciphertext, envelope, policy_id, audit_id) = self
                                    .encrypt_embedding(
                                        &embedding,
                                        &scope.tenant,
                                        scope.org_unit.clone(),
                                        scope.subject.clone(),
                                        &chunk.id,
                                    )?;
                                (
                                    None,
                                    Some(ciphertext),
                                    envelope.map(|value| json_value(&value)).transpose()?,
                                    Some(policy_id),
                                    Some(audit_id),
                                )
                            }
                            PostgresSearchSurfaceMode::Disabled => (None, None, None, None, None),
                        };
                        let (data, data_ciphertext, data_envelope, data_policy_id, data_audit_id) =
                            self.encode_payload(
                                &chunk,
                                &scope.tenant,
                                scope.org_unit.clone(),
                                scope.subject.clone(),
                                &chunk.id,
                            )?;
                        let tenant_shared = tenant_shared_from_metadata(chunk.metadata.as_ref());
                        transaction.execute(
                            "INSERT INTO tandem_memory_chunks
                               (id,tenant_org_id,tenant_workspace_id,tenant_deployment_id,
                                owner_org_unit_id,owner_subject,tenant_shared,tier,project_id,session_id,source_path,
                                created_at,data,data_ciphertext,data_envelope,data_policy_decision_id,data_audit_id,
                                embedding,embedding_ciphertext,embedding_envelope,search_policy_decision_id,search_audit_id)
                             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22)",
                            &[&chunk.id,&scope.tenant.org_id,&scope.tenant.workspace_id,
                              &deployment(&scope.tenant),&scope.org_unit,&scope.subject,
                              &tenant_shared,
                              &selector_tier(&MemoryChunkSelector { tier:chunk.tier, project_id:chunk.project_id.clone(), session_id:chunk.session_id.clone() }),
                              &chunk.project_id,&chunk.session_id,&chunk.source_path,&chunk.created_at,
                              &data,&data_ciphertext,&data_envelope,&data_policy_id,&data_audit_id,
                              &vector,&ciphertext,&envelope,&policy_id,&audit_id]
                        ).await.map_err(|error| store_error("write atomic PostgreSQL chunk", error, false))?;
                        MemoryStoreBatchValue::Write(MemoryStoreWriteResult::Stored)
                    }
                    MemoryStoreBatchOperation::Write(MemoryStoreWriteRequest::GlobalRecord {
                        scope,
                        record,
                    }) => {
                        let tenant = tenant_scope_from_global_record(&record);
                        let owner_org = owner_org_unit_id_from_metadata(record.metadata.as_ref());
                        let owner_subject = owner_subject_from_metadata(record.metadata.as_ref());
                        if tenant != scope.tenant
                            || owner_org != scope.org_unit
                            || owner_subject != scope.subject
                        {
                            return Err(MemoryStoreError::new(
                                MemoryStoreErrorKind::ScopeViolation,
                                "global record ownership does not match the PostgreSQL write scope",
                            ));
                        }
                        let (data, data_ciphertext, data_envelope, data_policy_id, data_audit_id) =
                            self.encode_payload(
                                &record,
                                &tenant,
                                owner_org.clone(),
                                owner_subject.clone(),
                                &record.id,
                            )?;
                        let search_content = if self.search_surface_mode
                            == PostgresSearchSurfaceMode::PlaintextPgvector
                        {
                            record.content.as_str()
                        } else {
                            ""
                        };
                        transaction.execute(
                            "INSERT INTO tandem_memory_global_records
                             (id,tenant_org_id,tenant_workspace_id,tenant_deployment_id,owner_org_unit_id,
                              owner_subject,private,user_id,source_type,content_hash,run_id,session_id,message_id,
                              tool_name,project_tag,channel_tag,demoted,expires_at_ms,created_at_ms,search_content,
                              data,data_ciphertext,data_envelope,data_policy_decision_id,data_audit_id)
                             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25)",
                            &[&record.id,&tenant.org_id,&tenant.workspace_id,&deployment(&tenant),&owner_org,
                              &owner_subject,&owner_subject.is_some(),&record.user_id,&record.source_type,
                              &record.content_hash,&record.run_id,&record.session_id,&record.message_id,
                              &record.tool_name,&record.project_tag,&record.channel_tag,&record.demoted,
                              &record.expires_at_ms.map(|value| value as i64),&(record.created_at_ms as i64),
                              &search_content,&data,&data_ciphertext,&data_envelope,&data_policy_id,&data_audit_id]
                        ).await.map_err(|error| store_error("write atomic PostgreSQL global memory", error, false))?;
                        MemoryStoreBatchValue::Write(MemoryStoreWriteResult::GlobalRecord(
                            GlobalMemoryWriteResult {
                                id: record.id,
                                stored: true,
                                deduped: false,
                            },
                        ))
                    }
                    MemoryStoreBatchOperation::Mutation(
                        MemoryStoreMutationRequest::DeleteGlobalRecord { scope, id },
                    ) => {
                        let changed=transaction.execute("DELETE FROM tandem_memory_global_records WHERE id=$1 AND tenant_org_id=$2 AND tenant_workspace_id=$3 AND tenant_deployment_id=$4 AND ($5::boolean OR private=false OR owner_subject=$6) AND ($7::text IS NULL OR owner_org_unit_id=$7)",
                            &[&id,&scope.tenant.org_id,&scope.tenant.workspace_id,&deployment(&scope.tenant),&(scope.access == MemoryReadAccess::TrustedUnrestricted),&scope.subject,&scope.org_unit]).await.map_err(|error| store_error("delete atomic PostgreSQL global memory", error, false))?;
                        MemoryStoreBatchValue::Mutation(MemoryStoreMutationResult::Changed(
                            changed > 0,
                        ))
                    }
                    MemoryStoreBatchOperation::Mutation(
                        MemoryStoreMutationRequest::UpdateGlobalRecordContext {
                            scope,
                            id,
                            visibility,
                            demoted,
                            metadata,
                            provenance,
                        },
                    ) => {
                        let row=transaction.query_opt("SELECT data,data_ciphertext,data_envelope,data_policy_decision_id,data_audit_id,owner_org_unit_id,owner_subject FROM tandem_memory_global_records WHERE id=$1 AND tenant_org_id=$2 AND tenant_workspace_id=$3 AND tenant_deployment_id=$4 AND ($5::boolean OR private=false OR owner_subject=$6) AND ($7::text IS NULL OR owner_org_unit_id=$7)",
                            &[&id,&scope.tenant.org_id,&scope.tenant.workspace_id,&deployment(&scope.tenant),&(scope.access == MemoryReadAccess::TrustedUnrestricted),&scope.subject,&scope.org_unit]).await.map_err(|error| store_error("read atomic PostgreSQL global memory", error, false))?;
                        if let Some(row) = row {
                            let mut record: crate::types::GlobalMemoryRecord = self
                                .decode_payload(
                                    row.get(0),
                                    row.get(1),
                                    row.get(2),
                                    &scope.tenant,
                                    row.get(5),
                                    row.get(6),
                                    row.get(3),
                                    row.get(4),
                                )?;
                            record.visibility = visibility;
                            record.demoted = demoted;
                            record.metadata = metadata;
                            record.provenance = provenance;
                            record.updated_at_ms = chrono::Utc::now().timestamp_millis() as u64;
                            let owner_org =
                                owner_org_unit_id_from_metadata(record.metadata.as_ref());
                            let owner_subject =
                                owner_subject_from_metadata(record.metadata.as_ref());
                            let (data, cipher, envelope, policy, audit) = self.encode_payload(
                                &record,
                                &scope.tenant,
                                owner_org.clone(),
                                owner_subject.clone(),
                                &id,
                            )?;
                            transaction.execute("UPDATE tandem_memory_global_records SET data=$2,data_ciphertext=$3,data_envelope=$4,data_policy_decision_id=$5,data_audit_id=$6,demoted=$7,owner_org_unit_id=$8,owner_subject=$9,private=$10 WHERE id=$1", &[&id,&data,&cipher,&envelope,&policy,&audit,&record.demoted,&owner_org,&owner_subject,&owner_subject.is_some()]).await.map_err(|error| store_error("update atomic PostgreSQL global memory", error, false))?;
                            MemoryStoreBatchValue::Mutation(MemoryStoreMutationResult::Changed(
                                true,
                            ))
                        } else {
                            MemoryStoreBatchValue::Mutation(MemoryStoreMutationResult::Changed(
                                false,
                            ))
                        }
                    }
                    _ => unreachable!("atomic batch was preflighted"),
                };
                items.push(MemoryStoreBatchItemResult {
                    index,
                    result: Ok(value),
                });
            }
            transaction
                .commit()
                .await
                .map_err(|error| store_error("commit PostgreSQL memory batch", error, true))?;
            return Ok(MemoryStoreBatchResult {
                completed: true,
                items,
            });
        }
        let mut items = Vec::with_capacity(request.operations.len());
        for (index, operation) in request.operations.into_iter().enumerate() {
            let result = match operation {
                MemoryStoreBatchOperation::Write(value) => self
                    .write_impl(value)
                    .await
                    .map(MemoryStoreBatchValue::Write),
                MemoryStoreBatchOperation::Mutation(value) => self
                    .mutate_impl(value)
                    .await
                    .map(MemoryStoreBatchValue::Mutation),
            };
            let failed = result.is_err();
            items.push(MemoryStoreBatchItemResult { index, result });
            if failed && request.mode == MemoryStoreBatchMode::StopOnError {
                break;
            }
        }
        Ok(MemoryStoreBatchResult {
            completed: items.iter().all(|item| item.result.is_ok()),
            items,
        })
    }

    pub(super) async fn health_impl(
        &self,
        _request: MemoryBackendHealthRequest,
    ) -> MemoryStoreResult<MemoryBackendHealthResult> {
        let client = self.client().await?;
        let version: String = client
            .query_one(
                "SELECT extversion FROM pg_extension WHERE extname='vector'",
                &[],
            )
            .await
            .map_err(|error| store_error("probe pgvector extension", error, true))?
            .get(0);
        let vector_type: String = client.query_one("SELECT format_type(atttypid, atttypmod) FROM pg_attribute WHERE attrelid='tandem_memory_chunks'::regclass AND attname='embedding'", &[]).await.map_err(|error| store_error("probe pgvector dimension", error, true))?.get(0);
        let expected_vector_type = format!("vector({})", self.embedding_dimension);
        Ok(MemoryBackendHealthResult {
            backend: MemoryBackendKind::Postgres,
            status: MemoryBackendHealthStatus::Healthy,
            repaired: false,
            checks: vec![
                MemoryBackendHealthCheck {
                    name: "connection".to_string(),
                    healthy: true,
                    detail: None,
                },
                MemoryBackendHealthCheck {
                    name: "pgvector".to_string(),
                    healthy: true,
                    detail: Some(version),
                },
                MemoryBackendHealthCheck {
                    name: "embedding_dimension".to_string(),
                    healthy: vector_type == expected_vector_type,
                    detail: Some(vector_type),
                },
            ],
        })
    }

    pub(super) async fn recover_impl(
        &self,
        request: MemoryBackendRecoveryRequest,
    ) -> MemoryStoreResult<MemoryBackendRecoveryResult> {
        let client = self.client().await?;
        let changed = match request.action {
            MemoryBackendRecoveryAction::RepairIndexes => {
                client.batch_execute("REINDEX TABLE tandem_memory_chunks; REINDEX TABLE tandem_memory_global_records").await.map_err(|error| store_error("repair PostgreSQL memory indexes", error, true))?;
                true
            }
            MemoryBackendRecoveryAction::ResetAllData => {
                if !request.confirm_data_loss {
                    return Err(MemoryStoreError::invalid(
                        "ResetAllData requires confirm_data_loss=true",
                    ));
                }
                client.batch_execute("TRUNCATE tandem_memory_chunks, tandem_memory_global_records, tandem_memory_entities").await.map_err(|error| store_error("reset PostgreSQL memory data", error, true))?;
                true
            }
        };
        Ok(MemoryBackendRecoveryResult {
            backend: MemoryBackendKind::Postgres,
            action: request.action,
            changed,
        })
    }
}
