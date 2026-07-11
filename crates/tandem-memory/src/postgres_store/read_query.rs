use pgvector::Vector;
use tokio_postgres::types::ToSql;

use super::*;
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

impl PostgresMemoryStore {
    pub(super) async fn entity<T: serde::de::DeserializeOwned>(
        &self,
        scope: &MemoryReadScope,
        entity_type: &str,
        key1: &str,
        key2: &str,
    ) -> MemoryStoreResult<Option<T>> {
        let client = self.client().await?;
        let row = client
            .query_opt(
                "SELECT data FROM tandem_memory_entities
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
        row.map(|row| from_json(row.get(0))).transpose()
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
                let client = self.client().await?;
                let rows = client
                    .query(
                        "SELECT data FROM tandem_memory_chunks
                         WHERE tenant_org_id=$1 AND tenant_workspace_id=$2
                           AND tenant_deployment_id=$3 AND tier=$4
                           AND ($5::text IS NULL OR project_id=$5)
                           AND ($6::text IS NULL OR session_id=$6)
                           AND ($7::text IS NULL OR owner_org_unit_id=$7)
                           AND (owner_subject IS NULL OR owner_subject=$8)
                         ORDER BY created_at DESC LIMIT $9",
                        &[
                            &scope.tenant.org_id,
                            &scope.tenant.workspace_id,
                            &deployment(&scope.tenant),
                            &selector_tier(&selector),
                            &selector.project_id,
                            &selector.session_id,
                            &scope.org_unit,
                            &scope.subject,
                            &limit.unwrap_or(1000).clamp(1, 10_000),
                        ],
                    )
                    .await
                    .map_err(|error| store_error("read PostgreSQL chunks", error, true))?;
                let chunks = rows
                    .into_iter()
                    .map(|row| from_json(row.get(0)))
                    .collect::<MemoryStoreResult<Vec<MemoryChunk>>>()?;
                Ok(MemoryStoreReadResult::Chunks(chunks))
            }
            MemoryStoreReadRequest::GlobalRecord { scope, id } => {
                let client = self.client().await?;
                let row = client
                    .query_opt(
                        "SELECT data FROM tandem_memory_global_records
                         WHERE id=$1 AND tenant_org_id=$2 AND tenant_workspace_id=$3
                           AND tenant_deployment_id=$4
                           AND ($5::text IS NULL OR owner_org_unit_id=$5)
                           AND ($6::boolean OR owner_subject=$7 OR
                                (private=false AND owner_org_unit_id IS NOT NULL))",
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
                    row.map(|row| from_json(row.get(0))).transpose()?,
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
                let client = self.client().await?;
                let row = client
                    .query_one(
                        "SELECT COUNT(*)::bigint,
                            COUNT(*) FILTER (WHERE tier='session')::bigint,
                            COUNT(*) FILTER (WHERE tier='project')::bigint,
                            COUNT(*) FILTER (WHERE tier='global')::bigint,
                            COALESCE(SUM(octet_length(data::text)),0)::bigint,
                            COALESCE(SUM(octet_length(data::text)) FILTER (WHERE tier='session'),0)::bigint,
                            COALESCE(SUM(octet_length(data::text)) FILTER (WHERE tier='project'),0)::bigint,
                            COALESCE(SUM(octet_length(data::text)) FILTER (WHERE tier='global'),0)::bigint
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
                let client = self.client().await?;
                let row = client
                    .query_one(
                        "SELECT COUNT(*)::bigint, COALESCE(SUM(octet_length(data::text)),0)::bigint,
                            COUNT(*) FILTER (WHERE data->>'source'='file')::bigint,
                            COALESCE(SUM(octet_length(data::text)) FILTER (WHERE data->>'source'='file'),0)::bigint
                         FROM tandem_memory_chunks WHERE tenant_org_id=$1 AND tenant_workspace_id=$2
                           AND tenant_deployment_id=$3 AND project_id=$4",
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
                Ok(MemoryStoreReadResult::ProjectStats(ProjectMemoryStats {
                    project_id,
                    project_chunks: row.get(0),
                    project_bytes: row.get(1),
                    file_index_chunks: row.get(2),
                    file_index_bytes: row.get(3),
                    indexed_files,
                    last_indexed_at: None,
                    last_total_files: None,
                    last_processed_files: None,
                    last_indexed_files: None,
                    last_skipped_files: None,
                    last_errors: None,
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
        let client = self.client().await?;
        let rows = client
            .query(
                "SELECT data FROM tandem_memory_entities WHERE tenant_org_id=$1
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
        rows.into_iter().map(|row| from_json(row.get(0))).collect()
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
                if query_embedding.len() != self.embedding_dimension {
                    return Err(MemoryStoreError::invalid(format!(
                        "embedding dimension mismatch: expected {}, got {}",
                        self.embedding_dimension,
                        query_embedding.len()
                    )));
                }
                let client = self.client().await?;
                let sql = format!(
                    "SELECT data, embedding {operator} $9 AS distance
                     FROM tandem_memory_chunks
                     WHERE tenant_org_id=$1 AND tenant_workspace_id=$2
                       AND tenant_deployment_id=$3 AND tier=$4
                       AND ($5::text IS NULL OR project_id=$5)
                       AND ($6::text IS NULL OR session_id=$6)
                       AND ($7::text IS NULL OR owner_org_unit_id=$7)
                       AND (owner_subject IS NULL OR owner_subject=$8)
                     ORDER BY embedding {operator} $9 LIMIT $10",
                    operator = self.distance_metric.operator()
                );
                let vector = Vector::from(query_embedding);
                let params: [&(dyn ToSql + Sync); 10] = [
                    &scope.tenant.org_id,
                    &scope.tenant.workspace_id,
                    &deployment(&scope.tenant),
                    &selector_tier(&selector),
                    &selector.project_id,
                    &selector.session_id,
                    &scope.org_unit,
                    &scope.subject,
                    &vector,
                    &limit.clamp(1, 1000),
                ];
                let rows = client.query(&sql, &params).await.map_err(|error| {
                    store_error("search PostgreSQL pgvector memory", error, true)
                })?;
                let hits = rows
                    .into_iter()
                    .map(|row| {
                        let chunk = from_json(row.get(0))?;
                        let distance: f64 = row.get(1);
                        let score = match self.distance_metric {
                            PostgresDistanceMetric::InnerProduct => -distance,
                            _ => 1.0 / (1.0 + distance.max(0.0)),
                        };
                        Ok((chunk, score))
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
                scope, parent_uri, ..
            } => {
                let values = self
                    .query_entity_values::<MemoryNode>(&scope, "context_node_uri", "")
                    .await?
                    .into_iter()
                    .filter(|node| node.parent_uri.as_deref() == Some(parent_uri.as_str()))
                    .map(|node| TreeNode {
                        node,
                        children: Vec::new(),
                        layer_summary: None,
                    })
                    .collect();
                Ok(MemoryStoreQueryResult::ContextTree(values))
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

    async fn global_records(
        &self,
        scope: &MemoryReadScope,
        user_id: &str,
        query: Option<&str>,
        project_tag: Option<&str>,
        channel_tag: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> MemoryStoreResult<Vec<GlobalMemoryRecord>> {
        let client = self.client().await?;
        let rows = client.query(
            "SELECT data FROM tandem_memory_global_records
             WHERE tenant_org_id=$1 AND tenant_workspace_id=$2 AND tenant_deployment_id=$3
               AND (owner_subject=$4 OR (private=false AND owner_org_unit_id IS NOT NULL)
                    OR (owner_subject IS NULL AND owner_org_unit_id IS NULL AND user_id=$5))
               AND ($6::text IS NULL OR owner_org_unit_id=$6)
               AND demoted=false AND (expires_at_ms IS NULL OR expires_at_ms>$7)
               AND ($8::text IS NULL OR project_tag=$8)
               AND ($9::text IS NULL OR channel_tag=$9)
               AND ($10::text IS NULL OR to_tsvector('simple', search_content) @@ plainto_tsquery('simple', $10))
             ORDER BY created_at_ms DESC LIMIT $11 OFFSET $12",
            &[&scope.tenant.org_id, &scope.tenant.workspace_id, &deployment(&scope.tenant),
              &scope.subject, &user_id, &scope.org_unit, &chrono::Utc::now().timestamp_millis(),
              &project_tag, &channel_tag, &query, &limit.clamp(1, 1000), &offset.max(0)]
        ).await.map_err(|error| store_error("query PostgreSQL global memory", error, true))?;
        rows.into_iter().map(|row| from_json(row.get(0))).collect()
    }
}
