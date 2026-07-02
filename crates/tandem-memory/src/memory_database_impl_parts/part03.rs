impl MemoryDatabase {
    pub async fn update_file_chunks_metadata_by_path_for_tenant(
        &self,
        tier: MemoryTier,
        session_id: Option<&str>,
        project_id: Option<&str>,
        source_path: &str,
        tenant_scope: &MemoryTenantScope,
        metadata: &serde_json::Value,
    ) -> MemoryResult<usize> {
        self.deny_local_scope_in_strict_mode("memory metadata backfill", tenant_scope)?;
        let metadata_plain = metadata.to_string();
        let metadata_stored = if metadata_plain.is_empty() {
            String::new()
        } else {
            self.crypto.encrypt_field(&metadata_plain)?
        };
        let conn = self.conn.lock().await;
        let changed = match tier {
            MemoryTier::Session => {
                let session_id = require_scope_id(tier, session_id)?;
                conn.execute(
                    "UPDATE session_memory_chunks
                     SET metadata = ?1
                     WHERE session_id = ?2 AND source = 'file' AND source_path = ?3
                       AND tenant_org_id = ?4
                       AND tenant_workspace_id = ?5
                       AND IFNULL(tenant_deployment_id, '') = IFNULL(?6, '')",
                    params![
                        metadata_stored,
                        session_id,
                        source_path,
                        tenant_scope.org_id.as_str(),
                        tenant_scope.workspace_id.as_str(),
                        tenant_scope.deployment_id.as_deref()
                    ],
                )?
            }
            MemoryTier::Project => {
                let project_id = require_scope_id(tier, project_id)?;
                conn.execute(
                    "UPDATE project_memory_chunks
                     SET metadata = ?1
                     WHERE project_id = ?2 AND source = 'file' AND source_path = ?3
                       AND tenant_org_id = ?4
                       AND tenant_workspace_id = ?5
                       AND IFNULL(tenant_deployment_id, '') = IFNULL(?6, '')",
                    params![
                        metadata_stored,
                        project_id,
                        source_path,
                        tenant_scope.org_id.as_str(),
                        tenant_scope.workspace_id.as_str(),
                        tenant_scope.deployment_id.as_deref()
                    ],
                )?
            }
            MemoryTier::Global => conn.execute(
                "UPDATE global_memory_chunks
                 SET metadata = ?1
                 WHERE source = 'file' AND source_path = ?2
                   AND tenant_org_id = ?3
                   AND tenant_workspace_id = ?4
                   AND IFNULL(tenant_deployment_id, '') = IFNULL(?5, '')",
                params![
                    metadata_stored,
                    source_path,
                    tenant_scope.org_id.as_str(),
                    tenant_scope.workspace_id.as_str(),
                    tenant_scope.deployment_id.as_deref()
                ],
            )?,
        };
        Ok(changed)
    }
}
