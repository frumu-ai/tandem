impl MemoryDatabase {
    pub async fn upsert_file_index_entry(
        &self,
        project_id: &str,
        path: &str,
        mtime: i64,
        size: i64,
        hash: &str,
    ) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        let indexed_at = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO project_file_index (project_id, path, mtime, size, hash, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(project_id, path) DO UPDATE SET
                mtime = excluded.mtime,
                size = excluded.size,
                hash = excluded.hash,
                indexed_at = excluded.indexed_at",
            params![project_id, path, mtime, size, hash, indexed_at],
        )?;
        Ok(())
    }

    pub async fn delete_file_index_entry(&self, project_id: &str, path: &str) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "DELETE FROM project_file_index WHERE project_id = ?1 AND path = ?2",
            params![project_id, path],
        )?;
        Ok(())
    }

    pub async fn list_file_index_paths(&self, project_id: &str) -> MemoryResult<Vec<String>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare("SELECT path FROM project_file_index WHERE project_id = ?1")?;
        let rows = stmt.query_map(params![project_id], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub async fn delete_project_file_chunks_by_path(
        &self,
        project_id: &str,
        source_path: &str,
    ) -> MemoryResult<(i64, i64)> {
        let conn = self.conn.lock().await;

        let chunks_deleted: i64 = conn.query_row(
            "SELECT COUNT(*) FROM project_memory_chunks
             WHERE project_id = ?1 AND source = 'file' AND source_path = ?2",
            params![project_id, source_path],
            |row| row.get(0),
        )?;

        let bytes_estimated: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM project_memory_chunks
             WHERE project_id = ?1 AND source = 'file' AND source_path = ?2",
            params![project_id, source_path],
            |row| row.get(0),
        )?;

        // Delete vectors first (keep order consistent with other clears)
        conn.execute(
            "DELETE FROM project_memory_vectors WHERE chunk_id IN
             (SELECT id FROM project_memory_chunks WHERE project_id = ?1 AND source = 'file' AND source_path = ?2)",
            params![project_id, source_path],
        )?;

        conn.execute(
            "DELETE FROM project_memory_chunks WHERE project_id = ?1 AND source = 'file' AND source_path = ?2",
            params![project_id, source_path],
        )?;

        Ok((chunks_deleted, bytes_estimated))
    }

    pub async fn get_import_index_entry(
        &self,
        tier: MemoryTier,
        session_id: Option<&str>,
        project_id: Option<&str>,
        path: &str,
    ) -> MemoryResult<Option<(i64, i64, String)>> {
        let conn = self.conn.lock().await;
        let row = match tier {
            MemoryTier::Session => {
                let session_id = require_scope_id(tier, session_id)?;
                conn.query_row(
                    "SELECT mtime, size, hash FROM session_file_index WHERE session_id = ?1 AND path = ?2",
                    params![session_id, path],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?
            }
            MemoryTier::Project => {
                let project_id = require_scope_id(tier, project_id)?;
                conn.query_row(
                    "SELECT mtime, size, hash FROM project_file_index WHERE project_id = ?1 AND path = ?2",
                    params![project_id, path],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?
            }
            MemoryTier::Global => conn
                .query_row(
                    "SELECT mtime, size, hash FROM global_file_index WHERE path = ?1",
                    params![path],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?,
        };
        Ok(row)
    }

    pub async fn upsert_import_index_entry(
        &self,
        tier: MemoryTier,
        session_id: Option<&str>,
        project_id: Option<&str>,
        path: &str,
        mtime: i64,
        size: i64,
        hash: &str,
    ) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        let indexed_at = Utc::now().to_rfc3339();
        match tier {
            MemoryTier::Session => {
                let session_id = require_scope_id(tier, session_id)?;
                conn.execute(
                    "INSERT INTO session_file_index (session_id, path, mtime, size, hash, indexed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(session_id, path) DO UPDATE SET
                        mtime = excluded.mtime,
                        size = excluded.size,
                        hash = excluded.hash,
                        indexed_at = excluded.indexed_at",
                    params![session_id, path, mtime, size, hash, indexed_at],
                )?;
            }
            MemoryTier::Project => {
                let project_id = require_scope_id(tier, project_id)?;
                conn.execute(
                    "INSERT INTO project_file_index (project_id, path, mtime, size, hash, indexed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(project_id, path) DO UPDATE SET
                        mtime = excluded.mtime,
                        size = excluded.size,
                        hash = excluded.hash,
                        indexed_at = excluded.indexed_at",
                    params![project_id, path, mtime, size, hash, indexed_at],
                )?;
            }
            MemoryTier::Global => {
                conn.execute(
                    "INSERT INTO global_file_index (path, mtime, size, hash, indexed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(path) DO UPDATE SET
                        mtime = excluded.mtime,
                        size = excluded.size,
                        hash = excluded.hash,
                        indexed_at = excluded.indexed_at",
                    params![path, mtime, size, hash, indexed_at],
                )?;
            }
        }
        Ok(())
    }

    pub async fn list_import_index_paths(
        &self,
        tier: MemoryTier,
        session_id: Option<&str>,
        project_id: Option<&str>,
    ) -> MemoryResult<Vec<String>> {
        let conn = self.conn.lock().await;
        let rows = match tier {
            MemoryTier::Session => {
                let session_id = require_scope_id(tier, session_id)?;
                let mut stmt =
                    conn.prepare("SELECT path FROM session_file_index WHERE session_id = ?1")?;
                let rows = stmt.query_map(params![session_id], |row| row.get::<_, String>(0))?;
                rows.collect::<Result<Vec<_>, _>>()?
            }
            MemoryTier::Project => {
                let project_id = require_scope_id(tier, project_id)?;
                let mut stmt =
                    conn.prepare("SELECT path FROM project_file_index WHERE project_id = ?1")?;
                let rows = stmt.query_map(params![project_id], |row| row.get::<_, String>(0))?;
                rows.collect::<Result<Vec<_>, _>>()?
            }
            MemoryTier::Global => {
                let mut stmt = conn.prepare("SELECT path FROM global_file_index")?;
                let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                rows.collect::<Result<Vec<_>, _>>()?
            }
        };
        Ok(rows)
    }

    pub async fn delete_import_index_entry(
        &self,
        tier: MemoryTier,
        session_id: Option<&str>,
        project_id: Option<&str>,
        path: &str,
    ) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        match tier {
            MemoryTier::Session => {
                let session_id = require_scope_id(tier, session_id)?;
                conn.execute(
                    "DELETE FROM session_file_index WHERE session_id = ?1 AND path = ?2",
                    params![session_id, path],
                )?;
            }
            MemoryTier::Project => {
                let project_id = require_scope_id(tier, project_id)?;
                conn.execute(
                    "DELETE FROM project_file_index WHERE project_id = ?1 AND path = ?2",
                    params![project_id, path],
                )?;
            }
            MemoryTier::Global => {
                conn.execute(
                    "DELETE FROM global_file_index WHERE path = ?1",
                    params![path],
                )?;
            }
        }
        Ok(())
    }

    pub async fn delete_file_chunks_by_path(
        &self,
        tier: MemoryTier,
        session_id: Option<&str>,
        project_id: Option<&str>,
        source_path: &str,
    ) -> MemoryResult<(i64, i64)> {
        let conn = self.conn.lock().await;
        let result = match tier {
            MemoryTier::Session => {
                let session_id = require_scope_id(tier, session_id)?;
                let chunks_deleted: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM session_memory_chunks
                     WHERE session_id = ?1 AND source = 'file' AND source_path = ?2",
                    params![session_id, source_path],
                    |row| row.get(0),
                )?;
                let bytes_estimated: i64 = conn.query_row(
                    "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM session_memory_chunks
                     WHERE session_id = ?1 AND source = 'file' AND source_path = ?2",
                    params![session_id, source_path],
                    |row| row.get(0),
                )?;
                conn.execute(
                    "DELETE FROM session_memory_vectors WHERE chunk_id IN
                     (SELECT id FROM session_memory_chunks WHERE session_id = ?1 AND source = 'file' AND source_path = ?2)",
                    params![session_id, source_path],
                )?;
                conn.execute(
                    "DELETE FROM session_memory_chunks
                     WHERE session_id = ?1 AND source = 'file' AND source_path = ?2",
                    params![session_id, source_path],
                )?;
                (chunks_deleted, bytes_estimated)
            }
            MemoryTier::Project => {
                let project_id = require_scope_id(tier, project_id)?;
                let chunks_deleted: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM project_memory_chunks
                     WHERE project_id = ?1 AND source = 'file' AND source_path = ?2",
                    params![project_id, source_path],
                    |row| row.get(0),
                )?;
                let bytes_estimated: i64 = conn.query_row(
                    "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM project_memory_chunks
                     WHERE project_id = ?1 AND source = 'file' AND source_path = ?2",
                    params![project_id, source_path],
                    |row| row.get(0),
                )?;
                conn.execute(
                    "DELETE FROM project_memory_vectors WHERE chunk_id IN
                     (SELECT id FROM project_memory_chunks WHERE project_id = ?1 AND source = 'file' AND source_path = ?2)",
                    params![project_id, source_path],
                )?;
                conn.execute(
                    "DELETE FROM project_memory_chunks
                     WHERE project_id = ?1 AND source = 'file' AND source_path = ?2",
                    params![project_id, source_path],
                )?;
                (chunks_deleted, bytes_estimated)
            }
            MemoryTier::Global => {
                let chunks_deleted: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM global_memory_chunks
                     WHERE source = 'file' AND source_path = ?1",
                    params![source_path],
                    |row| row.get(0),
                )?;
                let bytes_estimated: i64 = conn.query_row(
                    "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM global_memory_chunks
                     WHERE source = 'file' AND source_path = ?1",
                    params![source_path],
                    |row| row.get(0),
                )?;
                conn.execute(
                    "DELETE FROM global_memory_vectors WHERE chunk_id IN
                     (SELECT id FROM global_memory_chunks WHERE source = 'file' AND source_path = ?1)",
                    params![source_path],
                )?;
                conn.execute(
                    "DELETE FROM global_memory_chunks
                     WHERE source = 'file' AND source_path = ?1",
                    params![source_path],
                )?;
                (chunks_deleted, bytes_estimated)
            }
        };
        Ok(result)
    }

    pub async fn upsert_project_index_status(
        &self,
        project_id: &str,
        total_files: i64,
        processed_files: i64,
        indexed_files: i64,
        skipped_files: i64,
        errors: i64,
    ) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        let last_indexed_at = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO project_index_status (
                project_id, last_indexed_at, last_total_files, last_processed_files,
                last_indexed_files, last_skipped_files, last_errors
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(project_id) DO UPDATE SET
                last_indexed_at = excluded.last_indexed_at,
                last_total_files = excluded.last_total_files,
                last_processed_files = excluded.last_processed_files,
                last_indexed_files = excluded.last_indexed_files,
                last_skipped_files = excluded.last_skipped_files,
                last_errors = excluded.last_errors",
            params![
                project_id,
                last_indexed_at,
                total_files,
                processed_files,
                indexed_files,
                skipped_files,
                errors
            ],
        )?;
        Ok(())
    }

    pub async fn get_project_stats(&self, project_id: &str) -> MemoryResult<ProjectMemoryStats> {
        let conn = self.conn.lock().await;

        let project_chunks: i64 = conn.query_row(
            "SELECT COUNT(*) FROM project_memory_chunks WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )?;

        let project_bytes: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM project_memory_chunks WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )?;

        let file_index_chunks: i64 = conn.query_row(
            "SELECT COUNT(*) FROM project_memory_chunks WHERE project_id = ?1 AND source = 'file'",
            params![project_id],
            |row| row.get(0),
        )?;

        let file_index_bytes: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM project_memory_chunks WHERE project_id = ?1 AND source = 'file'",
            params![project_id],
            |row| row.get(0),
        )?;

        let indexed_files: i64 = conn.query_row(
            "SELECT COUNT(*) FROM project_file_index WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )?;

        let status_row: Option<ProjectIndexStatusRow> =
            conn
                .query_row(
                    "SELECT last_indexed_at, last_total_files, last_processed_files, last_indexed_files, last_skipped_files, last_errors
                     FROM project_index_status WHERE project_id = ?1",
                    params![project_id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                        ))
                    },
                )
                .optional()?;

        let (
            last_indexed_at,
            last_total_files,
            last_processed_files,
            last_indexed_files,
            last_skipped_files,
            last_errors,
        ) = status_row.unwrap_or((None, None, None, None, None, None));

        let last_indexed_at = last_indexed_at.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        });

        Ok(ProjectMemoryStats {
            project_id: project_id.to_string(),
            project_chunks,
            project_bytes,
            file_index_chunks,
            file_index_bytes,
            indexed_files,
            last_indexed_at,
            last_total_files,
            last_processed_files,
            last_indexed_files,
            last_skipped_files,
            last_errors,
        })
    }

    pub async fn clear_project_file_index(
        &self,
        project_id: &str,
        vacuum: bool,
    ) -> MemoryResult<ClearFileIndexResult> {
        let conn = self.conn.lock().await;

        let chunks_deleted: i64 = conn.query_row(
            "SELECT COUNT(*) FROM project_memory_chunks WHERE project_id = ?1 AND source = 'file'",
            params![project_id],
            |row| row.get(0),
        )?;

        let bytes_estimated: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM project_memory_chunks WHERE project_id = ?1 AND source = 'file'",
            params![project_id],
            |row| row.get(0),
        )?;

        // Delete vectors first
        conn.execute(
            "DELETE FROM project_memory_vectors WHERE chunk_id IN
             (SELECT id FROM project_memory_chunks WHERE project_id = ?1 AND source = 'file')",
            params![project_id],
        )?;

        // Delete file chunks
        conn.execute(
            "DELETE FROM project_memory_chunks WHERE project_id = ?1 AND source = 'file'",
            params![project_id],
        )?;

        // Clear file index tracking + status
        conn.execute(
            "DELETE FROM project_file_index WHERE project_id = ?1",
            params![project_id],
        )?;
        conn.execute(
            "DELETE FROM project_index_status WHERE project_id = ?1",
            params![project_id],
        )?;

        drop(conn); // release lock before VACUUM (which needs exclusive access)

        if vacuum {
            self.vacuum().await?;
        }

        Ok(ClearFileIndexResult {
            chunks_deleted,
            bytes_estimated,
            did_vacuum: vacuum,
        })
    }

    // ------------------------------------------------------------------
    // Memory hygiene
    // ------------------------------------------------------------------

    /// Delete session memory chunks older than `retention_days` days.
    ///
    /// Also removes orphaned vector entries for the deleted chunks so the
    /// sqlite-vec virtual table stays consistent.
    ///
    /// Returns the number of chunk rows deleted.
    /// If `retention_days` is 0 hygiene is disabled and this returns Ok(0).
    pub async fn prune_old_session_chunks(&self, retention_days: u32) -> MemoryResult<u64> {
        if retention_days == 0 {
            return Ok(0);
        }

        let conn = self.conn.lock().await;

        // WAL is already active (set in new()) — no need to set it again here.
        let cutoff =
            (chrono::Utc::now() - chrono::Duration::days(i64::from(retention_days))).to_rfc3339();

        // Remove orphaned vector entries first (chunk_id FK would dangle otherwise)
        conn.execute(
            "DELETE FROM session_memory_vectors
             WHERE chunk_id IN (
                 SELECT id FROM session_memory_chunks WHERE created_at < ?1
             )",
            params![cutoff],
        )?;

        let deleted = conn.execute(
            "DELETE FROM session_memory_chunks WHERE created_at < ?1",
            params![cutoff],
        )?;

        if deleted > 0 {
            tracing::info!(
                retention_days,
                deleted,
                "memory hygiene: pruned old session chunks"
            );
        }

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Ok(deleted as u64)
    }

    /// Run scheduled hygiene: read `session_retention_days` from `memory_config`
    /// (falling back to `env_override` if provided) and prune stale session chunks.
    ///
    /// Returns `Ok(chunks_deleted)`. This method is intentionally best-effort —
    /// callers should log errors and continue.
    pub async fn run_hygiene(&self, env_override_days: u32) -> MemoryResult<u64> {
        // Prefer the env override, fall back to the DB config for the null project.
        let retention_days = if env_override_days > 0 {
            env_override_days
        } else {
            // Try to read the global (project_id = '__global__') config if present.
            let conn = self.conn.lock().await;
            let days: Option<i64> = conn
                .query_row(
                    "SELECT session_retention_days FROM memory_config
                     WHERE project_id = '__global__' LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .ok();
            drop(conn);
            days.unwrap_or(30) as u32
        };

        self.prune_old_session_chunks(retention_days).await
    }

    pub async fn put_global_memory_record(
        &self,
        record: &GlobalMemoryRecord,
    ) -> MemoryResult<GlobalMemoryWriteResult> {
        let conn = self.conn.lock().await;

        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM memory_records
                 WHERE user_id = ?1
                   AND source_type = ?2
                   AND content_hash = ?3
                   AND run_id = ?4
                   AND IFNULL(session_id, '') = IFNULL(?5, '')
                   AND IFNULL(message_id, '') = IFNULL(?6, '')
                   AND IFNULL(tool_name, '') = IFNULL(?7, '')
                 LIMIT 1",
                params![
                    record.user_id,
                    record.source_type,
                    record.content_hash,
                    record.run_id,
                    record.session_id,
                    record.message_id,
                    record.tool_name
                ],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(id) = existing {
            return Ok(GlobalMemoryWriteResult {
                id,
                stored: false,
                deduped: true,
            });
        }

        let metadata = record
            .metadata
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        let provenance = record
            .provenance
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        conn.execute(
            "INSERT INTO memory_records(
                id, user_id, source_type, content, content_hash, run_id, session_id, message_id, tool_name,
                project_tag, channel_tag, host_tag, metadata, provenance, redaction_status, redaction_count,
                visibility, demoted, score_boost, created_at_ms, updated_at_ms, expires_at_ms
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                ?17, ?18, ?19, ?20, ?21, ?22
            )",
            params![
                record.id,
                record.user_id,
                record.source_type,
                record.content,
                record.content_hash,
                record.run_id,
                record.session_id,
                record.message_id,
                record.tool_name,
                record.project_tag,
                record.channel_tag,
                record.host_tag,
                metadata,
                provenance,
                record.redaction_status,
                i64::from(record.redaction_count),
                record.visibility,
                if record.demoted { 1i64 } else { 0i64 },
                record.score_boost,
                record.created_at_ms as i64,
                record.updated_at_ms as i64,
                record.expires_at_ms.map(|v| v as i64),
            ],
        )?;

        Ok(GlobalMemoryWriteResult {
            id: record.id.clone(),
            stored: true,
            deduped: false,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn search_global_memory(
        &self,
        user_id: &str,
        query: &str,
        limit: i64,
        project_tag: Option<&str>,
        channel_tag: Option<&str>,
        host_tag: Option<&str>,
    ) -> MemoryResult<Vec<GlobalMemorySearchHit>> {
        let conn = self.conn.lock().await;
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut hits = Vec::new();

        let fts_query = build_fts_query(query);
        let search_limit = limit.clamp(1, 100);
        let maybe_rows = conn.prepare(
            "SELECT
                m.id, m.user_id, m.source_type, m.content, m.content_hash, m.run_id, m.session_id, m.message_id,
                m.tool_name, m.project_tag, m.channel_tag, m.host_tag, m.metadata, m.provenance,
                m.redaction_status, m.redaction_count, m.visibility, m.demoted, m.score_boost,
                m.created_at_ms, m.updated_at_ms, m.expires_at_ms,
                bm25(memory_records_fts) AS rank
             FROM memory_records_fts
             JOIN memory_records m ON m.id = memory_records_fts.id
             WHERE memory_records_fts MATCH ?1
               AND m.user_id = ?2
               AND m.demoted = 0
               AND (m.expires_at_ms IS NULL OR m.expires_at_ms > ?3)
               AND (?4 IS NULL OR m.project_tag = ?4)
               AND (?5 IS NULL OR m.channel_tag = ?5)
               AND (?6 IS NULL OR m.host_tag = ?6)
             ORDER BY rank ASC
             LIMIT ?7"
        );

        if let Ok(mut stmt) = maybe_rows {
            let rows = stmt.query_map(
                params![
                    fts_query,
                    user_id,
                    now_ms,
                    project_tag,
                    channel_tag,
                    host_tag,
                    search_limit
                ],
                |row| {
                    let record = row_to_global_record(row)?;
                    let rank = row.get::<_, f64>(22)?;
                    let score = 1.0 / (1.0 + rank.max(0.0));
                    Ok(GlobalMemorySearchHit { record, score })
                },
            )?;
            for row in rows {
                hits.push(row?);
            }
        }

        if !hits.is_empty() {
            return Ok(hits);
        }

        let like = format!("%{}%", query.trim());
        let mut stmt = conn.prepare(
            "SELECT
                id, user_id, source_type, content, content_hash, run_id, session_id, message_id,
                tool_name, project_tag, channel_tag, host_tag, metadata, provenance,
                redaction_status, redaction_count, visibility, demoted, score_boost,
                created_at_ms, updated_at_ms, expires_at_ms
             FROM memory_records
             WHERE user_id = ?1
               AND demoted = 0
               AND (expires_at_ms IS NULL OR expires_at_ms > ?2)
               AND (?3 IS NULL OR project_tag = ?3)
               AND (?4 IS NULL OR channel_tag = ?4)
               AND (?5 IS NULL OR host_tag = ?5)
               AND (?6 = '' OR content LIKE ?7)
             ORDER BY created_at_ms DESC
             LIMIT ?8",
        )?;
        let rows = stmt.query_map(
            params![
                user_id,
                now_ms,
                project_tag,
                channel_tag,
                host_tag,
                query.trim(),
                like,
                search_limit
            ],
            |row| {
                let record = row_to_global_record(row)?;
                Ok(GlobalMemorySearchHit {
                    record,
                    score: 0.25,
                })
            },
        )?;
        for row in rows {
            hits.push(row?);
        }

        Ok(hits)
    }

    pub async fn list_global_memory(
        &self,
        user_id: &str,
        q: Option<&str>,
        project_tag: Option<&str>,
        channel_tag: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> MemoryResult<Vec<GlobalMemoryRecord>> {
        let conn = self.conn.lock().await;
        let query = q.unwrap_or("").trim();
        let like = format!("%{}%", query);
        let mut stmt = conn.prepare(
            "SELECT
                id, user_id, source_type, content, content_hash, run_id, session_id, message_id,
                tool_name, project_tag, channel_tag, host_tag, metadata, provenance,
                redaction_status, redaction_count, visibility, demoted, score_boost,
                created_at_ms, updated_at_ms, expires_at_ms
             FROM memory_records
             WHERE user_id = ?1
               AND (?2 = '' OR content LIKE ?3 OR source_type LIKE ?3 OR run_id LIKE ?3)
               AND (?4 IS NULL OR project_tag = ?4)
               AND (?5 IS NULL OR channel_tag = ?5)
             ORDER BY created_at_ms DESC
             LIMIT ?6 OFFSET ?7",
        )?;
        let rows = stmt.query_map(
            params![
                user_id,
                query,
                like,
                project_tag,
                channel_tag,
                limit.clamp(1, 1000),
                offset.max(0)
            ],
            row_to_global_record,
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub async fn set_global_memory_visibility(
        &self,
        id: &str,
        visibility: &str,
        demoted: bool,
    ) -> MemoryResult<bool> {
        let conn = self.conn.lock().await;
        let now_ms = chrono::Utc::now().timestamp_millis();
        let changed = conn.execute(
            "UPDATE memory_records
             SET visibility = ?2, demoted = ?3, updated_at_ms = ?4
             WHERE id = ?1",
            params![id, visibility, if demoted { 1i64 } else { 0i64 }, now_ms],
        )?;
        Ok(changed > 0)
    }

    pub async fn update_global_memory_context(
        &self,
        id: &str,
        visibility: &str,
        demoted: bool,
        metadata: Option<&serde_json::Value>,
        provenance: Option<&serde_json::Value>,
    ) -> MemoryResult<bool> {
        let conn = self.conn.lock().await;
        let now_ms = chrono::Utc::now().timestamp_millis();
        let metadata = metadata.map(ToString::to_string).unwrap_or_default();
        let provenance = provenance.map(ToString::to_string).unwrap_or_default();
        let changed = conn.execute(
            "UPDATE memory_records
             SET visibility = ?2, demoted = ?3, metadata = ?4, provenance = ?5, updated_at_ms = ?6
             WHERE id = ?1",
            params![
                id,
                visibility,
                if demoted { 1i64 } else { 0i64 },
                metadata,
                provenance,
                now_ms,
            ],
        )?;
        Ok(changed > 0)
    }

    pub async fn get_global_memory(&self, id: &str) -> MemoryResult<Option<GlobalMemoryRecord>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT
                id, user_id, source_type, content, content_hash, run_id, session_id, message_id,
                tool_name, project_tag, channel_tag, host_tag, metadata, provenance,
                redaction_status, redaction_count, visibility, demoted, score_boost,
                created_at_ms, updated_at_ms, expires_at_ms
             FROM memory_records
             WHERE id = ?1
             LIMIT 1",
        )?;
        let record = stmt
            .query_row(params![id], row_to_global_record)
            .optional()?;
        Ok(record)
    }

    pub async fn delete_global_memory(&self, id: &str) -> MemoryResult<bool> {
        let conn = self.conn.lock().await;
        let changed = conn.execute("DELETE FROM memory_records WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }
}
