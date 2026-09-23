use crate::crypto::MemoryCryptoProvider;
use crate::envelope::{MemoryEnvelopeAuthority, MemoryEnvelopeMetadata, MemoryKeyScope};
use crate::types::{memory_key_scope_from_metadata, owner_subject_from_metadata};

pub(super) struct SealedGlobalRecordFields {
    pub content: String,
    pub metadata: String,
    pub provenance: String,
    pub content_envelope: Option<String>,
    pub metadata_envelope: Option<String>,
    pub provenance_envelope: Option<String>,
}

pub(super) struct SealedGlobalContext {
    pub metadata: String,
    pub provenance: String,
    pub metadata_envelope: Option<String>,
    pub provenance_envelope: Option<String>,
}

pub(super) fn global_record_scope(record: &GlobalMemoryRecord) -> MemoryKeyScope {
    let (org_id, workspace_id, deployment_id) = global_memory_record_tenant_scope(record);
    let tenant = MemoryTenantScope {
        org_id,
        workspace_id,
        deployment_id,
    };
    memory_key_scope_from_metadata(&tenant, record.metadata.as_ref())
        .with_owner_subject(owner_subject_from_metadata(record.metadata.as_ref()))
}

fn authorization_ids(id: &str, field: &str) -> (String, String) {
    (
        format!("global-record/{id}/{field}/policy"),
        format!("global-record/{id}/{field}/audit"),
    )
}

pub(super) fn seal_global_field(
    crypto: &MemoryCryptoProvider,
    id: &str,
    field: &str,
    plaintext: &str,
    scope: &MemoryKeyScope,
) -> MemoryResult<(String, Option<String>)> {
    if plaintext.is_empty() && field != "content" && !crypto.is_hosted() {
        return Ok((String::new(), None));
    }
    let (policy, audit) = authorization_ids(id, field);
    let (stored, envelope) = crypto.encrypt_field_scoped(plaintext, scope, &policy, &audit)?;
    Ok((stored, envelope.map(|value| serde_json::to_string(&value)).transpose()?))
}

pub(super) fn seal_global_record_fields(
    crypto: &MemoryCryptoProvider,
    record: &GlobalMemoryRecord,
) -> MemoryResult<SealedGlobalRecordFields> {
    let scope = global_record_scope(record);
    let metadata = record.metadata.as_ref().map(ToString::to_string).unwrap_or_default();
    let provenance = record.provenance.as_ref().map(ToString::to_string).unwrap_or_default();
    let (content, content_envelope) =
        seal_global_field(crypto, &record.id, "content", &record.content, &scope)?;
    let (metadata, metadata_envelope) =
        seal_global_field(crypto, &record.id, "metadata", &metadata, &scope)?;
    let (provenance, provenance_envelope) =
        seal_global_field(crypto, &record.id, "provenance", &provenance, &scope)?;
    Ok(SealedGlobalRecordFields {
        content,
        metadata,
        provenance,
        content_envelope,
        metadata_envelope,
        provenance_envelope,
    })
}

/// Context rewrites may replace metadata/provenance, but may not silently move
/// a hosted content DEK to a different tenant, department, owner, class or
/// source. A scope-changing rewrite needs an explicit read/reseal migration.
pub(super) fn seal_global_context_update(
    conn: &Connection,
    crypto: &MemoryCryptoProvider,
    id: &str,
    metadata: Option<&serde_json::Value>,
    provenance: Option<&serde_json::Value>,
) -> MemoryResult<Option<SealedGlobalContext>> {
    let existing: Option<(String, String, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT tenant_org_id, tenant_workspace_id, tenant_deployment_id, content_envelope
             FROM memory_records WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((org_id, workspace_id, deployment_id, content_envelope)) = existing else {
        return Ok(None);
    };
    let tenant = MemoryTenantScope { org_id, workspace_id, deployment_id };
    let next_scope = memory_key_scope_from_metadata(&tenant, metadata)
        .with_owner_subject(owner_subject_from_metadata(metadata));
    if crypto.is_hosted() {
        let provenance_scope = provenance
            .and_then(|value| value.get("tenant_context"))
            .and_then(memory_tenant_scope_from_value);
        if provenance_scope
            != Some((tenant.org_id.clone(), tenant.workspace_id.clone(), tenant.deployment_id.clone()))
        {
            return Err(MemoryError::TenantScopeViolation(
                "global record provenance cannot change its hosted tenant".to_string(),
            ));
        }
        let old_envelope: MemoryEnvelopeMetadata = serde_json::from_str(
            content_envelope.as_deref().ok_or_else(|| MemoryError::InvalidConfig(
                "hosted global record context update requires encrypted content".to_string(),
            ))?,
        )?;
        if old_envelope.key_scope != next_scope {
            return Err(MemoryError::TenantScopeViolation(
                "hosted global record scope change requires an authorized reseal".to_string(),
            ));
        }
    }
    let metadata_plain = metadata.map(ToString::to_string).unwrap_or_default();
    let provenance_plain = provenance.map(ToString::to_string).unwrap_or_default();
    let (metadata, metadata_envelope) =
        seal_global_field(crypto, id, "metadata", &metadata_plain, &next_scope)?;
    let (provenance, provenance_envelope) =
        seal_global_field(crypto, id, "provenance", &provenance_plain, &next_scope)?;
    Ok(Some(SealedGlobalContext {
        metadata,
        provenance,
        metadata_envelope,
        provenance_envelope,
    }))
}

pub(super) fn open_global_field(
    crypto: &MemoryCryptoProvider,
    id: &str,
    field: &str,
    stored: &str,
    envelope_json: Option<&str>,
    trusted_tenant: &MemoryTenantScope,
    trusted_org_unit: Option<&str>,
    trusted_owner: Option<&str>,
) -> MemoryResult<(String, Option<MemoryKeyScope>)> {
    let envelope: Option<MemoryEnvelopeMetadata> = envelope_json
        .filter(|raw| !raw.trim().is_empty())
        .map(serde_json::from_str)
        .transpose()?;
    if envelope.is_some() && !crypto.is_hosted() {
        return Err(MemoryError::InvalidConfig(
            "hosted global record requires a hosted KMS provider".to_string(),
        ));
    }
    if stored.is_empty() && field != "content" && !crypto.is_hosted() {
        if envelope.is_some() {
            return Err(MemoryError::InvalidConfig(
                "empty global record field carries an envelope".to_string(),
            ));
        }
        return Ok((String::new(), None));
    }
    let (policy, audit) = authorization_ids(id, field);
    let expected_scope = envelope.as_ref().map(|value| {
        let mut scope = value.key_scope.clone();
        scope.org_id.clone_from(&trusted_tenant.org_id);
        scope.workspace_id.clone_from(&trusted_tenant.workspace_id);
        scope.deployment_id.clone_from(&trusted_tenant.deployment_id);
        scope.org_unit = trusted_org_unit.map(ToString::to_string);
        scope.owner_subject = trusted_owner.map(ToString::to_string);
        scope
    });
    let authority = expected_scope
        .as_ref()
        .map(|scope| MemoryEnvelopeAuthority::new(scope.clone(), policy, audit));
    let principal = crate::decrypt_context::current_decrypt_principal();
    let plaintext = match authority.as_ref() {
        Some(authority) => crypto.decrypt_field_scoped_authorized(
            stored,
            envelope.as_ref(),
            principal.as_ref(),
            authority,
            None,
        )?,
        None => crypto.decrypt_field(stored)?,
    };
    Ok((plaintext, expected_scope))
}

/// Only the broker's explicit grant denials are skippable while scanning a
/// mixed-class hosted result set. Corrupt envelopes, wrong keys and malformed
/// rows remain hard errors. The broker prefixes its audited denial reason with
/// MemoryError's display text before returning it from authorize_unwrap().
pub(super) fn is_global_record_grant_denial(error: &rusqlite::Error) -> bool {
    let rusqlite::Error::FromSqlConversionFailure(_, _, source) = error else {
        return false;
    };
    let Some(MemoryError::InvalidConfig(reason)) = source.downcast_ref::<MemoryError>() else {
        return false;
    };
    matches!(reason.as_str(),
        "Invalid configuration: memory decrypt principal lacks data-class grant"
            | "Invalid configuration: memory decrypt principal lacks source-binding grant"
            | "Invalid configuration: memory decrypt principal lacks owner-subject grant")
}

impl MemoryDatabase {
    #[allow(clippy::too_many_arguments)]
    async fn search_encrypted_global_memory_unscoped(
        &self,
        user_id: &str,
        query: &str,
        limit: i64,
        project_tag: Option<&str>,
        channel_tag: Option<&str>,
        host_tag: Option<&str>,
    ) -> MemoryResult<Vec<GlobalMemorySearchHit>> {
        let conn = self.conn.lock().await;
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut stmt = conn.prepare(
            "SELECT id, user_id, source_type, content, content_hash, run_id, session_id,
                    message_id, tool_name, project_tag, channel_tag, host_tag, metadata,
                    provenance, redaction_status, redaction_count, visibility, demoted,
                    score_boost, created_at_ms, updated_at_ms, expires_at_ms,
                    content_envelope, metadata_envelope, provenance_envelope,
                    tenant_org_id, tenant_workspace_id, tenant_deployment_id,
                    owner_org_unit_id, owner_subject
             FROM memory_records
             WHERE user_id = ?1 AND demoted = 0
               AND (expires_at_ms IS NULL OR expires_at_ms > ?2)
               AND (?3 IS NULL OR project_tag = ?3)
               AND (?4 IS NULL OR channel_tag = ?4)
               AND (?5 IS NULL OR host_tag = ?5)
             ORDER BY created_at_ms DESC",
        )?;
        let rows = stmt.query_map(
            params![user_id, now_ms, project_tag, channel_tag, host_tag],
            |row| row_to_global_record(row, &self.crypto),
        )?;
        let mut hits = Vec::new();
        for row in rows {
            let record = row?;
            if hosted_global_text_matches(&record.content, query) {
                hits.push(GlobalMemorySearchHit { record, score: 0.25 });
                if hits.len() >= limit.clamp(1, 100) as usize {
                    break;
                }
            }
        }
        Ok(hits)
    }

    #[allow(clippy::too_many_arguments)]
    async fn list_encrypted_global_memory_unscoped(
        &self,
        user_id: &str,
        query: &str,
        project_tag: Option<&str>,
        channel_tag: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> MemoryResult<Vec<GlobalMemoryRecord>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, user_id, source_type, content, content_hash, run_id, session_id,
                    message_id, tool_name, project_tag, channel_tag, host_tag, metadata,
                    provenance, redaction_status, redaction_count, visibility, demoted,
                    score_boost, created_at_ms, updated_at_ms, expires_at_ms,
                    content_envelope, metadata_envelope, provenance_envelope,
                    tenant_org_id, tenant_workspace_id, tenant_deployment_id,
                    owner_org_unit_id, owner_subject
             FROM memory_records
             WHERE user_id = ?1
               AND (?2 IS NULL OR project_tag = ?2)
               AND (?3 IS NULL OR channel_tag = ?3)
             ORDER BY created_at_ms DESC",
        )?;
        let rows = stmt.query_map(params![user_id, project_tag, channel_tag],
            |row| row_to_global_record(row, &self.crypto))?;
        let query = query.to_lowercase();
        let mut skipped = 0i64;
        let mut out = Vec::new();
        for row in rows {
            let record = row?;
            if !record.content.to_lowercase().contains(&query)
                && !record.source_type.to_lowercase().contains(&query)
                && !record.run_id.to_lowercase().contains(&query)
            {
                continue;
            }
            if skipped < offset.max(0) {
                skipped += 1;
                continue;
            }
            out.push(record);
            if out.len() >= limit.clamp(1, 1000) as usize {
                break;
            }
        }
        Ok(out)
    }
}
