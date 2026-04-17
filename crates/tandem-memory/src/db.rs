// Database Layer Module
// SQLite + sqlite-vec for vector storage

use crate::types::{
    ClearFileIndexResult, GlobalMemoryRecord, GlobalMemorySearchHit, GlobalMemoryWriteResult,
    KnowledgeCoverageRecord, KnowledgeItemRecord, KnowledgeItemStatus, KnowledgePromotionRequest,
    KnowledgePromotionResult, KnowledgeSpaceRecord, MemoryChunk, MemoryConfig, MemoryError,
    MemoryResult, MemoryStats, MemoryTier, ProjectMemoryStats, DEFAULT_EMBEDDING_DIMENSION,
};
use chrono::{DateTime, Utc};
use rusqlite::{ffi::sqlite3_auto_extension, params, Connection, OptionalExtension, Row};
use sqlite_vec::sqlite3_vec_init;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
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
}

include!("memory_database_impl_parts/part01.rs");
include!("memory_database_impl_parts/part02.rs");

/// Convert a database row to a MemoryChunk
fn row_to_chunk(row: &Row, tier: MemoryTier) -> Result<MemoryChunk, rusqlite::Error> {
    let id: String = row.get(0)?;
    let content: String = row.get(1)?;
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
    let metadata_str: Option<String> = row.get(metadata_idx)?;

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

impl MemoryDatabase {
    pub async fn get_node_by_uri(
        &self,
        uri: &str,
    ) -> MemoryResult<Option<crate::types::MemoryNode>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, uri, parent_uri, node_type, created_at, updated_at, metadata
             FROM memory_nodes WHERE uri = ?1",
        )?;

        let result = stmt.query_row(params![uri], |row| {
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
        });

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
    ) -> MemoryResult<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let metadata_str = metadata.map(|m| serde_json::to_string(m)).transpose()?;

        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO memory_nodes (id, uri, parent_uri, node_type, created_at, updated_at, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, uri, parent_uri, node_type.to_string(), now, now, metadata_str],
        )?;

        Ok(id)
    }

    pub async fn list_directory(&self, uri: &str) -> MemoryResult<Vec<crate::types::MemoryNode>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, uri, parent_uri, node_type, created_at, updated_at, metadata
             FROM memory_nodes WHERE parent_uri = ?1 ORDER BY node_type DESC, uri ASC",
        )?;

        let rows = stmt.query_map(params![uri], |row| {
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
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(MemoryError::Database)
    }

    pub async fn get_layer(
        &self,
        node_id: &str,
        layer_type: crate::types::LayerType,
    ) -> MemoryResult<Option<crate::types::MemoryLayer>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, node_id, layer_type, content, token_count, embedding_id, created_at, source_chunk_id
             FROM memory_layers WHERE node_id = ?1 AND layer_type = ?2"
        )?;

        let result = stmt.query_row(params![node_id, layer_type.to_string()], |row| {
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
        });

        match result {
            Ok(layer) => Ok(Some(layer)),
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
    ) -> MemoryResult<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO memory_layers (id, node_id, layer_type, content, token_count, created_at, source_chunk_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, node_id, layer_type.to_string(), content, token_count, now, source_chunk_id],
        )?;

        Ok(id)
    }

    pub async fn get_children_tree(
        &self,
        parent_uri: &str,
        max_depth: usize,
    ) -> MemoryResult<Vec<crate::types::TreeNode>> {
        if max_depth == 0 {
            return Ok(Vec::new());
        }

        let children = self.list_directory(parent_uri).await?;
        let mut tree_nodes = Vec::new();

        for child in children {
            let layer_summary = self.get_layer_summary(&child.id).await?;

            let grandchildren = if child.node_type == crate::types::NodeType::Directory {
                Box::pin(self.get_children_tree(&child.uri, max_depth.saturating_sub(1))).await?
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
    ) -> MemoryResult<Option<crate::types::LayerSummary>> {
        let l0 = self.get_layer(node_id, crate::types::LayerType::L0).await?;
        let l1 = self.get_layer(node_id, crate::types::LayerType::L1).await?;
        let has_l2 = self
            .get_layer(node_id, crate::types::LayerType::L2)
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

    pub async fn node_exists(&self, uri: &str) -> MemoryResult<bool> {
        let conn = self.conn.lock().await;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memory_nodes WHERE uri = ?1",
            params![uri],
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tandem_orchestrator::{KnowledgeScope, KnowledgeTrustLevel};
    use tempfile::TempDir;

    async fn setup_test_db() -> (MemoryDatabase, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_memory.db");
        let db = MemoryDatabase::new(&db_path).await.unwrap();
        (db, temp_dir)
    }

    #[tokio::test]
    async fn test_init_schema() {
        let (db, _temp) = setup_test_db().await;
        // If we get here, schema was initialized successfully
        let stats = db.get_stats().await.unwrap();
        assert_eq!(stats.total_chunks, 0);
    }

    #[tokio::test]
    async fn test_knowledge_registry_roundtrip() {
        let (db, _temp) = setup_test_db().await;

        let space = KnowledgeSpaceRecord {
            id: "space-1".to_string(),
            scope: tandem_orchestrator::KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("support".to_string()),
            title: Some("Support Knowledge".to_string()),
            description: Some("Reusable support guidance".to_string()),
            trust_level: tandem_orchestrator::KnowledgeTrustLevel::Promoted,
            metadata: Some(serde_json::json!({"owner": "ops"})),
            created_at_ms: 1,
            updated_at_ms: 2,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-1".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-1/support/debugging/slow-start".to_string(),
            dedupe_key: "dedupe-1".to_string(),
            item_type: "decision".to_string(),
            title: "Restart service before retry".to_string(),
            summary: Some("When the service is stale, restart before retrying.".to_string()),
            payload: serde_json::json!({"action": "restart"}),
            trust_level: tandem_orchestrator::KnowledgeTrustLevel::Promoted,
            status: KnowledgeItemStatus::Promoted,
            run_id: Some("run-1".to_string()),
            artifact_refs: vec!["artifact://run-1/report".to_string()],
            source_memory_ids: vec!["memory-1".to_string()],
            freshness_expires_at_ms: Some(10),
            metadata: Some(serde_json::json!({"source": "run"})),
            created_at_ms: 3,
            updated_at_ms: 4,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let coverage = KnowledgeCoverageRecord {
            coverage_key: item.coverage_key.clone(),
            space_id: space.id.clone(),
            latest_item_id: Some(item.id.clone()),
            latest_dedupe_key: Some(item.dedupe_key.clone()),
            last_seen_at_ms: 5,
            last_promoted_at_ms: Some(6),
            freshness_expires_at_ms: Some(10),
            metadata: Some(serde_json::json!({"coverage": true})),
        };
        db.upsert_knowledge_coverage(&coverage).await.unwrap();

        let loaded_space = db.get_knowledge_space(&space.id).await.unwrap().unwrap();
        assert_eq!(loaded_space.namespace.as_deref(), Some("support"));

        let loaded_items = db
            .list_knowledge_items(&space.id, Some(&item.coverage_key))
            .await
            .unwrap();
        assert_eq!(loaded_items.len(), 1);
        assert_eq!(loaded_items[0].title, item.title);

        let loaded_coverage = db
            .get_knowledge_coverage(&item.coverage_key, &space.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded_coverage.latest_item_id.as_deref(), Some("item-1"));
    }

    #[tokio::test]
    async fn test_store_and_retrieve_chunk() {
        let (db, _temp) = setup_test_db().await;

        let chunk = MemoryChunk {
            id: "test-1".to_string(),
            content: "Test content".to_string(),
            tier: MemoryTier::Session,
            session_id: Some("session-1".to_string()),
            project_id: Some("project-1".to_string()),
            source: "user_message".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            created_at: Utc::now(),
            token_count: 10,
            metadata: None,
        };

        let embedding = vec![0.1f32; DEFAULT_EMBEDDING_DIMENSION];
        db.store_chunk(&chunk, &embedding).await.unwrap();

        let chunks = db.get_session_chunks("session-1").await.unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "Test content");
    }

    #[tokio::test]
    async fn test_store_and_retrieve_global_chunk() {
        let (db, _temp) = setup_test_db().await;

        let chunk = MemoryChunk {
            id: "global-1".to_string(),
            content: "Global note".to_string(),
            tier: MemoryTier::Global,
            session_id: None,
            project_id: None,
            source: "agent_note".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            created_at: Utc::now(),
            token_count: 7,
            metadata: Some(serde_json::json!({"kind":"test"})),
        };

        let embedding = vec![0.2f32; DEFAULT_EMBEDDING_DIMENSION];
        db.store_chunk(&chunk, &embedding).await.unwrap();

        let chunks = db.get_global_chunks(10).await.unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "Global note");
        assert_eq!(chunks[0].source, "agent_note");
        assert_eq!(chunks[0].token_count, 7);
        assert_eq!(chunks[0].tier, MemoryTier::Global);
    }

    #[tokio::test]
    async fn test_global_chunk_exists_by_source_hash() {
        let (db, _temp) = setup_test_db().await;

        let chunk = MemoryChunk {
            id: "global-hash".to_string(),
            content: "Global hash note".to_string(),
            tier: MemoryTier::Global,
            session_id: None,
            project_id: None,
            source: "chat_exchange".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: Some("hash-123".to_string()),
            created_at: Utc::now(),
            token_count: 5,
            metadata: None,
        };

        let embedding = vec![0.3f32; DEFAULT_EMBEDDING_DIMENSION];
        db.store_chunk(&chunk, &embedding).await.unwrap();

        assert!(db
            .global_chunk_exists_by_source_hash("hash-123")
            .await
            .unwrap());
        assert!(!db
            .global_chunk_exists_by_source_hash("missing-hash")
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_config_crud() {
        let (db, _temp) = setup_test_db().await;

        let config = db.get_or_create_config("project-1").await.unwrap();
        assert_eq!(config.max_chunks, 10000);

        let new_config = MemoryConfig {
            max_chunks: 5000,
            ..Default::default()
        };
        db.update_config("project-1", &new_config).await.unwrap();

        let updated = db.get_or_create_config("project-1").await.unwrap();
        assert_eq!(updated.max_chunks, 5000);
    }

    #[tokio::test]
    async fn test_global_memory_put_search_and_dedup() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let record = GlobalMemoryRecord {
            id: "gm-1".to_string(),
            user_id: "user-a".to_string(),
            source_type: "user_message".to_string(),
            content: "remember rust workspace layout".to_string(),
            content_hash: "h1".to_string(),
            run_id: "run-1".to_string(),
            session_id: Some("s1".to_string()),
            message_id: Some("m1".to_string()),
            tool_name: None,
            project_tag: Some("proj-x".to_string()),
            channel_tag: None,
            host_tag: None,
            metadata: None,
            provenance: None,
            redaction_status: "passed".to_string(),
            redaction_count: 0,
            visibility: "private".to_string(),
            demoted: false,
            score_boost: 0.0,
            created_at_ms: now,
            updated_at_ms: now,
            expires_at_ms: None,
        };
        let first = db.put_global_memory_record(&record).await.unwrap();
        assert!(first.stored);
        let second = db.put_global_memory_record(&record).await.unwrap();
        assert!(second.deduped);

        let hits = db
            .search_global_memory("user-a", "rust workspace", 5, Some("proj-x"), None, None)
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].record.id, "gm-1");
    }

    #[tokio::test]
    async fn test_knowledge_registry_round_trip() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-1".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("marketing/positioning".to_string()),
            title: Some("Marketing positioning".to_string()),
            description: Some("Reusable positioning guidance".to_string()),
            trust_level: KnowledgeTrustLevel::ApprovedDefault,
            metadata: Some(serde_json::json!({"owner":"marketing"})),
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let loaded_space = db.get_knowledge_space("space-1").await.unwrap().unwrap();
        assert_eq!(loaded_space.id, "space-1");
        assert_eq!(loaded_space.scope, KnowledgeScope::Project);
        assert_eq!(loaded_space.project_id.as_deref(), Some("project-1"));
        assert_eq!(
            loaded_space.namespace.as_deref(),
            Some("marketing/positioning")
        );

        let item = KnowledgeItemRecord {
            id: "item-1".to_string(),
            space_id: "space-1".to_string(),
            coverage_key: "project-1::marketing/positioning::strategy::pricing".to_string(),
            dedupe_key: "item-1-dedupe".to_string(),
            item_type: "evidence".to_string(),
            title: "Pricing sensitivity observation".to_string(),
            summary: Some("Customers reacted to annual pricing changes".to_string()),
            payload: serde_json::json!({"claim":"Annual pricing changes created friction"}),
            trust_level: KnowledgeTrustLevel::Promoted,
            status: KnowledgeItemStatus::Promoted,
            run_id: Some("run-1".to_string()),
            artifact_refs: vec!["artifact://run-1/research-sources".to_string()],
            source_memory_ids: vec!["memory-1".to_string()],
            freshness_expires_at_ms: Some(now + 86_400_000),
            metadata: Some(serde_json::json!({"source_kind":"web"})),
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let loaded_item = db.get_knowledge_item("item-1").await.unwrap().unwrap();
        assert_eq!(loaded_item.id, "item-1");
        assert_eq!(loaded_item.space_id, "space-1");
        assert_eq!(
            loaded_item.coverage_key,
            "project-1::marketing/positioning::strategy::pricing"
        );
        assert_eq!(loaded_item.status, KnowledgeItemStatus::Promoted);
        assert_eq!(
            loaded_item.artifact_refs,
            vec!["artifact://run-1/research-sources".to_string()]
        );

        let by_space = db.list_knowledge_items("space-1", None).await.unwrap();
        assert_eq!(by_space.len(), 1);
        let by_coverage = db
            .list_knowledge_items(
                "space-1",
                Some("project-1::marketing/positioning::strategy::pricing"),
            )
            .await
            .unwrap();
        assert_eq!(by_coverage.len(), 1);

        let coverage = KnowledgeCoverageRecord {
            coverage_key: "project-1::marketing/positioning::strategy::pricing".to_string(),
            space_id: "space-1".to_string(),
            latest_item_id: Some("item-1".to_string()),
            latest_dedupe_key: Some("item-1-dedupe".to_string()),
            last_seen_at_ms: now,
            last_promoted_at_ms: Some(now),
            freshness_expires_at_ms: Some(now + 86_400_000),
            metadata: Some(serde_json::json!({"reuse_reason":"same topic"})),
        };
        db.upsert_knowledge_coverage(&coverage).await.unwrap();

        let loaded_coverage = db
            .get_knowledge_coverage(
                "project-1::marketing/positioning::strategy::pricing",
                "space-1",
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded_coverage.space_id, "space-1");
        assert_eq!(loaded_coverage.latest_item_id.as_deref(), Some("item-1"));
        assert_eq!(
            loaded_coverage.latest_dedupe_key.as_deref(),
            Some("item-1-dedupe")
        );
    }

    #[tokio::test]
    async fn test_knowledge_promotion_working_to_promoted_updates_coverage() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-promote-1".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("engineering/debugging".to_string()),
            title: Some("Engineering debugging".to_string()),
            description: Some("Reusable debugging guidance".to_string()),
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-promote-1".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-1::engineering/debugging::startup::race".to_string(),
            dedupe_key: "dedupe-promote-1".to_string(),
            item_type: "decision".to_string(),
            title: "Delay startup-dependent retries".to_string(),
            summary: Some("Retry only after startup completed.".to_string()),
            payload: serde_json::json!({"action":"delay_retry"}),
            trust_level: KnowledgeTrustLevel::Working,
            status: KnowledgeItemStatus::Working,
            run_id: Some("run-1".to_string()),
            artifact_refs: vec!["artifact://run-1/debug".to_string()],
            source_memory_ids: vec!["memory-1".to_string()],
            freshness_expires_at_ms: None,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let promote = KnowledgePromotionRequest {
            item_id: item.id.clone(),
            target_status: KnowledgeItemStatus::Promoted,
            promoted_at_ms: now + 10,
            freshness_expires_at_ms: Some(now + 86_400_000),
            reviewer_id: None,
            approval_id: None,
            reason: Some("validated in workflow".to_string()),
        };

        let result = db.promote_knowledge_item(&promote).await.unwrap().unwrap();
        assert!(result.promoted);
        assert_eq!(result.item.status, KnowledgeItemStatus::Promoted);
        assert_eq!(result.item.trust_level, KnowledgeTrustLevel::Promoted);
        assert_eq!(
            result.coverage.latest_item_id.as_deref(),
            Some("item-promote-1")
        );
        assert_eq!(
            result.coverage.latest_dedupe_key.as_deref(),
            Some("dedupe-promote-1")
        );
        assert_eq!(result.coverage.last_promoted_at_ms, Some(now + 10));
    }

    #[tokio::test]
    async fn test_knowledge_promotion_promoted_to_approved_default_requires_review() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-promote-2".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("marketing/positioning".to_string()),
            title: Some("Marketing positioning".to_string()),
            description: Some("Reusable positioning guidance".to_string()),
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-promote-2".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-1::marketing/positioning::strategy::pricing".to_string(),
            dedupe_key: "dedupe-promote-2".to_string(),
            item_type: "evidence".to_string(),
            title: "Pricing observation".to_string(),
            summary: Some("Annual pricing changes created friction".to_string()),
            payload: serde_json::json!({"claim":"pricing friction"}),
            trust_level: KnowledgeTrustLevel::Promoted,
            status: KnowledgeItemStatus::Promoted,
            run_id: Some("run-2".to_string()),
            artifact_refs: vec!["artifact://run-2/research".to_string()],
            source_memory_ids: vec!["memory-2".to_string()],
            freshness_expires_at_ms: None,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let promote = KnowledgePromotionRequest {
            item_id: item.id.clone(),
            target_status: KnowledgeItemStatus::ApprovedDefault,
            promoted_at_ms: now + 5,
            freshness_expires_at_ms: None,
            reviewer_id: None,
            approval_id: None,
            reason: Some("should require review".to_string()),
        };

        let err = db.promote_knowledge_item(&promote).await.unwrap_err();
        match err {
            MemoryError::InvalidConfig(_) => {}
            other => panic!("unexpected error: {}", other),
        }
    }

    #[tokio::test]
    async fn test_knowledge_promotion_promoted_to_approved_default_updates_coverage() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-promote-3".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("support/runbooks".to_string()),
            title: Some("Support runbooks".to_string()),
            description: Some("Reusable runbook guidance".to_string()),
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-promote-3".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-1::support/runbooks::oncall::restart".to_string(),
            dedupe_key: "dedupe-promote-3".to_string(),
            item_type: "runbook".to_string(),
            title: "Restart service and verify".to_string(),
            summary: Some("Restart then validate health endpoint.".to_string()),
            payload: serde_json::json!({"steps":["restart","healthcheck"]}),
            trust_level: KnowledgeTrustLevel::Promoted,
            status: KnowledgeItemStatus::Promoted,
            run_id: Some("run-3".to_string()),
            artifact_refs: vec!["artifact://run-3/runbook".to_string()],
            source_memory_ids: vec!["memory-3".to_string()],
            freshness_expires_at_ms: None,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let promote = KnowledgePromotionRequest {
            item_id: item.id.clone(),
            target_status: KnowledgeItemStatus::ApprovedDefault,
            promoted_at_ms: now + 12,
            freshness_expires_at_ms: Some(now + 172_800_000),
            reviewer_id: Some("reviewer-1".to_string()),
            approval_id: Some("approval-1".to_string()),
            reason: Some("reviewed by ops".to_string()),
        };

        let result = db.promote_knowledge_item(&promote).await.unwrap().unwrap();
        assert!(result.promoted);
        assert_eq!(result.item.status, KnowledgeItemStatus::ApprovedDefault);
        assert_eq!(
            result.item.trust_level,
            KnowledgeTrustLevel::ApprovedDefault
        );
        assert_eq!(result.coverage.last_promoted_at_ms, Some(now + 12));
        assert_eq!(
            result.coverage.latest_item_id.as_deref(),
            Some("item-promote-3")
        );
    }

    #[tokio::test]
    async fn test_knowledge_promotion_rejects_deprecated() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-promote-4".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("ops".to_string()),
            title: Some("Ops knowledge".to_string()),
            description: Some("Reusable ops knowledge".to_string()),
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-promote-4".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-1::ops::incident::latency".to_string(),
            dedupe_key: "dedupe-promote-4".to_string(),
            item_type: "decision".to_string(),
            title: "Ignore deprecated item".to_string(),
            summary: None,
            payload: serde_json::json!({"decision":"deprecated"}),
            trust_level: KnowledgeTrustLevel::Promoted,
            status: KnowledgeItemStatus::Deprecated,
            run_id: Some("run-4".to_string()),
            artifact_refs: vec![],
            source_memory_ids: vec![],
            freshness_expires_at_ms: None,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let promote = KnowledgePromotionRequest {
            item_id: item.id.clone(),
            target_status: KnowledgeItemStatus::Promoted,
            promoted_at_ms: now + 1,
            freshness_expires_at_ms: None,
            reviewer_id: None,
            approval_id: None,
            reason: None,
        };

        let err = db.promote_knowledge_item(&promote).await.unwrap_err();
        match err {
            MemoryError::InvalidConfig(_) => {}
            other => panic!("unexpected error: {}", other),
        }
    }

    #[tokio::test]
    async fn test_knowledge_promotion_idempotent_updates_coverage() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-promote-5".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("engineering/ops".to_string()),
            title: Some("Engineering ops".to_string()),
            description: None,
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-promote-5".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-1::engineering/ops::deploy::guardrails".to_string(),
            dedupe_key: "dedupe-promote-5".to_string(),
            item_type: "pattern".to_string(),
            title: "Deploy guardrails".to_string(),
            summary: None,
            payload: serde_json::json!({"pattern":"guardrails"}),
            trust_level: KnowledgeTrustLevel::Promoted,
            status: KnowledgeItemStatus::Promoted,
            run_id: Some("run-5".to_string()),
            artifact_refs: vec![],
            source_memory_ids: vec![],
            freshness_expires_at_ms: None,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let promote = KnowledgePromotionRequest {
            item_id: item.id.clone(),
            target_status: KnowledgeItemStatus::Promoted,
            promoted_at_ms: now + 20,
            freshness_expires_at_ms: None,
            reviewer_id: None,
            approval_id: None,
            reason: None,
        };

        let result = db.promote_knowledge_item(&promote).await.unwrap().unwrap();
        assert!(!result.promoted);
        assert_eq!(result.coverage.last_promoted_at_ms, Some(now + 20));
        assert_eq!(
            result.coverage.latest_item_id.as_deref(),
            Some("item-promote-5")
        );
    }

    #[tokio::test]
    async fn test_knowledge_item_promotion_updates_coverage() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-promote".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("engineering/debugging".to_string()),
            title: Some("Engineering debugging".to_string()),
            description: Some("Reusable debugging guidance".to_string()),
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-promote".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-1::engineering/debugging::startup::race".to_string(),
            dedupe_key: "dedupe-promote".to_string(),
            item_type: "decision".to_string(),
            title: "Delay startup-dependent retries".to_string(),
            summary: Some("Retry only after startup completes.".to_string()),
            payload: serde_json::json!({"action": "delay_retry"}),
            trust_level: KnowledgeTrustLevel::Working,
            status: KnowledgeItemStatus::Working,
            run_id: Some("run-promote".to_string()),
            artifact_refs: vec!["artifact://run-promote/report".to_string()],
            source_memory_ids: vec!["memory-promote".to_string()],
            freshness_expires_at_ms: None,
            metadata: Some(serde_json::json!({"source_kind":"run"})),
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let request = KnowledgePromotionRequest {
            item_id: item.id.clone(),
            target_status: KnowledgeItemStatus::Promoted,
            promoted_at_ms: now + 10,
            freshness_expires_at_ms: Some(now + 86_400_000),
            reviewer_id: None,
            approval_id: None,
            reason: Some("validated".to_string()),
        };
        let promoted = db
            .promote_knowledge_item(&request)
            .await
            .unwrap()
            .expect("promotion result");
        assert_eq!(promoted.previous_status, KnowledgeItemStatus::Working);
        assert!(promoted.promoted);
        assert_eq!(promoted.item.status, KnowledgeItemStatus::Promoted);
        assert_eq!(promoted.item.trust_level, KnowledgeTrustLevel::Promoted);
        assert_eq!(
            promoted.item.freshness_expires_at_ms,
            Some(now + 86_400_000)
        );
        assert_eq!(
            promoted
                .item
                .metadata
                .as_ref()
                .and_then(|value| value.get("promotion"))
                .and_then(|value| value.get("to_status"))
                .and_then(Value::as_str),
            Some("promoted")
        );
        assert_eq!(
            promoted.coverage.latest_item_id.as_deref(),
            Some("item-promote")
        );
        assert_eq!(
            promoted.coverage.latest_dedupe_key.as_deref(),
            Some("dedupe-promote")
        );
        assert_eq!(promoted.coverage.last_promoted_at_ms, Some(now + 10));
        assert_eq!(
            promoted.coverage.freshness_expires_at_ms,
            Some(now + 86_400_000)
        );

        let loaded = db
            .get_knowledge_item("item-promote")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.status, KnowledgeItemStatus::Promoted);
        assert_eq!(
            loaded
                .metadata
                .as_ref()
                .and_then(|value| value.get("promotion"))
                .and_then(|value| value.get("from_status"))
                .and_then(Value::as_str),
            Some("working")
        );
    }

    #[tokio::test]
    async fn test_knowledge_item_approved_default_requires_review() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-approved".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("marketing/positioning".to_string()),
            title: Some("Marketing positioning".to_string()),
            description: Some("Reusable positioning guidance".to_string()),
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-approved".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-1::marketing/positioning::strategy::pricing".to_string(),
            dedupe_key: "dedupe-approved".to_string(),
            item_type: "evidence".to_string(),
            title: "Pricing sensitivity observation".to_string(),
            summary: Some("Customers reacted to annual pricing changes".to_string()),
            payload: serde_json::json!({"claim":"Annual pricing changes created friction"}),
            trust_level: KnowledgeTrustLevel::Promoted,
            status: KnowledgeItemStatus::Promoted,
            run_id: Some("run-approved".to_string()),
            artifact_refs: vec!["artifact://run-approved/research".to_string()],
            source_memory_ids: vec!["memory-approved".to_string()],
            freshness_expires_at_ms: Some(now + 1234),
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let request = KnowledgePromotionRequest {
            item_id: item.id.clone(),
            target_status: KnowledgeItemStatus::ApprovedDefault,
            promoted_at_ms: now + 20,
            freshness_expires_at_ms: Some(now + 90_000_000),
            reviewer_id: Some("reviewer-1".to_string()),
            approval_id: Some("approval-1".to_string()),
            reason: Some("approved as default guidance".to_string()),
        };
        let promoted = db
            .promote_knowledge_item(&request)
            .await
            .unwrap()
            .expect("promotion result");
        assert_eq!(promoted.previous_status, KnowledgeItemStatus::Promoted);
        assert_eq!(promoted.item.status, KnowledgeItemStatus::ApprovedDefault);
        assert_eq!(
            promoted.item.trust_level,
            KnowledgeTrustLevel::ApprovedDefault
        );
        assert_eq!(promoted.coverage.last_promoted_at_ms, Some(now + 20));
        assert_eq!(
            promoted
                .item
                .metadata
                .as_ref()
                .and_then(|value| value.get("promotion"))
                .and_then(|value| value.get("approval_id"))
                .and_then(Value::as_str),
            Some("approval-1")
        );
    }

    #[tokio::test]
    async fn test_knowledge_item_promotion_rejects_invalid_transition() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-invalid".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("support".to_string()),
            title: Some("Support".to_string()),
            description: Some("Support guidance".to_string()),
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-invalid".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-1::support::workflow::triage".to_string(),
            dedupe_key: "dedupe-invalid".to_string(),
            item_type: "decision".to_string(),
            title: "Triage first".to_string(),
            summary: None,
            payload: serde_json::json!({"action":"triage"}),
            trust_level: KnowledgeTrustLevel::Working,
            status: KnowledgeItemStatus::Working,
            run_id: Some("run-invalid".to_string()),
            artifact_refs: vec![],
            source_memory_ids: vec![],
            freshness_expires_at_ms: None,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let request = KnowledgePromotionRequest {
            item_id: item.id.clone(),
            target_status: KnowledgeItemStatus::ApprovedDefault,
            promoted_at_ms: now + 1,
            freshness_expires_at_ms: None,
            reviewer_id: Some("reviewer-1".to_string()),
            approval_id: Some("approval-1".to_string()),
            reason: Some("should fail".to_string()),
        };
        let err = db.promote_knowledge_item(&request).await.unwrap_err();
        assert!(matches!(err, MemoryError::InvalidConfig(_)));
        let loaded = db.get_knowledge_item(&item.id).await.unwrap().unwrap();
        assert_eq!(loaded.status, KnowledgeItemStatus::Working);
    }
}
