use super::*;

use serial_test::serial;
use std::collections::HashMap;
use tandem_enterprise_contract::authority::fixtures;

struct EnvRestore {
    provider: Option<String>,
    key_file: Option<String>,
    required: Option<String>,
    principal: Option<String>,
}

impl EnvRestore {
    fn capture() -> Self {
        Self {
            provider: std::env::var("TANDEM_MEMORY_DECRYPT_PROVIDER").ok(),
            key_file: std::env::var("TANDEM_MEMORY_LOCAL_KEY_FILE").ok(),
            required: std::env::var("TANDEM_MEMORY_ENCRYPTION_REQUIRED").ok(),
            principal: std::env::var("TANDEM_MEMORY_DECRYPT_PRINCIPAL_ID").ok(),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        restore_var("TANDEM_MEMORY_DECRYPT_PROVIDER", self.provider.as_deref());
        restore_var("TANDEM_MEMORY_LOCAL_KEY_FILE", self.key_file.as_deref());
        restore_var(
            "TANDEM_MEMORY_ENCRYPTION_REQUIRED",
            self.required.as_deref(),
        );
        restore_var(
            "TANDEM_MEMORY_DECRYPT_PRINCIPAL_ID",
            self.principal.as_deref(),
        );
    }
}

fn restore_var(key: &str, value: Option<&str>) {
    match value {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

fn enable_local_file_encryption(dir: &tempfile::TempDir) -> EnvRestore {
    let restore = EnvRestore::capture();
    std::env::set_var("TANDEM_MEMORY_DECRYPT_PROVIDER", "local-file");
    std::env::set_var(
        "TANDEM_MEMORY_LOCAL_KEY_FILE",
        dir.path().join("local_memory.key"),
    );
    std::env::remove_var("TANDEM_MEMORY_ENCRYPTION_REQUIRED");
    std::env::remove_var("TANDEM_MEMORY_DECRYPT_PRINCIPAL_ID");
    restore
}

fn tenant() -> TenantContext {
    TenantContext::explicit_user_workspace("org-sec", "workspace-sec", None, "user-sec")
}

fn policy_decision(decision_id: &str, tenant_context: TenantContext) -> PolicyDecisionRecord {
    PolicyDecisionRecord {
        decision_id: decision_id.to_string(),
        tenant_context,
        requester_context: None,
        actor_id: Some("agent-encrypted-store-test".to_string()),
        session_id: Some("session-encrypted-store-test".to_string()),
        message_id: Some("message-encrypted-store-test".to_string()),
        run_id: Some("run-encrypted-store-test".to_string()),
        automation_id: Some("automation-encrypted-store-test".to_string()),
        node_id: None,
        tool: Some("mcp.secure.release".to_string()),
        resource: None,
        data_classes: Vec::new(),
        risk_tier: Some("privileged".to_string()),
        decision: PolicyDecisionEffect::ApprovalRequired,
        reason_code: "encrypted_file_store_required".to_string(),
        reason: "finance-decision-secret should not be plaintext".to_string(),
        policy_id: Some("policy-encrypted-store".to_string()),
        grant_id: None,
        approval_id: None,
        audit_event_id: None,
        created_at_ms: 42,
        metadata: json!({"secret_marker": "finance-decision-secret"}),
    }
}

#[tokio::test]
#[serial]
async fn protected_audit_hash_chain_verifies_with_encrypted_rows() {
    let state = crate::test_support::test_state().await;
    let _env_lock = crate::test_support::TEST_STATE_ENV_LOCK.lock().await;
    let crypto_dir = tempfile::tempdir().expect("crypto tempdir");
    let _restore = enable_local_file_encryption(&crypto_dir);
    let tenant_context = tenant();

    crate::audit::append_protected_audit_event(
        &state,
        "governance.secret_allowed",
        &tenant_context,
        Some("agent-encrypted-store-test".to_string()),
        json!({"secret_marker": "audit-chain-secret-one"}),
    )
    .await
    .expect("append first audit row");
    crate::audit::append_protected_audit_event(
        &state,
        "governance.secret_denied",
        &tenant_context,
        Some("agent-encrypted-store-test".to_string()),
        json!({"secret_marker": "audit-chain-secret-two"}),
    )
    .await
    .expect("append second audit row");

    let raw = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("raw audit file");
    assert!(raw
        .lines()
        .all(crate::encrypted_file_store::is_encrypted_payload));
    assert!(!raw.contains("audit-chain-secret"));

    let result = crate::audit::verify_protected_audit_ledger(&state.protected_audit_path).await;
    assert!(result.valid, "unexpected violation: {:?}", result.violation);
    assert_eq!(result.record_count, 2);
    assert_eq!(result.hashed_record_count, 2);

    let loaded =
        crate::audit::load_protected_audit_events_for_tenant(&state, &tenant_context).await;
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].seq, 1);
    assert_eq!(loaded[1].seq, 2);
}

#[tokio::test]
#[serial]
async fn policy_and_org_unit_files_round_trip_encrypted() {
    let state = crate::test_support::test_state().await;
    let _env_lock = crate::test_support::TEST_STATE_ENV_LOCK.lock().await;
    let crypto_dir = tempfile::tempdir().expect("crypto tempdir");
    let _restore = enable_local_file_encryption(&crypto_dir);

    state
        .record_policy_decision(policy_decision("decision-encrypted", tenant()))
        .await
        .expect("record encrypted policy decision");
    let raw_policy = tokio::fs::read_to_string(&state.policy_decisions_path)
        .await
        .expect("raw policy decisions");
    assert!(crate::encrypted_file_store::is_encrypted_payload(
        &raw_policy
    ));
    assert!(!raw_policy.contains("finance-decision-secret"));

    state.policy_decisions.write().await.clear();
    state
        .load_policy_decisions()
        .await
        .expect("reload encrypted policy decisions");
    assert!(state
        .get_policy_decision("decision-encrypted")
        .await
        .is_some());

    let fixture = fixtures::acme_company();
    let units = fixture
        .graph
        .units
        .iter()
        .map(|unit| {
            (
                format!("{}/{}", unit.taxonomy_id, unit.unit_id),
                unit.clone(),
            )
        })
        .collect::<HashMap<_, _>>();
    let memberships = fixture
        .graph
        .memberships
        .iter()
        .map(|membership| (membership.membership_id.clone(), membership.clone()))
        .collect::<HashMap<_, _>>();
    let grants = fixture
        .graph
        .unit_access_grants
        .iter()
        .map(|grant| (grant.grant_id.clone(), grant.clone()))
        .collect::<HashMap<_, _>>();

    crate::encrypted_file_store::write_json_file(&state.enterprise.org_units_path, &units)
        .await
        .expect("write encrypted org units");
    crate::encrypted_file_store::write_json_file(
        &state.enterprise.org_unit_memberships_path,
        &memberships,
    )
    .await
    .expect("write encrypted memberships");
    crate::encrypted_file_store::write_json_file(
        &state.enterprise.org_unit_access_grants_path,
        &grants,
    )
    .await
    .expect("write encrypted grants");

    let raw_units = tokio::fs::read_to_string(&state.enterprise.org_units_path)
        .await
        .expect("raw org units");
    assert!(crate::encrypted_file_store::is_encrypted_payload(
        &raw_units
    ));
    assert!(!raw_units.contains("Engineering"));

    state.load_enterprise_org_units().await.expect("load units");
    state
        .load_enterprise_org_unit_memberships()
        .await
        .expect("load memberships");
    state
        .load_enterprise_org_unit_access_grants()
        .await
        .expect("load grants");

    assert_eq!(state.enterprise.org_units.read().await.len(), units.len());
    assert_eq!(
        state.enterprise.org_unit_memberships.read().await.len(),
        memberships.len()
    );
    assert_eq!(
        state.enterprise.org_unit_access_grants.read().await.len(),
        grants.len()
    );
}
