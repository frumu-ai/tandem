use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::collections::BTreeMap;
use tandem_types::VerifiedTenantContext;

use crate::AppState;

pub(crate) async fn enrich_verified_context_with_inbound_cross_tenant_grants(
    state: &AppState,
    verified: &mut VerifiedTenantContext,
) {
    if verified.strict_projection.is_none() || verified.tenant_context.is_local_implicit() {
        return;
    }

    let now = crate::now_ms();
    let records = state
        .enterprise
        .cross_tenant_grants
        .read()
        .await
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let Some(strict_projection) = verified.strict_projection.as_mut() else {
        return;
    };
    for record in records {
        if cross_tenant_grant_signature_verifies(&record) {
            record.project_into_strict_context(strict_projection, now);
        }
    }
}

fn cross_tenant_grant_signature_verifies(record: &tandem_types::CrossTenantGrantRecord) -> bool {
    let Some(public_key) = cross_tenant_grant_verifying_key(&record.grant.header.kid) else {
        return false;
    };
    let Ok(encoded_header) = encode_json_base64url(&record.grant.header) else {
        return false;
    };
    let Ok(encoded_claims) = encode_json_base64url(&record.grant.claims) else {
        return false;
    };
    let Some(signature_bytes) = decode_bytes::<64>(&record.grant.signature) else {
        return false;
    };
    let Ok(verifying_key) = VerifyingKey::from_bytes(&public_key) else {
        return false;
    };
    let signature = Signature::from_bytes(&signature_bytes);
    let signing_input = format!("{encoded_header}.{encoded_claims}");
    verifying_key
        .verify(signing_input.as_bytes(), &signature)
        .is_ok()
}

fn cross_tenant_grant_verifying_key(kid: &str) -> Option<[u8; 32]> {
    let kid = kid.trim();
    if kid.is_empty() {
        return None;
    }
    if let Some(raw_keyring) = raw_cross_tenant_grant_public_keyring() {
        return parse_cross_tenant_grant_public_keyring(&raw_keyring)
            .and_then(|keyring| keyring.get(kid).copied());
    }
    let configured_kid = std::env::var("TANDEM_CROSS_TENANT_GRANT_SIGNING_KEY_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "cross-tenant-grant-local".to_string());
    if configured_kid != kid {
        return None;
    }
    let raw_key = std::env::var("TANDEM_CROSS_TENANT_GRANT_SIGNING_KEY")
        .ok()
        .or_else(|| {
            let path = std::env::var("TANDEM_CROSS_TENANT_GRANT_SIGNING_KEY_FILE").ok()?;
            std::fs::read_to_string(path).ok()
        })?;
    let key_bytes = decode_bytes::<32>(&raw_key)?;
    Some(
        ed25519_dalek::SigningKey::from_bytes(&key_bytes)
            .verifying_key()
            .to_bytes(),
    )
}

fn raw_cross_tenant_grant_public_keyring() -> Option<String> {
    std::env::var("TANDEM_CROSS_TENANT_GRANT_PUBLIC_KEYS")
        .ok()
        .or_else(|| {
            let path = std::env::var("TANDEM_CROSS_TENANT_GRANT_PUBLIC_KEYS_FILE").ok()?;
            std::fs::read_to_string(path).ok()
        })
}

fn parse_cross_tenant_grant_public_keyring(raw_keys: &str) -> Option<BTreeMap<String, [u8; 32]>> {
    let raw_keys = raw_keys.trim();
    if raw_keys.is_empty() {
        return None;
    }
    if raw_keys.starts_with('{') {
        let entries = serde_json::from_str::<BTreeMap<String, serde_json::Value>>(raw_keys).ok()?;
        let mut decoded = BTreeMap::new();
        for (kid, value) in entries {
            let raw_key = match value {
                serde_json::Value::String(value) => value,
                serde_json::Value::Object(mut object) => object
                    .remove("public_key")
                    .or_else(|| object.remove("publicKey"))?
                    .as_str()?
                    .to_string(),
                _ => return None,
            };
            decoded.insert(kid, decode_bytes::<32>(&raw_key)?);
        }
        return Some(decoded);
    }

    let mut decoded = BTreeMap::new();
    for entry in raw_keys.split([',', ';', '\n']) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (kid, raw_key) = entry.split_once('=').or_else(|| entry.split_once(':'))?;
        decoded.insert(kid.trim().to_string(), decode_bytes::<32>(raw_key.trim())?);
    }
    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}

fn encode_json_base64url<T: serde::Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_vec(value)
        .map(|bytes| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

fn decode_bytes<const N: usize>(raw: &str) -> Option<[u8; N]> {
    let raw = raw.trim();
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(raw))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(raw))
        .ok()
        .or_else(|| decode_hex(raw))?;
    decoded.as_slice().try_into().ok()
}

fn decode_hex(raw: &str) -> Option<Vec<u8>> {
    let raw = raw.trim();
    if raw.len() % 2 != 0 {
        return None;
    }
    (0..raw.len())
        .step_by(2)
        .map(|idx| u8::from_str_radix(&raw[idx..idx + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Signer;
    use tandem_types::{
        AccessDecision, AccessPermission, AssertionMetadata, AuthorityChain, CrossTenantGrant,
        CrossTenantGrantClaims, CrossTenantGrantHeader, CrossTenantGrantParty,
        CrossTenantGrantRecord, DataBoundary, DataClass, HumanActor, PrincipalRef,
        RequestPrincipal, ResourceKind, ResourceRef, ResourceScope, StrictTenantContext,
        TenantContext, VerifiedTenantContext,
    };

    #[tokio::test]
    #[serial_test::serial]
    async fn enrich_projects_active_inbound_grant_into_strict_context() {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[42u8; 32]);
        std::env::set_var(
            "TANDEM_CROSS_TENANT_GRANT_PUBLIC_KEYS",
            format!(
                "grant-key={}",
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .encode(signing_key.verifying_key().to_bytes())
            ),
        );
        let state = AppState::new_starting("cross-tenant-grants-test".to_string(), true);
        let issuer =
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "admin-a");
        let audience =
            TenantContext::explicit_user_workspace("org-b", "workspace-b", None, "user-b");
        let subject = PrincipalRef::human_user("user-b");
        let resource = ResourceRef::new(
            "org-a",
            "workspace-a",
            ResourceKind::DocumentCollection,
            "finance-drive",
        );
        let claims = CrossTenantGrantClaims::new_v1(
            "grant-finance",
            CrossTenantGrantParty::from_tenant_context(&issuer),
            CrossTenantGrantParty::from_tenant_context(&audience),
            subject.clone(),
            ResourceScope::root(resource.clone()),
            vec![AccessPermission::Read],
            vec![DataClass::FinancialRecord],
            1,
            u64::MAX,
            PrincipalRef::human_user("admin-a"),
        );
        let header = CrossTenantGrantHeader::ed25519("grant-key");
        let signature = sign_test_grant(&header, &claims, &signing_key);
        state.enterprise.cross_tenant_grants.write().await.insert(
            "org-a::workspace-a::local::grant-finance".to_string(),
            CrossTenantGrantRecord::active(CrossTenantGrant::new(header, claims, signature), 1),
        );

        let request_principal = RequestPrincipal::authenticated_user("user-b", "test");
        let strict_context = StrictTenantContext::new(
            audience.clone(),
            subject,
            AuthorityChain::from_request(request_principal.clone()),
            ResourceScope::root(ResourceRef::new(
                "org-b",
                "workspace-b",
                ResourceKind::Workspace,
                "workspace-b",
            )),
            AssertionMetadata::new("issuer", "runtime", 1, u64::MAX, "assertion-b"),
        )
        .with_data_boundary(DataBoundary::allow(vec![DataClass::FinancialRecord]));
        let mut verified = VerifiedTenantContext {
            tenant_context: audience,
            human_actor: HumanActor::tandem_user("user-b"),
            authority_chain: AuthorityChain::from_request(request_principal),
            roles: Vec::new(),
            org_units: Vec::new(),
            capabilities: Vec::new(),
            policy_version: None,
            strict_projection: Some(strict_context),
            issuer: "issuer".to_string(),
            audience: "runtime".to_string(),
            issued_at_ms: 1,
            expires_at_ms: u64::MAX,
            assertion_id: "assertion-b".to_string(),
            assertion_key_id: None,
        };

        enrich_verified_context_with_inbound_cross_tenant_grants(&state, &mut verified).await;

        let strict = verified.strict_projection.expect("strict projection");
        let decision = strict.evaluate_access(
            &resource,
            AccessPermission::Read,
            DataClass::FinancialRecord,
            crate::now_ms(),
        );
        assert_eq!(decision.decision, AccessDecision::Allow);
        assert_eq!(decision.grant_id.as_deref(), Some("grant-finance"));
        std::env::remove_var("TANDEM_CROSS_TENANT_GRANT_PUBLIC_KEYS");
    }

    fn sign_test_grant(
        header: &CrossTenantGrantHeader,
        claims: &CrossTenantGrantClaims,
        signing_key: &ed25519_dalek::SigningKey,
    ) -> String {
        let encoded_header = encode_json_base64url(header).expect("header");
        let encoded_claims = encode_json_base64url(claims).expect("claims");
        let signing_input = format!("{encoded_header}.{encoded_claims}");
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(signing_key.sign(signing_input.as_bytes()).to_bytes())
    }
}
