use super::*;
use crate::types::{
    memory_key_scope_from_metadata, owner_org_unit_id_from_metadata, owner_subject_from_metadata,
    tenant_shared_from_metadata, GlobalMemoryRecord, GlobalMemoryWriteResult,
};

fn deployment(tenant: &crate::types::MemoryTenantScope) -> &str {
    tenant.deployment_id.as_deref().unwrap_or("")
}

impl PostgresMemoryStore {
    pub(super) async fn guarded_write_impl(
        &self,
        request: MemoryStoreWriteRequest,
        authority: MemoryCommitAuthority,
    ) -> MemoryStoreResult<MemoryStoreWriteResult> {
        let MemoryStoreWriteRequest::GlobalRecord { scope, record } = request else {
            return Err(MemoryStoreError::unsupported(
                "PostgreSQL guarded write supports GlobalRecord only",
            ));
        };
        let tenant = tenant_scope_from_global_record(&record);
        let owner_org = owner_org_unit_id_from_metadata(record.metadata.as_ref());
        let owner_subject = owner_subject_from_metadata(record.metadata.as_ref());
        if tenant != scope.tenant || owner_org != scope.org_unit || owner_subject != scope.subject {
            return Err(MemoryStoreError::new(
                MemoryStoreErrorKind::ScopeViolation,
                "global record ownership does not match the PostgreSQL write scope",
            ));
        }
        let lineage_digest = crate::derived_lineage_dedupe_digest(record.metadata.as_ref())
            .map_err(MemoryStoreError::from)?;
        let mut client = self.client().await?;
        let tx = client
            .transaction()
            .await
            .map_err(|error| store_error("begin guarded PostgreSQL write", error, true))?;
        authority()?;
        let key_scope = memory_key_scope_from_metadata(&tenant, record.metadata.as_ref())
            .with_owner_subject(owner_subject.clone());
        let (data_class, source_binding_id) = Self::key_scope_columns(&key_scope)?;
        let (data, cipher, envelope, policy, audit) =
            self.encode_payload(&record, &key_scope, &record.id)?;
        let search_content =
            if self.search_surface_mode == PostgresSearchSurfaceMode::PlaintextPgvector {
                record.content.as_str()
            } else {
                ""
            };
        let inserted = tx.query_opt(
            "INSERT INTO tandem_memory_global_records
             (id,tenant_org_id,tenant_workspace_id,tenant_deployment_id,owner_org_unit_id,
              owner_subject,private,data_class,source_binding_id,user_id,source_type,content_hash,run_id,session_id,message_id,
              tool_name,project_tag,channel_tag,demoted,expires_at_ms,created_at_ms,search_content,
              data,data_ciphertext,data_envelope,data_policy_decision_id,data_audit_id,tenant_shared,derived_lineage_digest)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29)
             ON CONFLICT (tenant_org_id,tenant_workspace_id,tenant_deployment_id,user_id,
               source_type,content_hash,run_id,(COALESCE(session_id,'')),(COALESCE(message_id,'')),(COALESCE(tool_name,'')),
               (COALESCE(owner_org_unit_id,'')),private,(COALESCE(owner_subject,'')),data_class,
               (COALESCE(source_binding_id,'')),tenant_shared,derived_lineage_digest)
             DO NOTHING RETURNING id",
            &[&record.id,&tenant.org_id,&tenant.workspace_id,&deployment(&tenant),&owner_org,&owner_subject,&owner_subject.is_some(),
              &data_class,&source_binding_id,&record.user_id,&record.source_type,&record.content_hash,&record.run_id,&record.session_id,
              &record.message_id,&record.tool_name,&record.project_tag,&record.channel_tag,&record.demoted,
              &record.expires_at_ms.map(|value| value as i64),&(record.created_at_ms as i64),&search_content,&data,&cipher,&envelope,
              &policy,&audit,&tenant_shared_from_metadata(record.metadata.as_ref()),&lineage_digest]
        ).await.map_err(|error| store_error("guarded PostgreSQL global insert",error,false))?;
        // INSERT may have waited for an independent transaction's row/index lock.
        authority()?;
        let result = if let Some(row) = inserted {
            GlobalMemoryWriteResult {
                id: row.get(0),
                stored: true,
                deduped: false,
            }
        } else {
            let row = tx.query_one(
                "SELECT id FROM tandem_memory_global_records WHERE tenant_org_id=$1 AND tenant_workspace_id=$2
                   AND tenant_deployment_id=$3 AND user_id=$4 AND source_type=$5 AND content_hash=$6 AND run_id=$7
                   AND COALESCE(session_id,'')=COALESCE($8,'') AND COALESCE(message_id,'')=COALESCE($9,'')
                   AND COALESCE(tool_name,'')=COALESCE($10,'') AND COALESCE(owner_org_unit_id,'')=COALESCE($11,'')
                   AND private=$12 AND COALESCE(owner_subject,'')=COALESCE($13,'') AND data_class=$14
                   AND COALESCE(source_binding_id,'')=COALESCE($15,'') AND tenant_shared=$16 AND derived_lineage_digest=$17 LIMIT 1",
                &[&tenant.org_id,&tenant.workspace_id,&deployment(&tenant),&record.user_id,&record.source_type,&record.content_hash,
                  &record.run_id,&record.session_id,&record.message_id,&record.tool_name,&owner_org,&owner_subject.is_some(),&owner_subject,
                  &data_class,&source_binding_id,&tenant_shared_from_metadata(record.metadata.as_ref()),&lineage_digest]
            ).await.map_err(|error| store_error("guarded PostgreSQL dedupe",error,false))?;
            GlobalMemoryWriteResult {
                id: row.get(0),
                stored: false,
                deduped: true,
            }
        };
        authority()?;
        tx.commit()
            .await
            .map_err(|error| store_error("commit guarded PostgreSQL write", error, false))?;
        Ok(MemoryStoreWriteResult::GlobalRecord(result))
    }

    pub(super) async fn guarded_mutate_impl(
        &self,
        request: MemoryStoreMutationRequest,
        expected: Option<&tandem_types::MemorySourceReference>,
        authority: MemoryCommitAuthority,
    ) -> MemoryStoreResult<MemoryStoreMutationResult> {
        let MemoryStoreMutationRequest::UpdateGlobalRecordContext {
            scope,
            id,
            visibility,
            demoted,
            metadata,
            provenance,
        } = request
        else {
            return Err(MemoryStoreError::unsupported(
                "PostgreSQL guarded mutation supports UpdateGlobalRecordContext only",
            ));
        };
        if expected.is_some_and(|expected| expected.memory_id != id) {
            return Err(MemoryStoreError::new(
                MemoryStoreErrorKind::ScopeViolation,
                "guarded memory target id mismatch",
            ));
        }
        let mut client = self.client().await?;
        let tx = client
            .transaction()
            .await
            .map_err(|error| store_error("begin guarded PostgreSQL mutation", error, true))?;
        authority()?;
        let row = tx.query_opt(
            "SELECT data,data_ciphertext,data_envelope,data_policy_decision_id,data_audit_id,owner_org_unit_id,owner_subject,data_class,source_binding_id
             FROM tandem_memory_global_records WHERE id=$1 AND tenant_org_id=$2 AND tenant_workspace_id=$3 AND tenant_deployment_id=$4
               AND ($5::boolean OR private=false OR owner_subject=$6)
               AND ($7::text IS NULL OR owner_org_unit_id=$7 OR (owner_org_unit_id IS NULL AND tenant_shared=true)) FOR UPDATE",
            &[&id,&scope.tenant.org_id,&scope.tenant.workspace_id,&deployment(&scope.tenant),
              &(scope.access==MemoryReadAccess::TrustedUnrestricted),&scope.subject,&scope.org_unit]
        ).await.map_err(|error| store_error("lock guarded PostgreSQL context row",error,false))?;
        authority()?;
        let Some(row) = row else {
            if let Some(expected) = expected {
                crate::derived_lineage_store::ensure_expected_target(
                    None,
                    &scope.tenant,
                    expected,
                )?;
            }
            authority()?;
            tx.commit()
                .await
                .map_err(|error| store_error("commit guarded PostgreSQL no-op", error, false))?;
            return Ok(MemoryStoreMutationResult::Changed(false));
        };
        let stored_key_scope = Self::persisted_key_scope(
            &scope.tenant,
            row.get(5),
            row.get(6),
            row.get(7),
            row.get(8),
        )?;
        let mut record: GlobalMemoryRecord = self.decode_payload(
            row.get(0),
            row.get(1),
            row.get(2),
            &stored_key_scope,
            row.get(3),
            row.get(4),
        )?;
        if let Some(expected) = expected {
            crate::derived_lineage_store::ensure_expected_target(
                Some(&record),
                &scope.tenant,
                expected,
            )?;
        }
        record.visibility = visibility;
        record.demoted = demoted;
        record.metadata = metadata;
        record.provenance = provenance;
        record.updated_at_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
        let owner_org = owner_org_unit_id_from_metadata(record.metadata.as_ref());
        let owner_subject = owner_subject_from_metadata(record.metadata.as_ref());
        let key_scope = memory_key_scope_from_metadata(&scope.tenant, record.metadata.as_ref())
            .with_owner_subject(owner_subject.clone());
        let (data_class, source_binding_id) = Self::key_scope_columns(&key_scope)?;
        let (data, cipher, envelope, policy, audit) =
            self.encode_payload(&record, &key_scope, &id)?;
        let lineage_digest = crate::derived_lineage_dedupe_digest(record.metadata.as_ref())
            .map_err(MemoryStoreError::from)?;
        tx.execute("UPDATE tandem_memory_global_records SET data=$2,data_ciphertext=$3,data_envelope=$4,data_policy_decision_id=$5,
            data_audit_id=$6,demoted=$7,owner_org_unit_id=$8,owner_subject=$9,private=$10,data_class=$11,source_binding_id=$12,
            tenant_shared=$13,derived_lineage_digest=$14 WHERE id=$1",
            &[&id,&data,&cipher,&envelope,&policy,&audit,&record.demoted,&owner_org,&owner_subject,&owner_subject.is_some(),
              &data_class,&source_binding_id,&tenant_shared_from_metadata(record.metadata.as_ref()),&lineage_digest]
        ).await.map_err(|error| store_error("guarded PostgreSQL context update",error,false))?;
        authority()?;
        tx.commit()
            .await
            .map_err(|error| store_error("commit guarded PostgreSQL mutation", error, false))?;
        Ok(MemoryStoreMutationResult::Changed(true))
    }
}
