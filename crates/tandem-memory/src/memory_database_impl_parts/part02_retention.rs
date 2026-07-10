// Memory hygiene & retention (session pruning, expired-record reaping,
// exchange retention, project chunk caps, global-tier retention, cleanup log).

/// Rows deleted per statement while reaping so the connection mutex is
/// released between batches and stays responsive for foreground queries.
const HYGIENE_DELETE_BATCH: usize = 500;

impl MemoryDatabase {
    /// Delete session memory chunks older than `retention_days` days.
    ///
    /// Also removes orphaned vector entries for the deleted chunks so the
    /// sqlite-vec virtual table stays consistent.
    ///
    /// Returns the number of chunk rows deleted.
    /// If `retention_days` is 0 hygiene is disabled and this returns Ok(0).
    pub async fn prune_old_session_chunks(&self, retention_days: u32) -> MemoryResult<u64> {
        self.prune_old_session_chunks_for_tenant(retention_days, &MemoryTenantScope::local())
            .await
    }

    pub async fn prune_old_session_chunks_for_tenant(
        &self,
        retention_days: u32,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<u64> {
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
                 SELECT id FROM session_memory_chunks
                 WHERE created_at < ?1
                   AND tenant_org_id = ?2
                   AND tenant_workspace_id = ?3
                   AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')
             )",
            params![
                cutoff,
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref()
            ],
        )?;

        let deleted = conn.execute(
            "DELETE FROM session_memory_chunks
             WHERE created_at < ?1
               AND tenant_org_id = ?2
               AND tenant_workspace_id = ?3
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')",
            params![
                cutoff,
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref()
            ],
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

    /// Delete global-tier memory chunks (and their paired vector rows) older
    /// than `retention_days` days. 0 disables age-pruning and returns Ok(0) —
    /// global chunks are user-facing archived memory, so this is opt-in.
    pub async fn prune_old_global_chunks_for_tenant(
        &self,
        retention_days: u32,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<u64> {
        if retention_days == 0 {
            return Ok(0);
        }

        let conn = self.conn.lock().await;
        let cutoff =
            (chrono::Utc::now() - chrono::Duration::days(i64::from(retention_days))).to_rfc3339();

        conn.execute(
            "DELETE FROM global_memory_vectors
             WHERE chunk_id IN (
                 SELECT id FROM global_memory_chunks
                 WHERE created_at < ?1
                   AND tenant_org_id = ?2
                   AND tenant_workspace_id = ?3
                   AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')
             )",
            params![
                cutoff,
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref()
            ],
        )?;

        let deleted = conn.execute(
            "DELETE FROM global_memory_chunks
             WHERE created_at < ?1
               AND tenant_org_id = ?2
               AND tenant_workspace_id = ?3
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')",
            params![
                cutoff,
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref()
            ],
        )?;

        if deleted > 0 {
            tracing::info!(
                retention_days,
                deleted,
                "memory hygiene: pruned old global chunks"
            );
        }

        Ok(deleted as u64)
    }

    /// Hard-delete `memory_records` rows whose TTL has elapsed. These rows are
    /// already invisible to every read path (`expires_at_ms IS NULL OR
    /// expires_at_ms > now` filters), so this is pure space reclamation. The
    /// FTS mirror is kept in sync by the AD trigger.
    pub async fn reap_expired_memory_records_for_tenant(
        &self,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<u64> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut total = 0u64;
        loop {
            let deleted = {
                let conn = self.conn.lock().await;
                conn.execute(
                    "DELETE FROM memory_records
                     WHERE id IN (
                         SELECT id FROM memory_records
                         WHERE expires_at_ms IS NOT NULL
                           AND expires_at_ms <= ?1
                           AND tenant_org_id = ?2
                           AND tenant_workspace_id = ?3
                           AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')
                         LIMIT ?5
                     )",
                    params![
                        now_ms,
                        tenant_scope.org_id.as_str(),
                        tenant_scope.workspace_id.as_str(),
                        tenant_scope.deployment_id.as_deref(),
                        HYGIENE_DELETE_BATCH as i64
                    ],
                )?
            };
            total += deleted as u64;
            if deleted < HYGIENE_DELETE_BATCH {
                break;
            }
        }
        if total > 0 {
            tracing::info!(deleted = total, "memory hygiene: reaped expired records");
        }
        Ok(total)
    }

    /// Delete raw chat exchange records (`user_message`/`assistant_final`)
    /// older than `retention_days` days by `created_at_ms`.
    /// 0 = keep forever (returns Ok(0)).
    pub async fn prune_exchange_memory_records_for_tenant(
        &self,
        retention_days: i64,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<u64> {
        if retention_days <= 0 {
            return Ok(0);
        }

        let cutoff_ms =
            (chrono::Utc::now() - chrono::Duration::days(retention_days)).timestamp_millis();
        let mut total = 0u64;
        loop {
            let deleted = {
                let conn = self.conn.lock().await;
                conn.execute(
                    "DELETE FROM memory_records
                     WHERE id IN (
                         SELECT id FROM memory_records
                         WHERE source_type IN ('user_message', 'assistant_final')
                           AND created_at_ms < ?1
                           AND tenant_org_id = ?2
                           AND tenant_workspace_id = ?3
                           AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')
                         LIMIT ?5
                     )",
                    params![
                        cutoff_ms,
                        tenant_scope.org_id.as_str(),
                        tenant_scope.workspace_id.as_str(),
                        tenant_scope.deployment_id.as_deref(),
                        HYGIENE_DELETE_BATCH as i64
                    ],
                )?
            };
            total += deleted as u64;
            if deleted < HYGIENE_DELETE_BATCH {
                break;
            }
        }
        if total > 0 {
            tracing::info!(
                retention_days,
                deleted = total,
                "memory hygiene: pruned old exchange records"
            );
        }
        Ok(total)
    }

    /// Evict the oldest project-tier chunks for `project_id` until the count
    /// is back at `max_chunks`. Each batch deletes the chunk row and its
    /// paired vector row in one transaction. `max_chunks <= 0` means no cap.
    pub async fn enforce_project_chunk_cap_for_tenant(
        &self,
        project_id: &str,
        max_chunks: i64,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<u64> {
        if max_chunks <= 0 {
            return Ok(0);
        }

        let mut total = 0u64;
        loop {
            let mut conn = self.conn.lock().await;
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM project_memory_chunks
                 WHERE project_id = ?1
                   AND tenant_org_id = ?2
                   AND tenant_workspace_id = ?3
                   AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')",
                params![
                    project_id,
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref()
                ],
                |row| row.get(0),
            )?;
            let excess = count - max_chunks;
            if excess <= 0 {
                break;
            }
            let batch = excess.min(HYGIENE_DELETE_BATCH as i64);

            let oldest_sql = "SELECT id FROM project_memory_chunks
                 WHERE project_id = ?1
                   AND tenant_org_id = ?2
                   AND tenant_workspace_id = ?3
                   AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')
                 ORDER BY created_at ASC, id ASC
                 LIMIT ?5";
            let tx = conn.transaction()?;
            // Vector rows first: once the chunk rows are gone the subquery can
            // no longer name the orphaned vector entries.
            tx.execute(
                &format!(
                    "DELETE FROM project_memory_vectors WHERE chunk_id IN ({oldest_sql})"
                ),
                params![
                    project_id,
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref(),
                    batch
                ],
            )?;
            let deleted = tx.execute(
                &format!("DELETE FROM project_memory_chunks WHERE id IN ({oldest_sql})"),
                params![
                    project_id,
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref(),
                    batch
                ],
            )?;
            tx.commit()?;
            total += deleted as u64;
            if deleted == 0 {
                break;
            }
        }
        if total > 0 {
            tracing::info!(
                project_id,
                max_chunks,
                evicted = total,
                "memory hygiene: evicted oldest project chunks over cap"
            );
        }
        Ok(total)
    }

    /// Run scheduled hygiene for a tenant scope:
    /// - prune session chunks older than `session_retention_days` (env
    ///   override wins when non-zero, else the `__global__` config row,
    ///   else 30 days),
    /// - reap expired `memory_records` (TTL elapsed),
    /// - prune raw chat exchange records past `exchange_retention_days`,
    /// - enforce per-project `max_chunks` caps (oldest-first eviction),
    /// - age-prune global-tier chunks when `global_retention_days` is set.
    ///
    /// Every deletion writes a `memory_cleanup_log` row. Returns the total
    /// number of rows deleted. This method is intentionally best-effort —
    /// callers should log errors and continue.
    pub async fn run_hygiene(&self, env_override_days: u32) -> MemoryResult<u64> {
        self.run_hygiene_for_tenant(env_override_days, &MemoryTenantScope::local())
            .await
    }

    /// Enumerate every tenant scope that has memory rows in any table the
    /// reapers touch. The local scope is always included so a fresh database
    /// with no rows yet still gets its hygiene pass.
    pub async fn list_memory_tenant_scopes(&self) -> MemoryResult<Vec<MemoryTenantScope>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT tenant_org_id, tenant_workspace_id, tenant_deployment_id
               FROM session_memory_chunks
             UNION
             SELECT DISTINCT tenant_org_id, tenant_workspace_id, tenant_deployment_id
               FROM project_memory_chunks
             UNION
             SELECT DISTINCT tenant_org_id, tenant_workspace_id, tenant_deployment_id
               FROM global_memory_chunks
             UNION
             SELECT DISTINCT tenant_org_id, tenant_workspace_id, tenant_deployment_id
               FROM memory_records",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(MemoryTenantScope {
                org_id: row.get(0)?,
                workspace_id: row.get(1)?,
                deployment_id: row.get(2)?,
            })
        })?;
        let mut scopes = rows.collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);
        let local = MemoryTenantScope::local();
        if !scopes.contains(&local) {
            scopes.push(local);
        }
        Ok(scopes)
    }

    /// Run scheduled hygiene for every tenant scope present in the database
    /// (hosted/enterprise deployments stamp real tenant scopes on rows, which
    /// the local-scope wrapper above would never touch). Per-scope failures
    /// are logged and skipped so one bad partition cannot starve the rest.
    /// Returns the total number of rows deleted across all scopes.
    pub async fn run_hygiene_all_tenants(&self, env_override_days: u32) -> MemoryResult<u64> {
        let mut total = 0u64;
        for scope in self.list_memory_tenant_scopes().await? {
            match self.run_hygiene_for_tenant(env_override_days, &scope).await {
                Ok(deleted) => total += deleted,
                Err(error) => tracing::warn!(
                    tenant_org_id = %scope.org_id,
                    tenant_workspace_id = %scope.workspace_id,
                    %error,
                    "memory hygiene failed for tenant scope"
                ),
            }
        }
        Ok(total)
    }

    pub async fn run_hygiene_for_tenant(
        &self,
        env_override_days: u32,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<u64> {
        let defaults = MemoryConfig::default();
        // Read the global (project_id = '__global__') config row if present.
        let global_config: Option<(i64, i64, i64)> = {
            let conn = self.conn.lock().await;
            conn.query_row(
                "SELECT session_retention_days, exchange_retention_days, global_retention_days
                 FROM memory_config
                 WHERE project_id = '__global__'
                   AND tenant_org_id = ?1
                   AND tenant_workspace_id = ?2
                   AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')
                 LIMIT 1",
                params![
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref()
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .ok()
        };
        let (config_session_days, exchange_retention_days, global_retention_days) = global_config
            .unwrap_or((
                defaults.session_retention_days,
                defaults.exchange_retention_days,
                defaults.global_retention_days,
            ));

        // Prefer the env override, fall back to the DB config for the null project.
        let session_retention_days = if env_override_days > 0 {
            env_override_days
        } else {
            config_session_days.max(0) as u32
        };

        let mut total = 0u64;

        let session_pruned = self
            .prune_old_session_chunks_for_tenant(session_retention_days, tenant_scope)
            .await?;
        if session_pruned > 0 {
            self.log_cleanup_for_tenant(
                "hygiene_session_retention",
                MemoryTier::Session,
                None,
                None,
                session_pruned as i64,
                0,
                tenant_scope,
            )
            .await?;
        }
        total += session_pruned;

        let expired_reaped = self
            .reap_expired_memory_records_for_tenant(tenant_scope)
            .await?;
        if expired_reaped > 0 {
            self.log_cleanup_for_tenant(
                "hygiene_expired_records",
                MemoryTier::Global,
                None,
                None,
                expired_reaped as i64,
                0,
                tenant_scope,
            )
            .await?;
        }
        total += expired_reaped;

        let exchanges_pruned = self
            .prune_exchange_memory_records_for_tenant(exchange_retention_days, tenant_scope)
            .await?;
        if exchanges_pruned > 0 {
            self.log_cleanup_for_tenant(
                "hygiene_exchange_retention",
                MemoryTier::Global,
                None,
                None,
                exchanges_pruned as i64,
                0,
                tenant_scope,
            )
            .await?;
        }
        total += exchanges_pruned;

        // Enforce the project-tier chunk cap for every project in this tenant,
        // using the project's own config row when one exists.
        let project_ids: Vec<String> = {
            let conn = self.conn.lock().await;
            let mut stmt = conn.prepare(
                "SELECT DISTINCT project_id FROM project_memory_chunks
                 WHERE tenant_org_id = ?1
                   AND tenant_workspace_id = ?2
                   AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')",
            )?;
            let rows = stmt.query_map(
                params![
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref()
                ],
                |row| row.get::<_, String>(0),
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for project_id in project_ids {
            let max_chunks: i64 = {
                let conn = self.conn.lock().await;
                conn.query_row(
                    "SELECT max_chunks FROM memory_config
                     WHERE project_id = ?1
                       AND tenant_org_id = ?2
                       AND tenant_workspace_id = ?3
                       AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')",
                    params![
                        project_id,
                        tenant_scope.org_id.as_str(),
                        tenant_scope.workspace_id.as_str(),
                        tenant_scope.deployment_id.as_deref()
                    ],
                    |row| row.get(0),
                )
                .optional()?
                .unwrap_or(defaults.max_chunks)
            };
            let evicted = self
                .enforce_project_chunk_cap_for_tenant(&project_id, max_chunks, tenant_scope)
                .await?;
            if evicted > 0 {
                self.log_cleanup_for_tenant(
                    "hygiene_project_cap",
                    MemoryTier::Project,
                    Some(&project_id),
                    None,
                    evicted as i64,
                    0,
                    tenant_scope,
                )
                .await?;
            }
            total += evicted;
        }

        let global_pruned = self
            .prune_old_global_chunks_for_tenant(global_retention_days.max(0) as u32, tenant_scope)
            .await?;
        if global_pruned > 0 {
            self.log_cleanup_for_tenant(
                "hygiene_global_retention",
                MemoryTier::Global,
                None,
                None,
                global_pruned as i64,
                0,
                tenant_scope,
            )
            .await?;
        }
        total += global_pruned;

        Ok(total)
    }

    /// Read recent cleanup log entries (newest first).
    pub async fn get_cleanup_log(&self, limit: i64) -> MemoryResult<Vec<CleanupLogEntry>> {
        self.get_cleanup_log_for_tenant(limit, &MemoryTenantScope::local())
            .await
    }

    pub async fn get_cleanup_log_for_tenant(
        &self,
        limit: i64,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Vec<CleanupLogEntry>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, cleanup_type, tier, project_id, session_id,
                    chunks_deleted, bytes_reclaimed, created_at
             FROM memory_cleanup_log
             WHERE tenant_org_id = ?1
               AND tenant_workspace_id = ?2
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')
             ORDER BY created_at DESC
             LIMIT ?4",
        )?;
        let rows = stmt.query_map(
            params![
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref(),
                limit.clamp(1, 1000)
            ],
            |row| {
                let tier = match row.get::<_, String>(2)?.as_str() {
                    "session" => MemoryTier::Session,
                    "project" => MemoryTier::Project,
                    _ => MemoryTier::Global,
                };
                let created_at = DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                    .map(|value| value.with_timezone(&Utc))
                    .unwrap_or_default();
                Ok(CleanupLogEntry {
                    id: row.get(0)?,
                    cleanup_type: row.get(1)?,
                    tier,
                    project_id: row.get(3)?,
                    session_id: row.get(4)?,
                    chunks_deleted: row.get(5)?,
                    bytes_reclaimed: row.get(6)?,
                    created_at,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(MemoryError::from)
    }

    pub async fn delete_global_memory(&self, id: &str) -> MemoryResult<bool> {
        let conn = self.conn.lock().await;
        let changed = conn.execute("DELETE FROM memory_records WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }

    pub async fn delete_global_memory_for_tenant(
        &self,
        id: &str,
        tenant_org_id: &str,
        tenant_workspace_id: &str,
        tenant_deployment_id: Option<&str>,
    ) -> MemoryResult<bool> {
        let conn = self.conn.lock().await;
        let changed = conn.execute(
            "DELETE FROM memory_records
             WHERE id = ?1
               AND tenant_org_id = ?2
               AND tenant_workspace_id = ?3
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')",
            params![id, tenant_org_id, tenant_workspace_id, tenant_deployment_id],
        )?;
        Ok(changed > 0)
    }
}
