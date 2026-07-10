use super::*;

use serial_test::serial;
use std::collections::HashMap;
use tandem_enterprise_contract::authority::fixtures;
use tandem_memory::decrypt_broker::{MemoryDecryptBroker, MemoryDecryptBrokerConfig};
use tandem_memory::dek_cache::MemoryDekCache;
use tandem_memory::envelope_crypto::HostedMemoryEnvelopeCrypto;
use tandem_memory::kms_providers::{
    GoogleCloudKmsDecryptClient, GoogleCloudKmsDecryptRequest, GoogleCloudKmsDekUnwrapProvider,
    GoogleCloudKmsDekWrapProvider, GoogleCloudKmsEncryptClient, GoogleCloudKmsEncryptRequest,
};
use tandem_memory::types::{MemoryError, MemoryResult};
use tandem_memory::MemoryCryptoProvider;

const PROVIDER_ID: &str = "google_cloud_kms";
const RUNTIME_PRINCIPAL: &str = "runtime-tandem";
const KEK_ID: &str = "projects/test/locations/global/keyRings/tandem/cryptoKeys/governance";

#[derive(Clone)]
struct XorFixtureKms {
    fail_encrypt: bool,
    fail_decrypt: bool,
}

impl GoogleCloudKmsEncryptClient for XorFixtureKms {
    fn encrypt(&self, request: &GoogleCloudKmsEncryptRequest) -> MemoryResult<Vec<u8>> {
        if self.fail_encrypt {
            return Err(MemoryError::InvalidConfig(
                "fixture KMS encrypt unavailable".to_string(),
            ));
        }
        assert!(!request.additional_authenticated_data.is_empty());
        Ok(request.plaintext.iter().map(|byte| byte ^ 0x5a).collect())
    }
}

impl GoogleCloudKmsDecryptClient for XorFixtureKms {
    fn decrypt(&self, request: &GoogleCloudKmsDecryptRequest) -> MemoryResult<Vec<u8>> {
        if self.fail_decrypt {
            return Err(MemoryError::InvalidConfig(
                "fixture KMS decrypt unavailable".to_string(),
            ));
        }
        assert!(!request.additional_authenticated_data.is_empty());
        Ok(request.ciphertext.iter().map(|byte| byte ^ 0x5a).collect())
    }
}

fn hosted_provider(fail_encrypt: bool, fail_decrypt: bool) -> MemoryCryptoProvider {
    let config = MemoryDecryptBrokerConfig::hosted(PROVIDER_ID, RUNTIME_PRINCIPAL).expect("config");
    let broker = MemoryDecryptBroker::new(config).expect("broker");
    let kms = XorFixtureKms {
        fail_encrypt,
        fail_decrypt,
    };
    let wrap =
        GoogleCloudKmsDekWrapProvider::new(kms.clone(), RUNTIME_PRINCIPAL).expect("wrap provider");
    let unwrap =
        GoogleCloudKmsDekUnwrapProvider::new(kms, RUNTIME_PRINCIPAL).expect("unwrap provider");
    MemoryCryptoProvider::hosted(HostedMemoryEnvelopeCrypto::new(
        broker,
        Box::new(wrap),
        Box::new(unwrap),
        MemoryDekCache::new(64),
        PROVIDER_ID,
        RUNTIME_PRINCIPAL,
        KEK_ID,
        "1",
        0,
    ))
}

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
    let unit_records = units
        .iter()
        .map(|(key, unit)| {
            crate::governance_store::GovernanceStoreFile::OrgUnits.json_record(
                key,
                unit,
                &unit.tenant_context,
                Some(key),
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()
        .expect("unit records");
    let membership_records = memberships
        .iter()
        .map(|(key, membership)| {
            crate::governance_store::GovernanceStoreFile::OrgUnitMemberships.json_record(
                key,
                membership,
                &membership.tenant_context,
                Some(&membership.unit.id),
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()
        .expect("membership records");
    let grant_records = grants
        .iter()
        .map(|(key, grant)| {
            crate::governance_store::GovernanceStoreFile::OrgUnitAccessGrants.json_record(
                key,
                grant,
                &grant.tenant_context,
                Some(&grant.unit.id),
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()
        .expect("grant records");
    let store = crate::governance_store::for_state(&state);
    store
        .write_json_records(
            crate::governance_store::GovernanceStoreFile::OrgUnits,
            &unit_records,
        )
        .await
        .expect("write encrypted org units");
    store
        .write_json_records(
            crate::governance_store::GovernanceStoreFile::OrgUnitMemberships,
            &membership_records,
        )
        .await
        .expect("write encrypted memberships");
    store
        .write_json_records(
            crate::governance_store::GovernanceStoreFile::OrgUnitAccessGrants,
            &grant_records,
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

#[tokio::test]
#[serial]
async fn hosted_governance_stores_round_trip_after_crypto_restart() {
    let state = crate::test_support::test_state().await;
    let _env_lock = crate::test_support::TEST_STATE_ENV_LOCK.lock().await;
    let tenant_context = tenant();
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
    let membership_secret = memberships
        .values()
        .next()
        .expect("membership fixture")
        .member
        .id
        .clone();
    let grant_secret = fixture.engineering_secret.resource_id.clone();
    let memory_event = crate::MemoryAuditEvent {
        audit_id: "memory-audit-hosted".to_string(),
        action: "retrieve".to_string(),
        run_id: "run-hosted".to_string(),
        tenant_context: tenant_context.clone(),
        memory_id: Some("memory-hosted".to_string()),
        source_memory_id: None,
        to_tier: None,
        partition_key: "hosted-memory-secret".to_string(),
        actor: "hosted-test".to_string(),
        status: "allowed".to_string(),
        detail: Some("hosted-memory-audit-detail".to_string()),
        created_at_ms: 42,
    };
    let memory_line = serde_json::to_string(&memory_event).expect("memory audit JSON");

    crate::encrypted_file_store::with_test_crypto_provider(
        hosted_provider(false, false),
        Some(RUNTIME_PRINCIPAL),
        async {
            crate::audit::append_protected_audit_event(
                &state,
                "governance.hosted.first",
                &tenant_context,
                Some("hosted-test".to_string()),
                json!({"secret_marker": "hosted-audit-secret-one"}),
            )
            .await
            .expect("append hosted audit");
            crate::audit::append_protected_audit_event(
                &state,
                "governance.hosted.second",
                &tenant_context,
                Some("hosted-test".to_string()),
                json!({"secret_marker": "hosted-audit-secret-two"}),
            )
            .await
            .expect("append second hosted audit");

            state
                .record_policy_decision(policy_decision("decision-hosted", tenant_context.clone()))
                .await
                .expect("record hosted policy decision");

            let store = crate::governance_store::for_state(&state);
            store
                .append_jsonl_line(
                    crate::governance_store::GovernanceStoreFile::MemoryAudit,
                    &memory_line,
                    &tenant_context,
                    None,
                    &memory_event.audit_id,
                    true,
                )
                .await
                .expect("append hosted memory audit");

            let unit_records = units
                .iter()
                .map(|(key, unit)| {
                    crate::governance_store::GovernanceStoreFile::OrgUnits.json_record(
                        key,
                        unit,
                        &unit.tenant_context,
                        Some(key),
                    )
                })
                .collect::<anyhow::Result<Vec<_>>>()
                .expect("unit records");
            let membership_records = memberships
                .iter()
                .map(|(key, membership)| {
                    crate::governance_store::GovernanceStoreFile::OrgUnitMemberships.json_record(
                        key,
                        membership,
                        &membership.tenant_context,
                        Some(&membership.unit.id),
                    )
                })
                .collect::<anyhow::Result<Vec<_>>>()
                .expect("membership records");
            let grant_records =
                grants
                    .iter()
                    .map(|(key, grant)| {
                        crate::governance_store::GovernanceStoreFile::OrgUnitAccessGrants
                            .json_record(key, grant, &grant.tenant_context, Some(&grant.unit.id))
                    })
                    .collect::<anyhow::Result<Vec<_>>>()
                    .expect("grant records");
            store
                .write_json_records(
                    crate::governance_store::GovernanceStoreFile::OrgUnits,
                    &unit_records,
                )
                .await
                .expect("write hosted units");
            store
                .write_json_records(
                    crate::governance_store::GovernanceStoreFile::OrgUnitMemberships,
                    &membership_records,
                )
                .await
                .expect("write hosted memberships");
            store
                .write_json_records(
                    crate::governance_store::GovernanceStoreFile::OrgUnitAccessGrants,
                    &grant_records,
                )
                .await
                .expect("write hosted grants");

            for (path, secret) in [
                (&state.protected_audit_path, "hosted-audit-secret"),
                (&state.memory_audit_path, "hosted-memory-audit-detail"),
                (&state.policy_decisions_path, "finance-decision-secret"),
                (&state.enterprise.org_units_path, "Engineering"),
                (
                    &state.enterprise.org_unit_memberships_path,
                    membership_secret.as_str(),
                ),
                (
                    &state.enterprise.org_unit_access_grants_path,
                    grant_secret.as_str(),
                ),
            ] {
                let raw = tokio::fs::read_to_string(path).await.expect("raw store");
                assert!(
                    !raw.contains(secret),
                    "plaintext leaked in {}",
                    path.display()
                );
                assert!(
                    raw.lines()
                        .all(crate::encrypted_file_store::is_encrypted_payload),
                    "store is not envelope encrypted: {}",
                    path.display()
                );
            }
        },
    )
    .await;

    crate::audit::reset_protected_audit_tail_for_test(&state.protected_audit_path).await;
    crate::encrypted_file_store::with_test_crypto_provider(
        hosted_provider(false, false),
        Some(RUNTIME_PRINCIPAL),
        async {
            crate::audit::append_protected_audit_event(
                &state,
                "governance.hosted.after_restart",
                &tenant_context,
                Some("hosted-test".to_string()),
                json!({"secret_marker": "hosted-audit-secret-three"}),
            )
            .await
            .expect("append after crypto restart");
            let verification =
                crate::audit::verify_protected_audit_ledger(&state.protected_audit_path).await;
            assert!(
                verification.valid,
                "unexpected violation: {:?}",
                verification.violation
            );
            assert_eq!(verification.record_count, 3);

            state.policy_decisions.write().await.clear();
            state
                .load_policy_decisions()
                .await
                .expect("reload hosted policy decisions");
            assert!(state.get_policy_decision("decision-hosted").await.is_some());

            state.enterprise.org_units.write().await.clear();
            state.enterprise.org_unit_memberships.write().await.clear();
            state
                .enterprise
                .org_unit_access_grants
                .write()
                .await
                .clear();
            state
                .load_enterprise_org_units()
                .await
                .expect("reload units");
            state
                .load_enterprise_org_unit_memberships()
                .await
                .expect("reload memberships");
            state
                .load_enterprise_org_unit_access_grants()
                .await
                .expect("reload grants");
            assert_eq!(state.enterprise.org_units.read().await.len(), units.len());
            assert_eq!(
                state.enterprise.org_unit_memberships.read().await.len(),
                memberships.len()
            );
            assert_eq!(
                state.enterprise.org_unit_access_grants.read().await.len(),
                grants.len()
            );
            let memory_lines = crate::governance_store::for_state(&state)
                .read_jsonl_lines(crate::governance_store::GovernanceStoreFile::MemoryAudit)
                .await
                .expect("read hosted memory audit")
                .expect("memory audit exists");
            assert_eq!(memory_lines, vec![memory_line]);
            state
                .validate_hosted_governance_readiness()
                .await
                .expect("hosted governance readiness after restart");
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn hosted_kms_write_failure_is_returned_without_persisted_success() {
    let state = crate::test_support::test_state().await;
    let _env_lock = crate::test_support::TEST_STATE_ENV_LOCK.lock().await;
    let tenant_context = tenant();
    crate::encrypted_file_store::with_test_crypto_provider(
        hosted_provider(true, false),
        Some(RUNTIME_PRINCIPAL),
        async {
            let audit_error = crate::audit::append_protected_audit_event(
                &state,
                "governance.hosted.must_fail",
                &tenant_context,
                Some("hosted-test".to_string()),
                json!({"secret_marker": "must-not-persist"}),
            )
            .await
            .expect_err("audit write must fail");
            assert!(
                format!("{audit_error:?}").contains("fixture KMS encrypt unavailable"),
                "unexpected error: {audit_error:?}"
            );
            assert!(!state.protected_audit_path.exists());

            let policy_error = state
                .record_policy_decision(policy_decision("decision-must-fail", tenant_context))
                .await
                .expect_err("policy write must fail");
            assert!(
                format!("{policy_error:?}").contains("fixture KMS encrypt unavailable"),
                "unexpected error: {policy_error:?}"
            );
            assert!(state
                .get_policy_decision("decision-must-fail")
                .await
                .is_none());
            assert!(!state.policy_decisions_path.exists());
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn hosted_policy_and_action_allows_fail_closed_when_evidence_write_fails() {
    let state = crate::test_support::test_state().await;
    let _env_lock = crate::test_support::TEST_STATE_ENV_LOCK.lock().await;
    let fixture = fixtures::acme_company();
    let request = tandem_enterprise_contract::authority::AuthorityAccessRequest::new(
        fixture.finance_analyst.clone(),
        fixture.internal_architecture_doc.clone(),
        tandem_enterprise_contract::AccessPermission::Read,
        tandem_enterprise_contract::DataClass::Restricted,
    );

    crate::encrypted_file_store::with_test_crypto_provider(
        hosted_provider(true, false),
        Some(RUNTIME_PRINCIPAL),
        async {
            let (authority, authority_id) = state
                .enforce_intra_tenant_access(
                    &fixture.tenant_context,
                    &request,
                    fixture.graph.direct_grants.clone(),
                    fixtures::SHARE_VALID_NOW_MS,
                )
                .await;
            assert!(authority.is_deny());
            assert_eq!(authority.reason_code, "authority_persistence_failed");
            assert!(authority_id.is_none());

            let (gate, gate_id) = state
                .enforce_action_gate(
                    &fixture.tenant_context,
                    &tandem_types::GateRequest::new(
                        Some(tandem_types::ToolRiskTier::ReadDiscover),
                        Some(tandem_enterprise_contract::DataClass::Internal),
                    ),
                    Some("mcp.search".to_string()),
                    Some("agent-hosted".to_string()),
                    fixtures::BASE_NOW_MS,
                )
                .await;
            assert!(gate.is_denied());
            assert_eq!(gate.reason_code, "authority_persistence_failed");
            assert!(gate_id.is_none());
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn hosted_readiness_probe_fails_when_kms_is_unavailable() {
    let state = crate::test_support::test_state().await;
    let _env_lock = crate::test_support::TEST_STATE_ENV_LOCK.lock().await;
    let _restore = EnvRestore::capture();
    std::env::set_var("TANDEM_MEMORY_ENCRYPTION_REQUIRED", "true");

    for (fail_encrypt, fail_decrypt, expected) in [
        (true, false, "fixture KMS encrypt unavailable"),
        (false, true, "fixture KMS decrypt unavailable"),
    ] {
        crate::encrypted_file_store::with_test_crypto_provider(
            hosted_provider(fail_encrypt, fail_decrypt),
            Some(RUNTIME_PRINCIPAL),
            async {
                let error = state
                    .validate_hosted_governance_readiness()
                    .await
                    .expect_err("readiness must fail when a KMS path is unavailable");
                assert!(
                    format!("{error:?}").contains(expected),
                    "unexpected error: {error:?}"
                );
            },
        )
        .await;
    }
}

#[tokio::test]
#[serial]
async fn provider_selected_hosted_readiness_failure_does_not_install_runtime() {
    let _env_lock = crate::test_support::TEST_STATE_ENV_LOCK.lock().await;
    let _restore = EnvRestore::capture();
    std::env::remove_var("TANDEM_MEMORY_ENCRYPTION_REQUIRED");
    std::env::set_var("TANDEM_MEMORY_DECRYPT_PROVIDER", PROVIDER_ID);
    std::env::set_var("TANDEM_MEMORY_DECRYPT_PRINCIPAL_ID", RUNTIME_PRINCIPAL);
    let (state, runtime) = starting_test_state_and_runtime().await;

    let error = crate::encrypted_file_store::with_test_crypto_provider(
        hosted_provider(true, false),
        Some(RUNTIME_PRINCIPAL),
        async { state.mark_ready(runtime).await },
    )
    .await
    .expect_err("hosted readiness failure must abort runtime installation");

    assert!(
        format!("{error:?}").contains("fixture KMS encrypt unavailable"),
        "unexpected error: {error:?}"
    );
    assert!(!state.is_ready(), "failed readiness must remain HTTP-gated");
    assert!(!state.oauth.provider_refresh_task_is_running());
}
