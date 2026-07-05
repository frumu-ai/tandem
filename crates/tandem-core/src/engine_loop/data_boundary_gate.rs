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
use tandem_types::{EngineEvent, TenantContext};

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
        // Strict enterprise posture: missing tenant context or an Unknown
        // provider classification fails closed in enforce mode (TAN-393).
        strict_fail_closed: std::env::var("TANDEM_DATA_BOUNDARY_STRICT")
            .ok()
            .map(|raw| {
                matches!(
                    raw.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false),
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

/// TAN-393: provider_id → boundary class, with the classification source kept
/// for the audit trail. Only the explicit `TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES`
/// mapping can classify a provider: builtin ids like `ollama`/`llama_cpp`
/// default to loopback URLs but can be reconfigured to remote endpoints, and
/// this gate cannot resolve the configured base URL — so trusting the id
/// alone would let sensitive prompts flow raw to a remote host. Everything
/// unmapped stays `Unknown` (permissive policies treat it as unapproved
/// external; strict policies fail closed). Endpoint-verified classification
/// is a routing-contract TODO (provider-declared boundary_class).
pub(super) fn classify_provider(provider_id: &str) -> (ProviderBoundaryClass, &'static str) {
    if let Ok(raw) = std::env::var("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES") {
        for entry in raw.split(',') {
            let Some((id, class)) = entry.split_once('=') else {
                continue;
            };
            if id.trim() == provider_id {
                if let Some(class) = ProviderBoundaryClass::parse(class) {
                    return (class, "env_mapping");
                }
            }
        }
    }
    (ProviderBoundaryClass::Unknown, "unclassified")
}

/// What the dispatch call site must do with the provider request.
pub(super) enum DataBoundaryDispatchOutcome {
    /// Boundary off: no evaluation ran.
    Off,
    /// Dispatch proceeds unchanged; publish the evidence event.
    Proceed { event: EngineEvent },
    /// Enforce mode: dispatch proceeds with the transformed messages instead
    /// of the originals.
    ProceedTransformed {
        event: EngineEvent,
        messages: Vec<ChatMessage>,
    },
    /// Enforce mode: human approval is required before dispatch. The call
    /// site raises the approval ask with `evidence` (classes/counts/hashes
    /// only) and blocks with `denial_reason` unless explicitly approved.
    RequireApproval {
        event: EngineEvent,
        evidence: Value,
        denial_reason: String,
    },
    /// Enforce mode: the dispatch must not happen.
    Blocked { event: EngineEvent, reason: String },
}

/// Audit-safe one-line explanation for blocked dispatches: class labels and
/// reason codes only — this string is persisted into the session as the
/// user-visible error.
fn blocked_reason(prefix: &str, decision: &tandem_data_boundary::DataBoundaryDecision) -> String {
    let classes = decision
        .finding_summary
        .by_class
        .keys()
        .map(|class| class.placeholder_label())
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{prefix}: provider={} classes=[{}] reason_codes=[{}]",
        decision.provider.provider_id,
        classes,
        decision.reason_codes.join(",")
    )
}

/// Per-message transform for enforce-mode Redact/Tokenize decisions. The
/// crate's flat `transformed_payload` cannot be spliced back into discrete
/// messages, so detection and transformation re-run per message content.
/// Attachment URLs cannot be safely rewritten, so a scannable attachment URL
/// carrying findings fails closed instead of dispatching raw.
fn transform_messages_for_dispatch(
    messages: &[ChatMessage],
    action: tandem_data_boundary::DataBoundaryAction,
) -> Result<(Vec<ChatMessage>, usize), &'static str> {
    let mut transformed = Vec::with_capacity(messages.len());
    let mut transformed_spans = 0usize;
    for message in messages {
        for attachment in &message.attachments {
            let tandem_providers::ChatAttachment::ImageUrl { url } = attachment;
            // Scan exactly what the dispatch evaluator scans: the full URL
            // for remote refs, the metadata prefix (through the comma) for
            // data: URLs — credentials can hide in data-URL parameters too.
            // A URL cannot be safely rewritten, so findings fail closed.
            let scan_target = match data_url_scan_prefix_len(url) {
                Some(prefix_len) => &url[..prefix_len],
                None => url.as_str(),
            };
            if !tandem_data_boundary::detect_sensitive_data(scan_target).is_empty() {
                return Err("attachment_untransformable");
            }
        }
        let findings = tandem_data_boundary::detect_sensitive_data(&message.content);
        if findings.is_empty() {
            transformed.push(message.clone());
            continue;
        }
        transformed_spans += findings.len();
        let content = if action == tandem_data_boundary::DataBoundaryAction::Tokenize {
            tandem_data_boundary::tokenize_sensitive_data(&message.content, &findings).tokenized
        } else {
            tandem_data_boundary::redact_sensitive_data(&message.content, &findings).redacted
        };
        transformed.push(ChatMessage {
            role: message.role.clone(),
            content,
            attachments: message.attachments.clone(),
        });
    }
    Ok((transformed, transformed_spans))
}

/// Evaluates the fully assembled provider request. In audit mode the outcome
/// is always `Proceed` (evidence only); in enforce mode the outcome carries
/// the action the call site must honor. `Off` when the boundary is disabled.
pub(super) fn evaluate_dispatch_boundary(
    ctx: &DataBoundaryDispatchContext<'_>,
    messages: &[ChatMessage],
) -> DataBoundaryDispatchOutcome {
    let mode = data_boundary_mode();
    if mode == DataBoundaryMode::Off {
        return DataBoundaryDispatchOutcome::Off;
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

    let (boundary_class, classification_source) = classify_provider(ctx.provider_id);
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
            boundary_class,
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

    // The crate's flat transformed payload cannot be mapped back onto
    // discrete messages; enforce-mode transforms re-run per message below.
    let decided_event_kind = evaluation.event_kind;
    drop(evaluation.transformed_payload.take());

    let build_event = |kind: tandem_data_boundary::DataBoundaryEventKind,
                       decision: &tandem_data_boundary::DataBoundaryDecision,
                       enforced: bool,
                       extra: &[(&str, Value)]| {
        let boundary_event = DataBoundaryEvent::from_decision(
            format!("dbe_{}", decision.decision_id.trim_start_matches("dbd_")),
            kind,
            chrono::Utc::now().timestamp_millis().max(0) as u64,
            started.elapsed().as_millis() as u64,
            decision,
            Vec::new(),
        );
        let mut properties = serde_json::to_value(&boundary_event).unwrap_or_else(|_| json!({}));
        if let Value::Object(ref mut map) = properties {
            // Envelope keys so the bus derives session scoping (see
            // RuntimeEventEnvelope::derive), plus the dispatch correlation
            // ids the rest of the provider-call event family carries.
            map.insert("sessionID".to_string(), json!(ctx.session_id));
            map.insert("messageID".to_string(), json!(ctx.message_id));
            map.insert("correlationID".to_string(), json!(ctx.correlation_id));
            map.insert("providerID".to_string(), json!(ctx.provider_id));
            map.insert("modelID".to_string(), json!(ctx.model_id));
            map.insert("mode".to_string(), json!(mode.as_str()));
            map.insert(
                "classificationSource".to_string(),
                json!(classification_source),
            );
            map.insert("auditOnly".to_string(), json!(!enforced));
            map.insert("enforced".to_string(), json!(enforced));
            map.insert(
                "decidedEventKind".to_string(),
                json!(decided_event_kind.event_name()),
            );
            for (key, value) in extra {
                map.insert((*key).to_string(), value.clone());
            }
        }
        EngineEvent::new(boundary_event.event_name.clone(), properties)
    };

    // Audit mode never alters dispatch: every decision emits as `.evaluated`
    // with the decided action carried as evidence (never claiming an outcome
    // that did not happen to the dispatched payload).
    if mode == DataBoundaryMode::Audit {
        let event = build_event(
            tandem_data_boundary::DataBoundaryEventKind::Evaluated,
            &evaluation.decision,
            false,
            &[],
        );
        return DataBoundaryDispatchOutcome::Proceed { event };
    }

    use tandem_data_boundary::DataBoundaryAction as Action;
    use tandem_data_boundary::DataBoundaryEventKind as Kind;
    match evaluation.decision.action {
        Action::Allow | Action::AllowWithAudit => {
            let event = build_event(Kind::Evaluated, &evaluation.decision, true, &[]);
            DataBoundaryDispatchOutcome::Proceed { event }
        }
        Action::Redact | Action::Tokenize => {
            match transform_messages_for_dispatch(messages, evaluation.decision.action) {
                Ok((transformed, transformed_spans)) => {
                    let kind = if evaluation.decision.action == Action::Tokenize {
                        Kind::Tokenized
                    } else {
                        Kind::Redacted
                    };
                    let event = build_event(
                        kind,
                        &evaluation.decision,
                        true,
                        &[("transformedSpans", json!(transformed_spans))],
                    );
                    DataBoundaryDispatchOutcome::ProceedTransformed {
                        event,
                        messages: transformed,
                    }
                }
                Err(reason_code) => {
                    let mut decision = evaluation.decision.clone();
                    decision.reason_codes.push(reason_code.to_string());
                    let reason = blocked_reason("DATA_BOUNDARY_BLOCKED", &decision);
                    let event = build_event(Kind::Blocked, &decision, true, &[]);
                    DataBoundaryDispatchOutcome::Blocked { event, reason }
                }
            }
        }
        Action::RequireApproval => {
            let event = build_event(Kind::ApprovalRequired, &evaluation.decision, true, &[]);
            // The approval ask carries class/count evidence only — never
            // payload content. The original raw payload dispatches only on an
            // explicit approval; deny/timeout/cancel all fail closed.
            let evidence = json!({
                "kind": "data_boundary_egress",
                "providerID": ctx.provider_id,
                "modelID": ctx.model_id,
                "decisionID": evaluation.decision.decision_id,
                "payloadHash": evaluation.decision.payload_hash,
                "policyFingerprint": evaluation.decision.policy_fingerprint,
                "findingSummary": serde_json::to_value(&evaluation.decision.finding_summary)
                    .unwrap_or_else(|_| json!({})),
                "reasonCodes": evaluation.decision.reason_codes,
            });
            let denial_reason =
                blocked_reason("DATA_BOUNDARY_APPROVAL_REQUIRED", &evaluation.decision);
            DataBoundaryDispatchOutcome::RequireApproval {
                event,
                evidence,
                denial_reason,
            }
        }
        Action::RouteToLocal => {
            // Routing contract (docs/DATA_BOUNDARY_ROUTING_CONTRACT.md): no
            // routing capability exists yet, so RouteToLocal fails closed
            // rather than silently continuing to the external provider.
            let mut decision = evaluation.decision.clone();
            decision
                .reason_codes
                .push("route_to_local_unavailable".to_string());
            let reason = blocked_reason("DATA_BOUNDARY_BLOCKED", &decision);
            let event = build_event(Kind::RoutedLocal, &decision, true, &[]);
            DataBoundaryDispatchOutcome::Blocked { event, reason }
        }
        Action::Block => {
            let reason = blocked_reason("DATA_BOUNDARY_BLOCKED", &evaluation.decision);
            let event = build_event(Kind::Blocked, &evaluation.decision, true, &[]);
            DataBoundaryDispatchOutcome::Blocked { event, reason }
        }
    }
}

/// TAN-397: audit-only guard for payload sources that become prompt context
/// (tool/MCP results, hook-injected memory/docs/KB). Always evaluates with an
/// audit-mode policy — enforcement stays at the provider-dispatch choke
/// point, which re-scans the fully assembled request. Returns an event only
/// when the source carries findings, so clean sources add no event volume.
pub(super) fn evaluate_context_source(
    session_id: &str,
    source_kind: &str,
    tool_name: Option<&str>,
    content: &str,
    operation_kind: DataBoundaryOperationKind,
    tenant_context: Option<&TenantContext>,
) -> Option<EngineEvent> {
    if data_boundary_mode() == DataBoundaryMode::Off {
        return None;
    }
    let started = Instant::now();
    // Sources are scanned before a provider is chosen; enforcement decisions
    // are meaningless here, so the policy is pinned to audit mode.
    let policy = data_boundary_policy_from_env(DataBoundaryMode::Audit);
    // Carry the session's tenant so the audit bridge attributes the record
    // to the right tenant; a local-implicit tenant stays unattributed (the
    // same "never positively established" rule as the dispatch gate).
    let tenant = tenant_context
        .filter(|tenant| !tenant.is_local_implicit())
        .map(|tenant| DataBoundaryTenantRef {
            organization_id: Some(tenant.org_id.clone()),
            workspace_id: Some(tenant.workspace_id.clone()),
            deployment_id: tenant.deployment_id.clone(),
        })
        .unwrap_or_default();
    let input = DataBoundaryInput {
        input_id: format!("dbi_src_{session_id}"),
        tenant,
        provider: DataBoundaryProviderRef {
            provider_id: "pending_dispatch".to_string(),
            model_id: None,
            boundary_class: ProviderBoundaryClass::Unknown,
        },
        operation: DataBoundaryOperationRef {
            operation_id: format!("src_{source_kind}"),
            kind: operation_kind,
            tool_name: tool_name.map(str::to_string),
            source_ref: Some(format!("context_source.{source_kind}")),
        },
        payload_hash: payload_hash(content.as_bytes()),
        payload_bytes: content.len() as u64,
        source_refs: Vec::new(),
        data_classes: Vec::new(),
        action_tags: Vec::new(),
    };
    let evaluation = evaluate_data_boundary(
        &DataBoundaryEvaluationRequest {
            input: &input,
            payload: Some(content),
            detector_config: None,
        },
        &policy,
    );
    if evaluation.decision.action == tandem_data_boundary::DataBoundaryAction::Allow {
        return None;
    }
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
        map.insert("sessionID".to_string(), json!(session_id));
        map.insert("sourceKind".to_string(), json!(source_kind));
        map.insert("mode".to_string(), json!(data_boundary_mode().as_str()));
        map.insert("toolName".to_string(), json!(tool_name));
        map.insert("auditOnly".to_string(), json!(true));
        map.insert("enforced".to_string(), json!(false));
    }
    Some(EngineEvent::new(
        boundary_event.event_name.clone(),
        properties,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_proceed_event(outcome: DataBoundaryDispatchOutcome) -> EngineEvent {
        match outcome {
            DataBoundaryDispatchOutcome::Proceed { event } => event,
            _ => panic!("expected Proceed outcome"),
        }
    }

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
        assert!(matches!(
            evaluate_dispatch_boundary(&ctx(), &messages),
            DataBoundaryDispatchOutcome::Off
        ));
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
        let event = expect_proceed_event(evaluate_dispatch_boundary(&ctx(), &messages));
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
        let event = expect_proceed_event(evaluate_dispatch_boundary(&ctx(), &messages));
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
        let event = expect_proceed_event(evaluate_dispatch_boundary(&ctx(), &[signed]));
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
        let event = expect_proceed_event(evaluate_dispatch_boundary(&ctx(), &[inline]));
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
    fn classifier_uses_env_mapping_then_builtin_then_unknown() {
        std::env::set_var(
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            "openai=approved_external, azure=customer_hosted",
        );
        assert_eq!(
            classify_provider("openai"),
            (ProviderBoundaryClass::ApprovedExternal, "env_mapping")
        );
        assert_eq!(
            classify_provider("azure"),
            (ProviderBoundaryClass::CustomerHosted, "env_mapping")
        );
        std::env::remove_var("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES");
        // Builtin loopback ids get no id-based trust: their base URLs can be
        // reconfigured to remote endpoints, so unmapped ids stay Unknown.
        assert_eq!(
            classify_provider("ollama"),
            (ProviderBoundaryClass::Unknown, "unclassified")
        );
        assert_eq!(
            classify_provider("openai"),
            (ProviderBoundaryClass::Unknown, "unclassified")
        );
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn enforce_blocks_raw_sensitive_to_unclassified_provider() {
        std::env::set_var("TANDEM_DATA_BOUNDARY_MODE", "enforce");
        let secret = "sk-live-abcdef1234567890";
        let messages = vec![chat("user", &format!("api_key={secret}"))];
        let outcome = evaluate_dispatch_boundary(&ctx(), &messages);
        std::env::remove_var("TANDEM_DATA_BOUNDARY_MODE");

        match outcome {
            DataBoundaryDispatchOutcome::Blocked { event, reason } => {
                assert_eq!(event.event_type, "data_boundary.blocked");
                assert_eq!(event.properties["enforced"], true);
                assert_eq!(event.properties["classificationSource"], "unclassified");
                assert!(reason.starts_with("DATA_BOUNDARY_BLOCKED"));
                assert!(reason.contains("CREDENTIAL"));
                assert!(!reason.contains(secret), "reason must be audit-safe");
            }
            _ => panic!("expected Blocked outcome"),
        }
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn enforce_redact_policy_transforms_dispatched_messages() {
        std::env::set_var("TANDEM_DATA_BOUNDARY_MODE", "enforce");
        std::env::set_var(
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            "openai=approved_external",
        );
        std::env::set_var("TANDEM_DATA_BOUNDARY_REDACT_CLASSES", "credential,pii");
        let secret = "sk-live-abcdef1234567890";
        let messages = vec![
            chat("system", "you are helpful"),
            chat("user", &format!("use api_key={secret} please")),
        ];
        let outcome = evaluate_dispatch_boundary(&ctx(), &messages);
        std::env::remove_var("TANDEM_DATA_BOUNDARY_MODE");
        std::env::remove_var("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES");
        std::env::remove_var("TANDEM_DATA_BOUNDARY_REDACT_CLASSES");

        match outcome {
            DataBoundaryDispatchOutcome::ProceedTransformed { event, messages } => {
                assert_eq!(event.event_type, "data_boundary.redacted");
                assert_eq!(event.properties["enforced"], true);
                assert!(event.properties["transformedSpans"].as_u64().unwrap_or(0) > 0);
                let joined = messages
                    .iter()
                    .map(|m| m.content.clone())
                    .collect::<Vec<_>>()
                    .join("\n");
                assert!(
                    !joined.contains(secret),
                    "secret must be redacted: {joined}"
                );
                assert!(joined.contains("[REDACTED:"));
                assert!(
                    joined.contains("you are helpful"),
                    "clean content untouched"
                );
            }
            _ => panic!("expected ProceedTransformed outcome"),
        }
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn enforce_approval_classes_require_approval_with_safe_evidence() {
        std::env::set_var("TANDEM_DATA_BOUNDARY_MODE", "enforce");
        std::env::set_var(
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            "openai=approved_external",
        );
        std::env::set_var("TANDEM_DATA_BOUNDARY_APPROVAL_CLASSES", "credential");
        let secret = "sk-live-abcdef1234567890";
        let messages = vec![chat("user", &format!("api_key={secret}"))];
        let outcome = evaluate_dispatch_boundary(&ctx(), &messages);
        std::env::remove_var("TANDEM_DATA_BOUNDARY_MODE");
        std::env::remove_var("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES");
        std::env::remove_var("TANDEM_DATA_BOUNDARY_APPROVAL_CLASSES");

        match outcome {
            DataBoundaryDispatchOutcome::RequireApproval {
                event,
                evidence,
                denial_reason,
            } => {
                assert_eq!(event.event_type, "data_boundary.approval_required");
                let serialized = serde_json::to_string(&evidence).expect("evidence json");
                assert!(!serialized.contains(secret), "evidence must be safe");
                assert!(serialized.contains("findingSummary"));
                assert!(denial_reason.starts_with("DATA_BOUNDARY_APPROVAL_REQUIRED"));
            }
            _ => panic!("expected RequireApproval outcome"),
        }
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn enforce_require_local_fails_closed_without_routing() {
        std::env::set_var("TANDEM_DATA_BOUNDARY_MODE", "enforce");
        std::env::set_var(
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            "openai=approved_external",
        );
        std::env::set_var("TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY", "require_local");
        let messages = vec![chat("user", "api_key=sk-live-abcdef1234567890")];
        let outcome = evaluate_dispatch_boundary(&ctx(), &messages);
        std::env::remove_var("TANDEM_DATA_BOUNDARY_MODE");
        std::env::remove_var("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES");
        std::env::remove_var("TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY");

        match outcome {
            DataBoundaryDispatchOutcome::Blocked { event, reason } => {
                assert_eq!(event.event_type, "data_boundary.routed_local");
                assert!(reason.contains("route_to_local_unavailable"));
            }
            _ => panic!("expected Blocked outcome for unroutable require_local"),
        }
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn enforce_strict_fails_closed_on_unclassified_provider_even_when_clean() {
        std::env::set_var("TANDEM_DATA_BOUNDARY_MODE", "enforce");
        std::env::set_var("TANDEM_DATA_BOUNDARY_STRICT", "1");
        let messages = vec![chat("user", "hello there")];
        let outcome = evaluate_dispatch_boundary(&ctx(), &messages);
        std::env::remove_var("TANDEM_DATA_BOUNDARY_MODE");
        std::env::remove_var("TANDEM_DATA_BOUNDARY_STRICT");

        match outcome {
            DataBoundaryDispatchOutcome::Blocked { reason, .. } => {
                assert!(reason.contains("unknown_provider_boundary_class"));
            }
            _ => panic!("expected strict fail-closed Blocked outcome"),
        }
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn source_guard_attributes_explicit_tenant_and_drops_local_implicit() {
        // Codex P2 (PR #1788): source-guard events must carry the session's
        // tenant so the audit bridge files them under the right tenant and
        // the tenant-scoped monitoring read model can see them.
        std::env::set_var("TANDEM_DATA_BOUNDARY_MODE", "audit");
        let mut tenant = TenantContext::local_implicit();
        tenant.org_id = "org-src".to_string();
        tenant.workspace_id = "workspace-src".to_string();
        let event = evaluate_context_source(
            "session-1",
            "tool_result",
            Some("web_fetch"),
            "api_key=sk-live-abcdef1234567890",
            DataBoundaryOperationKind::ToolCall,
            Some(&tenant),
        )
        .expect("findings must produce an event");
        assert_eq!(event.properties["tenant"]["organization_id"], "org-src");
        assert_eq!(event.properties["tenant"]["workspace_id"], "workspace-src");

        let implicit = evaluate_context_source(
            "session-1",
            "tool_result",
            Some("web_fetch"),
            "api_key=sk-live-abcdef1234567890",
            DataBoundaryOperationKind::ToolCall,
            Some(&TenantContext::local_implicit()),
        )
        .expect("findings must produce an event");
        std::env::remove_var("TANDEM_DATA_BOUNDARY_MODE");
        assert!(
            implicit.properties["tenant"]
                .get("organization_id")
                .is_none(),
            "local-implicit tenancy must stay unattributed"
        );
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn enforce_allows_env_classified_local_provider_with_sensitive_payload() {
        std::env::set_var("TANDEM_DATA_BOUNDARY_MODE", "enforce");
        std::env::set_var("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES", "ollama=local");
        let mut context = ctx();
        context.provider_id = "ollama";
        let messages = vec![chat("user", "api_key=sk-live-abcdef1234567890")];
        let outcome = evaluate_dispatch_boundary(&context, &messages);
        std::env::remove_var("TANDEM_DATA_BOUNDARY_MODE");
        std::env::remove_var("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES");

        match outcome {
            DataBoundaryDispatchOutcome::Proceed { event } => {
                assert_eq!(event.event_type, "data_boundary.evaluated");
                assert_eq!(event.properties["action"], "allow_with_audit");
                assert_eq!(event.properties["classificationSource"], "env_mapping");
                assert_eq!(event.properties["enforced"], true);
            }
            _ => panic!("expected Proceed outcome for local provider"),
        }
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn enforce_blocks_untransformable_data_url_prefix_findings() {
        // Codex P1 (PR #1787): a credential hidden in a data: URL's metadata
        // parameters (before the comma) is detected by the evaluator, so the
        // transform path must fail closed on it too — the attachment cannot
        // be rewritten and must not dispatch raw under a transform policy.
        std::env::set_var("TANDEM_DATA_BOUNDARY_MODE", "enforce");
        std::env::set_var(
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            "openai=approved_external",
        );
        std::env::set_var("TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY", "redact");
        let message = ChatMessage {
            role: "user".to_string(),
            content: "see attached".to_string(),
            attachments: vec![tandem_providers::ChatAttachment::ImageUrl {
                url: format!(
                    "data:image/png;api_key=sk-live-abcdef1234567890;base64,{}",
                    "iVBORw0KGgo".repeat(20)
                ),
            }],
        };
        let outcome = evaluate_dispatch_boundary(&ctx(), &[message]);
        std::env::remove_var("TANDEM_DATA_BOUNDARY_MODE");
        std::env::remove_var("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES");
        std::env::remove_var("TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY");

        match outcome {
            DataBoundaryDispatchOutcome::Blocked { reason, .. } => {
                assert!(reason.contains("attachment_untransformable"), "{reason}");
                assert!(!reason.contains("sk-live-abcdef1234567890"));
            }
            _ => panic!("expected Blocked outcome for untransformable data URL"),
        }
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
