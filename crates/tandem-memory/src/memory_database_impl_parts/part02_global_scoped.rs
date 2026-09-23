// Department-scoped variants of the global-memory-record read path (TAN-645),
// split out of `part02.rs` to satisfy the 2000-line file gate. These carry the
// `owner_org_unit_id` SQL predicate; the tenant-only wrappers in `part02.rs`
// delegate here with `None`. Included into `db.rs` alongside the other parts.

impl MemoryDatabase {
    /// Department-scoped variant of [`Self::search_global_memory_for_tenant`]
    /// (TAN-645). `owner_org_unit_id = None` matches all rows (tenant-only, the
    /// behavior-preserving default); `Some(dept)` admits that department and
    /// explicitly tenant-shared rows without a department. Unlabelled NULL rows
    /// remain excluded (TAN-647), and private ownership is checked independently.
    /// Enforced in-query rather than post-filtered, so
    /// LIMIT/ranking see the scoped set.
    #[allow(clippy::too_many_arguments)]
    pub async fn search_global_memory_for_tenant_scoped(
        &self,
        tenant_org_id: &str,
        tenant_workspace_id: &str,
        tenant_deployment_id: Option<&str>,
        caller_subject: Option<&str>,
        legacy_user_id: &str,
        query: &str,
        limit: i64,
        project_tag: Option<&str>,
        channel_tag: Option<&str>,
        host_tag: Option<&str>,
        owner_org_unit_id: Option<&str>,
    ) -> MemoryResult<Vec<GlobalMemorySearchHit>> {
        if !self.crypto.is_plaintext() {
            return self.search_encrypted_global_memory_for_tenant_scoped(
                tenant_org_id, tenant_workspace_id, tenant_deployment_id, caller_subject,
                legacy_user_id, query, limit, project_tag, channel_tag, host_tag,
                owner_org_unit_id,
            ).await;
        }
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
                m.content_envelope, m.metadata_envelope, m.provenance_envelope,
                m.tenant_org_id, m.tenant_workspace_id, m.tenant_deployment_id,
                m.owner_org_unit_id, m.owner_subject,
                bm25(memory_records_fts) AS rank
             FROM memory_records_fts
             JOIN memory_records m ON m.id = memory_records_fts.id
             WHERE memory_records_fts MATCH ?1
               AND m.tenant_org_id = ?2
               AND m.tenant_workspace_id = ?3
               AND IFNULL(m.tenant_deployment_id, '') = IFNULL(?4, '')
               AND (
                   m.owner_subject = ?5
                   OR (m.private = 0 AND (m.owner_org_unit_id IS NOT NULL OR m.tenant_shared = 1))
                   OR (m.owner_subject IS NULL AND m.owner_org_unit_id IS NULL AND m.user_id = ?12)
               )
               AND m.demoted = 0
               AND (m.expires_at_ms IS NULL OR m.expires_at_ms > ?6)
               AND (?7 IS NULL OR m.project_tag = ?7)
               AND (?8 IS NULL OR m.channel_tag = ?8)
               AND (?9 IS NULL OR m.host_tag = ?9)
               AND (?11 IS NULL OR m.owner_org_unit_id = ?11 OR (m.owner_org_unit_id IS NULL AND m.tenant_shared = 1))
             ORDER BY rank ASC
             LIMIT ?10"
        );

        if let Ok(mut stmt) = maybe_rows {
            let rows = stmt.query_map(
                params![
                    fts_query,
                    tenant_org_id,
                    tenant_workspace_id,
                    tenant_deployment_id,
                    caller_subject,
                    now_ms,
                    project_tag,
                    channel_tag,
                    host_tag,
                    search_limit,
                    owner_org_unit_id,
                    legacy_user_id
                ],
                |row| {
                    let record = row_to_global_record(row, &self.crypto)?;
                    let rank = row.get::<_, f64>(30)?;
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
                created_at_ms, updated_at_ms, expires_at_ms,
                content_envelope, metadata_envelope, provenance_envelope,
                tenant_org_id, tenant_workspace_id, tenant_deployment_id,
                owner_org_unit_id, owner_subject
             FROM memory_records
             WHERE tenant_org_id = ?1
               AND tenant_workspace_id = ?2
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')
               AND (
                   owner_subject = ?4
                   OR (private = 0 AND (owner_org_unit_id IS NOT NULL OR tenant_shared = 1))
                   OR (owner_subject IS NULL AND owner_org_unit_id IS NULL AND user_id = ?13)
               )
               AND demoted = 0
               AND (expires_at_ms IS NULL OR expires_at_ms > ?5)
               AND (?6 IS NULL OR project_tag = ?6)
               AND (?7 IS NULL OR channel_tag = ?7)
               AND (?8 IS NULL OR host_tag = ?8)
               AND (?9 = '' OR content LIKE ?10)
               AND (?12 IS NULL OR owner_org_unit_id = ?12 OR (owner_org_unit_id IS NULL AND tenant_shared = 1))
             ORDER BY created_at_ms DESC
             LIMIT ?11",
        )?;
        let rows = stmt.query_map(
            params![
                tenant_org_id,
                tenant_workspace_id,
                tenant_deployment_id,
                caller_subject,
                now_ms,
                project_tag,
                channel_tag,
                host_tag,
                query.trim(),
                like,
                search_limit,
                owner_org_unit_id,
                legacy_user_id
            ],
            |row| {
                let record = row_to_global_record(row, &self.crypto)?;
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

    /// FTS5 would persist readable tokens for encrypted rows. Scan only rows
    /// authorized by the SQL tenant/owner predicates, then decrypt and match in
    /// process. LIMIT applies after matching so late rows are not lost.
    #[allow(clippy::too_many_arguments)]
    async fn search_encrypted_global_memory_for_tenant_scoped(
        &self,
        tenant_org_id: &str,
        tenant_workspace_id: &str,
        tenant_deployment_id: Option<&str>,
        caller_subject: Option<&str>,
        legacy_user_id: &str,
        query: &str,
        limit: i64,
        project_tag: Option<&str>,
        channel_tag: Option<&str>,
        host_tag: Option<&str>,
        owner_org_unit_id: Option<&str>,
    ) -> MemoryResult<Vec<GlobalMemorySearchHit>> {
        let conn = self.conn.lock().await;
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut stmt = conn.prepare(
            "SELECT id, user_id, source_type, content, content_hash, run_id, session_id,
                    message_id, tool_name, project_tag, channel_tag, host_tag, metadata,
                    provenance, redaction_status, redaction_count, visibility, demoted,
                    score_boost, created_at_ms, updated_at_ms, expires_at_ms,
                    content_envelope, metadata_envelope, provenance_envelope,
                    tenant_org_id, tenant_workspace_id, tenant_deployment_id,
                    owner_org_unit_id, owner_subject
             FROM memory_records
             WHERE tenant_org_id = ?1 AND tenant_workspace_id = ?2
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')
               AND (owner_subject = ?4
                    OR (private = 0 AND (owner_org_unit_id IS NOT NULL OR tenant_shared = 1))
                    OR (owner_subject IS NULL AND owner_org_unit_id IS NULL AND user_id = ?10))
               AND demoted = 0
               AND (expires_at_ms IS NULL OR expires_at_ms > ?5)
               AND (?6 IS NULL OR project_tag = ?6)
               AND (?7 IS NULL OR channel_tag = ?7)
               AND (?8 IS NULL OR host_tag = ?8)
               AND (?9 IS NULL OR owner_org_unit_id = ?9
                    OR (owner_org_unit_id IS NULL AND tenant_shared = 1))
             ORDER BY created_at_ms DESC",
        )?;
        let rows = stmt.query_map(
            params![tenant_org_id, tenant_workspace_id, tenant_deployment_id,
                caller_subject, now_ms, project_tag, channel_tag, host_tag,
                owner_org_unit_id, legacy_user_id],
            |row| row_to_global_record(row, &self.crypto),
        )?;
        let mut hits = Vec::new();
        for row in rows {
            let record = match row {
                Ok(record) => record,
                Err(error) if self.crypto.is_hosted() && is_global_record_grant_denial(&error) => {
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            if hosted_global_text_matches(&record.content, query) {
                hits.push(GlobalMemorySearchHit { record, score: 0.25 });
                if hits.len() >= limit.clamp(1, 100) as usize {
                    break;
                }
            }
        }
        Ok(hits)
    }

    /// Department-scoped variant of [`Self::list_global_memory_for_tenant`]
    /// (TAN-645). See [`Self::search_global_memory_for_tenant_scoped`] for the
    /// `owner_org_unit_id` predicate semantics (`None` = tenant-wide;
    /// `Some(dept)` restricts, admitting department-free rows only when explicitly shared).
    #[allow(clippy::too_many_arguments)]
    pub async fn list_global_memory_for_tenant_scoped(
        &self,
        tenant_org_id: &str,
        tenant_workspace_id: &str,
        tenant_deployment_id: Option<&str>,
        caller_subject: Option<&str>,
        legacy_user_id: &str,
        q: Option<&str>,
        project_tag: Option<&str>,
        channel_tag: Option<&str>,
        limit: i64,
        offset: i64,
        owner_org_unit_id: Option<&str>,
    ) -> MemoryResult<Vec<GlobalMemoryRecord>> {
        // Even an empty hosted listing can span data classes. Apply grant
        // filtering before LIMIT/OFFSET so one denied row cannot hide later
        // authorized rows or turn the whole listing into an error.
        if !self.crypto.is_plaintext() {
            return self.list_encrypted_global_memory_for_tenant_scoped(
                tenant_org_id, tenant_workspace_id, tenant_deployment_id, caller_subject,
                legacy_user_id, q.unwrap_or_default().trim(), project_tag, channel_tag, limit,
                offset, owner_org_unit_id,
            ).await;
        }
        let conn = self.conn.lock().await;
        let query = q.unwrap_or("").trim();
        let like = format!("%{}%", query);
        let mut stmt = conn.prepare(
            "SELECT
                id, user_id, source_type, content, content_hash, run_id, session_id, message_id,
                tool_name, project_tag, channel_tag, host_tag, metadata, provenance,
                redaction_status, redaction_count, visibility, demoted, score_boost,
                created_at_ms, updated_at_ms, expires_at_ms,
                content_envelope, metadata_envelope, provenance_envelope,
                tenant_org_id, tenant_workspace_id, tenant_deployment_id,
                owner_org_unit_id, owner_subject
             FROM memory_records
             WHERE tenant_org_id = ?1
               AND tenant_workspace_id = ?2
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')
               AND (
                   owner_subject = ?4
                   OR (private = 0 AND (owner_org_unit_id IS NOT NULL OR tenant_shared = 1))
                   OR (owner_subject IS NULL AND owner_org_unit_id IS NULL AND user_id = ?12)
               )
               AND (?5 = '' OR content LIKE ?6 OR source_type LIKE ?6 OR run_id LIKE ?6)
               AND (?7 IS NULL OR project_tag = ?7)
               AND (?8 IS NULL OR channel_tag = ?8)
               AND (?11 IS NULL OR owner_org_unit_id = ?11 OR (owner_org_unit_id IS NULL AND tenant_shared = 1))
             ORDER BY created_at_ms DESC
             LIMIT ?9 OFFSET ?10",
        )?;
        let rows = stmt.query_map(
            params![
                tenant_org_id,
                tenant_workspace_id,
                tenant_deployment_id,
                caller_subject,
                query,
                like,
                project_tag,
                channel_tag,
                limit.clamp(1, 1000),
                offset.max(0),
                owner_org_unit_id,
                legacy_user_id
            ],
            |row| row_to_global_record(row, &self.crypto),
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    async fn list_encrypted_global_memory_for_tenant_scoped(
        &self,
        tenant_org_id: &str,
        tenant_workspace_id: &str,
        tenant_deployment_id: Option<&str>,
        caller_subject: Option<&str>,
        legacy_user_id: &str,
        query: &str,
        project_tag: Option<&str>,
        channel_tag: Option<&str>,
        limit: i64,
        offset: i64,
        owner_org_unit_id: Option<&str>,
    ) -> MemoryResult<Vec<GlobalMemoryRecord>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, user_id, source_type, content, content_hash, run_id, session_id,
                    message_id, tool_name, project_tag, channel_tag, host_tag, metadata,
                    provenance, redaction_status, redaction_count, visibility, demoted,
                    score_boost, created_at_ms, updated_at_ms, expires_at_ms,
                    content_envelope, metadata_envelope, provenance_envelope,
                    tenant_org_id, tenant_workspace_id, tenant_deployment_id,
                    owner_org_unit_id, owner_subject
             FROM memory_records
             WHERE tenant_org_id = ?1 AND tenant_workspace_id = ?2
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')
               AND (owner_subject = ?4
                    OR (private = 0 AND (owner_org_unit_id IS NOT NULL OR tenant_shared = 1))
                    OR (owner_subject IS NULL AND owner_org_unit_id IS NULL AND user_id = ?8))
               AND (?5 IS NULL OR project_tag = ?5)
               AND (?6 IS NULL OR channel_tag = ?6)
               AND (?7 IS NULL OR owner_org_unit_id = ?7
                    OR (owner_org_unit_id IS NULL AND tenant_shared = 1))
             ORDER BY created_at_ms DESC",
        )?;
        let rows = stmt.query_map(
            params![tenant_org_id, tenant_workspace_id, tenant_deployment_id,
                caller_subject, project_tag, channel_tag, owner_org_unit_id, legacy_user_id],
            |row| row_to_global_record(row, &self.crypto),
        )?;
        let query_lower = query.to_lowercase();
        let mut skipped = 0i64;
        let mut out = Vec::new();
        for row in rows {
            let record = match row {
                Ok(record) => record,
                Err(error) if self.crypto.is_hosted() && is_global_record_grant_denial(&error) => {
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            if !record.content.to_lowercase().contains(&query_lower)
                && !record.source_type.to_lowercase().contains(&query_lower)
                && !record.run_id.to_lowercase().contains(&query_lower)
            {
                continue;
            }
            if skipped < offset.max(0) {
                skipped += 1;
                continue;
            }
            out.push(record);
            if out.len() >= limit.clamp(1, 1000) as usize {
                break;
            }
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn get_global_memory_for_tenant_scoped(
        &self,
        id: &str,
        tenant_org_id: &str,
        tenant_workspace_id: &str,
        tenant_deployment_id: Option<&str>,
        owner_org_unit_id: Option<&str>,
        caller_subject: Option<&str>,
    ) -> MemoryResult<Option<GlobalMemoryRecord>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT
                id, user_id, source_type, content, content_hash, run_id, session_id, message_id,
                tool_name, project_tag, channel_tag, host_tag, metadata, provenance,
                redaction_status, redaction_count, visibility, demoted, score_boost,
                created_at_ms, updated_at_ms, expires_at_ms,
                content_envelope, metadata_envelope, provenance_envelope,
                tenant_org_id, tenant_workspace_id, tenant_deployment_id,
                owner_org_unit_id, owner_subject
             FROM memory_records
             WHERE id = ?1
               AND tenant_org_id = ?2
               AND tenant_workspace_id = ?3
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')
               AND (?5 IS NULL OR owner_org_unit_id = ?5 OR (owner_org_unit_id IS NULL AND tenant_shared = 1))
               AND (private = 0 OR owner_subject = ?6)
             LIMIT 1",
        )?;
        stmt.query_row(
            params![
                id,
                tenant_org_id,
                tenant_workspace_id,
                tenant_deployment_id,
                owner_org_unit_id,
                caller_subject,
            ],
            |row| row_to_global_record(row, &self.crypto),
        )
        .optional()
        .map_err(MemoryError::from)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_global_memory_context_for_tenant_scoped(
        &self,
        id: &str,
        tenant_org_id: &str,
        tenant_workspace_id: &str,
        tenant_deployment_id: Option<&str>,
        owner_org_unit_id: Option<&str>,
        caller_subject: Option<&str>,
        visibility: &str,
        demoted: bool,
        metadata: Option<&serde_json::Value>,
        provenance: Option<&serde_json::Value>,
    ) -> MemoryResult<bool> {
        let conn = self.conn.lock().await;
        let now_ms = chrono::Utc::now().timestamp_millis();
        let next_owner_org_unit_id = owner_org_unit_id_from_metadata(metadata);
        let next_owner_subject = crate::types::owner_subject_from_metadata(metadata);
        let next_private = next_owner_subject.is_some();
        let next_tenant_shared = crate::types::tenant_shared_from_metadata(metadata);
        let Some(sealed) = seal_global_context_update(&conn, &self.crypto, id, metadata, provenance)? else {
            return Ok(false);
        };
        let changed = conn.execute(
            "UPDATE memory_records
             SET visibility = ?7, demoted = ?8, metadata = ?9, provenance = ?10,
                 updated_at_ms = ?11, owner_org_unit_id = ?12, private = ?13,
                 owner_subject = ?14, tenant_shared = ?15,
                 metadata_envelope = ?16, provenance_envelope = ?17
             WHERE id = ?1
               AND tenant_org_id = ?2
               AND tenant_workspace_id = ?3
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')
               AND (?5 IS NULL OR owner_org_unit_id = ?5 OR (owner_org_unit_id IS NULL AND tenant_shared = 1))
               AND (private = 0 OR owner_subject = ?6)",
            params![
                id,
                tenant_org_id,
                tenant_workspace_id,
                tenant_deployment_id,
                owner_org_unit_id,
                caller_subject,
                visibility,
                i64::from(demoted),
                sealed.metadata,
                sealed.provenance,
                now_ms,
                next_owner_org_unit_id,
                i64::from(next_private),
                next_owner_subject,
                i64::from(next_tenant_shared),
                sealed.metadata_envelope,
                sealed.provenance_envelope,
            ],
        )?;
        Ok(changed > 0)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn delete_global_memory_for_tenant_scoped(
        &self,
        id: &str,
        tenant_org_id: &str,
        tenant_workspace_id: &str,
        tenant_deployment_id: Option<&str>,
        owner_org_unit_id: Option<&str>,
        caller_subject: Option<&str>,
    ) -> MemoryResult<bool> {
        let conn = self.conn.lock().await;
        let changed = conn.execute(
            "DELETE FROM memory_records
             WHERE id = ?1
               AND tenant_org_id = ?2
               AND tenant_workspace_id = ?3
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')
               AND (?5 IS NULL OR owner_org_unit_id = ?5 OR (owner_org_unit_id IS NULL AND tenant_shared = 1))
               AND (private = 0 OR owner_subject = ?6)",
            params![
                id,
                tenant_org_id,
                tenant_workspace_id,
                tenant_deployment_id,
                owner_org_unit_id,
                caller_subject,
            ],
        )?;
        Ok(changed > 0)
    }
}
