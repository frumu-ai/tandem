impl MemoryDatabase {
/// Get chunks by session ID
    pub async fn get_session_chunks(&self, session_id: &str) -> MemoryResult<Vec<MemoryChunk>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT id, content, session_id, project_id, source, created_at, token_count, metadata,
                    source_path, source_mtime, source_size, source_hash,
                    tenant_org_id, tenant_workspace_id, tenant_deployment_id
             FROM session_memory_chunks
             WHERE session_id = ?1
             ORDER BY created_at DESC",
        )?;

        let chunks = stmt
            .query_map(params![session_id], |row| {
                row_to_chunk(row, MemoryTier::Session, &self.crypto)
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(chunks)
    }

    pub async fn get_session_chunks_for_tenant(
        &self,
        session_id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Vec<MemoryChunk>> {
        let conn = self.conn.lock().await;
        let tenant_clause = tenant_scope_matches_sql_clause("session_memory_chunks", 2);

        let sql = format!(
            "SELECT id, content, session_id, project_id, source, created_at, token_count, metadata,
                    source_path, source_mtime, source_size, source_hash,
                    tenant_org_id, tenant_workspace_id, tenant_deployment_id
             FROM session_memory_chunks
             WHERE session_id = ?1 AND {}
             ORDER BY created_at DESC",
            tenant_clause
        );
        let mut stmt = conn.prepare(&sql)?;

        let chunks = stmt
            .query_map(
                params![
                    session_id,
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref()
                ],
                |row| row_to_chunk(row, MemoryTier::Session, &self.crypto),
            )?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(chunks)
    }

    /// Get chunks by project ID
    pub async fn get_project_chunks(&self, project_id: &str) -> MemoryResult<Vec<MemoryChunk>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT id, content, session_id, project_id, source, created_at, token_count, metadata,
                    source_path, source_mtime, source_size, source_hash,
                    tenant_org_id, tenant_workspace_id, tenant_deployment_id
             FROM project_memory_chunks
             WHERE project_id = ?1
             ORDER BY created_at DESC",
        )?;

        let chunks = stmt
            .query_map(params![project_id], |row| {
                row_to_chunk(row, MemoryTier::Project, &self.crypto)
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(chunks)
    }

    pub async fn get_project_chunks_for_tenant(
        &self,
        project_id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Vec<MemoryChunk>> {
        let conn = self.conn.lock().await;
        let tenant_clause = tenant_scope_matches_sql_clause("project_memory_chunks", 2);

        let sql = format!(
            "SELECT id, content, session_id, project_id, source, created_at, token_count, metadata,
                    source_path, source_mtime, source_size, source_hash,
                    tenant_org_id, tenant_workspace_id, tenant_deployment_id
             FROM project_memory_chunks
             WHERE project_id = ?1 AND {}
             ORDER BY created_at DESC",
            tenant_clause
        );
        let mut stmt = conn.prepare(&sql)?;

        let chunks = stmt
            .query_map(
                params![
                    project_id,
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref()
                ],
                |row| row_to_chunk(row, MemoryTier::Project, &self.crypto),
            )?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(chunks)
    }

    /// Get global chunks
    pub async fn get_global_chunks(&self, limit: i64) -> MemoryResult<Vec<MemoryChunk>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT id, content, source, created_at, token_count, metadata,
                    source_path, source_mtime, source_size, source_hash,
                    tenant_org_id, tenant_workspace_id, tenant_deployment_id
             FROM global_memory_chunks
             ORDER BY created_at DESC
             LIMIT ?1",
        )?;

        let chunks = stmt
            .query_map(params![limit], |row| row_to_chunk(row, MemoryTier::Global, &self.crypto))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(chunks)
    }

    pub async fn get_global_chunks_for_tenant(
        &self,
        tenant_scope: &MemoryTenantScope,
        limit: i64,
    ) -> MemoryResult<Vec<MemoryChunk>> {
        let conn = self.conn.lock().await;
        let tenant_clause = tenant_scope_matches_sql_clause("global_memory_chunks", 1);

        let sql = format!(
            "SELECT id, content, source, created_at, token_count, metadata,
                    source_path, source_mtime, source_size, source_hash,
                    tenant_org_id, tenant_workspace_id, tenant_deployment_id
             FROM global_memory_chunks
             WHERE {}
             ORDER BY created_at DESC
             LIMIT ?4",
            tenant_clause
        );
        let mut stmt = conn.prepare(&sql)?;

        let chunks = stmt
            .query_map(
                params![
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref(),
                    limit
                ],
                |row| row_to_chunk(row, MemoryTier::Global, &self.crypto),
            )?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(chunks)
    }

    pub async fn global_chunk_exists_by_source_hash(
        &self,
        source_hash: &str,
    ) -> MemoryResult<bool> {
        self.global_chunk_exists_by_source_hash_for_tenant(source_hash, &MemoryTenantScope::local())
            .await
    }

    pub async fn global_chunk_exists_by_source_hash_for_tenant(
        &self,
        source_hash: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<bool> {
        let conn = self.conn.lock().await;
        let tenant_clause = tenant_scope_matches_sql_clause("global_memory_chunks", 2);
        let sql = format!(
            "SELECT 1 FROM global_memory_chunks
             WHERE source_hash = ?1 AND {}
             LIMIT 1",
            tenant_clause
        );
        let exists = conn
            .query_row(
                &sql,
                params![
                    source_hash,
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref()
                ],
                |_row| Ok(()),
            )
            .optional()?
            .is_some();
        Ok(exists)
    }

    /// Clear session memory
    pub async fn clear_session_memory(&self, session_id: &str) -> MemoryResult<u64> {
        self.clear_session_memory_for_tenant(session_id, &MemoryTenantScope::local())
            .await
    }

    pub async fn clear_session_memory_for_tenant(
        &self,
        session_id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<u64> {
        let conn = self.conn.lock().await;

        // Get count before deletion
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_memory_chunks
             WHERE session_id = ?1
               AND tenant_org_id = ?2
               AND tenant_workspace_id = ?3
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')",
            params![
                session_id,
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref()
            ],
            |row| row.get(0),
        )?;

        // Delete vectors first (foreign key constraint)
        conn.execute(
            "DELETE FROM session_memory_vectors WHERE chunk_id IN 
             (SELECT id FROM session_memory_chunks
              WHERE session_id = ?1
                AND tenant_org_id = ?2
                AND tenant_workspace_id = ?3
                AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, ''))",
            params![
                session_id,
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref()
            ],
        )?;

        // Delete chunks
        conn.execute(
            "DELETE FROM session_memory_chunks
             WHERE session_id = ?1
               AND tenant_org_id = ?2
               AND tenant_workspace_id = ?3
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')",
            params![
                session_id,
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref()
            ],
        )?;

        Ok(count as u64)
    }

    /// Clear project memory
    pub async fn clear_project_memory(&self, project_id: &str) -> MemoryResult<u64> {
        self.clear_project_memory_for_tenant(project_id, &MemoryTenantScope::local())
            .await
    }

    pub async fn clear_project_memory_for_tenant(
        &self,
        project_id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<u64> {
        let conn = self.conn.lock().await;

        // Get count before deletion
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

        // Delete vectors first
        conn.execute(
            "DELETE FROM project_memory_vectors WHERE chunk_id IN 
             (SELECT id FROM project_memory_chunks
              WHERE project_id = ?1
                AND tenant_org_id = ?2
                AND tenant_workspace_id = ?3
                AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, ''))",
            params![
                project_id,
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref()
            ],
        )?;

        // Delete chunks
        conn.execute(
            "DELETE FROM project_memory_chunks
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
        )?;

        Ok(count as u64)
    }

    /// Clear global memory chunks by source prefix (and matching vectors).
    pub async fn clear_global_memory_by_source_prefix(
        &self,
        source_prefix: &str,
    ) -> MemoryResult<u64> {
        let conn = self.conn.lock().await;
        let like = format!("{}%", source_prefix);

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM global_memory_chunks WHERE source LIKE ?1",
            params![like],
            |row| row.get(0),
        )?;

        conn.execute(
            "DELETE FROM global_memory_vectors WHERE chunk_id IN
             (SELECT id FROM global_memory_chunks WHERE source LIKE ?1)",
            params![like],
        )?;

        conn.execute(
            "DELETE FROM global_memory_chunks WHERE source LIKE ?1",
            params![like],
        )?;

        Ok(count as u64)
    }

    /// Delete a single memory chunk by id within the requested scope.
    pub async fn delete_chunk(
        &self,
        tier: MemoryTier,
        chunk_id: &str,
        project_id: Option<&str>,
        session_id: Option<&str>,
    ) -> MemoryResult<u64> {
        self.delete_chunk_for_tenant(
            tier,
            chunk_id,
            project_id,
            session_id,
            &MemoryTenantScope::local(),
        )
        .await
    }

    pub async fn delete_chunk_for_tenant(
        &self,
        tier: MemoryTier,
        chunk_id: &str,
        project_id: Option<&str>,
        session_id: Option<&str>,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<u64> {
        let conn = self.conn.lock().await;

        let deleted = match tier {
            MemoryTier::Session => {
                let Some(session_id) = session_id else {
                    return Err(MemoryError::InvalidConfig(
                        "session_id is required to delete session memory chunks".to_string(),
                    ));
                };
                conn.execute(
                    "DELETE FROM session_memory_vectors WHERE chunk_id IN
                     (SELECT id FROM session_memory_chunks
                      WHERE id = ?1 AND session_id = ?2
                        AND tenant_org_id = ?3
                        AND tenant_workspace_id = ?4
                        AND IFNULL(tenant_deployment_id, '') = IFNULL(?5, ''))",
                    params![
                        chunk_id,
                        session_id,
                        tenant_scope.org_id.as_str(),
                        tenant_scope.workspace_id.as_str(),
                        tenant_scope.deployment_id.as_deref()
                    ],
                )?;
                conn.execute(
                    "DELETE FROM session_memory_chunks
                     WHERE id = ?1 AND session_id = ?2
                       AND tenant_org_id = ?3
                       AND tenant_workspace_id = ?4
                       AND IFNULL(tenant_deployment_id, '') = IFNULL(?5, '')",
                    params![
                        chunk_id,
                        session_id,
                        tenant_scope.org_id.as_str(),
                        tenant_scope.workspace_id.as_str(),
                        tenant_scope.deployment_id.as_deref()
                    ],
                )?
            }
            MemoryTier::Project => {
                let Some(project_id) = project_id else {
                    return Err(MemoryError::InvalidConfig(
                        "project_id is required to delete project memory chunks".to_string(),
                    ));
                };
                conn.execute(
                    "DELETE FROM project_memory_vectors WHERE chunk_id IN
                     (SELECT id FROM project_memory_chunks
                      WHERE id = ?1 AND project_id = ?2
                        AND tenant_org_id = ?3
                        AND tenant_workspace_id = ?4
                        AND IFNULL(tenant_deployment_id, '') = IFNULL(?5, ''))",
                    params![
                        chunk_id,
                        project_id,
                        tenant_scope.org_id.as_str(),
                        tenant_scope.workspace_id.as_str(),
                        tenant_scope.deployment_id.as_deref()
                    ],
                )?;
                conn.execute(
                    "DELETE FROM project_memory_chunks
                     WHERE id = ?1 AND project_id = ?2
                       AND tenant_org_id = ?3
                       AND tenant_workspace_id = ?4
                       AND IFNULL(tenant_deployment_id, '') = IFNULL(?5, '')",
                    params![
                        chunk_id,
                        project_id,
                        tenant_scope.org_id.as_str(),
                        tenant_scope.workspace_id.as_str(),
                        tenant_scope.deployment_id.as_deref()
                    ],
                )?
            }
            MemoryTier::Global => {
                conn.execute(
                    "DELETE FROM global_memory_vectors WHERE chunk_id IN
                     (SELECT id FROM global_memory_chunks
                      WHERE id = ?1
                        AND tenant_org_id = ?2
                        AND tenant_workspace_id = ?3
                        AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, ''))",
                    params![
                        chunk_id,
                        tenant_scope.org_id.as_str(),
                        tenant_scope.workspace_id.as_str(),
                        tenant_scope.deployment_id.as_deref()
                    ],
                )?;
                conn.execute(
                    "DELETE FROM global_memory_chunks
                     WHERE id = ?1
                       AND tenant_org_id = ?2
                       AND tenant_workspace_id = ?3
                       AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')",
                    params![
                        chunk_id,
                        tenant_scope.org_id.as_str(),
                        tenant_scope.workspace_id.as_str(),
                        tenant_scope.deployment_id.as_deref()
                    ],
                )?
            }
        };

        Ok(deleted as u64)
    }

    /// Clear old session memory based on retention policy
    pub async fn cleanup_old_sessions(&self, retention_days: i64) -> MemoryResult<u64> {
        self.cleanup_old_sessions_for_tenant(retention_days, &MemoryTenantScope::local())
            .await
    }

    pub async fn cleanup_old_sessions_for_tenant(
        &self,
        retention_days: i64,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<u64> {
        let conn = self.conn.lock().await;

        let cutoff = Utc::now() - chrono::Duration::days(retention_days);
        let cutoff_str = cutoff.to_rfc3339();

        // Get count before deletion
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_memory_chunks
             WHERE created_at < ?1
               AND tenant_org_id = ?2
               AND tenant_workspace_id = ?3
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')",
            params![
                cutoff_str,
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref()
            ],
            |row| row.get(0),
        )?;

        // Delete vectors first
        conn.execute(
            "DELETE FROM session_memory_vectors WHERE chunk_id IN 
             (SELECT id FROM session_memory_chunks
              WHERE created_at < ?1
                AND tenant_org_id = ?2
                AND tenant_workspace_id = ?3
                AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, ''))",
            params![
                cutoff_str,
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref()
            ],
        )?;

        // Delete chunks
        conn.execute(
            "DELETE FROM session_memory_chunks
             WHERE created_at < ?1
               AND tenant_org_id = ?2
               AND tenant_workspace_id = ?3
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')",
            params![
                cutoff_str,
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref()
            ],
        )?;

        Ok(count as u64)
    }

    /// Get or create memory config for a project
    pub async fn get_or_create_config(&self, project_id: &str) -> MemoryResult<MemoryConfig> {
        self.get_or_create_config_for_tenant(project_id, &MemoryTenantScope::local())
            .await
    }

    /// Get or create memory config for a project in a tenant scope.
    pub async fn get_or_create_config_for_tenant(
        &self,
        project_id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<MemoryConfig> {
        let conn = self.conn.lock().await;

        let result: Option<MemoryConfig> = conn
            .query_row(
                "SELECT max_chunks, chunk_size, retrieval_k, auto_cleanup, 
                        session_retention_days, token_budget, chunk_overlap
                 FROM memory_config
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
                |row| {
                    Ok(MemoryConfig {
                        max_chunks: row.get(0)?,
                        chunk_size: row.get(1)?,
                        retrieval_k: row.get(2)?,
                        auto_cleanup: row.get::<_, i64>(3)? != 0,
                        session_retention_days: row.get(4)?,
                        token_budget: row.get(5)?,
                        chunk_overlap: row.get(6)?,
                    })
                },
            )
            .optional()?;

        match result {
            Some(config) => Ok(config),
            None => {
                // Create default config
                let config = MemoryConfig::default();
                let updated_at = Utc::now().to_rfc3339();

                conn.execute(
                    "INSERT INTO memory_config
                     (tenant_org_id, tenant_workspace_id, tenant_deployment_id,
                      project_id, max_chunks, chunk_size, retrieval_k, auto_cleanup,
                      session_retention_days, token_budget, chunk_overlap, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        tenant_scope.org_id.as_str(),
                        tenant_scope.workspace_id.as_str(),
                        tenant_scope.deployment_id.as_deref().unwrap_or(""),
                        project_id,
                        config.max_chunks,
                        config.chunk_size,
                        config.retrieval_k,
                        config.auto_cleanup as i64,
                        config.session_retention_days,
                        config.token_budget,
                        config.chunk_overlap,
                        updated_at
                    ],
                )?;

                Ok(config)
            }
        }
    }

    /// Update memory config for a project
    pub async fn update_config(&self, project_id: &str, config: &MemoryConfig) -> MemoryResult<()> {
        self.update_config_for_tenant(project_id, config, &MemoryTenantScope::local())
            .await
    }

    /// Update memory config for a project in a tenant scope.
    pub async fn update_config_for_tenant(
        &self,
        project_id: &str,
        config: &MemoryConfig,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<()> {
        let conn = self.conn.lock().await;

        let updated_at = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO memory_config
             (tenant_org_id, tenant_workspace_id, tenant_deployment_id,
              project_id, max_chunks, chunk_size, retrieval_k, auto_cleanup,
              session_retention_days, token_budget, chunk_overlap, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(tenant_org_id, tenant_workspace_id, tenant_deployment_id, project_id)
             DO UPDATE SET
                max_chunks = excluded.max_chunks,
                chunk_size = excluded.chunk_size,
                retrieval_k = excluded.retrieval_k,
                auto_cleanup = excluded.auto_cleanup,
                session_retention_days = excluded.session_retention_days,
                token_budget = excluded.token_budget,
                chunk_overlap = excluded.chunk_overlap,
                updated_at = excluded.updated_at",
            params![
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref().unwrap_or(""),
                project_id,
                config.max_chunks,
                config.chunk_size,
                config.retrieval_k,
                config.auto_cleanup as i64,
                config.session_retention_days,
                config.token_budget,
                config.chunk_overlap,
                updated_at
            ],
        )?;

        Ok(())
    }

    /// Insert or update a reusable knowledge space.
    pub async fn upsert_knowledge_space(&self, space: &KnowledgeSpaceRecord) -> MemoryResult<()> {
        self.upsert_knowledge_space_for_tenant(space, &MemoryTenantScope::local())
            .await
    }

    /// Insert or update a reusable knowledge space in a tenant scope.
    pub async fn upsert_knowledge_space_for_tenant(
        &self,
        space: &KnowledgeSpaceRecord,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO knowledge_spaces
             (id, tenant_org_id, tenant_workspace_id, tenant_deployment_id, scope, project_id, namespace, title, description, trust_level, metadata, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(id) DO UPDATE SET
                tenant_org_id = excluded.tenant_org_id,
                tenant_workspace_id = excluded.tenant_workspace_id,
                tenant_deployment_id = excluded.tenant_deployment_id,
                scope = excluded.scope,
                project_id = excluded.project_id,
                namespace = excluded.namespace,
                title = excluded.title,
                description = excluded.description,
                trust_level = excluded.trust_level,
                metadata = excluded.metadata,
                created_at_ms = excluded.created_at_ms,
                updated_at_ms = excluded.updated_at_ms",
            params![
                space.id,
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref().unwrap_or(""),
                space.scope.to_string(),
                space.project_id,
                space.namespace,
                space.title,
                space.description,
                space.trust_level.to_string(),
                space.metadata.as_ref().map(|value| value.to_string()),
                space.created_at_ms as i64,
                space.updated_at_ms as i64,
            ],
        )?;
        Ok(())
    }

    /// Fetch a knowledge space by ID.
    pub async fn get_knowledge_space(
        &self,
        id: &str,
    ) -> MemoryResult<Option<KnowledgeSpaceRecord>> {
        self.get_knowledge_space_for_tenant(id, &MemoryTenantScope::local())
            .await
    }

    pub async fn get_knowledge_space_for_tenant(
        &self,
        id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<KnowledgeSpaceRecord>> {
        let conn = self.conn.lock().await;
        Ok(
            conn.query_row(
                "SELECT id, scope, project_id, namespace, title, description, trust_level, metadata, created_at_ms, updated_at_ms
                 FROM knowledge_spaces
                 WHERE id = ?1
                   AND tenant_org_id = ?2
                   AND tenant_workspace_id = ?3
                   AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')",
                params![
                    id,
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref()
                ],
                row_to_knowledge_space,
            )
            .optional()?,
        )
    }

    /// List knowledge spaces, optionally filtered by project.
    pub async fn list_knowledge_spaces(
        &self,
        project_id: Option<&str>,
    ) -> MemoryResult<Vec<KnowledgeSpaceRecord>> {
        self.list_knowledge_spaces_for_tenant(project_id, &MemoryTenantScope::local())
            .await
    }

    pub async fn list_knowledge_spaces_for_tenant(
        &self,
        project_id: Option<&str>,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Vec<KnowledgeSpaceRecord>> {
        let conn = self.conn.lock().await;
        let mut stmt = if project_id.is_some() {
            conn.prepare(
                "SELECT id, scope, project_id, namespace, title, description, trust_level, metadata, created_at_ms, updated_at_ms
                 FROM knowledge_spaces
                 WHERE project_id = ?1
                   AND tenant_org_id = ?2
                   AND tenant_workspace_id = ?3
                   AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')
                 ORDER BY updated_at_ms DESC",
            )?
        } else {
            conn.prepare(
                "SELECT id, scope, project_id, namespace, title, description, trust_level, metadata, created_at_ms, updated_at_ms
                 FROM knowledge_spaces
                 WHERE tenant_org_id = ?1
                   AND tenant_workspace_id = ?2
                   AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')
                 ORDER BY updated_at_ms DESC",
            )?
        };
        let rows = if let Some(project_id) = project_id {
            stmt.query_map(
                params![
                    project_id,
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref()
                ],
                row_to_knowledge_space,
            )?
        } else {
            stmt.query_map(
                params![
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref()
                ],
                row_to_knowledge_space,
            )?
        };
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Insert or update a reusable knowledge item.
    pub async fn upsert_knowledge_item(&self, item: &KnowledgeItemRecord) -> MemoryResult<()> {
        self.upsert_knowledge_item_for_tenant(item, &MemoryTenantScope::local())
            .await
    }

    pub async fn upsert_knowledge_item_for_tenant(
        &self,
        item: &KnowledgeItemRecord,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<()> {
        if self
            .get_knowledge_space_for_tenant(&item.space_id, tenant_scope)
            .await?
            .is_none()
        {
            return Err(crate::types::MemoryError::InvalidConfig(
                "knowledge item space not found for tenant".to_string(),
            ));
        }

        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT OR REPLACE INTO knowledge_items
             (id, space_id, coverage_key, dedupe_key, item_type, title, summary, payload, trust_level, status, run_id, artifact_refs, source_memory_ids, freshness_expires_at_ms, metadata, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                item.id,
                item.space_id,
                item.coverage_key,
                item.dedupe_key,
                item.item_type,
                item.title,
                item.summary,
                item.payload.to_string(),
                item.trust_level.to_string(),
                item.status.to_string(),
                item.run_id,
                serde_json::to_string(&item.artifact_refs)?,
                serde_json::to_string(&item.source_memory_ids)?,
                item.freshness_expires_at_ms.map(|value| value as i64),
                item.metadata.as_ref().map(|value| value.to_string()),
                item.created_at_ms as i64,
                item.updated_at_ms as i64,
            ],
        )?;
        Ok(())
    }

    /// List knowledge items for a knowledge space.
    pub async fn list_knowledge_items(
        &self,
        space_id: &str,
        coverage_key: Option<&str>,
    ) -> MemoryResult<Vec<KnowledgeItemRecord>> {
        self.list_knowledge_items_for_tenant(space_id, coverage_key, &MemoryTenantScope::local())
            .await
    }

    pub async fn list_knowledge_items_for_tenant(
        &self,
        space_id: &str,
        coverage_key: Option<&str>,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Vec<KnowledgeItemRecord>> {
        let conn = self.conn.lock().await;
        let mut stmt = if coverage_key.is_some() {
            conn.prepare(
                "SELECT i.id, i.space_id, i.coverage_key, i.dedupe_key, i.item_type, i.title, i.summary, i.payload, i.trust_level, i.status, i.run_id, i.artifact_refs, i.source_memory_ids, i.freshness_expires_at_ms, i.metadata, i.created_at_ms, i.updated_at_ms
                 FROM knowledge_items i
                 JOIN knowledge_spaces s ON s.id = i.space_id
                 WHERE i.space_id = ?1 AND i.coverage_key = ?2
                   AND s.tenant_org_id = ?3
                   AND s.tenant_workspace_id = ?4
                   AND IFNULL(s.tenant_deployment_id, '') = IFNULL(?5, '')
                 ORDER BY i.created_at_ms DESC",
            )?
        } else {
            conn.prepare(
                "SELECT i.id, i.space_id, i.coverage_key, i.dedupe_key, i.item_type, i.title, i.summary, i.payload, i.trust_level, i.status, i.run_id, i.artifact_refs, i.source_memory_ids, i.freshness_expires_at_ms, i.metadata, i.created_at_ms, i.updated_at_ms
                 FROM knowledge_items i
                 JOIN knowledge_spaces s ON s.id = i.space_id
                 WHERE i.space_id = ?1
                   AND s.tenant_org_id = ?2
                   AND s.tenant_workspace_id = ?3
                   AND IFNULL(s.tenant_deployment_id, '') = IFNULL(?4, '')
                 ORDER BY i.created_at_ms DESC",
            )?
        };
        let rows = if let Some(coverage_key) = coverage_key {
            stmt.query_map(
                params![
                    space_id,
                    coverage_key,
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref()
                ],
                row_to_knowledge_item,
            )?
        } else {
            stmt.query_map(
                params![
                    space_id,
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref()
                ],
                row_to_knowledge_item,
            )?
        };
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Fetch a knowledge item by ID.
    pub async fn get_knowledge_item(&self, id: &str) -> MemoryResult<Option<KnowledgeItemRecord>> {
        self.get_knowledge_item_for_tenant(id, &MemoryTenantScope::local())
            .await
    }

    pub async fn get_knowledge_item_for_tenant(
        &self,
        id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<KnowledgeItemRecord>> {
        let conn = self.conn.lock().await;
        Ok(
            conn.query_row(
                "SELECT i.id, i.space_id, i.coverage_key, i.dedupe_key, i.item_type, i.title, i.summary, i.payload, i.trust_level, i.status, i.run_id, i.artifact_refs, i.source_memory_ids, i.freshness_expires_at_ms, i.metadata, i.created_at_ms, i.updated_at_ms
                 FROM knowledge_items i
                 JOIN knowledge_spaces s ON s.id = i.space_id
                 WHERE i.id = ?1
                   AND s.tenant_org_id = ?2
                   AND s.tenant_workspace_id = ?3
                   AND IFNULL(s.tenant_deployment_id, '') = IFNULL(?4, '')",
                params![
                    id,
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref()
                ],
                row_to_knowledge_item,
            )
            .optional()?,
        )
    }

    /// Promote or retire a knowledge item and update its coverage record atomically.
    pub async fn promote_knowledge_item(
        &self,
        request: &KnowledgePromotionRequest,
    ) -> MemoryResult<Option<KnowledgePromotionResult>> {
        self.promote_knowledge_item_for_tenant(request, &MemoryTenantScope::local())
            .await
    }

    pub async fn promote_knowledge_item_for_tenant(
        &self,
        request: &KnowledgePromotionRequest,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<KnowledgePromotionResult>> {
        let mut conn = self.conn.lock().await;
        let tx = conn.transaction()?;

        let Some(mut item) = tx
            .query_row(
                "SELECT i.id, i.space_id, i.coverage_key, i.dedupe_key, i.item_type, i.title, i.summary, i.payload, i.trust_level, i.status, i.run_id, i.artifact_refs, i.source_memory_ids, i.freshness_expires_at_ms, i.metadata, i.created_at_ms, i.updated_at_ms
                 FROM knowledge_items i
                 JOIN knowledge_spaces s ON s.id = i.space_id
                 WHERE i.id = ?1
                   AND s.tenant_org_id = ?2
                   AND s.tenant_workspace_id = ?3
                   AND IFNULL(s.tenant_deployment_id, '') = IFNULL(?4, '')",
                params![
                    request.item_id,
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref()
                ],
                row_to_knowledge_item,
            )
            .optional()? else {
            return Ok(None);
        };

        let previous_status = item.status;
        let previous_trust_level = item.trust_level;

        if previous_status == KnowledgeItemStatus::Deprecated
            && request.target_status != KnowledgeItemStatus::Deprecated
        {
            return Err(crate::types::MemoryError::InvalidConfig(
                "cannot promote a deprecated knowledge item".to_string(),
            ));
        }

        let next_status = request.target_status;
        match (previous_status, next_status) {
            (KnowledgeItemStatus::Working, KnowledgeItemStatus::Promoted)
            | (KnowledgeItemStatus::Promoted, KnowledgeItemStatus::Promoted)
            | (KnowledgeItemStatus::Promoted, KnowledgeItemStatus::ApprovedDefault)
            | (KnowledgeItemStatus::ApprovedDefault, KnowledgeItemStatus::ApprovedDefault)
            | (KnowledgeItemStatus::Working, KnowledgeItemStatus::Deprecated)
            | (KnowledgeItemStatus::Promoted, KnowledgeItemStatus::Deprecated)
            | (KnowledgeItemStatus::ApprovedDefault, KnowledgeItemStatus::Deprecated) => {}
            (KnowledgeItemStatus::Working, KnowledgeItemStatus::ApprovedDefault) => {
                return Err(crate::types::MemoryError::InvalidConfig(
                    "approved_default requires an intermediate promoted item".to_string(),
                ));
            }
            (KnowledgeItemStatus::ApprovedDefault, KnowledgeItemStatus::Promoted) => {
                return Err(crate::types::MemoryError::InvalidConfig(
                    "approved_default items do not downgrade back to promoted".to_string(),
                ));
            }
            (KnowledgeItemStatus::Promoted, KnowledgeItemStatus::Working)
            | (KnowledgeItemStatus::ApprovedDefault, KnowledgeItemStatus::Working) => {
                return Err(crate::types::MemoryError::InvalidConfig(
                    "knowledge items cannot be demoted back to working".to_string(),
                ));
            }
            (KnowledgeItemStatus::Working, KnowledgeItemStatus::Working) => {}
            (KnowledgeItemStatus::Deprecated, _) => {}
        }

        if next_status == KnowledgeItemStatus::ApprovedDefault
            && (request.reviewer_id.is_none() || request.approval_id.is_none())
        {
            return Err(crate::types::MemoryError::InvalidConfig(
                "approved_default promotion requires reviewer_id and approval_id".to_string(),
            ));
        }

        let promoted = next_status.is_active()
            && (previous_status != next_status || request.freshness_expires_at_ms.is_some());

        let mut metadata_obj = item
            .metadata
            .clone()
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        metadata_obj.insert(
            "promotion".to_string(),
            serde_json::json!({
                "from_status": previous_status.to_string(),
                "to_status": next_status.to_string(),
                "promoted_at_ms": request.promoted_at_ms,
                "reason": request.reason,
                "reviewer_id": request.reviewer_id,
                "approval_id": request.approval_id,
                "freshness_expires_at_ms": request.freshness_expires_at_ms,
            }),
        );

        item.status = next_status;
        if let Some(next_trust) = next_status.as_trust_level() {
            item.trust_level = next_trust;
        }
        if let Some(freshness_expires_at_ms) = request.freshness_expires_at_ms {
            item.freshness_expires_at_ms = Some(freshness_expires_at_ms);
        }
        item.metadata = Some(serde_json::Value::Object(metadata_obj));
        item.updated_at_ms = request.promoted_at_ms;
        let persisted_item = item.clone();
        let item_id = persisted_item.id.clone();
        let space_id = persisted_item.space_id.clone();
        let coverage_key = persisted_item.coverage_key.clone();
        let dedupe_key = persisted_item.dedupe_key.clone();

        tx.execute(
            "INSERT OR REPLACE INTO knowledge_items
             (id, space_id, coverage_key, dedupe_key, item_type, title, summary, payload, trust_level, status, run_id, artifact_refs, source_memory_ids, freshness_expires_at_ms, metadata, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                persisted_item.id,
                persisted_item.space_id,
                persisted_item.coverage_key,
                persisted_item.dedupe_key,
                persisted_item.item_type,
                persisted_item.title,
                persisted_item.summary,
                persisted_item.payload.to_string(),
                persisted_item.trust_level.to_string(),
                persisted_item.status.to_string(),
                persisted_item.run_id,
                serde_json::to_string(&persisted_item.artifact_refs)?,
                serde_json::to_string(&persisted_item.source_memory_ids)?,
                persisted_item.freshness_expires_at_ms.map(|value| value as i64),
                persisted_item.metadata.as_ref().map(|value| value.to_string()),
                persisted_item.created_at_ms as i64,
                persisted_item.updated_at_ms as i64,
            ],
        )?;

        let mut coverage = tx
            .query_row(
                "SELECT coverage_key, space_id, latest_item_id, latest_dedupe_key, last_seen_at_ms, last_promoted_at_ms, freshness_expires_at_ms, metadata
                 FROM knowledge_coverage WHERE coverage_key = ?1 AND space_id = ?2",
                params![coverage_key.as_str(), space_id.as_str()],
                row_to_knowledge_coverage,
            )
            .optional()?
            .unwrap_or(KnowledgeCoverageRecord {
                coverage_key: coverage_key.clone(),
                space_id: space_id.clone(),
                latest_item_id: None,
                latest_dedupe_key: None,
                last_seen_at_ms: request.promoted_at_ms,
                last_promoted_at_ms: None,
                freshness_expires_at_ms: None,
                metadata: None,
            });
        coverage.latest_item_id = Some(item_id.clone());
        coverage.latest_dedupe_key = Some(dedupe_key.clone());
        coverage.last_seen_at_ms = request.promoted_at_ms;
        if next_status.is_active() {
            coverage.last_promoted_at_ms = Some(request.promoted_at_ms);
        }
        if let Some(freshness_expires_at_ms) = request.freshness_expires_at_ms {
            coverage.freshness_expires_at_ms = Some(freshness_expires_at_ms);
        }
        let mut coverage_metadata = coverage
            .metadata
            .clone()
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        coverage_metadata.insert(
            "promotion".to_string(),
            serde_json::json!({
                "item_id": item_id,
                "from_status": previous_status.to_string(),
                "to_status": next_status.to_string(),
                "promoted_at_ms": request.promoted_at_ms,
                "reason": request.reason,
                "reviewer_id": request.reviewer_id,
                "approval_id": request.approval_id,
            }),
        );
        coverage.metadata = Some(serde_json::Value::Object(coverage_metadata));

        tx.execute(
            "INSERT OR REPLACE INTO knowledge_coverage
             (coverage_key, space_id, latest_item_id, latest_dedupe_key, last_seen_at_ms, last_promoted_at_ms, freshness_expires_at_ms, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                coverage.coverage_key,
                coverage.space_id,
                coverage.latest_item_id,
                coverage.latest_dedupe_key,
                coverage.last_seen_at_ms as i64,
                coverage.last_promoted_at_ms.map(|value| value as i64),
                coverage.freshness_expires_at_ms.map(|value| value as i64),
                coverage.metadata.as_ref().map(|value| value.to_string()),
            ],
        )?;

        tx.commit()?;
        Ok(Some(KnowledgePromotionResult {
            previous_status,
            previous_trust_level,
            promoted,
            item: persisted_item,
            coverage,
        }))
    }

    /// Insert or update a coverage record for a reusable knowledge key.
    pub async fn upsert_knowledge_coverage(
        &self,
        coverage: &KnowledgeCoverageRecord,
    ) -> MemoryResult<()> {
        self.upsert_knowledge_coverage_for_tenant(coverage, &MemoryTenantScope::local())
            .await
    }

    pub async fn upsert_knowledge_coverage_for_tenant(
        &self,
        coverage: &KnowledgeCoverageRecord,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<()> {
        if self
            .get_knowledge_space_for_tenant(&coverage.space_id, tenant_scope)
            .await?
            .is_none()
        {
            return Err(crate::types::MemoryError::InvalidConfig(
                "knowledge coverage space not found for tenant".to_string(),
            ));
        }

        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT OR REPLACE INTO knowledge_coverage
             (coverage_key, space_id, latest_item_id, latest_dedupe_key, last_seen_at_ms, last_promoted_at_ms, freshness_expires_at_ms, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                coverage.coverage_key,
                coverage.space_id,
                coverage.latest_item_id,
                coverage.latest_dedupe_key,
                coverage.last_seen_at_ms as i64,
                coverage.last_promoted_at_ms.map(|value| value as i64),
                coverage.freshness_expires_at_ms.map(|value| value as i64),
                coverage.metadata.as_ref().map(|value| value.to_string()),
            ],
        )?;
        Ok(())
    }

    /// Fetch a coverage row for a key and space.
    pub async fn get_knowledge_coverage(
        &self,
        coverage_key: &str,
        space_id: &str,
    ) -> MemoryResult<Option<KnowledgeCoverageRecord>> {
        self.get_knowledge_coverage_for_tenant(coverage_key, space_id, &MemoryTenantScope::local())
            .await
    }

    pub async fn get_knowledge_coverage_for_tenant(
        &self,
        coverage_key: &str,
        space_id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<KnowledgeCoverageRecord>> {
        let conn = self.conn.lock().await;
        Ok(
            conn.query_row(
                "SELECT c.coverage_key, c.space_id, c.latest_item_id, c.latest_dedupe_key, c.last_seen_at_ms, c.last_promoted_at_ms, c.freshness_expires_at_ms, c.metadata
                 FROM knowledge_coverage c
                 JOIN knowledge_spaces s ON s.id = c.space_id
                 WHERE c.coverage_key = ?1 AND c.space_id = ?2
                   AND s.tenant_org_id = ?3
                   AND s.tenant_workspace_id = ?4
                   AND IFNULL(s.tenant_deployment_id, '') = IFNULL(?5, '')",
                params![
                    coverage_key,
                    space_id,
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref()
                ],
                row_to_knowledge_coverage,
            )
            .optional()?,
        )
    }

    /// Get memory statistics
    pub async fn get_stats(&self) -> MemoryResult<MemoryStats> {
        let mut stats = self
            .get_stats_for_tenant(&MemoryTenantScope::local())
            .await?;
        stats.file_size = std::fs::metadata(&self.db_path)?.len() as i64;
        Ok(stats)
    }

    /// Get memory statistics scoped to a single tenant partition.
    pub async fn get_stats_for_tenant(
        &self,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<MemoryStats> {
        let conn = self.conn.lock().await;

        let session_chunks: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_memory_chunks
             WHERE tenant_org_id = ?1
               AND tenant_workspace_id = ?2
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')",
            params![
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref()
            ],
            |row| row.get(0),
        )?;

        let project_chunks: i64 = conn.query_row(
            "SELECT COUNT(*) FROM project_memory_chunks
             WHERE tenant_org_id = ?1
               AND tenant_workspace_id = ?2
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')",
            params![
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref()
            ],
            |row| row.get(0),
        )?;

        let global_chunks: i64 = conn.query_row(
            "SELECT COUNT(*) FROM global_memory_chunks
             WHERE tenant_org_id = ?1
               AND tenant_workspace_id = ?2
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')",
            params![
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref()
            ],
            |row| row.get(0),
        )?;

        let session_bytes: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM session_memory_chunks
             WHERE tenant_org_id = ?1
               AND tenant_workspace_id = ?2
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')",
            params![
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref()
            ],
            |row| row.get(0),
        )?;

        let project_bytes: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM project_memory_chunks
             WHERE tenant_org_id = ?1
               AND tenant_workspace_id = ?2
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')",
            params![
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref()
            ],
            |row| row.get(0),
        )?;

        let global_bytes: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM global_memory_chunks
             WHERE tenant_org_id = ?1
               AND tenant_workspace_id = ?2
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')",
            params![
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref()
            ],
            |row| row.get(0),
        )?;

        let last_cleanup: Option<String> = conn
            .query_row(
                "SELECT created_at FROM memory_cleanup_log
                 WHERE tenant_org_id = ?1
                   AND tenant_workspace_id = ?2
                   AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')
                 ORDER BY created_at DESC
                 LIMIT 1",
                params![
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref()
                ],
                |row| row.get(0),
            )
            .optional()?;

        let last_cleanup = last_cleanup.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        });

        let total_bytes = session_bytes + project_bytes + global_bytes;
        Ok(MemoryStats {
            total_chunks: session_chunks + project_chunks + global_chunks,
            session_chunks,
            project_chunks,
            global_chunks,
            total_bytes,
            session_bytes,
            project_bytes,
            global_bytes,
            file_size: total_bytes,
            last_cleanup,
        })
    }

    /// Log cleanup operation
    pub async fn log_cleanup(
        &self,
        cleanup_type: &str,
        tier: MemoryTier,
        project_id: Option<&str>,
        session_id: Option<&str>,
        chunks_deleted: i64,
        bytes_reclaimed: i64,
    ) -> MemoryResult<()> {
        self.log_cleanup_for_tenant(
            cleanup_type,
            tier,
            project_id,
            session_id,
            chunks_deleted,
            bytes_reclaimed,
            &MemoryTenantScope::local(),
        )
        .await
    }

    pub async fn log_cleanup_for_tenant(
        &self,
        cleanup_type: &str,
        tier: MemoryTier,
        project_id: Option<&str>,
        session_id: Option<&str>,
        chunks_deleted: i64,
        bytes_reclaimed: i64,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<()> {
        let conn = self.conn.lock().await;

        let id = uuid::Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO memory_cleanup_log 
             (id, cleanup_type, tier, project_id, session_id, chunks_deleted, bytes_reclaimed, created_at,
              tenant_org_id, tenant_workspace_id, tenant_deployment_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                cleanup_type,
                tier.to_string(),
                project_id,
                session_id,
                chunks_deleted,
                bytes_reclaimed,
                created_at,
                tenant_scope.org_id.as_str(),
                tenant_scope.workspace_id.as_str(),
                tenant_scope.deployment_id.as_deref()
            ],
        )?;

        Ok(())
    }

    /// Vacuum the database to reclaim space
    pub async fn vacuum(&self) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        conn.execute("VACUUM", [])?;
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Project file indexing helpers
    // ---------------------------------------------------------------------

    pub async fn project_file_index_count(&self, project_id: &str) -> MemoryResult<i64> {
        self.project_file_index_count_for_tenant(project_id, &MemoryTenantScope::local())
            .await
    }

    pub async fn project_file_index_count_for_tenant(
        &self,
        project_id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<i64> {
        let conn = self.conn.lock().await;
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM project_file_index
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
        Ok(n)
    }

    pub async fn project_has_file_chunks(&self, project_id: &str) -> MemoryResult<bool> {
        self.project_has_file_chunks_for_tenant(project_id, &MemoryTenantScope::local())
            .await
    }

    pub async fn project_has_file_chunks_for_tenant(
        &self,
        project_id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<bool> {
        let conn = self.conn.lock().await;
        let exists: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM project_memory_chunks
                 WHERE project_id = ?1 AND source = 'file'
                   AND tenant_org_id = ?2
                   AND tenant_workspace_id = ?3
                   AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')
                 LIMIT 1",
                params![
                    project_id,
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref()
                ],
                |row| row.get(0),
            )
            .optional()?;
        Ok(exists.is_some())
    }

    pub async fn get_file_index_entry(
        &self,
        project_id: &str,
        path: &str,
    ) -> MemoryResult<Option<(i64, i64, String)>> {
        self.get_file_index_entry_for_tenant(project_id, path, &MemoryTenantScope::local())
            .await
    }

    pub async fn get_file_index_entry_for_tenant(
        &self,
        project_id: &str,
        path: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<(i64, i64, String)>> {
        let conn = self.conn.lock().await;
        let row: Option<(i64, i64, String)> = conn
            .query_row(
                "SELECT mtime, size, hash FROM project_file_index
                 WHERE project_id = ?1 AND path = ?2
                   AND tenant_org_id = ?3
                   AND tenant_workspace_id = ?4
                   AND IFNULL(tenant_deployment_id, '') = IFNULL(?5, '')",
                params![
                    project_id,
                    path,
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref()
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        Ok(row)
    }
}
