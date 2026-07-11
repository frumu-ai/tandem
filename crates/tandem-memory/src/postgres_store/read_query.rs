use pgvector::Vector;
use tokio_postgres::types::ToSql;

use super::*;

fn reject_narrowed_entity_scope(scope: &MemoryReadScope) -> MemoryStoreResult<()> {
    if scope.org_unit.is_some() || scope.subject.is_some() {
        return Err(MemoryStoreError::new(
            MemoryStoreErrorKind::ScopeViolation,
            "PostgreSQL entity reads cannot widen an org-unit/subject scope",
        ));
    }
    Ok(())
}

fn validate_chunk_list_selector(selector: &MemoryChunkSelector) -> MemoryStoreResult<()> {
    match selector.tier {
        crate::types::MemoryTier::Session
            if selector.session_id.as_deref().is_none_or(str::is_empty) =>
        {
            Err(MemoryStoreError::invalid(
                "tier=session chunk lists require a non-empty session_id",
            ))
        }
        crate::types::MemoryTier::Project
            if selector.project_id.as_deref().is_none_or(str::is_empty) =>
        {
            Err(MemoryStoreError::invalid(
                "tier=project chunk lists require a non-empty project_id",
            ))
        }
        _ => Ok(()),
    }
}

fn validate_chunk_search_selector(selector: &MemoryChunkSelector) -> MemoryStoreResult<()> {
    match selector.tier {
        crate::types::MemoryTier::Session
            if selector.session_id.as_deref().is_some_and(str::is_empty) =>
        {
            Err(MemoryStoreError::invalid(
                "session_id must be non-empty when provided",
            ))
        }
        crate::types::MemoryTier::Project | crate::types::MemoryTier::Global => {
            validate_chunk_list_selector(selector)
        }
        crate::types::MemoryTier::Session => Ok(()),
    }
}

fn current_principal_allows_row(
    tenant: &crate::types::MemoryTenantScope,
    owner_subject: Option<&str>,
    data_class: &str,
    source_binding_id: Option<&str>,
) -> bool {
    let Some(principal) = crate::decrypt_context::current_decrypt_principal() else {
        return true;
    };
    if principal.tenant_scope != *tenant {
        return false;
    }
    let Ok(data_class) = serde_json::from_value::<tandem_enterprise_contract::DataClass>(
        serde_json::Value::String(data_class.to_string()),
    ) else {
        return false;
    };
    principal.allowed_data_classes.contains(&data_class)
        && source_binding_id.is_none_or(|source| {
            principal
                .allowed_source_binding_ids
                .iter()
                .any(|allowed| allowed == source)
        })
        && owner_subject.is_none_or(|owner| {
            principal
                .allowed_owner_subjects
                .iter()
                .any(|allowed| allowed == owner)
        })
}

struct PrincipalSqlGrants {
    bypass: bool,
    tenant_matches: bool,
    data_classes: Vec<String>,
    source_binding_ids: Vec<String>,
    owner_subjects: Vec<String>,
}

fn current_principal_sql_grants(tenant: &crate::types::MemoryTenantScope) -> PrincipalSqlGrants {
    let Some(principal) = crate::decrypt_context::current_decrypt_principal() else {
        return PrincipalSqlGrants {
            bypass: true,
            tenant_matches: true,
            data_classes: Vec::new(),
            source_binding_ids: Vec::new(),
            owner_subjects: Vec::new(),
        };
    };
    PrincipalSqlGrants {
        bypass: false,
        tenant_matches: principal.tenant_scope == *tenant,
        data_classes: principal
            .allowed_data_classes
            .into_iter()
            .filter_map(|class| {
                serde_json::to_value(class)
                    .ok()
                    .and_then(|value| value.as_str().map(ToString::to_string))
            })
            .collect(),
        source_binding_ids: principal.allowed_source_binding_ids,
        owner_subjects: principal.allowed_owner_subjects,
    }
}

fn build_context_tree(nodes: &[MemoryNode], parent_uri: &str, max_depth: usize) -> Vec<TreeNode> {
    if max_depth == 0 {
        return Vec::new();
    }
    nodes
        .iter()
        .filter(|node| node.parent_uri.as_deref() == Some(parent_uri))
        .map(|node| TreeNode {
            children: if node.node_type == crate::types::NodeType::Directory {
                build_context_tree(nodes, &node.uri, max_depth.saturating_sub(1))
            } else {
                Vec::new()
            },
            node: node.clone(),
            layer_summary: None,
        })
        .collect()
}
use crate::types::{
    CleanupLogEntry, GlobalMemoryRecord, GlobalMemorySearchHit, KnowledgeItemRecord,
    KnowledgeSpaceRecord, MemoryChunk, MemoryNode, MemoryStats, ProjectMemoryStats,
    SourceObjectLifecycleRecord, TreeNode,
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

fn rerank_distance(metric: PostgresDistanceMetric, query: &[f32], candidate: &[f32]) -> f64 {
    let dot = query
        .iter()
        .zip(candidate)
        .map(|(left, right)| f64::from(*left) * f64::from(*right))
        .sum::<f64>();
    match metric {
        PostgresDistanceMetric::InnerProduct => -dot,
        PostgresDistanceMetric::Euclidean => query
            .iter()
            .zip(candidate)
            .map(|(left, right)| {
                let delta = f64::from(*left) - f64::from(*right);
                delta * delta
            })
            .sum::<f64>()
            .sqrt(),
        PostgresDistanceMetric::Cosine => {
            let query_norm = query
                .iter()
                .map(|value| f64::from(*value).powi(2))
                .sum::<f64>()
                .sqrt();
            let candidate_norm = candidate
                .iter()
                .map(|value| f64::from(*value).powi(2))
                .sum::<f64>()
                .sqrt();
            if query_norm == 0.0 || candidate_norm == 0.0 {
                1.0
            } else {
                1.0 - dot / (query_norm * candidate_norm)
            }
        }
    }
}

impl PostgresMemoryStore {
    pub(super) async fn entity<T: serde::de::DeserializeOwned>(
        &self,
        scope: &MemoryReadScope,
        entity_type: &str,
        key1: &str,
        key2: &str,
    ) -> MemoryStoreResult<Option<T>> {
        reject_narrowed_entity_scope(scope)?;
        let client = self.client().await?;
        let row = client
            .query_opt(
                "SELECT data,data_ciphertext,data_envelope,data_policy_decision_id,data_audit_id
                 FROM tandem_memory_entities
                 WHERE tenant_org_id=$1 AND tenant_workspace_id=$2
                   AND tenant_deployment_id=$3 AND entity_type=$4 AND key1=$5 AND key2=$6",
                &[
                    &scope.tenant.org_id,
                    &scope.tenant.workspace_id,
                    &deployment(&scope.tenant),
                    &entity_type,
                    &key1,
                    &key2,
                ],
            )
            .await
            .map_err(|error| store_error("read PostgreSQL memory entity", error, true))?;
        row.map(|row| {
            let key_scope = MemoryKeyScope::new(
                &scope.tenant,
                tandem_enterprise_contract::DataClass::Internal,
                None,
            );
            self.decode_payload(
                row.get(0),
                row.get(1),
                row.get(2),
                &key_scope,
                row.get(3),
                row.get(4),
            )
        })
        .transpose()
    }

    pub(super) async fn read_impl(
        &self,
        request: MemoryStoreReadRequest,
    ) -> MemoryStoreResult<MemoryStoreReadResult> {
        match request {
            MemoryStoreReadRequest::Chunks {
                scope,
                selector,
                limit,
            } => {
                validate_chunk_list_selector(&selector)?;
                let client = self.client().await?;
                let grants = current_principal_sql_grants(&scope.tenant);
                let rows = client
                    .query(
                        "SELECT data,data_ciphertext,data_envelope,data_policy_decision_id,
                                data_audit_id,owner_org_unit_id,owner_subject,data_class,source_binding_id FROM tandem_memory_chunks
                         WHERE tenant_org_id=$1 AND tenant_workspace_id=$2
                           AND tenant_deployment_id=$3 AND tier=$4
                           AND ($5::text IS NULL OR project_id=$5)
                           AND ($6::text IS NULL OR session_id=$6)
                           AND ($7::text IS NULL OR owner_org_unit_id=$7 OR tenant_shared=true)
                           AND (owner_subject IS NULL OR owner_subject=$8)
                           AND ($9::boolean OR ($10::boolean
                                AND data_class=ANY($11::text[])
                                AND (source_binding_id IS NULL OR source_binding_id=ANY($12::text[]))
                                AND (owner_subject IS NULL OR owner_subject=ANY($13::text[]))))
                         ORDER BY created_at DESC LIMIT $14",
                        &[
                            &scope.tenant.org_id,
                            &scope.tenant.workspace_id,
                            &deployment(&scope.tenant),
                            &selector_tier(&selector),
                            &selector.project_id,
                            &selector.session_id,
                            &scope.org_unit,
                            &scope.subject,
                            &grants.bypass,
                            &grants.tenant_matches,
                            &grants.data_classes,
                            &grants.source_binding_ids,
                            &grants.owner_subjects,
                            &limit.unwrap_or(1000).clamp(1, 10_000),
                        ],
                    )
                    .await
                    .map_err(|error| store_error("read PostgreSQL chunks", error, true))?;
                let chunks = rows
                    .into_iter()
                    .filter(|row| {
                        current_principal_allows_row(
                            &scope.tenant,
                            row.get::<_, Option<String>>(6).as_deref(),
                            &row.get::<_, String>(7),
                            row.get::<_, Option<String>>(8).as_deref(),
                        )
                    })
                    .map(|row| {
                        let key_scope = Self::persisted_key_scope(
                            &scope.tenant,
                            row.get(5),
                            row.get(6),
                            row.get(7),
                            row.get(8),
                        )?;
                        self.decode_payload(
                            row.get(0),
                            row.get(1),
                            row.get(2),
                            &key_scope,
                            row.get(3),
                            row.get(4),
                        )
                    })
                    .collect::<MemoryStoreResult<Vec<MemoryChunk>>>()?;
                Ok(MemoryStoreReadResult::Chunks(chunks))
            }
            MemoryStoreReadRequest::GlobalRecord { scope, id } => {
                let client = self.client().await?;
                let row = client
                    .query_opt(
                        "SELECT data,data_ciphertext,data_envelope,data_policy_decision_id,
                                data_audit_id,owner_org_unit_id,owner_subject,data_class,source_binding_id FROM tandem_memory_global_records
                         WHERE id=$1 AND tenant_org_id=$2 AND tenant_workspace_id=$3
                           AND tenant_deployment_id=$4
                           AND ($5::text IS NULL OR owner_org_unit_id=$5)
                           AND ($6::boolean OR private=false OR owner_subject=$7)",
                        &[
                            &id,
                            &scope.tenant.org_id,
                            &scope.tenant.workspace_id,
                            &deployment(&scope.tenant),
                            &scope.org_unit,
                            &(scope.access == MemoryReadAccess::TrustedUnrestricted),
                            &scope.subject,
                        ],
                    )
                    .await
                    .map_err(|error| store_error("read PostgreSQL global memory", error, true))?;
                Ok(MemoryStoreReadResult::GlobalRecord(
                    row.filter(|row| {
                        current_principal_allows_row(
                            &scope.tenant,
                            row.get::<_, Option<String>>(6).as_deref(),
                            &row.get::<_, String>(7),
                            row.get::<_, Option<String>>(8).as_deref(),
                        )
                    })
                    .map(|row| {
                        let key_scope = Self::persisted_key_scope(
                            &scope.tenant,
                            row.get(5),
                            row.get(6),
                            row.get(7),
                            row.get(8),
                        )?;
                        self.decode_payload(
                            row.get(0),
                            row.get(1),
                            row.get(2),
                            &key_scope,
                            row.get(3),
                            row.get(4),
                        )
                    })
                    .transpose()?,
                ))
            }
            MemoryStoreReadRequest::ProjectConfig { scope, project_id } => {
                Ok(MemoryStoreReadResult::ProjectConfig(
                    self.entity(&scope, "project_config", &project_id, "")
                        .await?
                        .unwrap_or_default(),
                ))
            }
            MemoryStoreReadRequest::Stats { scope } => {
                reject_narrowed_entity_scope(&scope)?;
                let client = self.client().await?;
                let row = client
                    .query_one(
                        "SELECT COUNT(*)::bigint,
                            COUNT(*) FILTER (WHERE tier='session')::bigint,
                            COUNT(*) FILTER (WHERE tier='project')::bigint,
                            COUNT(*) FILTER (WHERE tier='global')::bigint,
                            COALESCE(SUM(COALESCE(octet_length(data::text),octet_length(data_ciphertext))),0)::bigint,
                            COALESCE(SUM(COALESCE(octet_length(data::text),octet_length(data_ciphertext))) FILTER (WHERE tier='session'),0)::bigint,
                            COALESCE(SUM(COALESCE(octet_length(data::text),octet_length(data_ciphertext))) FILTER (WHERE tier='project'),0)::bigint,
                            COALESCE(SUM(COALESCE(octet_length(data::text),octet_length(data_ciphertext))) FILTER (WHERE tier='global'),0)::bigint
                          FROM tandem_memory_chunks WHERE tenant_org_id=$1
                            AND tenant_workspace_id=$2 AND tenant_deployment_id=$3",
                        &[
                            &scope.tenant.org_id,
                            &scope.tenant.workspace_id,
                            &deployment(&scope.tenant),
                        ],
                    )
                    .await
                    .map_err(|error| store_error("read PostgreSQL memory stats", error, true))?;
                Ok(MemoryStoreReadResult::Stats(MemoryStats {
                    total_chunks: row.get(0),
                    session_chunks: row.get(1),
                    project_chunks: row.get(2),
                    global_chunks: row.get(3),
                    total_bytes: row.get(4),
                    session_bytes: row.get(5),
                    project_bytes: row.get(6),
                    global_bytes: row.get(7),
                    file_size: 0,
                    last_cleanup: None,
                }))
            }
            MemoryStoreReadRequest::ProjectStats { scope, project_id } => {
                reject_narrowed_entity_scope(&scope)?;
                let client = self.client().await?;
                let row = client
                    .query_one(
                        "SELECT COUNT(*)::bigint, COALESCE(SUM(COALESCE(octet_length(data::text),octet_length(data_ciphertext))),0)::bigint,
                            COUNT(*) FILTER (WHERE source='file')::bigint,
                            COALESCE(SUM(COALESCE(octet_length(data::text),octet_length(data_ciphertext))) FILTER (WHERE source='file'),0)::bigint
                         FROM tandem_memory_chunks WHERE tenant_org_id=$1 AND tenant_workspace_id=$2
                           AND tenant_deployment_id=$3 AND tier='project' AND project_id=$4",
                        &[
                            &scope.tenant.org_id,
                            &scope.tenant.workspace_id,
                            &deployment(&scope.tenant),
                            &project_id,
                        ],
                    )
                    .await
                    .map_err(|error| store_error("read PostgreSQL project stats", error, true))?;
                let indexed_files = self
                    .query_entity_values::<MemoryImportIndexEntry>(
                        &scope,
                        "import_index",
                        &project_id,
                    )
                    .await?
                    .len() as i64;
                let index_status = self
                    .entity::<serde_json::Value>(&scope, "project_index_status", &project_id, "")
                    .await?;
                let status_value = |field: &str| {
                    index_status
                        .as_ref()
                        .and_then(|status| status.get(field))
                        .and_then(serde_json::Value::as_i64)
                };
                Ok(MemoryStoreReadResult::ProjectStats(ProjectMemoryStats {
                    project_id,
                    project_chunks: row.get(0),
                    project_bytes: row.get(1),
                    file_index_chunks: row.get(2),
                    file_index_bytes: row.get(3),
                    indexed_files,
                    last_indexed_at: status_value("updated_at_ms")
                        .and_then(chrono::DateTime::from_timestamp_millis),
                    last_total_files: status_value("total_files"),
                    last_processed_files: status_value("processed_files"),
                    last_indexed_files: status_value("indexed_files"),
                    last_skipped_files: status_value("skipped_files"),
                    last_errors: status_value("errors"),
                }))
            }
            MemoryStoreReadRequest::KnowledgeSpace { scope, id } => {
                Ok(MemoryStoreReadResult::KnowledgeSpace(
                    self.entity(&scope, "knowledge_space", &id, "").await?,
                ))
            }
            MemoryStoreReadRequest::KnowledgeItem { scope, id } => {
                Ok(MemoryStoreReadResult::KnowledgeItem(
                    self.entity(&scope, "knowledge_item", &id, "").await?,
                ))
            }
            MemoryStoreReadRequest::KnowledgeCoverage {
                scope,
                coverage_key,
                space_id,
            } => Ok(MemoryStoreReadResult::KnowledgeCoverage(
                self.entity(&scope, "knowledge_coverage", &space_id, &coverage_key)
                    .await?,
            )),
            MemoryStoreReadRequest::ImportIndexEntry {
                scope,
                selector,
                path,
            } => Ok(MemoryStoreReadResult::ImportIndexEntry(
                self.entity(
                    &scope,
                    "import_index",
                    &selector
                        .project_id
                        .or(selector.session_id)
                        .unwrap_or_default(),
                    &path,
                )
                .await?,
            )),
            MemoryStoreReadRequest::ContextNode { scope, uri } => {
                Ok(MemoryStoreReadResult::ContextNode(
                    self.entity(&scope, "context_node_uri", &uri, "").await?,
                ))
            }
            MemoryStoreReadRequest::ContextLayer {
                scope,
                node_id,
                layer_type,
            } => Ok(MemoryStoreReadResult::ContextLayer(
                self.entity(
                    &scope,
                    "context_layer",
                    &node_id,
                    &serde_json::to_string(&layer_type).unwrap_or_default(),
                )
                .await?,
            )),
        }
    }

    pub(super) async fn query_entity_values<T: serde::de::DeserializeOwned>(
        &self,
        scope: &MemoryReadScope,
        entity_type: &str,
        key1: &str,
    ) -> MemoryStoreResult<Vec<T>> {
        reject_narrowed_entity_scope(scope)?;
        let client = self.client().await?;
        let rows = client
            .query(
                "SELECT data,data_ciphertext,data_envelope,data_policy_decision_id,data_audit_id
                 FROM tandem_memory_entities WHERE tenant_org_id=$1
                  AND tenant_workspace_id=$2 AND tenant_deployment_id=$3
                  AND entity_type=$4 AND ($5='' OR key1=$5) ORDER BY updated_at DESC",
                &[
                    &scope.tenant.org_id,
                    &scope.tenant.workspace_id,
                    &deployment(&scope.tenant),
                    &entity_type,
                    &key1,
                ],
            )
            .await
            .map_err(|error| store_error("list PostgreSQL memory entities", error, true))?;
        let key_scope = MemoryKeyScope::new(
            &scope.tenant,
            tandem_enterprise_contract::DataClass::Internal,
            None,
        );
        rows.into_iter()
            .map(|row| {
                self.decode_payload(
                    row.get(0),
                    row.get(1),
                    row.get(2),
                    &key_scope,
                    row.get(3),
                    row.get(4),
                )
            })
            .collect()
    }

    pub(super) async fn query_impl(
        &self,
        request: MemoryStoreQueryRequest,
    ) -> MemoryStoreResult<MemoryStoreQueryResult> {
        match request {
            MemoryStoreQueryRequest::SimilarChunks {
                scope,
                selector,
                query_embedding,
                limit,
            } => {
                validate_chunk_search_selector(&selector)?;
                if query_embedding.len() != self.embedding_dimension {
                    return Err(MemoryStoreError::invalid(format!(
                        "embedding dimension mismatch: expected {}, got {}",
                        self.embedding_dimension,
                        query_embedding.len()
                    )));
                }
                let client = self.client().await?;
                if self.search_surface_mode == PostgresSearchSurfaceMode::Disabled {
                    return Err(MemoryStoreError::unsupported(
                        "PostgreSQL vector search is disabled by TANDEM_MEMORY_SEARCH_SURFACE_MODE",
                    ));
                }
                if self.search_surface_mode == PostgresSearchSurfaceMode::EncryptedRerank {
                    let grants = current_principal_sql_grants(&scope.tenant);
                    let rows = client.query(
                        "SELECT data,data_ciphertext,data_envelope,data_policy_decision_id,
                                data_audit_id,owner_org_unit_id,owner_subject,data_class,source_binding_id,embedding_ciphertext,embedding_envelope,
                                search_policy_decision_id,search_audit_id
                         FROM tandem_memory_chunks
                         WHERE tenant_org_id=$1 AND tenant_workspace_id=$2
                           AND tenant_deployment_id=$3 AND tier=$4
                           AND ($5::text IS NULL OR project_id=$5)
                           AND ($6::text IS NULL OR session_id=$6)
                           AND ($7::text IS NULL OR owner_org_unit_id=$7 OR tenant_shared=true)
                           AND (owner_subject IS NULL OR owner_subject=$8)
                           AND embedding_ciphertext IS NOT NULL
                           AND ($9::boolean OR ($10::boolean
                                AND data_class=ANY($11::text[])
                                AND (source_binding_id IS NULL OR source_binding_id=ANY($12::text[]))
                                AND (owner_subject IS NULL OR owner_subject=ANY($13::text[]))))
                         ORDER BY created_at DESC LIMIT $14",
                        &[&scope.tenant.org_id,&scope.tenant.workspace_id,&deployment(&scope.tenant),
                          &selector_tier(&selector),&selector.project_id,&selector.session_id,
                          &scope.org_unit,&scope.subject,&grants.bypass,&grants.tenant_matches,
                          &grants.data_classes,&grants.source_binding_ids,&grants.owner_subjects,
                          &self.rerank_candidate_limit]
                    ).await.map_err(|error| store_error("load encrypted PostgreSQL vector candidates", error, true))?;
                    let mut hits = rows
                        .into_iter()
                        .filter(|row| {
                            current_principal_allows_row(
                                &scope.tenant,
                                row.get::<_, Option<String>>(6).as_deref(),
                                &row.get::<_, String>(7),
                                row.get::<_, Option<String>>(8).as_deref(),
                            )
                        })
                        .map(|row| {
                            let org_unit: Option<String> = row.get(5);
                            let owner_subject: Option<String> = row.get(6);
                            let key_scope = Self::persisted_key_scope(
                                &scope.tenant,
                                org_unit,
                                owner_subject,
                                row.get(7),
                                row.get(8),
                            )?;
                            let chunk: MemoryChunk = self.decode_payload(
                                row.get(0),
                                row.get(1),
                                row.get(2),
                                &key_scope,
                                row.get(3),
                                row.get(4),
                            )?;
                            let ciphertext: String = row.get(9);
                            let envelope = row
                                .get::<_, Option<serde_json::Value>>(10)
                                .map(from_json)
                                .transpose()?;
                            let policy_id: String = row.get(11);
                            let audit_id: String = row.get(12);
                            let candidate = self.decrypt_embedding(
                                &ciphertext,
                                envelope.as_ref(),
                                &key_scope,
                                &policy_id,
                                &audit_id,
                            )?;
                            if candidate.len() != self.embedding_dimension {
                                return Err(MemoryStoreError::new(
                                    MemoryStoreErrorKind::CorruptData,
                                    "encrypted PostgreSQL embedding has the wrong dimension",
                                ));
                            }
                            Ok((
                                chunk,
                                rerank_distance(self.distance_metric, &query_embedding, &candidate),
                            ))
                        })
                        .collect::<MemoryStoreResult<Vec<(MemoryChunk, f64)>>>()?;
                    hits.sort_by(|left, right| {
                        left.1
                            .total_cmp(&right.1)
                            .then_with(|| left.0.id.cmp(&right.0.id))
                    });
                    hits.truncate(limit.clamp(1, 1000) as usize);
                    return Ok(MemoryStoreQueryResult::SimilarChunks(hits));
                }
                let grants = current_principal_sql_grants(&scope.tenant);
                let sql = format!(
                    "SELECT data,data_ciphertext,data_envelope,data_policy_decision_id,
                            data_audit_id,owner_org_unit_id,owner_subject,data_class,source_binding_id,embedding {operator} $14 AS distance
                     FROM tandem_memory_chunks
                     WHERE tenant_org_id=$1 AND tenant_workspace_id=$2
                       AND tenant_deployment_id=$3 AND tier=$4
                       AND ($5::text IS NULL OR project_id=$5)
                       AND ($6::text IS NULL OR session_id=$6)
                       AND ($7::text IS NULL OR owner_org_unit_id=$7 OR tenant_shared=true)
                       AND (owner_subject IS NULL OR owner_subject=$8)
                       AND embedding IS NOT NULL
                       AND ($9::boolean OR ($10::boolean
                            AND data_class=ANY($11::text[])
                            AND (source_binding_id IS NULL OR source_binding_id=ANY($12::text[]))
                            AND (owner_subject IS NULL OR owner_subject=ANY($13::text[]))))
                     ORDER BY embedding {operator} $14 LIMIT $15",
                    operator = self.distance_metric.operator()
                );
                let vector = Vector::from(query_embedding);
                let params: [&(dyn ToSql + Sync); 15] = [
                    &scope.tenant.org_id,
                    &scope.tenant.workspace_id,
                    &deployment(&scope.tenant),
                    &selector_tier(&selector),
                    &selector.project_id,
                    &selector.session_id,
                    &scope.org_unit,
                    &scope.subject,
                    &grants.bypass,
                    &grants.tenant_matches,
                    &grants.data_classes,
                    &grants.source_binding_ids,
                    &grants.owner_subjects,
                    &vector,
                    &limit.clamp(1, 1000),
                ];
                let rows = client.query(&sql, &params).await.map_err(|error| {
                    store_error("search PostgreSQL pgvector memory", error, true)
                })?;
                let hits = rows
                    .into_iter()
                    .filter(|row| {
                        current_principal_allows_row(
                            &scope.tenant,
                            row.get::<_, Option<String>>(6).as_deref(),
                            &row.get::<_, String>(7),
                            row.get::<_, Option<String>>(8).as_deref(),
                        )
                    })
                    .map(|row| {
                        let key_scope = Self::persisted_key_scope(
                            &scope.tenant,
                            row.get(5),
                            row.get(6),
                            row.get(7),
                            row.get(8),
                        )?;
                        let chunk = self.decode_payload(
                            row.get(0),
                            row.get(1),
                            row.get(2),
                            &key_scope,
                            row.get(3),
                            row.get(4),
                        )?;
                        let distance: f64 = row.get(9);
                        Ok((chunk, distance))
                    })
                    .collect::<MemoryStoreResult<Vec<(MemoryChunk, f64)>>>()?;
                Ok(MemoryStoreQueryResult::SimilarChunks(hits))
            }
            MemoryStoreQueryRequest::SearchGlobalRecords {
                scope,
                user_id,
                query,
                limit,
                project_tag,
            } => {
                let records = self
                    .global_records(
                        &scope,
                        &user_id,
                        Some(&query),
                        project_tag.as_deref(),
                        None,
                        limit,
                        0,
                        false,
                    )
                    .await?;
                Ok(MemoryStoreQueryResult::GlobalSearchHits(
                    records
                        .into_iter()
                        .map(|record| GlobalMemorySearchHit { record, score: 1.0 })
                        .collect(),
                ))
            }
            MemoryStoreQueryRequest::ListGlobalRecords {
                scope,
                user_id,
                query,
                project_tag,
                channel_tag,
                limit,
                offset,
            } => Ok(MemoryStoreQueryResult::GlobalRecords(
                self.global_records(
                    &scope,
                    &user_id,
                    query.as_deref(),
                    project_tag.as_deref(),
                    channel_tag.as_deref(),
                    limit,
                    offset,
                    true,
                )
                .await?,
            )),
            MemoryStoreQueryRequest::KnowledgeSpaces { scope, project_id } => {
                let mut values = self
                    .query_entity_values::<KnowledgeSpaceRecord>(&scope, "knowledge_space", "")
                    .await?;
                if let Some(project_id) = project_id {
                    values.retain(|value| value.project_id.as_deref() == Some(project_id.as_str()));
                }
                Ok(MemoryStoreQueryResult::KnowledgeSpaces(values))
            }
            MemoryStoreQueryRequest::KnowledgeItems {
                scope,
                space_id,
                coverage_key,
            } => {
                let mut values = self
                    .query_entity_values::<KnowledgeItemRecord>(&scope, "knowledge_item", "")
                    .await?;
                values.retain(|value| value.space_id == space_id);
                if let Some(coverage_key) = coverage_key {
                    values.retain(|value| value.coverage_key == coverage_key);
                }
                Ok(MemoryStoreQueryResult::KnowledgeItems(values))
            }
            MemoryStoreQueryRequest::ImportIndexPaths { scope, selector } => {
                reject_narrowed_entity_scope(&scope)?;
                let key = selector
                    .project_id
                    .or(selector.session_id)
                    .unwrap_or_default();
                let client = self.client().await?;
                let rows = client
                    .query(
                        "SELECT key2 FROM tandem_memory_entities WHERE tenant_org_id=$1
                     AND tenant_workspace_id=$2 AND tenant_deployment_id=$3
                     AND entity_type='import_index' AND key1=$4 ORDER BY key2",
                        &[
                            &scope.tenant.org_id,
                            &scope.tenant.workspace_id,
                            &deployment(&scope.tenant),
                            &key,
                        ],
                    )
                    .await
                    .map_err(|error| store_error("list PostgreSQL import paths", error, true))?;
                Ok(MemoryStoreQueryResult::Paths(
                    rows.into_iter().map(|row| row.get(0)).collect(),
                ))
            }
            MemoryStoreQueryRequest::CleanupLog { scope, limit } => {
                Ok(MemoryStoreQueryResult::CleanupLog(
                    self.query_entity_values::<CleanupLogEntry>(&scope, "cleanup_log", "")
                        .await?
                        .into_iter()
                        .take(limit.max(0) as usize)
                        .collect(),
                ))
            }
            MemoryStoreQueryRequest::ContextNodes { scope, parent_uri } => {
                let values = self
                    .query_entity_values::<MemoryNode>(&scope, "context_node_uri", "")
                    .await?
                    .into_iter()
                    .filter(|node| node.parent_uri.as_deref() == Some(parent_uri.as_str()))
                    .collect();
                Ok(MemoryStoreQueryResult::ContextNodes(values))
            }
            MemoryStoreQueryRequest::ContextTree {
                scope,
                parent_uri,
                max_depth,
            } => {
                let values = self
                    .query_entity_values::<MemoryNode>(&scope, "context_node_uri", "")
                    .await?;
                Ok(MemoryStoreQueryResult::ContextTree(build_context_tree(
                    &values,
                    &parent_uri,
                    max_depth,
                )))
            }
            MemoryStoreQueryRequest::SourceObjectLifecyclesForBinding {
                scope,
                source_binding_id,
            } => Ok(MemoryStoreQueryResult::SourceObjectLifecycles(
                self.query_entity_values::<SourceObjectLifecycleRecord>(
                    &scope,
                    "source_lifecycle",
                    &source_binding_id,
                )
                .await?,
            )),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn global_records(
        &self,
        scope: &MemoryReadScope,
        user_id: &str,
        query: Option<&str>,
        project_tag: Option<&str>,
        channel_tag: Option<&str>,
        limit: i64,
        offset: i64,
        include_demoted: bool,
    ) -> MemoryStoreResult<Vec<GlobalMemoryRecord>> {
        let client = self.client().await?;
        let query = query.map(str::trim).filter(|query| !query.is_empty());
        let database_query =
            if self.search_surface_mode == PostgresSearchSurfaceMode::PlaintextPgvector {
                query
            } else {
                None
            };
        let database_limit = if query.is_some()
            && self.search_surface_mode != PostgresSearchSurfaceMode::PlaintextPgvector
        {
            self.rerank_candidate_limit
        } else {
            limit.clamp(1, 1000)
        };
        let grants = current_principal_sql_grants(&scope.tenant);
        let rows = client.query(
            "SELECT data,data_ciphertext,data_envelope,data_policy_decision_id,
                    data_audit_id,owner_org_unit_id,owner_subject,data_class,source_binding_id FROM tandem_memory_global_records
             WHERE tenant_org_id=$1 AND tenant_workspace_id=$2 AND tenant_deployment_id=$3
               AND (owner_subject=$4 OR (private=false AND owner_org_unit_id IS NOT NULL)
                    OR (owner_subject IS NULL AND owner_org_unit_id IS NULL AND user_id=$5))
               AND ($6::text IS NULL OR owner_org_unit_id=$6)
               AND ($7::boolean OR demoted=false)
               AND (expires_at_ms IS NULL OR expires_at_ms>$8)
               AND ($9::text IS NULL OR project_tag=$9)
               AND ($10::text IS NULL OR channel_tag=$10)
               AND ($11::text IS NULL
                    OR to_tsvector('simple', search_content) @@ plainto_tsquery('simple', $11)
                    OR source_type ILIKE '%' || $11 || '%'
                    OR run_id ILIKE '%' || $11 || '%')
               AND ($14::boolean OR ($15::boolean
                    AND data_class=ANY($16::text[])
                    AND (source_binding_id IS NULL OR source_binding_id=ANY($17::text[]))
                    AND (owner_subject IS NULL OR owner_subject=ANY($18::text[]))))
             ORDER BY created_at_ms DESC LIMIT $12 OFFSET $13",
            &[&scope.tenant.org_id, &scope.tenant.workspace_id, &deployment(&scope.tenant),
              &scope.subject, &user_id, &scope.org_unit, &include_demoted,
              &chrono::Utc::now().timestamp_millis(), &project_tag, &channel_tag,
              &database_query, &database_limit, &offset.max(0), &grants.bypass,
              &grants.tenant_matches, &grants.data_classes, &grants.source_binding_ids,
              &grants.owner_subjects]
        ).await.map_err(|error| store_error("query PostgreSQL global memory", error, true))?;
        let mut records = rows
            .into_iter()
            .filter(|row| {
                current_principal_allows_row(
                    &scope.tenant,
                    row.get::<_, Option<String>>(6).as_deref(),
                    &row.get::<_, String>(7),
                    row.get::<_, Option<String>>(8).as_deref(),
                )
            })
            .map(|row| {
                let key_scope = Self::persisted_key_scope(
                    &scope.tenant,
                    row.get(5),
                    row.get(6),
                    row.get(7),
                    row.get(8),
                )?;
                self.decode_payload(
                    row.get(0),
                    row.get(1),
                    row.get(2),
                    &key_scope,
                    row.get(3),
                    row.get(4),
                )
            })
            .collect::<MemoryStoreResult<Vec<GlobalMemoryRecord>>>()?;
        if let Some(query) = query
            .filter(|_| self.search_surface_mode != PostgresSearchSurfaceMode::PlaintextPgvector)
        {
            let terms = query
                .split_whitespace()
                .map(str::to_ascii_lowercase)
                .collect::<Vec<_>>();
            records.retain(|record| {
                let searchable = format!(
                    "{} {} {}",
                    record.content, record.source_type, record.run_id
                )
                .to_ascii_lowercase();
                terms.iter().all(|term| searchable.contains(term))
            });
            records.truncate(limit.clamp(1, 1000) as usize);
        }
        Ok(records)
    }
}
