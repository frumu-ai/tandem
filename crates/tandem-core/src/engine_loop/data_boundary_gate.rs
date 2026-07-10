//! Structured-chat adapter for the canonical `tandem-data-boundary` provider
//! egress evaluator. Detection runs once in the lower-level crate; this module
//! maps transformed fields back onto provider messages and preserves the
//! engine's approval and runtime-event contracts.

use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};
use std::time::Instant;
use tandem_data_boundary::{
    classify_provider_with, evaluate_data_boundary, evaluate_provider_egress_with_policy,
    payload_hash, provider_egress_mode_with, provider_egress_policy_with, DataBoundaryAction,
    DataBoundaryDetectorConfig, DataBoundaryEvaluationRequest, DataBoundaryEvent,
    DataBoundaryEventKind, DataBoundaryInput, DataBoundaryMode, DataBoundaryOperationKind,
    DataBoundaryOperationRef, DataBoundaryPolicy, DataBoundaryProviderRef, DataBoundaryTenantRef,
    ProviderBoundaryClass, ProviderEgressApproval, ProviderEgressAuditEvent,
    ProviderEgressAuthority, ProviderEgressDisposition, ProviderEgressField, ProviderEgressPermit,
    ProviderEgressRequest, SensitiveDataClass,
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

/// Test-support: per-scope-id (session or automation run) boundary
/// configuration. A registered scope fully defines the `TANDEM_DATA_BOUNDARY_*`
/// configuration for evaluations attributed to that scope — keys absent from
/// the map read as unset, and the process environment is not consulted.
/// Unregistered scopes (all of production) resolve from the environment.
/// This exists so tests can exercise boundary modes without `std::env`
/// mutation, which leaks into every concurrently running test in the process.
static SCOPED_BOUNDARY_CONFIG: LazyLock<RwLock<HashMap<String, HashMap<String, Option<String>>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// RAII override for one boundary configuration key, scoped to one session or
/// automation-run id. Dropping the guard removes the key (and the scope once
/// its last key is gone).
pub struct ScopedDataBoundaryConfigOverride {
    scope_id: String,
    key: String,
    previous: Option<Option<String>>,
}

impl ScopedDataBoundaryConfigOverride {
    pub fn set(scope_id: &str, key: &str, value: Option<&str>) -> Self {
        let mut scopes = SCOPED_BOUNDARY_CONFIG
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = scopes
            .entry(scope_id.to_string())
            .or_default()
            .insert(key.to_string(), value.map(str::to_string));
        Self {
            scope_id: scope_id.to_string(),
            key: key.to_string(),
            previous,
        }
    }
}

impl Drop for ScopedDataBoundaryConfigOverride {
    fn drop(&mut self) {
        let mut scopes = SCOPED_BOUNDARY_CONFIG
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(map) = scopes.get_mut(&self.scope_id) {
            match self.previous.take() {
                Some(previous) => {
                    map.insert(self.key.clone(), previous);
                }
                None => {
                    map.remove(&self.key);
                }
            }
            if map.is_empty() {
                scopes.remove(&self.scope_id);
            }
        }
    }
}

/// Boundary configuration lookup for one scope id: a registered scope is the
/// complete configuration; otherwise the process environment applies.
fn scoped_boundary_lookup(scope_id: &str) -> impl Fn(&str) -> Option<String> + '_ {
    move |name: &str| {
        let scopes = SCOPED_BOUNDARY_CONFIG
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match scopes.get(scope_id) {
            Some(map) => map.get(name).cloned().flatten(),
            None => std::env::var(name).ok(),
        }
    }
}

fn data_boundary_mode_for(scope_id: &str) -> DataBoundaryMode {
    provider_egress_mode_with(&scoped_boundary_lookup(scope_id))
}

fn data_boundary_policy_for(scope_id: &str, mode: DataBoundaryMode) -> DataBoundaryPolicy {
    provider_egress_policy_with(mode, &scoped_boundary_lookup(scope_id))
}

pub struct DataBoundaryDispatchContext<'a> {
    pub session_id: &'a str,
    pub run_id: Option<&'a str>,
    pub message_id: &'a str,
    pub correlation_id: Option<&'a str>,
    pub provider_id: &'a str,
    pub model_id: Option<&'a str>,
    pub tool_schema_payload: Option<&'a str>,
    pub source_ref: &'a str,
    pub data_classes: &'a [SensitiveDataClass],
    pub authority_ref: Option<&'a str>,
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
pub(super) fn classify_provider(
    scope_id: &str,
    provider_id: &str,
) -> (ProviderBoundaryClass, &'static str) {
    classify_provider_with(provider_id, &scoped_boundary_lookup(scope_id))
}

/// What the dispatch call site must do with the provider request.
pub enum DataBoundaryDispatchOutcome {
    /// Boundary off: no evaluation ran.
    Off { permit: ProviderEgressPermit },
    /// Dispatch proceeds unchanged; publish the evidence event.
    Proceed {
        event: EngineEvent,
        permit: ProviderEgressPermit,
    },
    /// Enforce mode: dispatch proceeds with the transformed messages instead
    /// of the originals.
    ProceedTransformed {
        event: EngineEvent,
        messages: Vec<ChatMessage>,
        permit: ProviderEgressPermit,
    },
    /// Enforce mode: human approval is required before dispatch. The call
    /// site raises the approval ask with `evidence` (classes/counts/hashes
    /// only) and blocks with `denial_reason` unless explicitly approved.
    RequireApproval {
        event: EngineEvent,
        evidence: Value,
        denial_reason: String,
        approval: ProviderEgressApproval,
    },
    /// Enforce mode: the dispatch must not happen.
    Blocked { event: EngineEvent, reason: String },
}

/// Audit-safe one-line explanation for blocked dispatches: class labels and
/// reason codes only — this string is persisted into the session as the
/// user-visible error.
fn chat_fields<'a>(
    messages: &'a [ChatMessage],
    tool_schema_payload: Option<&'a str>,
) -> (Vec<ProviderEgressField<'a>>, Vec<(usize, usize)>) {
    let mut fields = Vec::new();
    let mut message_content_fields = Vec::with_capacity(messages.len());
    for (message_index, message) in messages.iter().enumerate() {
        // An empty role represents a prompt-only completion request. Only the
        // prompt content crosses that provider boundary.
        if !message.role.is_empty() {
            fields.push(ProviderEgressField::untransformable(
                Cow::Owned(format!("message.{message_index}.role")),
                Cow::Borrowed(message.role.as_str()),
            ));
        }
        let field_index = fields.len();
        fields.push(ProviderEgressField::transformable(
            Cow::Owned(format!("message.{message_index}.content")),
            Cow::Borrowed(message.content.as_str()),
        ));
        message_content_fields.push((message_index, field_index));
        for (attachment_index, attachment) in message.attachments.iter().enumerate() {
            let tandem_providers::ChatAttachment::ImageUrl { url } = attachment;
            let label = Cow::Owned(format!(
                "message.{message_index}.attachment.{attachment_index}.url"
            ));
            if let Some(prefix_len) = data_url_scan_prefix_len(url) {
                fields.push(ProviderEgressField::untransformable_with_binding(
                    label,
                    Cow::Borrowed(&url[..prefix_len]),
                    Cow::Borrowed(url.as_str()),
                ));
            } else {
                fields.push(ProviderEgressField::untransformable(
                    label,
                    Cow::Borrowed(url.as_str()),
                ));
            }
        }
    }
    if let Some(tool_schema_payload) = tool_schema_payload {
        match serde_json::from_str::<Value>(tool_schema_payload) {
            Ok(value) => append_tool_schema_strings(&value, &mut fields),
            Err(_) => fields.push(ProviderEgressField::untransformable(
                Cow::Borrowed("tool_schema.invalid_json"),
                Cow::Borrowed(tool_schema_payload),
            )),
        }
    }
    (fields, message_content_fields)
}

/// Tool schemas are structured metadata. Scan the values that actually leave
/// the process, but not JSON property names: common schema keys such as
/// `secret`, `token`, and `password` are vocabulary rather than credentials.
/// Schema strings remain untransformable because rewriting a name,
/// description, or enum would change the provider/tool contract. The generic
/// high-entropy heuristic is disabled for schema identifiers; strong
/// credential, key, PII, and marker detectors remain enabled.
fn append_tool_schema_strings<'a>(value: &Value, fields: &mut Vec<ProviderEgressField<'a>>) {
    match value {
        Value::String(value) => {
            let index = fields.len();
            fields.push(ProviderEgressField::untransformable_with_detector_config(
                Cow::Owned(format!("tool_schema.value.{index}")),
                Cow::Owned(value.clone()),
                DataBoundaryDetectorConfig {
                    detect_high_entropy: false,
                    ..DataBoundaryDetectorConfig::default()
                },
            ));
        }
        Value::Array(values) => {
            for value in values {
                append_tool_schema_strings(value, fields);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                append_tool_schema_strings(value, fields);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn to_engine_event(
    event: ProviderEgressAuditEvent,
    message_id: &str,
    correlation_id: Option<&str>,
) -> EngineEvent {
    let event_name = event.boundary.event_name.clone();
    let mut properties = serde_json::to_value(event).unwrap_or_else(|_| json!({}));
    if let Value::Object(ref mut map) = properties {
        map.insert("messageID".to_string(), json!(message_id));
        map.insert("correlationID".to_string(), json!(correlation_id));
    }
    EngineEvent::new(event_name, properties)
}

/// Evaluates the fully assembled provider request. In audit mode the outcome
/// is always `Proceed` (evidence only); in enforce mode the outcome carries
/// the action the call site must honor. `Off` when the boundary is disabled.
pub fn evaluate_dispatch_boundary(
    ctx: &DataBoundaryDispatchContext<'_>,
    messages: &[ChatMessage],
) -> DataBoundaryDispatchOutcome {
    let mode = data_boundary_mode_for(ctx.session_id);
    let policy = data_boundary_policy_for(ctx.session_id, mode);
    let (boundary_class, classification_source) =
        classify_provider(ctx.session_id, ctx.provider_id);
    let must_block_uninspected_media = mode == DataBoundaryMode::Enforce
        && policy.strict_fail_closed
        && !boundary_class.is_internal()
        && messages
            .iter()
            .any(|message| !message.attachments.is_empty());
    let authority = ProviderEgressAuthority {
        tenant: DataBoundaryTenantRef {
            organization_id: ctx.org_id.map(str::to_string),
            workspace_id: ctx.workspace_id.map(str::to_string),
            deployment_id: ctx.deployment_id.map(str::to_string),
        },
        run_id: ctx.run_id.map(str::to_string),
        session_id: Some(ctx.session_id.to_string()),
        authority_ref: ctx.authority_ref.map(str::to_string),
    };
    let (fields, message_content_fields) = chat_fields(messages, ctx.tool_schema_payload);
    let request = ProviderEgressRequest {
        authority: &authority,
        operation_id: ctx.message_id,
        source_ref: ctx.source_ref,
        provider_id: ctx.provider_id,
        model_id: ctx.model_id,
        fields: &fields,
        data_classes: ctx.data_classes,
        action_tags: &[],
    };
    let mut evaluation = evaluate_provider_egress_with_policy(
        &request,
        &policy,
        boundary_class,
        classification_source,
    );
    if evaluation.disposition == ProviderEgressDisposition::Off {
        return DataBoundaryDispatchOutcome::Off {
            permit: evaluation
                .take_dispatch_permit()
                .expect("off boundary evaluation authorizes the dispatch route"),
        };
    }
    let mut audit_event = evaluation
        .event
        .take()
        .expect("enabled boundary emits an event");
    if must_block_uninspected_media {
        const REASON_CODE: &str = "uninspected_media_external_provider";
        audit_event.boundary.event_name = DataBoundaryEventKind::Blocked.event_name().to_string();
        audit_event.boundary.event_kind = DataBoundaryEventKind::Blocked;
        audit_event.boundary.action = DataBoundaryAction::Block;
        if !audit_event
            .boundary
            .reason_codes
            .iter()
            .any(|code| code == REASON_CODE)
        {
            audit_event
                .boundary
                .reason_codes
                .push(REASON_CODE.to_string());
        }
        audit_event.decided_event_kind = DataBoundaryEventKind::Blocked.event_name().to_string();
        evaluation.disposition = ProviderEgressDisposition::Blocked;
        evaluation.blocked_reason = Some(format!(
            "DATA_BOUNDARY_BLOCKED: provider={} reason_codes=[{REASON_CODE}]",
            ctx.provider_id
        ));
    }
    let evidence = json!({
        "kind": "data_boundary_egress",
        "providerID": ctx.provider_id,
        "modelID": ctx.model_id,
        "decisionID": &audit_event.boundary.decision_id,
        "payloadHash": &audit_event.boundary.payload_hash,
        "policyFingerprint": &audit_event.boundary.policy_fingerprint,
        "findingSummary": &audit_event.boundary.finding_summary,
        "semanticDataClasses": &audit_event.semantic_data_classes,
        "reasonCodes": &audit_event.boundary.reason_codes,
    });
    let event = to_engine_event(audit_event, ctx.message_id, ctx.correlation_id);
    match evaluation.disposition {
        ProviderEgressDisposition::Off => unreachable!("off disposition returned above"),
        ProviderEgressDisposition::Proceed => {
            let permit = evaluation
                .take_dispatch_permit()
                .expect("proceed boundary evaluation authorizes the dispatch route");
            if let Some(transformed_fields) = evaluation.transformed_fields {
                let mut transformed = messages.to_vec();
                for (message_index, field_index) in message_content_fields {
                    transformed[message_index].content = transformed_fields[field_index].clone();
                }
                DataBoundaryDispatchOutcome::ProceedTransformed {
                    event,
                    messages: transformed,
                    permit,
                }
            } else {
                DataBoundaryDispatchOutcome::Proceed { event, permit }
            }
        }
        ProviderEgressDisposition::RequireApproval => {
            let approval = evaluation
                .take_approval()
                .expect("approval disposition carries a pending permit");
            let denial_reason = evaluation.blocked_reason.unwrap_or_else(|| {
                "DATA_BOUNDARY_APPROVAL_REQUIRED: missing decision reason".to_string()
            });
            DataBoundaryDispatchOutcome::RequireApproval {
                event,
                evidence,
                denial_reason,
                approval,
            }
        }
        ProviderEgressDisposition::Blocked => DataBoundaryDispatchOutcome::Blocked {
            event,
            reason: evaluation
                .blocked_reason
                .unwrap_or_else(|| "DATA_BOUNDARY_BLOCKED: missing decision reason".to_string()),
        },
    }
}

/// What the scanned payload source belongs to: an interactive session or an
/// automation run (workflow artifacts fold into prompts before any session
/// exists). The id lands in the event as `sessionID` or `runID` accordingly,
/// so operators never see a run id masquerading as a session.
#[derive(Debug, Clone, Copy)]
pub enum ContextSourceScope<'a> {
    Session(&'a str),
    AutomationRun(&'a str),
}

impl<'a> ContextSourceScope<'a> {
    fn id(&self) -> &'a str {
        match self {
            Self::Session(id) | Self::AutomationRun(id) => id,
        }
    }

    fn property_key(&self) -> &'static str {
        match self {
            Self::Session(_) => "sessionID",
            Self::AutomationRun(_) => "runID",
        }
    }
}

/// TAN-397/TAN-600: audit-only guard for payload sources that become prompt
/// context (tool/MCP results, hook-injected memory/docs/KB, workflow
/// artifacts). Always evaluates with an audit-mode policy — enforcement
/// stays at the provider-dispatch choke point, which re-scans the fully
/// assembled request. Returns an event only when the source carries
/// findings, so clean sources add no event volume. Public so server-side
/// prompt builders (automation artifact folding) share this exact policy
/// parsing and event shape instead of growing a second implementation.
pub fn evaluate_context_source(
    scope: ContextSourceScope<'_>,
    source_kind: &str,
    tool_name: Option<&str>,
    content: &str,
    operation_kind: DataBoundaryOperationKind,
    tenant_context: Option<&TenantContext>,
) -> Option<EngineEvent> {
    let scope_id = scope.id();
    let mode = data_boundary_mode_for(scope_id);
    if mode == DataBoundaryMode::Off {
        return None;
    }
    let started = Instant::now();
    // Sources are scanned before a provider is chosen; enforcement decisions
    // are meaningless here, so the policy is pinned to audit mode.
    let policy = data_boundary_policy_for(scope_id, DataBoundaryMode::Audit);
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
        input_id: format!("dbi_src_{scope_id}"),
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
        map.insert(scope.property_key().to_string(), json!(scope_id));
        map.insert("sourceKind".to_string(), json!(source_kind));
        map.insert("mode".to_string(), json!(mode.as_str()));
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
    use tandem_data_boundary::{provider_egress_policy_with, SensitiveDataClass};

    // Scope id used by ctx(): overrides registered here are visible only to
    // evaluations attributed to this scope.
    const GATE_TEST_SCOPE: &str = "ses_db_1";

    fn expect_proceed_event(outcome: DataBoundaryDispatchOutcome) -> EngineEvent {
        match outcome {
            DataBoundaryDispatchOutcome::Proceed { event, .. } => event,
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
            run_id: Some("run_db_1"),
            message_id: "msg_db_1",
            correlation_id: None,
            provider_id: "openai",
            model_id: Some("gpt-test"),
            tool_schema_payload: None,
            source_ref: "engine_loop.provider_dispatch",
            data_classes: &[],
            authority_ref: None,
            org_id: Some("local"),
            workspace_id: Some("local"),
            deployment_id: None,
        }
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn off_mode_emits_nothing() {
        let _ovr_mode = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_MODE",
            None,
        );
        let messages = vec![chat("user", "api_key=sk-live-abcdef1234567890")];
        assert!(matches!(
            evaluate_dispatch_boundary(&ctx(), &messages),
            DataBoundaryDispatchOutcome::Off { .. }
        ));
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn audit_mode_emits_safe_event_with_findings() {
        let _ovr1 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_MODE",
            Some("audit"),
        );
        let secret = "sk-live-abcdef1234567890";
        let messages = vec![
            chat("system", "you are helpful"),
            chat("user", &format!("use api_key={secret} please")),
        ];
        let event = expect_proceed_event(evaluate_dispatch_boundary(&ctx(), &messages));

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
        let _ovr2 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_MODE",
            Some("audit"),
        );
        let _ovr3 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY",
            Some("redact"),
        );
        let messages = vec![chat("user", "use api_key=sk-live-abcdef1234567890")];
        let event = expect_proceed_event(evaluate_dispatch_boundary(&ctx(), &messages));

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
        let _ovr4 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_MODE",
            Some("audit"),
        );
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
    fn strict_enforce_blocks_uninspected_media_for_external_providers() {
        let _ovr5 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_MODE",
            Some("enforce"),
        );
        let _ovr6 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_STRICT",
            Some("1"),
        );
        let _ovr7 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            Some("openai=approved_external"),
        );

        for url in [
            "data:image/png;base64,iVBORw0KGgoAAAANSUhEUg",
            "https://cdn.example.com/clean-image.png",
        ] {
            let message = ChatMessage {
                role: "user".to_string(),
                content: "see attached".to_string(),
                attachments: vec![tandem_providers::ChatAttachment::ImageUrl {
                    url: url.to_string(),
                }],
            };
            match evaluate_dispatch_boundary(&ctx(), &[message]) {
                DataBoundaryDispatchOutcome::Blocked { event, reason } => {
                    assert_eq!(event.event_type, "data_boundary.blocked");
                    assert_eq!(event.properties["action"], "block");
                    assert_eq!(event.properties["enforced"], true);
                    assert!(reason.contains("uninspected_media_external_provider"));
                    assert!(event.properties["reason_codes"]
                        .as_array()
                        .is_some_and(|codes| codes
                            .iter()
                            .any(|code| code == "uninspected_media_external_provider")));
                }
                _ => panic!("strict external dispatch must block uninspected media"),
            }
        }
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn strict_enforce_allows_media_for_internal_providers() {
        let _ovr8 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_MODE",
            Some("enforce"),
        );
        let _ovr9 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_STRICT",
            Some("1"),
        );
        let _ovr10 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            Some("openai=local"),
        );
        let message = ChatMessage {
            role: "user".to_string(),
            content: "see attached".to_string(),
            attachments: vec![tandem_providers::ChatAttachment::ImageUrl {
                url: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUg".to_string(),
            }],
        };
        let outcome = evaluate_dispatch_boundary(&ctx(), &[message]);

        assert!(matches!(
            outcome,
            DataBoundaryDispatchOutcome::Proceed { .. }
        ));
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn classifier_uses_env_mapping_then_builtin_then_unknown() {
        let mapping_override = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            Some("openai=approved_external, azure=customer_hosted"),
        );
        assert_eq!(
            classify_provider(GATE_TEST_SCOPE, "openai"),
            (ProviderBoundaryClass::ApprovedExternal, "env_mapping")
        );
        assert_eq!(
            classify_provider(GATE_TEST_SCOPE, "azure"),
            (ProviderBoundaryClass::CustomerHosted, "env_mapping")
        );
        drop(mapping_override);
        // Builtin loopback ids get no id-based trust: their base URLs can be
        // reconfigured to remote endpoints, so unmapped ids stay Unknown.
        assert_eq!(
            classify_provider(GATE_TEST_SCOPE, "ollama"),
            (ProviderBoundaryClass::Unknown, "unclassified")
        );
        assert_eq!(
            classify_provider(GATE_TEST_SCOPE, "openai"),
            (ProviderBoundaryClass::Unknown, "unclassified")
        );
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn enforce_blocks_raw_sensitive_to_unclassified_provider() {
        let _ovr12 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_MODE",
            Some("enforce"),
        );
        let secret = "sk-live-abcdef1234567890";
        let messages = vec![chat("user", &format!("api_key={secret}"))];
        let outcome = evaluate_dispatch_boundary(&ctx(), &messages);

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
    fn semantic_source_code_is_blocked_without_regex_findings() {
        let _ovr13 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_MODE",
            Some("enforce"),
        );
        let _ovr14 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            Some("openai=approved_external"),
        );
        let _ovr15 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_BLOCK_CLASSES",
            Some("source_code"),
        );
        let classes = [SensitiveDataClass::SourceCode];
        let mut context = ctx();
        context.data_classes = &classes;
        let outcome = evaluate_dispatch_boundary(&context, &[chat("user", "ordinary text")]);

        match outcome {
            DataBoundaryDispatchOutcome::Blocked { event, reason } => {
                assert!(reason.contains("SOURCE_CODE"));
                assert_eq!(event.properties["finding_summary"]["total_findings"], 0);
                assert_eq!(event.properties["semanticDataClasses"][0], "source_code");
            }
            _ => panic!("semantic source code must be governed"),
        }
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn strict_mode_requires_run_and_session_authority() {
        // The blanked session id below is also the config scope the gate
        // resolves against, so the overrides register under that scope.
        let _ovr16 = ScopedDataBoundaryConfigOverride::set(
            " ",
            "TANDEM_DATA_BOUNDARY_MODE",
            Some("enforce"),
        );
        let _ovr17 =
            ScopedDataBoundaryConfigOverride::set(" ", "TANDEM_DATA_BOUNDARY_STRICT", Some("1"));
        let _ovr18 = ScopedDataBoundaryConfigOverride::set(
            " ",
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            Some("openai=approved_external"),
        );
        let mut context = ctx();
        context.run_id = None;
        context.session_id = " ";
        let outcome = evaluate_dispatch_boundary(&context, &[chat("user", "ordinary text")]);

        match outcome {
            DataBoundaryDispatchOutcome::Blocked { reason, .. } => {
                assert!(reason.contains("missing_run_authority"));
                assert!(reason.contains("missing_session_authority"));
            }
            _ => panic!("strict mode must require both execution identifiers"),
        }
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn enforce_redact_policy_transforms_dispatched_messages() {
        let _ovr19 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_MODE",
            Some("enforce"),
        );
        let _ovr20 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            Some("openai=approved_external"),
        );
        let _ovr21 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_REDACT_CLASSES",
            Some("credential,pii"),
        );
        let secret = "sk-live-abcdef1234567890";
        let messages = vec![
            chat("system", "you are helpful"),
            chat("user", &format!("use api_key={secret} please")),
        ];
        let outcome = evaluate_dispatch_boundary(&ctx(), &messages);

        match outcome {
            DataBoundaryDispatchOutcome::ProceedTransformed {
                event, messages, ..
            } => {
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
    fn tool_schema_keys_do_not_block_message_redaction() {
        let _ovr22 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_MODE",
            Some("enforce"),
        );
        let _ovr23 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            Some("openai=approved_external"),
        );
        let _ovr24 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY",
            Some("redact"),
        );
        let mut context = ctx();
        context.tool_schema_payload = Some(
            r#"[{"name":"configure_auth","description":"Configure secret credentials and API tokens.","parameters":{"type":"object","properties":{"secret":{"type":"string"},"password":{"type":"string"},"token":{"type":"string"}}}}]"#,
        );
        let messages = vec![chat("user", "api_key=sk-live-abcdef1234567890")];
        let outcome = evaluate_dispatch_boundary(&context, &messages);

        match outcome {
            DataBoundaryDispatchOutcome::ProceedTransformed { messages, .. } => {
                assert!(!messages[0].content.contains("sk-live-abcdef1234567890"));
            }
            _ => panic!("schema vocabulary must not block message redaction"),
        }
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn credential_in_tool_schema_value_fails_closed() {
        let _ovr25 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_MODE",
            Some("enforce"),
        );
        let _ovr26 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            Some("openai=approved_external"),
        );
        let _ovr27 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY",
            Some("redact"),
        );
        let mut context = ctx();
        context.tool_schema_payload =
            Some(r#"[{"description":"api_key=sk-live-abcdef1234567890"}]"#);
        let outcome = evaluate_dispatch_boundary(&context, &[chat("user", "hello")]);

        match outcome {
            DataBoundaryDispatchOutcome::Blocked { reason, .. } => {
                assert!(reason.contains("untransformable_sensitive_field"));
                assert!(!reason.contains("sk-live-abcdef1234567890"));
            }
            _ => panic!("credential-bearing schema value must fail closed"),
        }
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn enforce_approval_classes_require_approval_with_safe_evidence() {
        let _ovr28 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_MODE",
            Some("enforce"),
        );
        let _ovr29 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            Some("openai=approved_external"),
        );
        let _ovr30 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_APPROVAL_CLASSES",
            Some("credential"),
        );
        let secret = "sk-live-abcdef1234567890";
        let messages = vec![chat("user", &format!("api_key={secret}"))];
        let outcome = evaluate_dispatch_boundary(&ctx(), &messages);

        match outcome {
            DataBoundaryDispatchOutcome::RequireApproval {
                event,
                evidence,
                denial_reason,
                ..
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
        let _ovr31 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_MODE",
            Some("enforce"),
        );
        let _ovr32 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            Some("openai=approved_external"),
        );
        let _ovr33 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY",
            Some("require_local"),
        );
        let messages = vec![chat("user", "api_key=sk-live-abcdef1234567890")];
        let outcome = evaluate_dispatch_boundary(&ctx(), &messages);

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
        let _ovr34 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_MODE",
            Some("enforce"),
        );
        let _ovr35 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_STRICT",
            Some("1"),
        );
        let messages = vec![chat("user", "hello there")];
        let outcome = evaluate_dispatch_boundary(&ctx(), &messages);

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
        let _ovr36 = ScopedDataBoundaryConfigOverride::set(
            "session-1",
            "TANDEM_DATA_BOUNDARY_MODE",
            Some("audit"),
        );
        let mut tenant = TenantContext::local_implicit();
        tenant.org_id = "org-src".to_string();
        tenant.workspace_id = "workspace-src".to_string();
        let event = evaluate_context_source(
            ContextSourceScope::Session("session-1"),
            "tool_result",
            Some("web_fetch"),
            "api_key=sk-live-abcdef1234567890",
            DataBoundaryOperationKind::ToolCall,
            Some(&tenant),
        )
        .expect("findings must produce an event");
        assert_eq!(event.properties["tenant"]["organization_id"], "org-src");
        assert_eq!(event.properties["tenant"]["workspace_id"], "workspace-src");
        assert_eq!(event.properties["sessionID"], "session-1");

        let implicit = evaluate_context_source(
            ContextSourceScope::Session("session-1"),
            "tool_result",
            Some("web_fetch"),
            "api_key=sk-live-abcdef1234567890",
            DataBoundaryOperationKind::ToolCall,
            Some(&TenantContext::local_implicit()),
        )
        .expect("findings must produce an event");
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
        let _ovr37 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_MODE",
            Some("enforce"),
        );
        let _ovr38 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            Some("ollama=local"),
        );
        let mut context = ctx();
        context.provider_id = "ollama";
        let messages = vec![chat("user", "api_key=sk-live-abcdef1234567890")];
        let outcome = evaluate_dispatch_boundary(&context, &messages);

        match outcome {
            DataBoundaryDispatchOutcome::Proceed { event, .. } => {
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
        let _ovr39 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_MODE",
            Some("enforce"),
        );
        let _ovr40 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            Some("openai=approved_external"),
        );
        let _ovr41 = ScopedDataBoundaryConfigOverride::set(
            GATE_TEST_SCOPE,
            "TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY",
            Some("redact"),
        );
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

        match outcome {
            DataBoundaryDispatchOutcome::Blocked { reason, .. } => {
                assert!(
                    reason.contains("untransformable_sensitive_field"),
                    "{reason}"
                );
                assert!(!reason.contains("sk-live-abcdef1234567890"));
            }
            _ => panic!("expected Blocked outcome for untransformable data URL"),
        }
    }

    #[test]
    #[serial_test::serial(data_boundary_env)]
    fn policy_from_env_maps_external_raw_policy_and_classes() {
        let lookup = |name: &str| -> Option<String> {
            match name {
                "TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY" => Some("redact".to_string()),
                "TANDEM_DATA_BOUNDARY_BLOCK_CLASSES" => Some("phi, credential".to_string()),
                "TANDEM_DATA_BOUNDARY_MAX_PAYLOAD_BYTES" => Some("1024".to_string()),
                _ => None,
            }
        };
        let policy = provider_egress_policy_with(DataBoundaryMode::Audit, &lookup);

        assert_eq!(policy.redact_classes.len(), SensitiveDataClass::ALL.len());
        assert_eq!(
            policy.block_classes,
            vec![SensitiveDataClass::Phi, SensitiveDataClass::Credential]
        );
        assert_eq!(policy.max_payload_bytes, Some(1024));
        assert!(policy.policy_fingerprint.starts_with("sha256:"));
    }
}
