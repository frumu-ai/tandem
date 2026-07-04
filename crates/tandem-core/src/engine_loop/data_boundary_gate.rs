//! Audit-only data-boundary evaluation at the provider-dispatch seam
//! (TAN-389/TAN-390). Configured entirely through `TANDEM_DATA_BOUNDARY_*`
//! env vars following the engine-tunable convention in `loop_tuning.rs`;
//! tandem-server validates the same vars at startup so bad values fail boot
//! instead of silently weakening the policy.
//!
//! This gate observes and reports — it never blocks, transforms, or reroutes
//! the provider call. Enforcement is a later cycle (TAN-394); until a
//! provider classifier exists (TAN-393) every provider is classified
//! `Unknown`.

use serde_json::{json, Value};
use std::time::Instant;
use tandem_data_boundary::{
    evaluate_data_boundary, payload_hash, DataBoundaryEvaluationRequest, DataBoundaryEvent,
    DataBoundaryInput, DataBoundaryMode, DataBoundaryOperationKind, DataBoundaryOperationRef,
    DataBoundaryPolicy, DataBoundaryProviderRef, DataBoundaryTenantRef, ProviderBoundaryClass,
    SensitiveDataClass,
};
use tandem_providers::ChatMessage;
use tandem_types::EngineEvent;

/// For `data:` URLs, the byte length of the metadata prefix (through the
/// comma) that is safe and useful to scan; `None` for every other URL form.
fn data_url_scan_prefix_len(url: &str) -> Option<usize> {
    if !url.trim_start().to_ascii_lowercase().starts_with("data:") {
        return None;
    }
    Some(url.find(',').map(|comma| comma + 1).unwrap_or(url.len()))
}

pub(super) fn data_boundary_mode() -> DataBoundaryMode {
    std::env::var("TANDEM_DATA_BOUNDARY_MODE")
        .ok()
        .and_then(|raw| DataBoundaryMode::parse(&raw))
        .unwrap_or_default()
}

fn sensitive_class_list(var: &str) -> Vec<SensitiveDataClass> {
    std::env::var(var)
        .ok()
        .map(|raw| {
            raw.split(',')
                .filter(|item| !item.trim().is_empty())
                .filter_map(SensitiveDataClass::parse)
                .collect()
        })
        .unwrap_or_default()
}

/// How raw sensitive data headed for an unapproved external provider is
/// treated (`TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY`). Maps onto the
/// policy's class lists; `block` is the crate's built-in default (nothing to
/// add), so it and unset behave identically.
fn apply_external_raw_policy(policy: &mut DataBoundaryPolicy) {
    let Ok(raw) = std::env::var("TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY") else {
        return;
    };
    let all = SensitiveDataClass::ALL.to_vec();
    match raw.trim().to_ascii_lowercase().as_str() {
        "allow" | "audit" => policy.allow_raw_external_classes = all,
        "redact" => policy.redact_classes = all,
        "approval" => policy.approval_required_classes = all,
        "require_local" | "required_local" => policy.require_local_classes = all,
        // `block` is the built-in behavior; unrecognized values are rejected
        // by tandem-server startup validation before this code runs.
        _ => {}
    }
}

pub(super) fn data_boundary_policy_from_env(mode: DataBoundaryMode) -> DataBoundaryPolicy {
    let mut policy = DataBoundaryPolicy {
        policy_id: "env".to_string(),
        mode,
        policy_fingerprint: String::new(),
        approved_provider_classes: Vec::new(),
        approved_provider_ids: Vec::new(),
        prohibited_provider_ids: Vec::new(),
        redact_classes: sensitive_class_list("TANDEM_DATA_BOUNDARY_REDACT_CLASSES"),
        tokenize_classes: Vec::new(),
        approval_required_classes: sensitive_class_list("TANDEM_DATA_BOUNDARY_APPROVAL_CLASSES"),
        block_classes: sensitive_class_list("TANDEM_DATA_BOUNDARY_BLOCK_CLASSES"),
        require_local_classes: Vec::new(),
        allow_raw_external_classes: Vec::new(),
        strict_fail_closed: false,
        max_payload_bytes: std::env::var("TANDEM_DATA_BOUNDARY_MAX_PAYLOAD_BYTES")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .filter(|value| *value > 0),
        action_tags: Vec::new(),
    };
    apply_external_raw_policy(&mut policy);
    policy.policy_fingerprint = payload_hash(
        serde_json::to_string(&policy)
            .unwrap_or_default()
            .as_bytes(),
    );
    policy
}

pub(super) struct DataBoundaryDispatchContext<'a> {
    pub session_id: &'a str,
    pub message_id: &'a str,
    pub correlation_id: Option<&'a str>,
    pub provider_id: &'a str,
    pub model_id: &'a str,
    pub org_id: Option<&'a str>,
    pub workspace_id: Option<&'a str>,
    pub deployment_id: Option<&'a str>,
}

/// Evaluates the fully assembled provider request and returns the
/// `data_boundary.*` event to publish, or `None` when the boundary is off.
/// Audit-only: the caller must not alter dispatch based on the result.
pub(super) fn evaluate_dispatch_boundary(
    ctx: &DataBoundaryDispatchContext<'_>,
    messages: &[ChatMessage],
) -> Option<EngineEvent> {
    let mode = data_boundary_mode();
    if mode == DataBoundaryMode::Off {
        return None;
    }
    let started = Instant::now();
    let policy = data_boundary_policy_from_env(mode);

    // The assembled request text, rebuilt transiently for detection only —
    // never stored, logged, or attached to the emitted event. Attachment URLs
    // are dispatched to providers too (as image_url/input_image), so they are
    // part of what crosses the boundary and must be scanned: signed URLs and
    // query tokens are exactly where credentials leak.
    let mut payload_text = String::new();
    for message in messages {
        payload_text.push_str(&message.role);
        payload_text.push_str(": ");
        payload_text.push_str(&message.content);
        payload_text.push('\n');
        for attachment in &message.attachments {
            let tandem_providers::ChatAttachment::ImageUrl { url } = attachment;
            payload_text.push_str("attachment: ");
            if let Some(prefix_len) = data_url_scan_prefix_len(url) {
                // Inline data: URLs embed base64 image bytes; scanning the
                // body would flood findings with high-entropy false positives
                // on every image prompt. Record scheme/mediatype only.
                payload_text.push_str(&url[..prefix_len]);
                payload_text.push_str("<data elided>");
            } else {
                payload_text.push_str(url);
            }
            payload_text.push('\n');
        }
    }

    let input = DataBoundaryInput {
        input_id: format!("dbi_{}", ctx.message_id),
        tenant: DataBoundaryTenantRef {
            organization_id: ctx.org_id.map(str::to_string),
            workspace_id: ctx.workspace_id.map(str::to_string),
            deployment_id: ctx.deployment_id.map(str::to_string),
        },
        provider: DataBoundaryProviderRef {
            provider_id: ctx.provider_id.to_string(),
            model_id: Some(ctx.model_id.to_string()),
            // No provider classifier exists yet (TAN-393); Unknown is the
            // honest value and strict policies fail closed on it once
            // enforcement lands.
            boundary_class: ProviderBoundaryClass::Unknown,
        },
        operation: DataBoundaryOperationRef {
            operation_id: ctx.message_id.to_string(),
            kind: DataBoundaryOperationKind::ProviderRequest,
            tool_name: None,
            source_ref: Some("engine_loop.provider_dispatch".to_string()),
        },
        payload_hash: payload_hash(payload_text.as_bytes()),
        payload_bytes: payload_text.len() as u64,
        source_refs: Vec::new(),
        data_classes: Vec::new(),
        action_tags: Vec::new(),
    };

    let mut evaluation = evaluate_data_boundary(
        &DataBoundaryEvaluationRequest {
            input: &input,
            payload: Some(&payload_text),
            detector_config: None,
        },
        &policy,
    );
    drop(payload_text);

    // This gate enforces nothing — the raw messages dispatch unchanged, and
    // any transformed payload the evaluation produced is discarded. Emitting
    // the decision's own event kind (redacted/tokenized/blocked/...) would
    // therefore claim an outcome that never happened to the dispatched
    // payload. Everything emits as `data_boundary.evaluated`; the decided
    // action and would-be event kind ride along as evidence, and the
    // enforcement kinds stay reserved for the enforce-mode integration
    // (TAN-394).
    let decided_event_kind = evaluation.event_kind;
    drop(evaluation.transformed_payload.take());
    let boundary_event = DataBoundaryEvent::from_decision(
        format!(
            "dbe_{}",
            evaluation.decision.decision_id.trim_start_matches("dbd_")
        ),
        tandem_data_boundary::DataBoundaryEventKind::Evaluated,
        chrono::Utc::now().timestamp_millis().max(0) as u64,
        started.elapsed().as_millis() as u64,
        &evaluation.decision,
        Vec::new(),
    );

    let mut properties = serde_json::to_value(&boundary_event).unwrap_or_else(|_| json!({}));
    if let Value::Object(ref mut map) = properties {
        // Envelope keys so the bus derives session scoping (see
        // RuntimeEventEnvelope::derive), plus the dispatch correlation ids the
        // rest of the provider-call event family carries.
        map.insert("sessionID".to_string(), json!(ctx.session_id));
        map.insert("messageID".to_string(), json!(ctx.message_id));
        map.insert("correlationID".to_string(), json!(ctx.correlation_id));
        map.insert("providerID".to_string(), json!(ctx.provider_id));
        map.insert("modelID".to_string(), json!(ctx.model_id));
        map.insert("mode".to_string(), json!(mode.as_str()));
        map.insert("auditOnly".to_string(), json!(true));
        map.insert("enforced".to_string(), json!(false));
        map.insert(
            "decidedEventKind".to_string(),
            json!(decided_event_kind.event_name()),
        );
    }

    Some(EngineEvent::new(
        boundary_event.event_name.clone(),
        properties,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_string(),
            content: content.to_string(),
            attachments: Vec::new(),
        }
    }

    fn ctx<'a>() -> DataBoundaryDispatchContext<'a> {
        DataBoundaryDispatchContext {
            session_id: "ses_db_1",
            message_id: "msg_db_1",
            correlation_id: None,
            provider_id: "openai",
            model_id: "gpt-test",
            org_id: Some("local"),
            workspace_id: Some("local"),
            deployment_id: None,
        }
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn off_mode_emits_nothing() {
        std::env::remove_var("TANDEM_DATA_BOUNDARY_MODE");
        let messages = vec![chat("user", "api_key=sk-live-abcdef1234567890")];
        assert!(evaluate_dispatch_boundary(&ctx(), &messages).is_none());
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn audit_mode_emits_safe_event_with_findings() {
        std::env::set_var("TANDEM_DATA_BOUNDARY_MODE", "audit");
        let secret = "sk-live-abcdef1234567890";
        let messages = vec![
            chat("system", "you are helpful"),
            chat("user", &format!("use api_key={secret} please")),
        ];
        let event = evaluate_dispatch_boundary(&ctx(), &messages).expect("event");
        std::env::remove_var("TANDEM_DATA_BOUNDARY_MODE");

        assert_eq!(event.event_type, "data_boundary.evaluated");
        let serialized = serde_json::to_string(&event.properties).expect("json");
        assert!(
            !serialized.contains(secret),
            "raw secret leaked: {serialized}"
        );
        assert_eq!(event.properties["action"], "allow_with_audit");
        assert_eq!(event.properties["auditOnly"], true);
        assert_eq!(event.properties["sessionID"], "ses_db_1");
        assert!(
            event.properties["finding_summary"]["total_findings"]
                .as_u64()
                .unwrap_or(0)
                > 0
        );
        assert!(event.properties["payload_hash"]
            .as_str()
            .unwrap_or_default()
            .starts_with("sha256:"));
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn transform_decisions_emit_evaluated_evidence_without_claiming_enforcement() {
        // Codex P1 (PR #1785): the audit-only gate dispatches the raw
        // messages, so a redact decision must not emit
        // `data_boundary.redacted` — that would claim a transformation that
        // never reached the provider.
        std::env::set_var("TANDEM_DATA_BOUNDARY_MODE", "audit");
        std::env::set_var("TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY", "redact");
        let messages = vec![chat("user", "use api_key=sk-live-abcdef1234567890")];
        let event = evaluate_dispatch_boundary(&ctx(), &messages).expect("event");
        std::env::remove_var("TANDEM_DATA_BOUNDARY_MODE");
        std::env::remove_var("TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY");

        assert_eq!(event.event_type, "data_boundary.evaluated");
        assert_eq!(event.properties["action"], "redact");
        assert_eq!(event.properties["enforced"], false);
        assert_eq!(
            event.properties["decidedEventKind"],
            "data_boundary.redacted"
        );
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn attachment_urls_are_scanned_but_data_url_bodies_are_elided() {
        // Codex P2 (PR #1785): attachment URLs dispatch to providers, so a
        // signed URL carrying a credential must produce findings — while an
        // inline data: URL's base64 image body must not flood findings with
        // high-entropy false positives.
        std::env::set_var("TANDEM_DATA_BOUNDARY_MODE", "audit");
        let signed = ChatMessage {
            role: "user".to_string(),
            content: "see attached".to_string(),
            attachments: vec![tandem_providers::ChatAttachment::ImageUrl {
                url: "https://cdn.example.com/img.png?api_key=sk-live-abcdef1234567890".to_string(),
            }],
        };
        let event = evaluate_dispatch_boundary(&ctx(), &[signed]).expect("event");
        assert!(
            event.properties["finding_summary"]["total_findings"]
                .as_u64()
                .unwrap_or(0)
                > 0,
            "credential in attachment URL must be detected"
        );

        let inline = ChatMessage {
            role: "user".to_string(),
            content: "see attached".to_string(),
            attachments: vec![tandem_providers::ChatAttachment::ImageUrl {
                url: format!(
                    "data:image/png;base64,{}",
                    "iVBORw0KGgoAAAANSUhEUg".repeat(40)
                ),
            }],
        };
        let event = evaluate_dispatch_boundary(&ctx(), &[inline]).expect("event");
        std::env::remove_var("TANDEM_DATA_BOUNDARY_MODE");
        assert_eq!(
            event.properties["finding_summary"]["total_findings"]
                .as_u64()
                .unwrap_or(u64::MAX),
            0,
            "inline image bytes must not register as findings"
        );
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn policy_from_env_maps_external_raw_policy_and_classes() {
        std::env::set_var("TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY", "redact");
        std::env::set_var("TANDEM_DATA_BOUNDARY_BLOCK_CLASSES", "phi, credential");
        std::env::set_var("TANDEM_DATA_BOUNDARY_MAX_PAYLOAD_BYTES", "1024");
        let policy = data_boundary_policy_from_env(DataBoundaryMode::Audit);
        std::env::remove_var("TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY");
        std::env::remove_var("TANDEM_DATA_BOUNDARY_BLOCK_CLASSES");
        std::env::remove_var("TANDEM_DATA_BOUNDARY_MAX_PAYLOAD_BYTES");

        assert_eq!(policy.redact_classes.len(), SensitiveDataClass::ALL.len());
        assert_eq!(
            policy.block_classes,
            vec![SensitiveDataClass::Phi, SensitiveDataClass::Credential]
        );
        assert_eq!(policy.max_payload_bytes, Some(1024));
        assert!(policy.policy_fingerprint.starts_with("sha256:"));
    }
}
