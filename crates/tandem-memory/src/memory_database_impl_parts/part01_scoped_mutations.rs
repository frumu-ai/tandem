impl MemoryDatabase {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn delete_chunk_for_tenant_scoped(
        &self,
        tier: MemoryTier,
        chunk_id: &str,
        project_id: Option<&str>,
        session_id: Option<&str>,
        tenant_scope: &MemoryTenantScope,
        caller_subject: Option<&str>,
        owner_org_unit_id: Option<&str>,
    ) -> MemoryResult<u64> {
        let (chunk_table, vector_table, selector_predicate, selector_value) = match tier {
            MemoryTier::Session => (
                "session_memory_chunks",
                "session_memory_vectors",
                "session_id = ?7",
                Some(session_id.ok_or_else(|| {
                    MemoryError::InvalidConfig(
                        "session_id is required to delete session memory chunks".to_string(),
                    )
                })?),
            ),
            MemoryTier::Project => (
                "project_memory_chunks",
                "project_memory_vectors",
                "project_id = ?7",
                Some(project_id.ok_or_else(|| {
                    MemoryError::InvalidConfig(
                        "project_id is required to delete project memory chunks".to_string(),
                    )
                })?),
            ),
            MemoryTier::Global => (
                "global_memory_chunks",
                "global_memory_vectors",
                "?7 IS NULL",
                None,
            ),
        };

        let predicate = format!(
            "id = ?1
             AND tenant_org_id = ?2
             AND tenant_workspace_id = ?3
             AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')
             AND (private = 0 OR owner_subject = ?5)
             AND (?6 IS NULL OR owner_org_unit_id = ?6 OR tenant_shared = 1)
             AND {selector_predicate}"
        );
        let vector_sql = format!(
            "DELETE FROM {vector_table} WHERE chunk_id IN
             (SELECT id FROM {chunk_table} WHERE {predicate})"
        );
        let chunk_sql = format!("DELETE FROM {chunk_table} WHERE {predicate}");
        let mut conn = self.conn.lock().await;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            &vector_sql,
            params![
                chunk_id,
                tenant_scope.org_id,
                tenant_scope.workspace_id,
                tenant_scope.deployment_id,
                caller_subject,
                owner_org_unit_id,
                selector_value,
            ],
        )?;
        let deleted = tx.execute(
            &chunk_sql,
            params![
                chunk_id,
                tenant_scope.org_id,
                tenant_scope.workspace_id,
                tenant_scope.deployment_id,
                caller_subject,
                owner_org_unit_id,
                selector_value,
            ],
        )?;
        tx.commit()?;
        Ok(deleted as u64)
    }
}
