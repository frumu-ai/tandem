impl MemoryDatabase {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn get_chunks_for_tenant_scoped(
        &self,
        tier: MemoryTier,
        project_id: Option<&str>,
        session_id: Option<&str>,
        tenant_scope: &MemoryTenantScope,
        caller_subject: Option<&str>,
        owner_org_unit_id: Option<&str>,
        limit: i64,
    ) -> MemoryResult<Vec<MemoryChunk>> {
        let conn = self.conn.lock().await;
        let limit = limit.max(0);

        match tier {
            MemoryTier::Session => {
                let session_id = session_id.ok_or_else(|| {
                    MemoryError::InvalidConfig(
                        "session chunk reads require a session_id".to_string(),
                    )
                })?;
                let mut stmt = conn.prepare(
                    "SELECT id, content, session_id, project_id, source, created_at, token_count, metadata,
                            source_path, source_mtime, source_size, source_hash,
                            tenant_org_id, tenant_workspace_id, tenant_deployment_id, subject, crypto_envelope
                     FROM session_memory_chunks
                     WHERE session_id = ?1
                       AND tenant_org_id = ?2
                       AND tenant_workspace_id = ?3
                       AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')
                       AND (private = 0 OR owner_subject = ?5)
                       AND (?6 IS NULL OR owner_org_unit_id = ?6 OR tenant_shared = 1)
                     ORDER BY created_at DESC
                     LIMIT ?7",
                )?;
                let chunks = stmt
                    .query_map(
                    params![
                        session_id,
                        tenant_scope.org_id,
                        tenant_scope.workspace_id,
                        tenant_scope.deployment_id,
                        caller_subject,
                        owner_org_unit_id,
                        limit,
                    ],
                    |row| row_to_chunk(row, tier, &self.crypto),
                    )?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(MemoryError::from)?;
                Ok(chunks)
            }
            MemoryTier::Project => {
                let project_id = project_id.ok_or_else(|| {
                    MemoryError::InvalidConfig(
                        "project chunk reads require a project_id".to_string(),
                    )
                })?;
                let mut stmt = conn.prepare(
                    "SELECT id, content, session_id, project_id, source, created_at, token_count, metadata,
                            source_path, source_mtime, source_size, source_hash,
                            tenant_org_id, tenant_workspace_id, tenant_deployment_id, subject, crypto_envelope
                     FROM project_memory_chunks
                     WHERE project_id = ?1
                       AND tenant_org_id = ?2
                       AND tenant_workspace_id = ?3
                       AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')
                       AND (private = 0 OR owner_subject = ?5)
                       AND (?6 IS NULL OR owner_org_unit_id = ?6 OR tenant_shared = 1)
                     ORDER BY created_at DESC
                     LIMIT ?7",
                )?;
                let chunks = stmt
                    .query_map(
                    params![
                        project_id,
                        tenant_scope.org_id,
                        tenant_scope.workspace_id,
                        tenant_scope.deployment_id,
                        caller_subject,
                        owner_org_unit_id,
                        limit,
                    ],
                    |row| row_to_chunk(row, tier, &self.crypto),
                    )?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(MemoryError::from)?;
                Ok(chunks)
            }
            MemoryTier::Global => {
                let mut stmt = conn.prepare(
                    "SELECT id, content, source, created_at, token_count, metadata,
                            source_path, source_mtime, source_size, source_hash,
                            tenant_org_id, tenant_workspace_id, tenant_deployment_id, subject, crypto_envelope
                     FROM global_memory_chunks
                     WHERE tenant_org_id = ?1
                       AND tenant_workspace_id = ?2
                       AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')
                       AND (private = 0 OR owner_subject = ?4)
                       AND (?5 IS NULL OR owner_org_unit_id = ?5 OR tenant_shared = 1)
                     ORDER BY created_at DESC
                     LIMIT ?6",
                )?;
                let chunks = stmt
                    .query_map(
                    params![
                        tenant_scope.org_id,
                        tenant_scope.workspace_id,
                        tenant_scope.deployment_id,
                        caller_subject,
                        owner_org_unit_id,
                        limit,
                    ],
                    |row| row_to_chunk(row, tier, &self.crypto),
                    )?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(MemoryError::from)?;
                Ok(chunks)
            }
        }
    }
}
