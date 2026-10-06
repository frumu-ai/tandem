use super::*;
use crate::store::{
    MemoryCommitAuthority, MemoryReadAccess, MemoryReadScope, MemoryStoreError, MemoryStoreResult,
};

impl MemoryDatabase {
    pub(crate) async fn put_global_record_with_authority(
        &self,
        record: &GlobalMemoryRecord,
        authority: MemoryCommitAuthority,
    ) -> MemoryStoreResult<GlobalMemoryWriteResult> {
        let mut conn = self.conn.lock().await;
        #[cfg(feature = "test-hooks")]
        let _busy_observer = self.install_sqlite_writer_wait_observer(&conn)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(MemoryError::from)
            .map_err(MemoryStoreError::from)?;
        // BEGIN IMMEDIATE has acquired the real independent SQLite writer.
        authority()?;
        let result = self
            .put_global_memory_record_on_connection(&tx, record)
            .map_err(MemoryStoreError::from)?;
        authority()?;
        tx.commit()
            .map_err(MemoryError::from)
            .map_err(MemoryStoreError::from)?;
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn update_global_context_with_authority(
        &self,
        scope: &MemoryReadScope,
        id: &str,
        visibility: &str,
        demoted: bool,
        metadata: Option<&serde_json::Value>,
        provenance: Option<&serde_json::Value>,
        expected: Option<&tandem_types::MemorySourceReference>,
        authority: MemoryCommitAuthority,
    ) -> MemoryStoreResult<bool> {
        if expected.is_some_and(|expected| expected.memory_id != id) {
            return Err(crate::store::MemoryStoreError::new(
                crate::store::MemoryStoreErrorKind::ScopeViolation,
                "guarded memory target id mismatch",
            ));
        }
        let mut conn = self.conn.lock().await;
        #[cfg(feature = "test-hooks")]
        let _busy_observer = self.install_sqlite_writer_wait_observer(&conn)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(MemoryError::from)
            .map_err(MemoryStoreError::from)?;
        authority()?;
        if let Some(expected) = expected {
            let record = tx.query_row(
                "SELECT id,user_id,source_type,content,content_hash,run_id,session_id,message_id,
                    tool_name,project_tag,channel_tag,host_tag,metadata,provenance,redaction_status,
                    redaction_count,visibility,demoted,score_boost,created_at_ms,updated_at_ms,expires_at_ms,
                    content_envelope,metadata_envelope,provenance_envelope,tenant_org_id,tenant_workspace_id,
                    tenant_deployment_id,owner_org_unit_id,owner_subject
                 FROM memory_records WHERE id=?1 AND tenant_org_id=?2 AND tenant_workspace_id=?3
                    AND IFNULL(tenant_deployment_id,'')=IFNULL(?4,'')
                    AND (?7=1 OR ((?5 IS NULL OR owner_org_unit_id=?5 OR (owner_org_unit_id IS NULL AND tenant_shared=1))
                        AND (private=0 OR owner_subject=?6)))",
                params![id,scope.tenant.org_id,scope.tenant.workspace_id,scope.tenant.deployment_id,
                    scope.org_unit,scope.subject,i64::from(scope.access==MemoryReadAccess::TrustedUnrestricted)],
                |row| row_to_global_record(row,&self.crypto),
            ).optional().map_err(MemoryError::from).map_err(MemoryStoreError::from)?;
            crate::derived_lineage_store::ensure_expected_target(
                record.as_ref(),
                &scope.tenant,
                expected,
            )?;
        }
        let result = self
            .update_global_memory_context_on_connection(
                &tx,
                id,
                &scope.tenant.org_id,
                &scope.tenant.workspace_id,
                scope.tenant.deployment_id.as_deref(),
                scope.org_unit.as_deref(),
                scope.subject.as_deref(),
                visibility,
                demoted,
                metadata,
                provenance,
                scope.access == MemoryReadAccess::TrustedUnrestricted,
            )
            .map_err(MemoryStoreError::from)?;
        authority()?;
        tx.commit()
            .map_err(MemoryError::from)
            .map_err(MemoryStoreError::from)?;
        Ok(result)
    }
}

#[cfg(test)]
#[path = "sqlite_commit_authority_tests.rs"]
mod tests;
