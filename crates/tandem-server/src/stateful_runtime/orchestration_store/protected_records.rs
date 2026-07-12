use anyhow::Context;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tandem_enterprise_contract::DataClass;
use tandem_memory::{envelope::MemoryKeyScope, types::MemoryTenantScope};
use tandem_types::TenantContext;

const POLICY_DECISION_ID: &str = "tandem-stateful-runtime-record:v1";

#[derive(Debug, Serialize, Deserialize)]
struct BoundRecord<T> {
    tenant_context: TenantContext,
    kind: String,
    id: String,
    payload: T,
}

fn same_tenant_scope(left: &TenantContext, right: &TenantContext) -> bool {
    left.org_id == right.org_id
        && left.workspace_id == right.workspace_id
        && left.deployment_id == right.deployment_id
}

pub(crate) fn tenant_from_scope(
    org_id: &str,
    workspace_id: &str,
    deployment_id: Option<&str>,
) -> TenantContext {
    let deployment_id = deployment_id
        .map(str::to_string)
        .filter(|value| !value.is_empty());
    if org_id == "local" && workspace_id == "local" && deployment_id.is_none() {
        TenantContext::local_implicit()
    } else {
        TenantContext::explicit(org_id, workspace_id, deployment_id)
    }
}

fn context(
    tenant: &TenantContext,
    kind: &str,
    id: &str,
) -> crate::encrypted_file_store::ProtectedRecordContext {
    let tenant_scope = MemoryTenantScope {
        org_id: tenant.org_id.clone(),
        workspace_id: tenant.workspace_id.clone(),
        deployment_id: tenant.deployment_id.clone(),
    };
    let key_scope = MemoryKeyScope::new(
        &tenant_scope,
        DataClass::Restricted,
        Some(format!("tandem-stateful-runtime:{kind}")),
    );
    crate::encrypted_file_store::ProtectedRecordContext::new(
        key_scope,
        POLICY_DECISION_ID,
        format!("{kind}:{id}"),
    )
}

pub(crate) fn encode<T: Serialize>(
    tenant: &TenantContext,
    kind: &str,
    id: &str,
    value: &T,
) -> anyhow::Result<String> {
    let plaintext = serde_json::to_string(&BoundRecord {
        tenant_context: tenant.clone(),
        kind: kind.to_string(),
        id: id.to_string(),
        payload: value,
    })?;
    crate::encrypted_file_store::encrypt_text(&plaintext, &context(tenant, kind, id))
        .with_context(|| format!("protect {kind} record {id}"))
}

pub(crate) fn decode<T: DeserializeOwned>(
    tenant: &TenantContext,
    kind: &str,
    id: &str,
    stored: &str,
) -> anyhow::Result<T> {
    let plaintext = crate::encrypted_file_store::decrypt_text(stored, &context(tenant, kind, id))
        .with_context(|| format!("unprotect {kind} record {id}"))?;
    match serde_json::from_str::<BoundRecord<T>>(&plaintext) {
        Ok(bound) => {
            anyhow::ensure!(
                same_tenant_scope(&bound.tenant_context, tenant),
                "protected {kind} record {id} tenant binding does not match"
            );
            anyhow::ensure!(
                bound.kind == kind && bound.id == id,
                "protected {kind} record {id} kind/id binding does not match"
            );
            Ok(bound.payload)
        }
        Err(bound_error) if !crate::encrypted_file_store::is_encrypted_payload(stored) => {
            serde_json::from_str(&plaintext).with_context(|| {
                format!("decode legacy plaintext {kind} record {id}: {bound_error}")
            })
        }
        Err(error) => Err(error).with_context(|| format!("decode bound {kind} record {id}")),
    }
}

pub(crate) fn decode_scoped<T: DeserializeOwned>(
    org_id: &str,
    workspace_id: &str,
    deployment_id: Option<&str>,
    kind: &str,
    id: &str,
    stored: &str,
) -> anyhow::Result<T> {
    let scope_tenant = tenant_from_scope(org_id, workspace_id, deployment_id);
    let plaintext =
        crate::encrypted_file_store::decrypt_text(stored, &context(&scope_tenant, kind, id))
            .with_context(|| format!("unprotect {kind} record {id}"))?;
    match serde_json::from_str::<BoundRecord<T>>(&plaintext) {
        Ok(bound) => {
            anyhow::ensure!(
                bound.tenant_context.org_id == org_id
                    && bound.tenant_context.workspace_id == workspace_id
                    && bound.tenant_context.deployment_id.as_deref()
                        == deployment_id.filter(|value| !value.is_empty()),
                "protected {kind} record {id} tenant scope does not match trusted columns"
            );
            anyhow::ensure!(
                bound.kind == kind && bound.id == id,
                "protected {kind} record {id} kind/id binding does not match"
            );
            Ok(bound.payload)
        }
        Err(bound_error) if !crate::encrypted_file_store::is_encrypted_payload(stored) => {
            serde_json::from_str(&plaintext).with_context(|| {
                format!("decode legacy plaintext {kind} record {id}: {bound_error}")
            })
        }
        Err(error) => Err(error).with_context(|| format!("decode bound {kind} record {id}")),
    }
}

pub(crate) fn digest<T: Serialize>(
    tenant: &TenantContext,
    kind: &str,
    value: &T,
) -> anyhow::Result<String> {
    let canonical = serde_json::to_vec(&(
        &tenant.org_id,
        &tenant.workspace_id,
        &tenant.deployment_id,
        kind,
        value,
    ))?;
    Ok(format!("sha256:{:x}", Sha256::digest(canonical)))
}
