// Department-scoped variants of the global-memory-record read path (TAN-645),
// split out of `part02.rs` to satisfy the 2000-line file gate. These carry the
// `owner_org_unit_id` SQL predicate; the tenant-only wrappers in `part02.rs`
// delegate here with `None`. Included into `db.rs` alongside the other parts.

impl MemoryDatabase {
    /// Department-scoped variant of [`Self::search_global_memory_for_tenant`]
    /// (TAN-645). `owner_org_unit_id = None` matches all rows (tenant-only, the
    /// behavior-preserving default); `Some(dept)` restricts to rows stamped with
    /// that department via the SQL predicate `(?N IS NULL OR owner_org_unit_id =
    /// ?N)`, so unstamped (NULL) rows are excluded from a department read
    /// (fail-closed, TAN-647). Enforced in-query rather than post-filtered, so
    /// LIMIT/ranking see the scoped set.
    #[allow(clippy::too_many_arguments)]
    pub async fn search_global_memory_for_tenant_scoped(
        &self,
        tenant_org_id: &str,
        tenant_workspace_id: &str,
        tenant_deployment_id: Option<&str>,
        user_id: &str,
        query: &str,
        limit: i64,
        project_tag: Option<&str>,
        channel_tag: Option<&str>,
        host_tag: Option<&str>,
        owner_org_unit_id: Option<&str>,
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
               AND m.tenant_org_id = ?2
               AND m.tenant_workspace_id = ?3
               AND IFNULL(m.tenant_deployment_id, '') = IFNULL(?4, '')
               AND m.user_id = ?5
               AND m.demoted = 0
               AND (m.expires_at_ms IS NULL OR m.expires_at_ms > ?6)
               AND (?7 IS NULL OR m.project_tag = ?7)
               AND (?8 IS NULL OR m.channel_tag = ?8)
               AND (?9 IS NULL OR m.host_tag = ?9)
               AND (?11 IS NULL OR m.owner_org_unit_id = ?11)
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
                    user_id,
                    now_ms,
                    project_tag,
                    channel_tag,
                    host_tag,
                    search_limit,
                    owner_org_unit_id
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
             WHERE tenant_org_id = ?1
               AND tenant_workspace_id = ?2
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')
               AND user_id = ?4
               AND demoted = 0
               AND (expires_at_ms IS NULL OR expires_at_ms > ?5)
               AND (?6 IS NULL OR project_tag = ?6)
               AND (?7 IS NULL OR channel_tag = ?7)
               AND (?8 IS NULL OR host_tag = ?8)
               AND (?9 = '' OR content LIKE ?10)
               AND (?12 IS NULL OR owner_org_unit_id = ?12)
             ORDER BY created_at_ms DESC
             LIMIT ?11",
        )?;
        let rows = stmt.query_map(
            params![
                tenant_org_id,
                tenant_workspace_id,
                tenant_deployment_id,
                user_id,
                now_ms,
                project_tag,
                channel_tag,
                host_tag,
                query.trim(),
                like,
                search_limit,
                owner_org_unit_id
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

    /// Department-scoped variant of [`Self::list_global_memory_for_tenant`]
    /// (TAN-645). See [`Self::search_global_memory_for_tenant_scoped`] for the
    /// `owner_org_unit_id` predicate semantics (`None` = tenant-wide;
    /// `Some(dept)` restricts, excluding unstamped rows fail-closed).
    #[allow(clippy::too_many_arguments)]
    pub async fn list_global_memory_for_tenant_scoped(
        &self,
        tenant_org_id: &str,
        tenant_workspace_id: &str,
        tenant_deployment_id: Option<&str>,
        user_id: &str,
        q: Option<&str>,
        project_tag: Option<&str>,
        channel_tag: Option<&str>,
        limit: i64,
        offset: i64,
        owner_org_unit_id: Option<&str>,
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
             WHERE tenant_org_id = ?1
               AND tenant_workspace_id = ?2
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')
               AND user_id = ?4
               AND (?5 = '' OR content LIKE ?6 OR source_type LIKE ?6 OR run_id LIKE ?6)
               AND (?7 IS NULL OR project_tag = ?7)
               AND (?8 IS NULL OR channel_tag = ?8)
               AND (?11 IS NULL OR owner_org_unit_id = ?11)
             ORDER BY created_at_ms DESC
             LIMIT ?9 OFFSET ?10",
        )?;
        let rows = stmt.query_map(
            params![
                tenant_org_id,
                tenant_workspace_id,
                tenant_deployment_id,
                user_id,
                query,
                like,
                project_tag,
                channel_tag,
                limit.clamp(1, 1000),
                offset.max(0),
                owner_org_unit_id
            ],
            row_to_global_record,
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}
