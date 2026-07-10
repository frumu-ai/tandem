use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::store::{
    tenant_scope_from_global_record, MemoryStoreBatchItemResult, MemoryStoreBatchOperation,
    MemoryStoreBatchResult, MemoryStoreBatchValue, MemoryStoreError, MemoryStoreErrorKind,
    MemoryStoreMutationRequest, MemoryStoreMutationResult, MemoryStoreResult,
    MemoryStoreWriteRequest, MemoryStoreWriteResult, MemoryWriteScope,
};
use crate::types::{
    owner_org_unit_id_from_metadata, owner_subject_from_metadata, GlobalMemoryRecord,
    GlobalMemoryWriteResult, MemoryError, MemoryTenantScope,
};

use super::{global_memory_record_tenant_scope, MemoryDatabase};

impl MemoryDatabase {
    pub(crate) fn enforce_store_tenant_scope(
        &self,
        operation: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryStoreResult<()> {
        self.deny_local_scope_in_strict_mode(operation, tenant_scope)
            .map_err(MemoryStoreError::from)
    }

    /// Execute the SQLite adapter's deliberately narrow atomic subset while one
    /// connection guard and one transaction remain alive for the entire batch.
    pub(crate) async fn execute_atomic_store_batch(
        &self,
        operations: Vec<MemoryStoreBatchOperation>,
    ) -> MemoryStoreResult<MemoryStoreBatchResult> {
        self.preflight_atomic_operations(&operations)?;
        if operations.is_empty() {
            return Ok(MemoryStoreBatchResult {
                completed: true,
                items: Vec::new(),
            });
        }

        let mut conn = self.conn.lock().await;
        let transaction = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_database_error)?;
        let mut items = Vec::with_capacity(operations.len());

        for (index, operation) in operations.into_iter().enumerate() {
            let value = match execute_atomic_operation(&transaction, operation) {
                Ok(value) => value,
                Err(error) => {
                    return match transaction.rollback() {
                        Ok(()) => Err(error),
                        Err(rollback_error) => Err(store_database_error(rollback_error)),
                    };
                }
            };
            items.push(MemoryStoreBatchItemResult {
                index,
                result: Ok(value),
            });
        }

        transaction.commit().map_err(store_database_error)?;
        Ok(MemoryStoreBatchResult {
            completed: true,
            items,
        })
    }

    fn preflight_atomic_operations(
        &self,
        operations: &[MemoryStoreBatchOperation],
    ) -> MemoryStoreResult<()> {
        for operation in operations {
            let supported = matches!(
                operation,
                MemoryStoreBatchOperation::Write(MemoryStoreWriteRequest::GlobalRecord { .. })
                    | MemoryStoreBatchOperation::Mutation(
                        MemoryStoreMutationRequest::DeleteGlobalRecord { .. }
                            | MemoryStoreMutationRequest::UpdateGlobalRecordContext { .. }
                    )
            );
            if !supported {
                return Err(MemoryStoreError::unsupported(
                    "SQLite atomic batches support global-record writes, scoped context updates, and scoped deletes only",
                ));
            }
        }

        for operation in operations {
            match operation {
                MemoryStoreBatchOperation::Write(MemoryStoreWriteRequest::GlobalRecord {
                    scope,
                    record,
                }) => {
                    self.enforce_store_tenant_scope("atomic global memory write", &scope.tenant)?;
                    validate_global_write_scope(scope, record)?;
                }
                MemoryStoreBatchOperation::Mutation(
                    MemoryStoreMutationRequest::DeleteGlobalRecord { scope, .. },
                ) => {
                    self.enforce_store_tenant_scope("atomic global memory delete", &scope.tenant)?
                }
                MemoryStoreBatchOperation::Mutation(
                    MemoryStoreMutationRequest::UpdateGlobalRecordContext { scope, .. },
                ) => self.enforce_store_tenant_scope(
                    "atomic global memory context update",
                    &scope.tenant,
                )?,
                _ => unreachable!("unsupported atomic operation passed support preflight"),
            }
        }
        Ok(())
    }
}

fn validate_global_write_scope(
    scope: &MemoryWriteScope,
    record: &GlobalMemoryRecord,
) -> MemoryStoreResult<()> {
    if scope.tenant != tenant_scope_from_global_record(record) {
        return Err(MemoryStoreError::new(
            MemoryStoreErrorKind::ScopeViolation,
            "global record tenant context does not match the write request scope",
        ));
    }
    if scope.org_unit != owner_org_unit_id_from_metadata(record.metadata.as_ref()) {
        return Err(MemoryStoreError::new(
            MemoryStoreErrorKind::ScopeViolation,
            "global record owner_org_unit_id metadata does not match the write request scope",
        ));
    }
    if scope.subject != owner_subject_from_metadata(record.metadata.as_ref()) {
        return Err(MemoryStoreError::new(
            MemoryStoreErrorKind::ScopeViolation,
            "global record owner_subject metadata does not match the write request scope",
        ));
    }
    Ok(())
}

fn execute_atomic_operation(
    conn: &Connection,
    operation: MemoryStoreBatchOperation,
) -> MemoryStoreResult<MemoryStoreBatchValue> {
    match operation {
        MemoryStoreBatchOperation::Write(MemoryStoreWriteRequest::GlobalRecord {
            record, ..
        }) => put_global_record(conn, &record).map(|result| {
            MemoryStoreBatchValue::Write(MemoryStoreWriteResult::GlobalRecord(result))
        }),
        MemoryStoreBatchOperation::Mutation(MemoryStoreMutationRequest::DeleteGlobalRecord {
            scope,
            id,
        }) => delete_global_record(conn, &scope, &id).map(|changed| {
            MemoryStoreBatchValue::Mutation(MemoryStoreMutationResult::Changed(changed))
        }),
        MemoryStoreBatchOperation::Mutation(
            MemoryStoreMutationRequest::UpdateGlobalRecordContext {
                scope,
                id,
                visibility,
                demoted,
                metadata,
                provenance,
            },
        ) => update_global_record_context(
            conn,
            &scope,
            &id,
            &visibility,
            demoted,
            metadata.as_ref(),
            provenance.as_ref(),
        )
        .map(|changed| {
            MemoryStoreBatchValue::Mutation(MemoryStoreMutationResult::Changed(changed))
        }),
        _ => unreachable!("unsupported atomic operation passed preflight"),
    }
}

fn put_global_record(
    conn: &Connection,
    record: &GlobalMemoryRecord,
) -> MemoryStoreResult<GlobalMemoryWriteResult> {
    let (tenant_org_id, tenant_workspace_id, tenant_deployment_id) =
        global_memory_record_tenant_scope(record);
    let owner_org_unit_id = owner_org_unit_id_from_metadata(record.metadata.as_ref());
    let owner_subject = owner_subject_from_metadata(record.metadata.as_ref());
    let private = owner_subject.is_some();

    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM memory_records
             WHERE tenant_org_id = ?1
               AND tenant_workspace_id = ?2
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?3, '')
               AND user_id = ?4
               AND source_type = ?5
               AND content_hash = ?6
               AND run_id = ?7
               AND IFNULL(session_id, '') = IFNULL(?8, '')
               AND IFNULL(message_id, '') = IFNULL(?9, '')
               AND IFNULL(tool_name, '') = IFNULL(?10, '')
               AND IFNULL(owner_org_unit_id, '') = IFNULL(?11, '')
               AND private = ?12
               AND IFNULL(owner_subject, '') = IFNULL(?13, '')
             LIMIT 1",
            params![
                tenant_org_id,
                tenant_workspace_id,
                tenant_deployment_id,
                record.user_id,
                record.source_type,
                record.content_hash,
                record.run_id,
                record.session_id,
                record.message_id,
                record.tool_name,
                owner_org_unit_id,
                i64::from(private),
                owner_subject.as_deref(),
            ],
            |row| row.get(0),
        )
        .optional()
        .map_err(store_database_error)?;

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
            id, tenant_org_id, tenant_workspace_id, tenant_deployment_id,
            user_id, source_type, content, content_hash, run_id, session_id, message_id, tool_name,
            project_tag, channel_tag, host_tag, metadata, provenance, redaction_status, redaction_count,
            visibility, demoted, score_boost, created_at_ms, updated_at_ms, expires_at_ms, owner_org_unit_id,
            private, owner_subject
        ) VALUES (
            ?1, ?2, ?3, ?4,
            ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
            ?13, ?14, ?15, ?16, ?17, ?18, ?19,
            ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28
        )",
        params![
            record.id,
            tenant_org_id,
            tenant_workspace_id,
            tenant_deployment_id,
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
            i64::from(record.demoted),
            record.score_boost,
            record.created_at_ms as i64,
            record.updated_at_ms as i64,
            record.expires_at_ms.map(|value| value as i64),
            owner_org_unit_id,
            i64::from(private),
            owner_subject,
        ],
    )
    .map_err(store_database_error)?;

    Ok(GlobalMemoryWriteResult {
        id: record.id.clone(),
        stored: true,
        deduped: false,
    })
}

#[allow(clippy::too_many_arguments)]
fn update_global_record_context(
    conn: &Connection,
    scope: &crate::store::MemoryReadScope,
    id: &str,
    visibility: &str,
    demoted: bool,
    metadata: Option<&serde_json::Value>,
    provenance: Option<&serde_json::Value>,
) -> MemoryStoreResult<bool> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let next_owner_org_unit_id = owner_org_unit_id_from_metadata(metadata);
    let next_owner_subject = owner_subject_from_metadata(metadata);
    let next_private = next_owner_subject.is_some();
    let metadata = metadata.map(ToString::to_string).unwrap_or_default();
    let provenance = provenance.map(ToString::to_string).unwrap_or_default();
    let changed = conn
        .execute(
            "UPDATE memory_records
             SET visibility = ?7, demoted = ?8, metadata = ?9, provenance = ?10,
                 updated_at_ms = ?11, owner_org_unit_id = ?12, private = ?13,
                 owner_subject = ?14
             WHERE id = ?1
               AND tenant_org_id = ?2
               AND tenant_workspace_id = ?3
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')
               AND (?5 IS NULL OR owner_org_unit_id = ?5)
               AND (private = 0 OR owner_subject = ?6)",
            params![
                id,
                scope.tenant.org_id,
                scope.tenant.workspace_id,
                scope.tenant.deployment_id,
                scope.org_unit,
                scope.subject,
                visibility,
                i64::from(demoted),
                metadata,
                provenance,
                now_ms,
                next_owner_org_unit_id,
                i64::from(next_private),
                next_owner_subject,
            ],
        )
        .map_err(store_database_error)?;
    Ok(changed > 0)
}

fn delete_global_record(
    conn: &Connection,
    scope: &crate::store::MemoryReadScope,
    id: &str,
) -> MemoryStoreResult<bool> {
    let changed = conn
        .execute(
            "DELETE FROM memory_records
             WHERE id = ?1
               AND tenant_org_id = ?2
               AND tenant_workspace_id = ?3
               AND IFNULL(tenant_deployment_id, '') = IFNULL(?4, '')
               AND (?5 IS NULL OR owner_org_unit_id = ?5)
               AND (private = 0 OR owner_subject = ?6)",
            params![
                id,
                scope.tenant.org_id,
                scope.tenant.workspace_id,
                scope.tenant.deployment_id,
                scope.org_unit,
                scope.subject,
            ],
        )
        .map_err(store_database_error)?;
    Ok(changed > 0)
}

fn store_database_error(error: rusqlite::Error) -> MemoryStoreError {
    MemoryStoreError::from(MemoryError::from(error))
}
