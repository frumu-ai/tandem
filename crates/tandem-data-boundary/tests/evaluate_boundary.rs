use tandem_data_boundary::{
    detect_sensitive_data, evaluate_data_boundary, payload_hash, DataBoundaryAction,
    DataBoundaryEvaluationRequest, DataBoundaryEvent, DataBoundaryEvidenceKind,
    DataBoundaryEvidenceRef, DataBoundaryInput, DataBoundaryMode, DataBoundaryOperationKind,
    DataBoundaryOperationRef, DataBoundaryPolicy, DataBoundaryProviderRef, DataBoundaryTenantRef,
    ProviderBoundaryClass, SensitiveDataClass,
};

const RAW_EMAIL: &str = "jane.admin@example.com";
const RAW_CREDENTIAL: &str = "sk-live-abcdef1234567890";
const PRIVATE_KEY_MARKER: &str = "-----BEGIN RSA PRIVATE KEY-----";

fn sensitive_payload() -> String {
    format!(
        "contact {RAW_EMAIL} and use api_key={RAW_CREDENTIAL}\n{PRIVATE_KEY_MARKER}\nMIIEow==\n-----END RSA PRIVATE KEY-----"
    )
}

fn boundary_input(class: ProviderBoundaryClass) -> DataBoundaryInput {
    DataBoundaryInput {
        input_id: "dbi_it_1".to_string(),
        tenant: DataBoundaryTenantRef {
            organization_id: Some("org_1".to_string()),
            workspace_id: Some("ws_1".to_string()),
            deployment_id: None,
        },
        provider: DataBoundaryProviderRef {
            provider_id: "prov_ext".to_string(),
            model_id: Some("model_x".to_string()),
            boundary_class: class,
        },
        operation: DataBoundaryOperationRef {
            operation_id: "op_it_1".to_string(),
            kind: DataBoundaryOperationKind::ProviderRequest,
            tool_name: None,
            source_ref: Some("runtime.context.pack".to_string()),
        },
        payload_hash: String::new(),
        payload_bytes: sensitive_payload().len() as u64,
        source_refs: Vec::new(),
        data_classes: Vec::new(),
        action_tags: Vec::new(),
    }
}

fn boundary_policy(mode: DataBoundaryMode) -> DataBoundaryPolicy {
    DataBoundaryPolicy {
        policy_id: "pol_it_1".to_string(),
        mode,
        policy_fingerprint: "sha256:policy-it".to_string(),
        approved_provider_classes: Vec::new(),
        approved_provider_ids: Vec::new(),
        prohibited_provider_ids: Vec::new(),
        redact_classes: Vec::new(),
        tokenize_classes: Vec::new(),
        approval_required_classes: Vec::new(),
        block_classes: Vec::new(),
        require_local_classes: Vec::new(),
        allow_raw_external_classes: Vec::new(),
        strict_fail_closed: false,
        max_payload_bytes: None,
        action_tags: Vec::new(),
    }
}

#[test]
fn detector_finds_email_credential_and_private_key_marker() {
    let payload = sensitive_payload();
    let classes: Vec<SensitiveDataClass> = detect_sensitive_data(&payload)
        .into_iter()
        .map(|finding| finding.data_class)
        .collect();
    assert!(classes.contains(&SensitiveDataClass::Pii));
    assert!(classes.contains(&SensitiveDataClass::Credential));
    assert!(classes.contains(&SensitiveDataClass::Secret));
}

#[test]
fn evaluation_evidence_never_contains_raw_values() {
    let payload = sensitive_payload();
    let input = boundary_input(ProviderBoundaryClass::UnapprovedExternal);
    let policy = boundary_policy(DataBoundaryMode::Enforce);
    let evaluation = evaluate_data_boundary(
        &DataBoundaryEvaluationRequest {
            input: &input,
            payload: Some(&payload),
            detector_config: None,
        },
        &policy,
    );

    assert_eq!(evaluation.decision.action, DataBoundaryAction::Block);

    let event = DataBoundaryEvent::from_decision(
        "dbe_it_1",
        evaluation.event_kind,
        1_000,
        2,
        &evaluation.decision,
        vec![DataBoundaryEvidenceRef {
            kind: DataBoundaryEvidenceKind::PayloadHash,
            ref_id: "payload".to_string(),
            path: None,
            hash: Some(evaluation.decision.payload_hash.clone()),
        }],
    );

    let decision_json = serde_json::to_string(&evaluation.decision).expect("decision json");
    let findings_json = serde_json::to_string(&evaluation.findings).expect("findings json");
    let event_json = serde_json::to_string(&event).expect("event json");
    for (name, serialized) in [
        ("decision", &decision_json),
        ("findings", &findings_json),
        ("event", &event_json),
    ] {
        for raw in [RAW_EMAIL, RAW_CREDENTIAL, "MIIEow=="] {
            assert!(
                !serialized.contains(raw),
                "{name} payload must not contain raw value `{raw}`: {serialized}"
            );
        }
    }
    assert!(event_json.contains("data_boundary.blocked"));
    assert!(event_json.contains("decision_latency_ms"));
}

#[test]
fn audit_mode_allows_dispatch_but_records_findings() {
    let payload = sensitive_payload();
    let input = boundary_input(ProviderBoundaryClass::UnapprovedExternal);
    let policy = boundary_policy(DataBoundaryMode::Audit);
    let evaluation = evaluate_data_boundary(
        &DataBoundaryEvaluationRequest {
            input: &input,
            payload: Some(&payload),
            detector_config: None,
        },
        &policy,
    );
    assert_eq!(
        evaluation.decision.action,
        DataBoundaryAction::AllowWithAudit
    );
    assert!(!evaluation.findings.is_empty());
    assert!(evaluation.decision.finding_summary.total_findings > 0);
}

#[test]
fn local_provider_receives_what_external_provider_would_block() {
    let payload = sensitive_payload();
    let policy = boundary_policy(DataBoundaryMode::Enforce);

    let external = boundary_input(ProviderBoundaryClass::UnapprovedExternal);
    let blocked = evaluate_data_boundary(
        &DataBoundaryEvaluationRequest {
            input: &external,
            payload: Some(&payload),
            detector_config: None,
        },
        &policy,
    );
    assert_eq!(blocked.decision.action, DataBoundaryAction::Block);

    let local = boundary_input(ProviderBoundaryClass::Local);
    let allowed = evaluate_data_boundary(
        &DataBoundaryEvaluationRequest {
            input: &local,
            payload: Some(&payload),
            detector_config: None,
        },
        &policy,
    );
    assert_eq!(allowed.decision.action, DataBoundaryAction::AllowWithAudit);
}

#[test]
fn payload_hash_is_stable_and_content_free() {
    let payload = sensitive_payload();
    let first = payload_hash(payload.as_bytes());
    let second = payload_hash(payload.as_bytes());
    assert_eq!(first, second);
    assert!(first.starts_with("sha256:"));
    assert_ne!(first, payload_hash(b"different payload"));
    for raw in [RAW_EMAIL, RAW_CREDENTIAL] {
        assert!(!first.contains(raw));
    }

    let input = boundary_input(ProviderBoundaryClass::Local);
    let policy = boundary_policy(DataBoundaryMode::Audit);
    let evaluation = evaluate_data_boundary(
        &DataBoundaryEvaluationRequest {
            input: &input,
            payload: Some(&payload),
            detector_config: None,
        },
        &policy,
    );
    assert_eq!(evaluation.decision.payload_hash, first);
}
