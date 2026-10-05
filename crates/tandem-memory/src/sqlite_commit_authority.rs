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
        authority: MemoryCommitAuthority,
    ) -> MemoryStoreResult<bool> {
        let mut conn = self.conn.lock().await;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(MemoryError::from)
            .map_err(MemoryStoreError::from)?;
        authority()?;
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
