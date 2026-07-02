use tandem_data_boundary::{
    detect_sensitive_data, redact_sensitive_data, tokenize_sensitive_data,
    DataBoundaryDetectorFinding, DataBoundaryFindingSeverity, DataBoundarySpan, SensitiveDataClass,
};

#[test]
fn deterministic_detector_finds_mvp_patterns_without_raw_values() {
    let password_value = ["super", "-secret", "-123"].concat();
    let password_key = ["pass", "word"].concat();
    let bearer_value = ["eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9", ".token"].concat();
    let aws_key = ["AK", "IA", "IOSFODNN7EXAMPLE"].concat();
    let private_key = [
        "-----BEGIN ",
        "PRIVATE KEY-----\nabc123\n-----END ",
        "PRIVATE KEY----- ",
    ]
    .concat();
    let input = format!(
        "Contact jane.admin@example.com or +1 (415) 555-0134. Card 4111-1111-1111-1111. {password_key}='{password_value}' token=ghp_1234567890abcdefABCDEF Authorization: Bearer \
         {bearer_value} AWS {aws_key}. {private_key}entropy=AbCdEf1234567890+/AbCdEf1234567890+/ \
         ssn 123-45-6789"
    );

    let findings = detect_sensitive_data(&input);
    let detector_ids: Vec<_> = findings
        .iter()
        .map(|finding| finding.detector_id.as_str())
        .collect();

    for expected in [
        "email_address",
        "phone_like",
        "credit_card_luhn",
        "credential_assignment",
        "api_key_prefix",
        "bearer_token",
        "aws_access_key_id",
        "private_key_block",
        "high_entropy_token",
        "ssn_like",
    ] {
        assert!(
            detector_ids.contains(&expected),
            "missing detector {expected}: {detector_ids:?}"
        );
    }

    let serialized = serde_json::to_string(&findings).expect("serialize findings");
    for raw in [
        password_value.as_str(),
        "4111-1111-1111-1111",
        aws_key.as_str(),
        bearer_value.as_str(),
    ] {
        assert!(
            !serialized.contains(raw),
            "detector finding leaked raw value `{raw}`: {serialized}"
        );
    }
    assert!(serialized.contains("evidence_hash"));
    assert!(serialized.contains("redaction_preview"));
}

#[test]
fn detector_rejects_credit_card_like_numbers_that_fail_luhn() {
    let findings = detect_sensitive_data("Card 4111-1111-1111-1112 should not pass.");

    assert!(
        findings
            .iter()
            .all(|finding| finding.detector_id != "credit_card_luhn"),
        "invalid Luhn number should not produce a card finding: {findings:?}"
    );
}

#[test]
fn redaction_replaces_spans_with_stable_placeholders() {
    let input = "email jane.admin@example.com password=super-secret-123";
    let findings = detect_sensitive_data(input);
    let redaction = redact_sensitive_data(input, &findings);

    assert!(redaction.redacted.contains("[REDACTED:PII:1]"));
    assert!(redaction
        .redacted
        .contains("password=[REDACTED:CREDENTIAL:2]"));
    assert!(!redaction.redacted.contains("jane.admin@example.com"));
    assert!(!redaction.redacted.contains("super-secret-123"));
    assert_eq!(redaction.placeholders.len(), 2);

    let serialized = serde_json::to_string(&redaction).expect("serialize redaction");
    assert!(!serialized.contains("jane.admin@example.com"));
    assert!(!serialized.contains("super-secret-123"));
    assert!(serialized.contains("sha256:"));
}

#[test]
fn tokenization_returns_placeholder_map_without_raw_values() {
    let input = "use ghp_1234567890abcdefABCDEF";
    let findings = detect_sensitive_data(input);
    let tokenization = tokenize_sensitive_data(input, &findings);

    assert!(tokenization.tokenized.contains("[TOKEN:CREDENTIAL:1]"));
    assert!(!tokenization
        .tokenized
        .contains("ghp_1234567890abcdefABCDEF"));
    assert_eq!(tokenization.placeholders.len(), 1);
    assert_eq!(tokenization.placeholders[0].occurrence, 1);

    let serialized = serde_json::to_string(&tokenization).expect("serialize tokenization");
    assert!(!serialized.contains("ghp_1234567890abcdefABCDEF"));
    assert!(serialized.contains("api_key_prefix"));
}

#[test]
fn overlapping_spans_are_handled_deterministically() {
    let input = "token=secret-value";
    let findings = vec![
        DataBoundaryDetectorFinding {
            data_class: SensitiveDataClass::Pii,
            severity: DataBoundaryFindingSeverity::Low,
            confidence: 60,
            span: DataBoundarySpan { start: 6, end: 12 },
            detector_id: "short_low".to_string(),
            redaction_preview: "[REDACTED:PII]".to_string(),
            evidence_hash: "sha256:short".to_string(),
            reason_codes: vec!["test".to_string()],
        },
        DataBoundaryDetectorFinding {
            data_class: SensitiveDataClass::Credential,
            severity: DataBoundaryFindingSeverity::High,
            confidence: 90,
            span: DataBoundarySpan { start: 6, end: 18 },
            detector_id: "long_high".to_string(),
            redaction_preview: "[REDACTED:CREDENTIAL]".to_string(),
            evidence_hash: "sha256:long".to_string(),
            reason_codes: vec!["test".to_string()],
        },
    ];

    let redaction = redact_sensitive_data(input, &findings);

    assert_eq!(redaction.redacted, "token=[REDACTED:CREDENTIAL:1]");
    assert_eq!(redaction.placeholders.len(), 1);
    assert_eq!(redaction.placeholders[0].detector_id, "long_high");
}

#[test]
fn nested_key_detection_keeps_outer_assignment_redacted() {
    let aws_key = ["AK", "IA", "IOSFODNN7EXAMPLE"].concat();
    let input = format!("secret=prefix-{aws_key}-suffix");
    let findings = detect_sensitive_data(&input);
    let detector_ids: Vec<_> = findings
        .iter()
        .map(|finding| finding.detector_id.as_str())
        .collect();

    assert!(detector_ids.contains(&"credential_assignment"));
    assert!(detector_ids.contains(&"aws_access_key_id"));

    let redaction = redact_sensitive_data(&input, &findings);

    assert_eq!(redaction.redacted, "secret=[REDACTED:SECRET:1]");
    assert!(!redaction.redacted.contains("prefix-"));
    assert!(!redaction.redacted.contains("-suffix"));
    assert!(!redaction.redacted.contains(&aws_key));
}

#[test]
fn quoted_credential_scan_skips_escaped_delimiters() {
    let password_key = ["pass", "word"].concat();
    let password_value = ["abc", "\\\"", "defSECRET123"].concat();
    let input = format!("{password_key}=\"{password_value}\"");
    let findings = detect_sensitive_data(&input);
    let redaction = redact_sensitive_data(&input, &findings);

    assert_eq!(redaction.redacted, "password=\"[REDACTED:CREDENTIAL:1]\"");
    assert!(!redaction.redacted.contains("abc"));
    assert!(!redaction.redacted.contains("defSECRET123"));
}
