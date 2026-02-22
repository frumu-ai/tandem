//! LLM Response Cache — avoid burning tokens on repeated prompts.
//!
//! Stores LLM responses in a separate SQLite table keyed by a SHA-256 hash of
//! `(model, system_prompt_hash, user_prompt)`. Entries expire after a
//! configurable TTL. The cache is optional and disabled by default — users
//! opt in via `TANDEM_RESPONSE_CACHE_ENABLED=true`.
//!
//! Lives alongside `memory.sqlite` as `response_cache.db` so it can be
//! independently wiped without touching memory chunks.

use chrono::{Duration, Utc};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::types::{MemoryError, MemoryResult};

/// Response cache backed by a dedicated SQLite database.
pub struct ResponseCache {
    conn: Arc<Mutex<Connection>>,
    #[allow(dead_code)]
    db_path: PathBuf,
    ttl_minutes: i64,
    max_entries: usize,
}

impl ResponseCache {
    /// Open (or create) the response cache database at `{db_dir}/response_cache.db`.
    pub async fn new(db_dir: &Path, ttl_minutes: u32, max_entries: usize) -> MemoryResult<Self> {
        tokio::fs::create_dir_all(db_dir)
            .await
            .map_err(MemoryError::Io)?;

        let db_path = db_dir.join("response_cache.db");

        let conn = Connection::open(&db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous  = NORMAL;
             PRAGMA temp_store   = MEMORY;",
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS response_cache (
                prompt_hash  TEXT PRIMARY KEY,
                model        TEXT NOT NULL,
                response     TEXT NOT NULL,
                token_count  INTEGER NOT NULL DEFAULT 0,
                created_at   TEXT NOT NULL,
                accessed_at  TEXT NOT NULL,
                hit_count    INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_rc_accessed ON response_cache(accessed_at);
            CREATE INDEX IF NOT EXISTS idx_rc_created  ON response_cache(created_at);",
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path,
            ttl_minutes: i64::from(ttl_minutes),
            max_entries,
        })
    }

    /// Build a deterministic cache key from model + system prompt + user prompt.
    pub fn cache_key(model: &str, system_prompt: Option<&str>, user_prompt: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(model.as_bytes());
        hasher.update(b"|");
        if let Some(sys) = system_prompt {
            hasher.update(sys.as_bytes());
        }
        hasher.update(b"|");
        hasher.update(user_prompt.as_bytes());
        format!("{:064x}", hasher.finalize())
    }

    /// Look up a cached response. Returns `None` on miss or if the entry has expired.
    pub async fn get(&self, key: &str) -> MemoryResult<Option<String>> {
        let conn = self.conn.lock().await;
        let cutoff = (Utc::now() - Duration::minutes(self.ttl_minutes)).to_rfc3339();

        let result: Option<String> = conn
            .query_row(
                "SELECT response FROM response_cache
                 WHERE prompt_hash = ?1 AND created_at > ?2",
                params![key, cutoff],
                |row| row.get(0),
            )
            .ok();

        if result.is_some() {
            let now = Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE response_cache
                 SET accessed_at = ?1, hit_count = hit_count + 1
                 WHERE prompt_hash = ?2",
                params![now, key],
            )?;
        }

        Ok(result)
    }

    /// Store a response in the cache, evicting expired or least-recently-used entries.
    pub async fn put(
        &self,
        key: &str,
        model: &str,
        response: &str,
        token_count: u32,
    ) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT OR REPLACE INTO response_cache
             (prompt_hash, model, response, token_count, created_at, accessed_at, hit_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
            params![key, model, response, token_count, now, now],
        )?;

        // Evict expired entries
        let cutoff = (Utc::now() - Duration::minutes(self.ttl_minutes)).to_rfc3339();
        conn.execute(
            "DELETE FROM response_cache WHERE created_at <= ?1",
            params![cutoff],
        )?;

        // LRU eviction if over max_entries
        #[allow(clippy::cast_possible_wrap)]
        let max = self.max_entries as i64;
        conn.execute(
            "DELETE FROM response_cache WHERE prompt_hash IN (
                SELECT prompt_hash FROM response_cache
                ORDER BY accessed_at ASC
                LIMIT MAX(0, (SELECT COUNT(*) FROM response_cache) - ?1)
            )",
            params![max],
        )?;

        Ok(())
    }

    /// Return cache statistics: `(total_entries, total_hits, estimated_tokens_saved)`.
    pub async fn stats(&self) -> MemoryResult<(usize, u64, u64)> {
        let conn = self.conn.lock().await;

        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM response_cache", [], |row| row.get(0))?;

        let hits: i64 = conn.query_row(
            "SELECT COALESCE(SUM(hit_count), 0) FROM response_cache",
            [],
            |row| row.get(0),
        )?;

        let tokens_saved: i64 = conn.query_row(
            "SELECT COALESCE(SUM(token_count * hit_count), 0) FROM response_cache",
            [],
            |row| row.get(0),
        )?;

        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        Ok((count as usize, hits as u64, tokens_saved as u64))
    }

    /// Clear all cached entries.
    pub async fn clear(&self) -> MemoryResult<usize> {
        let conn = self.conn.lock().await;
        let affected = conn.execute("DELETE FROM response_cache", [])?;
        Ok(affected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn temp_cache(ttl_minutes: u32) -> (TempDir, ResponseCache) {
        let tmp = TempDir::new().unwrap();
        let cache = ResponseCache::new(tmp.path(), ttl_minutes, 1000)
            .await
            .unwrap();
        (tmp, cache)
    }

    #[tokio::test]
    async fn cache_key_is_deterministic() {
        let k1 = ResponseCache::cache_key("gpt-4", Some("sys"), "hello");
        let k2 = ResponseCache::cache_key("gpt-4", Some("sys"), "hello");
        assert_eq!(k1, k2);
        assert_eq!(k1.len(), 64);
    }

    #[tokio::test]
    async fn cache_key_varies_by_model() {
        let k1 = ResponseCache::cache_key("gpt-4", None, "hello");
        let k2 = ResponseCache::cache_key("claude-3", None, "hello");
        assert_ne!(k1, k2);
    }

    #[tokio::test]
    async fn put_and_get_roundtrip() {
        let (_tmp, cache) = temp_cache(60).await;
        let key = ResponseCache::cache_key("gpt-4", None, "What is Rust?");
        cache
            .put(&key, "gpt-4", "Rust is a systems programming language.", 25)
            .await
            .unwrap();
        let result = cache.get(&key).await.unwrap();
        assert_eq!(
            result.as_deref(),
            Some("Rust is a systems programming language.")
        );
    }

    #[tokio::test]
    async fn miss_returns_none() {
        let (_tmp, cache) = temp_cache(60).await;
        let result = cache.get("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn expired_entry_returns_none() {
        let (_tmp, cache) = temp_cache(0).await; // 0 TTL → instantly expired
        let key = ResponseCache::cache_key("gpt-4", None, "test");
        cache.put(&key, "gpt-4", "response", 10).await.unwrap();
        let result = cache.get(&key).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn stats_tracks_hits_and_tokens() {
        let (_tmp, cache) = temp_cache(60).await;
        let key = ResponseCache::cache_key("gpt-4", None, "explain rust");
        cache.put(&key, "gpt-4", "Rust is...", 100).await.unwrap();
        for _ in 0..5 {
            let _ = cache.get(&key).await.unwrap();
        }
        let (_, hits, tokens) = cache.stats().await.unwrap();
        assert_eq!(hits, 5);
        assert_eq!(tokens, 500);
    }

    #[tokio::test]
    async fn lru_eviction_respects_max_entries() {
        let tmp = TempDir::new().unwrap();
        let cache = ResponseCache::new(tmp.path(), 60, 3).await.unwrap();
        for i in 0..5 {
            let key = ResponseCache::cache_key("gpt-4", None, &format!("prompt {i}"));
            cache
                .put(&key, "gpt-4", &format!("response {i}"), 10)
                .await
                .unwrap();
        }
        let (count, _, _) = cache.stats().await.unwrap();
        assert!(count <= 3, "cache must not exceed max_entries");
    }
}
