#![allow(clippy::all)]

// Database Layer Module
// SQLite + sqlite-vec for vector storage

use crate::types::owner_org_unit_id_from_metadata;
use crate::types::{
    CleanupLogEntry, ClearFileIndexResult, GlobalMemoryRecord, GlobalMemorySearchHit,
    GlobalMemoryWriteResult, KnowledgeCoverageRecord, KnowledgeItemRecord, KnowledgeItemStatus,
    KnowledgePromotionRequest, KnowledgePromotionResult, KnowledgeSpaceRecord, MemoryChunk,
    MemoryConfig, MemoryError, MemoryResult, MemoryStats, MemoryTenantScope, MemoryTier,
    ProjectMemoryStats, SourceObjectLifecycleRecord, SourceObjectLifecycleState,
    DEFAULT_EMBEDDING_DIMENSION,
};
use chrono::{DateTime, Utc};
use rusqlite::{ffi::sqlite3_auto_extension, params, Connection, OptionalExtension, Row};
use sqlite_vec::sqlite3_vec_init;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::sync::Mutex;

type ProjectIndexStatusRow = (
    Option<String>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
);

/// Database connection manager
pub struct MemoryDatabase {
    conn: Arc<Mutex<Connection>>,
    db_path: std::path::PathBuf,
    crypto: crate::crypto::MemoryCryptoProvider,
    strict_tenant_enforcement: std::sync::atomic::AtomicBool,
}

static SCHEMA_INIT_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
const MEMORY_SCHEMA_MIGRATIONS: &[(i64, &str)] = &[
    (1, "bootstrap_memory_schema"),
    (2, "memory_config_retention_columns"),
];

fn ensure_schema_migrations_table(conn: &Connection) -> MemoryResult<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at_ms INTEGER NOT NULL
        )",
        [],
    )?;
    Ok(())
}

fn record_schema_migration(conn: &Connection, version: i64, name: &str) -> MemoryResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (version, name, applied_at_ms)
         VALUES (?1, ?2, ?3)",
        params![version, name, Utc::now().timestamp_millis()],
    )?;
    Ok(())
}

/// Process-wide default for strict tenant enforcement, set once at startup by
/// the host (engine `serve` in hosted/enterprise auth modes) before databases
/// are opened. New `MemoryDatabase` instances inherit this default, so the
/// many ad-hoc construction sites in tandem-server stay fail-closed without
/// each one threading a flag.
static STRICT_TENANT_ENFORCEMENT_DEFAULT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Enable (or disable) strict tenant enforcement for all `MemoryDatabase`
/// instances opened after this call. In strict mode, reads and writes carrying
/// the local-implicit tenant scope are rejected instead of landing in the
/// shared "local" partition.
pub fn set_strict_tenant_enforcement_default(enabled: bool) {
    STRICT_TENANT_ENFORCEMENT_DEFAULT.store(enabled, std::sync::atomic::Ordering::SeqCst);
}

pub fn strict_tenant_enforcement_default() -> bool {
    STRICT_TENANT_ENFORCEMENT_DEFAULT.load(std::sync::atomic::Ordering::SeqCst)
}

include!("memory_database_impl_parts/part01.rs");
include!("memory_database_impl_parts/part02.rs");
include!("memory_database_impl_parts/part02_global_scoped.rs");
include!("memory_database_impl_parts/part02_retention.rs");
include!("memory_database_impl_parts/part03.rs");

/// Convert a database row to a MemoryChunk
fn row_to_chunk(
    row: &Row,
    tier: MemoryTier,
    crypto: &crate::crypto::MemoryCryptoProvider,
) -> Result<MemoryChunk, rusqlite::Error> {
    let map_decrypt_err = |err: crate::types::MemoryError| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(err))
    };
    let id: String = row.get(0)?;
    let content_raw: String = row.get(1)?;
    let content = crypto
        .decrypt_field(&content_raw)
        .map_err(map_decrypt_err)?;
    let (session_id, project_id, source_idx, created_at_idx, token_count_idx, metadata_idx) =
        match tier {
            MemoryTier::Session => (
                Some(row.get(2)?),
                row.get(3)?,
                4usize,
                5usize,
                6usize,
                7usize,
            ),
            MemoryTier::Project => (
                row.get(2)?,
                Some(row.get(3)?),
                4usize,
                5usize,
                6usize,
                7usize,
            ),
            MemoryTier::Global => (None, None, 2usize, 3usize, 4usize, 5usize),
        };

    let source: String = row.get(source_idx)?;
    let created_at_str: String = row.get(created_at_idx)?;
    let token_count: i64 = row.get(token_count_idx)?;
    let metadata_raw: Option<String> = row.get(metadata_idx)?;
    let metadata_str = match metadata_raw {
        Some(s) if !s.is_empty() => Some(crypto.decrypt_field(&s).map_err(map_decrypt_err)?),
        other => other,
    };

    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
        })?
        .with_timezone(&Utc);

    let metadata = metadata_str
        .filter(|s| !s.is_empty())
        .and_then(|s| serde_json::from_str(&s).ok());

    let source_path = row.get::<_, Option<String>>("source_path").ok().flatten();
    let source_mtime = row.get::<_, Option<i64>>("source_mtime").ok().flatten();
    let source_size = row.get::<_, Option<i64>>("source_size").ok().flatten();
    let source_hash = row.get::<_, Option<String>>("source_hash").ok().flatten();
    let subject = row
        .get::<_, Option<String>>("subject")
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty());
    let tenant_scope = MemoryTenantScope {
        org_id: row
            .get::<_, Option<String>>("tenant_org_id")
            .ok()
            .flatten()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| LOCAL_TENANT_ORG_ID.to_string()),
        workspace_id: row
            .get::<_, Option<String>>("tenant_workspace_id")
            .ok()
            .flatten()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| LOCAL_TENANT_WORKSPACE_ID.to_string()),
        deployment_id: row
            .get::<_, Option<String>>("tenant_deployment_id")
            .ok()
            .flatten()
            .filter(|value| !value.trim().is_empty()),
    };

    Ok(MemoryChunk {
        id,
        content,
        tier,
        session_id,
        project_id,
        source,
        source_path,
        source_mtime,
        source_size,
        source_hash,
        tenant_scope,
        subject,
        created_at,
        token_count,
        metadata,
    })
}

fn require_scope_id<'a>(tier: MemoryTier, scope: Option<&'a str>) -> MemoryResult<&'a str> {
    scope
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            crate::types::MemoryError::InvalidConfig(match tier {
                MemoryTier::Session => "tier=session requires session_id".to_string(),
                MemoryTier::Project => "tier=project requires project_id".to_string(),
                MemoryTier::Global => "tier=global does not require a scope id".to_string(),
            })
        })
}

const LOCAL_TENANT_ORG_ID: &str = "local";
const LOCAL_TENANT_WORKSPACE_ID: &str = "local";

fn tenant_scope_matches_sql_clause(prefix: &str, first_param: usize) -> String {
    format!(
        "{prefix}.tenant_org_id = ?{first_param} AND {prefix}.tenant_workspace_id = ?{} AND IFNULL({prefix}.tenant_deployment_id, '') = IFNULL(?{}, '')",
        first_param + 1,
        first_param + 2
    )
}

fn global_memory_record_tenant_scope(
    record: &GlobalMemoryRecord,
) -> (String, String, Option<String>) {
    record
        .provenance
        .as_ref()
        .and_then(|value| value.get("tenant_context"))
        .and_then(memory_tenant_scope_from_value)
        .unwrap_or_else(|| {
            (
                LOCAL_TENANT_ORG_ID.to_string(),
                LOCAL_TENANT_WORKSPACE_ID.to_string(),
                None,
            )
        })
}

fn memory_tenant_scope_from_value(
    value: &serde_json::Value,
) -> Option<(String, String, Option<String>)> {
    let org_id = value.get("org_id")?.as_str()?.to_string();
    let workspace_id = value.get("workspace_id")?.as_str()?.to_string();
    let deployment_id = value
        .get("deployment_id")
        .and_then(|value| value.as_str())
        .map(ToString::to_string);
    Some((org_id, workspace_id, deployment_id))
}

fn row_to_global_record(row: &Row) -> Result<GlobalMemoryRecord, rusqlite::Error> {
    let metadata_str: Option<String> = row.get(12)?;
    let provenance_str: Option<String> = row.get(13)?;
    Ok(GlobalMemoryRecord {
        id: row.get(0)?,
        user_id: row.get(1)?,
        source_type: row.get(2)?,
        content: row.get(3)?,
        content_hash: row.get(4)?,
        run_id: row.get(5)?,
        session_id: row.get(6)?,
        message_id: row.get(7)?,
        tool_name: row.get(8)?,
        project_tag: row.get(9)?,
        channel_tag: row.get(10)?,
        host_tag: row.get(11)?,
        metadata: metadata_str
            .filter(|s| !s.is_empty())
            .and_then(|s| serde_json::from_str(&s).ok()),
        provenance: provenance_str
            .filter(|s| !s.is_empty())
            .and_then(|s| serde_json::from_str(&s).ok()),
        redaction_status: row.get(14)?,
        redaction_count: row.get::<_, i64>(15)? as u32,
        visibility: row.get(16)?,
        demoted: row.get::<_, i64>(17)? != 0,
        score_boost: row.get(18)?,
        created_at_ms: row.get::<_, i64>(19)? as u64,
        updated_at_ms: row.get::<_, i64>(20)? as u64,
        expires_at_ms: row.get::<_, Option<i64>>(21)?.map(|v| v as u64),
    })
}

fn row_to_source_object_lifecycle(
    row: &Row,
) -> Result<SourceObjectLifecycleRecord, rusqlite::Error> {
    let metadata_str: Option<String> = row.get("metadata")?;
    let resource_ref_str: String = row.get("resource_ref")?;
    let tenant_scope = MemoryTenantScope {
        org_id: row.get("tenant_org_id")?,
        workspace_id: row.get("tenant_workspace_id")?,
        deployment_id: row
            .get::<_, Option<String>>("tenant_deployment_id")?
            .filter(|value| !value.is_empty()),
    };
    let tier = match row.get::<_, String>("tier")?.as_str() {
        "session" => MemoryTier::Session,
        "project" => MemoryTier::Project,
        _ => MemoryTier::Global,
    };
    Ok(SourceObjectLifecycleRecord {
        source_object_id: row.get("source_object_id")?,
        tenant_scope,
        source_binding_id: row.get("source_binding_id")?,
        connector_id: row.get("connector_id")?,
        state: SourceObjectLifecycleState::parse(&row.get::<_, String>("state")?),
        tier,
        session_id: row.get("session_id")?,
        project_id: row.get("project_id")?,
        import_namespace: row.get("import_namespace")?,
        indexed_path: row.get("indexed_path")?,
        native_object_id: row.get("native_object_id")?,
        resource_ref: serde_json::from_str(&resource_ref_str).unwrap_or(serde_json::Value::Null),
        data_class: row.get("data_class")?,
        content_hash: row.get("content_hash")?,
        source_hash: row.get("source_hash")?,
        first_seen_at_ms: row.get::<_, i64>("first_seen_at_ms")? as u64,
        last_seen_at_ms: row.get::<_, i64>("last_seen_at_ms")? as u64,
        tombstoned_at_ms: row
            .get::<_, Option<i64>>("tombstoned_at_ms")?
            .map(|value| value as u64),
        metadata: metadata_str
            .filter(|value| !value.is_empty())
            .and_then(|value| serde_json::from_str(&value).ok()),
    })
}

impl MemoryDatabase {
    pub async fn get_node_by_uri(
        &self,
        uri: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<crate::types::MemoryNode>> {
        let conn = self.conn.lock().await;
        let tenant_clause = tenant_scope_matches_sql_clause("memory_nodes", 2);
        let sql = format!(
            "SELECT id, uri, parent_uri, node_type, created_at, updated_at, metadata
             FROM memory_nodes WHERE uri = ?1 AND {tenant_clause}"
        );
        let mut stmt = conn.prepare(&sql)?;

        let result = stmt.query_row(
            params![
                uri,
                tenant_scope.org_id,
                tenant_scope.workspace_id,
                tenant_scope.deployment_id
            ],
            |row| {
                let node_type_str: String = row.get(3)?;
                let node_type = node_type_str
                    .parse()
                    .unwrap_or(crate::types::NodeType::File);
                let metadata_str: Option<String> = row.get(6)?;
                Ok(crate::types::MemoryNode {
                    id: row.get(0)?,
                    uri: row.get(1)?,
                    parent_uri: row.get(2)?,
                    node_type,
                    created_at: row.get::<_, String>(4)?.parse().unwrap_or_default(),
                    updated_at: row.get::<_, String>(5)?.parse().unwrap_or_default(),
                    metadata: metadata_str.and_then(|s| serde_json::from_str(&s).ok()),
                })
            },
        );

        match result {
            Ok(node) => Ok(Some(node)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(MemoryError::Database(e)),
        }
    }

    pub async fn create_node(
        &self,
        uri: &str,
        parent_uri: Option<&str>,
        node_type: crate::types::NodeType,
        metadata: Option<&serde_json::Value>,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let metadata_str = metadata.map(|m| serde_json::to_string(m)).transpose()?;

        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO memory_nodes (id, uri, parent_uri, node_type, created_at, updated_at, metadata,
                                       tenant_org_id, tenant_workspace_id, tenant_deployment_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id,
                uri,
                parent_uri,
                node_type.to_string(),
                now,
                now,
                metadata_str,
                tenant_scope.org_id,
                tenant_scope.workspace_id,
                tenant_scope.deployment_id
            ],
        )?;

        Ok(id)
    }

    pub async fn list_directory(
        &self,
        uri: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Vec<crate::types::MemoryNode>> {
        let conn = self.conn.lock().await;
        let tenant_clause = tenant_scope_matches_sql_clause("memory_nodes", 2);
        let sql = format!(
            "SELECT id, uri, parent_uri, node_type, created_at, updated_at, metadata
             FROM memory_nodes WHERE parent_uri = ?1 AND {tenant_clause}
             ORDER BY node_type DESC, uri ASC"
        );
        let mut stmt = conn.prepare(&sql)?;

        let rows = stmt.query_map(
            params![
                uri,
                tenant_scope.org_id,
                tenant_scope.workspace_id,
                tenant_scope.deployment_id
            ],
            |row| {
                let node_type_str: String = row.get(3)?;
                let node_type = node_type_str
                    .parse()
                    .unwrap_or(crate::types::NodeType::File);
                let metadata_str: Option<String> = row.get(6)?;
                Ok(crate::types::MemoryNode {
                    id: row.get(0)?,
                    uri: row.get(1)?,
                    parent_uri: row.get(2)?,
                    node_type,
                    created_at: row.get::<_, String>(4)?.parse().unwrap_or_default(),
                    updated_at: row.get::<_, String>(5)?.parse().unwrap_or_default(),
                    metadata: metadata_str.and_then(|s| serde_json::from_str(&s).ok()),
                })
            },
        )?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(MemoryError::Database)
    }

    pub async fn get_layer(
        &self,
        node_id: &str,
        layer_type: crate::types::LayerType,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<crate::types::MemoryLayer>> {
        let conn = self.conn.lock().await;
        // Layers carry no tenant columns of their own; ownership is derived from
        // the parent node, so a foreign node id behaves exactly like a missing one.
        let tenant_clause = tenant_scope_matches_sql_clause("memory_nodes", 3);
        let sql = format!(
            "SELECT memory_layers.id, memory_layers.node_id, memory_layers.layer_type,
                    memory_layers.content, memory_layers.token_count, memory_layers.embedding_id,
                    memory_layers.created_at, memory_layers.source_chunk_id
             FROM memory_layers
             JOIN memory_nodes ON memory_nodes.id = memory_layers.node_id
             WHERE memory_layers.node_id = ?1 AND memory_layers.layer_type = ?2 AND {tenant_clause}"
        );
        let mut stmt = conn.prepare(&sql)?;

        let result = stmt.query_row(
            params![
                node_id,
                layer_type.to_string(),
                tenant_scope.org_id,
                tenant_scope.workspace_id,
                tenant_scope.deployment_id
            ],
            |row| {
                let layer_type_str: String = row.get(2)?;
                let layer_type = layer_type_str
                    .parse()
                    .unwrap_or(crate::types::LayerType::L2);
                Ok(crate::types::MemoryLayer {
                    id: row.get(0)?,
                    node_id: row.get(1)?,
                    layer_type,
                    content: row.get(3)?,
                    token_count: row.get(4)?,
                    embedding_id: row.get(5)?,
                    created_at: row.get::<_, String>(6)?.parse().unwrap_or_default(),
                    source_chunk_id: row.get(7)?,
                })
            },
        );

        match result {
            Ok(mut layer) => {
                layer.content = self.crypto.decrypt_field(&layer.content)?;
                Ok(Some(layer))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(MemoryError::Database(e)),
        }
    }

    pub async fn create_layer(
        &self,
        node_id: &str,
        layer_type: crate::types::LayerType,
        content: &str,
        token_count: i64,
        source_chunk_id: Option<&str>,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let content_stored = self.crypto.encrypt_field(content)?;

        let conn = self.conn.lock().await;
        // A layer write against a node outside the tenant scope must fail exactly
        // like a write against a nonexistent node, so foreign node ids are not
        // distinguishable from unknown ones.
        let tenant_clause = tenant_scope_matches_sql_clause("memory_nodes", 2);
        let owned: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM memory_nodes WHERE id = ?1 AND {tenant_clause}"),
            params![
                node_id,
                tenant_scope.org_id,
                tenant_scope.workspace_id,
                tenant_scope.deployment_id
            ],
            |row| row.get(0),
        )?;
        if owned == 0 {
            return Err(MemoryError::NotFound(format!(
                "context node not found: {node_id}"
            )));
        }
        conn.execute(
            "INSERT INTO memory_layers (id, node_id, layer_type, content, token_count, created_at, source_chunk_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, node_id, layer_type.to_string(), content_stored, token_count, now, source_chunk_id],
        )?;

        Ok(id)
    }

    pub async fn get_children_tree(
        &self,
        parent_uri: &str,
        max_depth: usize,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Vec<crate::types::TreeNode>> {
        if max_depth == 0 {
            return Ok(Vec::new());
        }

        let children = self.list_directory(parent_uri, tenant_scope).await?;
        let mut tree_nodes = Vec::new();

        for child in children {
            let layer_summary = self.get_layer_summary(&child.id, tenant_scope).await?;

            let grandchildren = if child.node_type == crate::types::NodeType::Directory {
                Box::pin(self.get_children_tree(
                    &child.uri,
                    max_depth.saturating_sub(1),
                    tenant_scope,
                ))
                .await?
            } else {
                Vec::new()
            };

            tree_nodes.push(crate::types::TreeNode {
                node: child,
                children: grandchildren,
                layer_summary,
            });
        }

        Ok(tree_nodes)
    }

    async fn get_layer_summary(
        &self,
        node_id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<crate::types::LayerSummary>> {
        let l0 = self
            .get_layer(node_id, crate::types::LayerType::L0, tenant_scope)
            .await?;
        let l1 = self
            .get_layer(node_id, crate::types::LayerType::L1, tenant_scope)
            .await?;
        let has_l2 = self
            .get_layer(node_id, crate::types::LayerType::L2, tenant_scope)
            .await?
            .is_some();

        if l0.is_none() && l1.is_none() && !has_l2 {
            return Ok(None);
        }

        Ok(Some(crate::types::LayerSummary {
            l0_preview: l0.map(|l| truncate_string(&l.content, 100)),
            l1_preview: l1.map(|l| truncate_string(&l.content, 200)),
            has_l2,
        }))
    }

    pub async fn node_exists(
        &self,
        uri: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<bool> {
        let conn = self.conn.lock().await;
        let tenant_clause = tenant_scope_matches_sql_clause("memory_nodes", 2);
        let count: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM memory_nodes WHERE uri = ?1 AND {tenant_clause}"),
            params![
                uri,
                tenant_scope.org_id,
                tenant_scope.workspace_id,
                tenant_scope.deployment_id
            ],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }
}

fn row_to_knowledge_space(row: &Row) -> Result<KnowledgeSpaceRecord, rusqlite::Error> {
    let scope = row
        .get::<_, String>(1)?
        .parse()
        .unwrap_or(tandem_orchestrator::KnowledgeScope::Project);
    let trust_level = row
        .get::<_, String>(6)?
        .parse()
        .unwrap_or(tandem_orchestrator::KnowledgeTrustLevel::Promoted);
    let metadata = row
        .get::<_, Option<String>>(7)?
        .and_then(|raw| serde_json::from_str(&raw).ok());
    Ok(KnowledgeSpaceRecord {
        id: row.get(0)?,
        scope,
        project_id: row.get(2)?,
        namespace: row.get(3)?,
        title: row.get(4)?,
        description: row.get(5)?,
        trust_level,
        metadata,
        created_at_ms: row.get::<_, i64>(8)? as u64,
        updated_at_ms: row.get::<_, i64>(9)? as u64,
    })
}

fn row_to_knowledge_item(row: &Row) -> Result<KnowledgeItemRecord, rusqlite::Error> {
    let trust_level = row
        .get::<_, String>(8)?
        .parse()
        .unwrap_or(tandem_orchestrator::KnowledgeTrustLevel::Promoted);
    let status = row
        .get::<_, String>(9)?
        .parse()
        .unwrap_or(KnowledgeItemStatus::Working);
    let payload = row
        .get::<_, String>(7)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or(serde_json::Value::Null);
    let artifact_refs = row
        .get::<_, String>(11)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    let source_memory_ids = row
        .get::<_, String>(12)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    let metadata = row
        .get::<_, Option<String>>(14)?
        .and_then(|raw| serde_json::from_str(&raw).ok());
    Ok(KnowledgeItemRecord {
        id: row.get(0)?,
        space_id: row.get(1)?,
        coverage_key: row.get(2)?,
        dedupe_key: row.get(3)?,
        item_type: row.get(4)?,
        title: row.get(5)?,
        summary: row.get(6)?,
        payload,
        trust_level,
        status,
        run_id: row.get(10)?,
        artifact_refs,
        source_memory_ids,
        freshness_expires_at_ms: row.get::<_, Option<i64>>(13)?.map(|value| value as u64),
        metadata,
        created_at_ms: row.get::<_, i64>(15)? as u64,
        updated_at_ms: row.get::<_, i64>(16)? as u64,
    })
}

fn row_to_knowledge_coverage(row: &Row) -> Result<KnowledgeCoverageRecord, rusqlite::Error> {
    let metadata = row
        .get::<_, Option<String>>(7)?
        .and_then(|raw| serde_json::from_str(&raw).ok());
    Ok(KnowledgeCoverageRecord {
        coverage_key: row.get(0)?,
        space_id: row.get(1)?,
        latest_item_id: row.get(2)?,
        latest_dedupe_key: row.get(3)?,
        last_seen_at_ms: row.get::<_, i64>(4)? as u64,
        last_promoted_at_ms: row.get::<_, Option<i64>>(5)?.map(|value| value as u64),
        freshness_expires_at_ms: row.get::<_, Option<i64>>(6)?.map(|value| value as u64),
        metadata,
    })
}

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

fn build_fts_query(query: &str) -> String {
    let tokens = query
        .split_whitespace()
        .filter_map(|tok| {
            let cleaned =
                tok.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-');
            if cleaned.is_empty() {
                None
            } else {
                Some(format!("\"{}\"", cleaned))
            }
        })
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        "\"\"".to_string()
    } else {
        tokens.join(" OR ")
    }
}

include!("memory_database_impl_parts/db_tests.rs");
