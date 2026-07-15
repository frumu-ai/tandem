const EGRESS_DLP_POLICY_ID: &str = "egress_dlp_preflight";
const EGRESS_ARRAY_INSPECTION_LIMIT: usize = 20;
const EGRESS_PREVIEW_LIMIT: usize = 700;

#[derive(Debug, Clone)]
struct EgressFinding {
    path: String,
    data_class: DataClass,
    kind: &'static str,
    preview: String,
    evidence_hash: String,
}

#[derive(Debug, Clone)]
struct EgressPreflightReport {
    external_action: bool,
    risk_tier: ToolRiskTier,
    data_classes: Vec<DataClass>,
    findings: Vec<EgressFinding>,
    target: Option<String>,
    safe_preview_markdown: String,
    inspected_field_count: usize,
    redaction_count: usize,
    inspection_truncated: bool,
}

impl EgressPreflightReport {
    fn has_class(&self, class: DataClass) -> bool {
        self.data_classes.contains(&class)
    }

    fn requires_approval(&self) -> bool {
        self.has_class(DataClass::CustomerData) || self.has_class(DataClass::FinancialRecord)
    }

    fn blocks(&self) -> bool {
        self.has_class(DataClass::Credential) || self.inspection_truncated
    }
}

pub(crate) async fn evaluate_egress_preflight_tool_policy(
    state: &AppState,
    ctx: &ToolPolicyContext,
    tool: &str,
) -> Option<ToolPolicyDecision> {
    let report = inspect_egress_preflight(tool, &ctx.args);
    if !report.external_action || (!report.blocks() && !report.requires_approval()) {
        return None;
    }

    let tenant_context = ctx.tenant_context.clone().unwrap_or_default();
    let actor_id = tenant_context.actor_id.clone();
    let now_ms = crate::now_ms();
    let mut approval_id = None;
    let mut approval_expires_at_ms = None;
    let (action_binding, mut approval_request_error) = if report.requires_approval()
        && !report.blocks()
    {
        let connector_generations =
            governed_connector_generation_bindings(state, tool, &tenant_context).await;
        match governed_exact_action_binding(
            tool,
            &ctx.args,
            &tenant_context,
            &connector_generations,
        ) {
            Ok(binding) => (Some(binding), None),
            Err(error) => (
                None,
                Some(format!(
                    "failed to derive deployment-scoped action binding: {error}"
                )),
            ),
        }
    } else {
        (None, None)
    };
    let mut receipt_state = None;

    if report.requires_approval()
        && !report.blocks()
        && state.premium_governance_enabled()
        && approval_request_error.is_none()
    {
        let action_binding = action_binding
            .as_ref()
            .expect("approval binding is present when binding derivation succeeded");
        match state
            .consume_egress_dlp_approval(action_binding, tool, &tenant_context)
            .await
        {
            Ok(Some(receipt)) => {
                approval_id = Some(receipt.approval_id);
                approval_expires_at_ms = Some(receipt.expires_at_ms);
                receipt_state = Some(receipt.state);
            }
            Ok(None) => {}
            Err(error) => approval_request_error = Some(error.to_string()),
        }
    }

    let (effect, reason_code, reason) = if report.has_class(DataClass::Credential) {
        (
            PolicyDecisionEffect::Deny,
            "egress_credential_content_blocked",
            "outbound payload contains credential or secret-looking content and was blocked",
        )
    } else if report.inspection_truncated {
        (
            PolicyDecisionEffect::Deny,
            "egress_payload_inspection_truncated",
            "outbound payload exceeded the DLP inspection limit and was blocked fail-closed",
        )
    } else if receipt_state
        == Some(
            crate::app::state::governance_action_gate::ActionGateApprovalState::ApprovedAndConsumed,
        )
    {
        (
            PolicyDecisionEffect::Allow,
            "egress_approval_receipt_consumed",
            "matching outbound action approval was consumed exactly once",
        )
    } else if receipt_state
        == Some(crate::app::state::governance_action_gate::ActionGateApprovalState::Denied)
    {
        (
            PolicyDecisionEffect::Deny,
            "egress_approval_receipt_denied_or_consumed",
            "matching outbound action approval was denied, expired, or already consumed",
        )
    } else {
        let approval_expires_at = now_ms.saturating_add(tandem_types::DEFAULT_APPROVAL_TTL_MS);
        if approval_id.is_none()
            && approval_request_error.is_none()
            && state.premium_governance_enabled()
        {
            let action_binding = action_binding
                .as_ref()
                .expect("approval binding is present when binding derivation succeeded");
            let requested_by = crate::automation_v2::governance::GovernanceActorRef::agent(
                actor_id.clone(),
                "egress_dlp_preflight",
            );
            let target_resource = crate::automation_v2::governance::GovernanceResourceRef {
                resource_type: "external_action".to_string(),
                id: report
                    .target
                    .clone()
                    .unwrap_or_else(|| action_binding.clone()),
            };
            match state
                .request_approval(
                    crate::automation_v2::governance::GovernanceApprovalRequestType::ExternalPost,
                    requested_by,
                    target_resource,
                    "Review outbound payload before external send".to_string(),
                    serde_json::json!({
                        "policy_id": EGRESS_DLP_POLICY_ID,
                        "tool": tool,
                        "action_hash": action_binding,
                        "safe_preview_markdown": report.safe_preview_markdown,
                        "data_classes": report.data_classes,
                        "target": report.target,
                        "findings": egress_findings_metadata(&report.findings),
                        "inspection_truncated": report.inspection_truncated,
                    }),
                    Some(approval_expires_at),
                    &tenant_context,
                )
                .await
            {
                Ok(request) => {
                    approval_expires_at_ms = Some(request.expires_at_ms);
                    approval_id = Some(request.approval_id);
                }
                Err(error) => {
                    approval_request_error = Some(error.to_string());
                }
            }
        } else if approval_id.is_none() && approval_request_error.is_none() {
            approval_request_error =
                Some("premium governance approval requests are unavailable".to_string());
        }
        (
            PolicyDecisionEffect::ApprovalRequired,
            "egress_customer_data_requires_approval",
            "outbound payload contains customer or financial data and requires approval",
        )
    };

    let policy_decision_id = record_egress_preflight_policy_decision(
        state,
        ctx,
        tool,
        &tenant_context,
        actor_id.clone(),
        &report,
        action_binding.as_deref(),
        effect,
        reason_code,
        reason,
        approval_id.clone(),
        approval_expires_at_ms,
        approval_request_error.clone(),
        now_ms,
    )
    .await;

    if matches!(effect, PolicyDecisionEffect::Allow) && policy_decision_id.is_none() {
        let mut failure_reason = format!(
            "tool `{tool}` denied because the consumed egress approval could not be linked to a durable policy decision receipt"
        );
        if let Err(error) = crate::audit::append_protected_audit_event(
            state,
            "egress.preflight.denied",
            &tenant_context,
            actor_id,
            serde_json::json!({
                "policy_id": EGRESS_DLP_POLICY_ID,
                "approval_id": approval_id,
                "session_id": ctx.session_id,
                "message_id": ctx.message_id,
                "tool": tool,
                "action_binding": action_binding,
                "reason": failure_reason,
                "receipt_write_failed": true,
            }),
        )
        .await
        {
            failure_reason.push_str(&format!(
                "; the required protected denial audit also failed: {error}"
            ));
        }
        return Some(ToolPolicyDecision {
            allowed: false,
            reason: Some(failure_reason),
            policy_decision_id: None,
            dispatch_decision: None,
        });
    }

    let event_type = match effect {
        PolicyDecisionEffect::Allow => "egress.preflight.approved_receipt_consumed",
        PolicyDecisionEffect::Deny => "egress.preflight.denied",
        PolicyDecisionEffect::ApprovalRequired => "egress.preflight.approval_required",
    };
    if let Err(error) = crate::audit::append_protected_audit_event(
        state,
        event_type,
        &tenant_context,
        actor_id.clone(),
        serde_json::json!({
            "policy_id": EGRESS_DLP_POLICY_ID,
            "decision_id": policy_decision_id,
            "approval_id": approval_id,
            "approval_expires_at_ms": approval_expires_at_ms,
            "approval_request_error": approval_request_error,
            "session_id": ctx.session_id,
            "message_id": ctx.message_id,
            "tool": tool,
            "risk_tier": report.risk_tier.as_str(),
            "data_classes": report.data_classes,
            "target": report.target,
            "action_binding": action_binding,
            "safe_preview_markdown": report.safe_preview_markdown,
            "findings": egress_findings_metadata(&report.findings),
            "redaction_count": report.redaction_count,
            "inspected_field_count": report.inspected_field_count,
            "inspection_truncated": report.inspection_truncated,
        }),
    )
    .await
    {
        return Some(ToolPolicyDecision {
            allowed: false,
            reason: Some(format!(
                "tool `{tool}` denied because its required egress audit receipt could not be written: {error}"
            )),
            policy_decision_id,
            dispatch_decision: None,
        });
    }

    if state.is_ready() {
        state.event_bus.publish(EngineEvent::new(
            event_type,
            serde_json::json!({
                "sessionID": ctx.session_id,
                "messageID": ctx.message_id,
                "tool": tool,
                "policyDecisionID": policy_decision_id,
                "approvalID": approval_id,
                "riskTier": report.risk_tier.as_str(),
                "dataClasses": report.data_classes,
                "target": report.target,
                "inspectionTruncated": report.inspection_truncated,
                "timestampMs": now_ms,
                "tenantContext": tenant_context.clone(),
            }),
        ));
    }

    let surfaced_reason = match effect {
        PolicyDecisionEffect::Allow => None,
        PolicyDecisionEffect::Deny => Some(format!(
            "tool `{tool}` blocked by egress DLP preflight: {reason}"
        )),
        PolicyDecisionEffect::ApprovalRequired => Some(format!(
            "tool `{tool}` paused by egress DLP preflight: {reason}{}",
            approval_id
                .as_deref()
                .map(|id| format!(" (approval `{id}`)"))
                .unwrap_or_default()
        )),
    };
    let dispatch_decision = if matches!(effect, PolicyDecisionEffect::ApprovalRequired)
        && approval_request_error.is_none()
    {
        policy_decision_id
            .as_ref()
            .zip(approval_id.as_ref())
            .zip(surfaced_reason.as_ref())
            .zip(action_binding.as_ref())
            .map(|(((decision_id, approval_id), surfaced_reason), action_binding)| {
                tandem_tools::ToolDispatchDecision::approval_required(
                    decision_id.clone(),
                    surfaced_reason.clone(),
                    tandem_tools::ToolDispatchApprovalRequirement {
                        approval_request_id: Some(approval_id.clone()),
                        policy_id: EGRESS_DLP_POLICY_ID.to_string(),
                        policy_version_id: "egress_dlp_preflight/v1".to_string(),
                        rule_id: reason_code.to_string(),
                        rule_version: 1,
                        approval_class: "external_post".to_string(),
                        action_binding: action_binding.clone(),
                    },
                )
            })
    } else {
        None
    };
    Some(ToolPolicyDecision {
        allowed: matches!(effect, PolicyDecisionEffect::Allow),
        reason: surfaced_reason,
        policy_decision_id,
        dispatch_decision,
    })
}

#[allow(clippy::too_many_arguments)]
async fn record_egress_preflight_policy_decision(
    state: &AppState,
    ctx: &ToolPolicyContext,
    tool: &str,
    tenant_context: &tandem_types::TenantContext,
    actor_id: Option<String>,
    report: &EgressPreflightReport,
    action_binding: Option<&str>,
    effect: PolicyDecisionEffect,
    reason_code: &str,
    reason: &str,
    approval_id: Option<String>,
    approval_expires_at_ms: Option<u64>,
    approval_request_error: Option<String>,
    now_ms: u64,
) -> Option<String> {
    let (run_id, automation_id) = state
        .automation_v2_session_runs
        .read()
        .await
        .get(&ctx.session_id)
        .cloned()
        .map(|run_id| {
            let automation_id = state
                .automation_v2_runs
                .try_read()
                .ok()
                .and_then(|runs| runs.get(&run_id).map(|run| run.automation_id.clone()));
            (Some(run_id), automation_id)
        })
        .unwrap_or((None, None));
    let decision_id = format!("policy_decision_{}", uuid::Uuid::new_v4().simple());
    let record = PolicyDecisionRecord {
        decision_id: decision_id.clone(),
        tenant_context: tenant_context.clone(),
        requester_context: ctx
            .verified_tenant_context
            .as_ref()
            .and_then(tandem_types::GovernanceRequesterContext::from_verified_context),
        actor_id,
        session_id: Some(ctx.session_id.clone()),
        message_id: Some(ctx.message_id.clone()),
        run_id,
        automation_id,
        node_id: None,
        tool: Some(tool.to_string()),
        resource: report.target.as_ref().map(|target| {
            tandem_types::ResourceRef::new(
                tenant_context.org_id.clone(),
                tenant_context.workspace_id.clone(),
                tandem_types::ResourceKind::ExternalIntegrationAccount,
                target.clone(),
            )
        }),
        data_classes: report.data_classes.clone(),
        risk_tier: Some(report.risk_tier.as_str().to_string()),
        decision: effect,
        reason_code: reason_code.to_string(),
        reason: reason.to_string(),
        policy_id: Some(EGRESS_DLP_POLICY_ID.to_string()),
        grant_id: None,
        approval_id,
        audit_event_id: None,
        created_at_ms: now_ms,
        metadata: serde_json::json!({
            "egress_preflight": {
                "safe_preview_markdown": report.safe_preview_markdown,
                "action_binding": action_binding,
                "target": report.target,
                "findings": egress_findings_metadata(&report.findings),
                "redaction_count": report.redaction_count,
                "inspected_field_count": report.inspected_field_count,
                "approval_expires_at_ms": approval_expires_at_ms,
                "approval_request_error": approval_request_error,
                "inspection_truncated": report.inspection_truncated,
                "tool_effect_ledger_link": "engine attaches this policy_decision_id to blocked tool-effect records",
            }
        }),
    };
    match state.record_policy_decision(record).await {
        Ok(record) => Some(record.decision_id),
        Err(error) => {
            tracing::warn!("failed to record egress preflight policy decision: {error:?}");
            None
        }
    }
}

fn inspect_egress_preflight(tool: &str, args: &Value) -> EgressPreflightReport {
    let risk_tier =
        tool_risk_tier_from_name_and_descriptor(tool, &ToolSecurityDescriptor::default());
    let external_action = matches!(
        risk_tier,
        ToolRiskTier::ExternalSend
            | ToolRiskTier::InternalWrite
            | ToolRiskTier::MoneyMovementContract
            | ToolRiskTier::CredentialAdmin
    ) || tool_name_looks_like_egress(tool);

    let mut findings = Vec::new();
    let mut inspected_field_count = 0;
    let mut inspection_truncated = false;
    inspect_value_for_egress(
        args,
        "$",
        &mut findings,
        &mut inspected_field_count,
        &mut inspection_truncated,
    );
    let mut data_classes = Vec::new();
    for finding in &findings {
        push_data_class(&mut data_classes, finding.data_class);
    }
    let target = egress_target(args);
    let redaction_count = findings.len();
    let safe_preview_markdown = build_egress_preview(
        tool,
        risk_tier,
        target.as_deref(),
        &findings,
        &data_classes,
        inspection_truncated,
    );

    EgressPreflightReport {
        external_action,
        risk_tier,
        data_classes,
        findings,
        target,
        safe_preview_markdown,
        inspected_field_count,
        redaction_count,
        inspection_truncated,
    }
}

fn inspect_value_for_egress(
    value: &Value,
    path: &str,
    findings: &mut Vec<EgressFinding>,
    inspected_field_count: &mut usize,
    inspection_truncated: &mut bool,
) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let child_path = format!("{path}.{}", sanitize_egress_path_segment(key));
                if key_stores_sensitive_runtime_context(key) {
                    continue;
                }
                inspect_value_for_egress(
                    child,
                    &child_path,
                    findings,
                    inspected_field_count,
                    inspection_truncated,
                );
            }
        }
        Value::Array(rows) => {
            if rows.len() > EGRESS_ARRAY_INSPECTION_LIMIT {
                *inspection_truncated = true;
            }
            for (index, child) in rows.iter().enumerate().take(EGRESS_ARRAY_INSPECTION_LIMIT) {
                let child_path = format!("{path}[{index}]");
                inspect_value_for_egress(
                    child,
                    &child_path,
                    findings,
                    inspected_field_count,
                    inspection_truncated,
                );
            }
        }
        Value::String(text) => {
            *inspected_field_count = inspected_field_count.saturating_add(1);
            inspect_text_for_egress(path, text, findings);
        }
        Value::Number(_) | Value::Bool(_) | Value::Null => {
            *inspected_field_count = inspected_field_count.saturating_add(1);
        }
    }
}

fn inspect_text_for_egress(path: &str, text: &str, findings: &mut Vec<EgressFinding>) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    let lower_path = path.to_ascii_lowercase();
    let lower_text = trimmed.to_ascii_lowercase();
    if path_or_text_looks_like_credential(&lower_path, &lower_text) {
        findings.push(EgressFinding {
            path: path.to_string(),
            data_class: DataClass::Credential,
            kind: "credential",
            preview: "[redacted credential]".to_string(),
            evidence_hash: crate::sha256_hex(&[trimmed]),
        });
        return;
    }
    if path_or_text_looks_like_financial(&lower_path, &lower_text) {
        findings.push(EgressFinding {
            path: path.to_string(),
            data_class: DataClass::FinancialRecord,
            kind: "financial_record",
            preview: redact_egress_text(trimmed),
            evidence_hash: crate::sha256_hex(&[trimmed]),
        });
    }
    if path_or_text_looks_like_customer_data(&lower_path, &lower_text) {
        findings.push(EgressFinding {
            path: path.to_string(),
            data_class: DataClass::CustomerData,
            kind: "customer_data",
            preview: redact_egress_text(trimmed),
            evidence_hash: crate::sha256_hex(&[trimmed]),
        });
    }
}

fn path_or_text_looks_like_credential(path: &str, text: &str) -> bool {
    let credential_path = [
        "password",
        "passwd",
        "secret",
        "api_key",
        "apikey",
        "access_token",
        "refresh_token",
        "private_key",
        "client_secret",
        "credential",
    ]
    .iter()
    .any(|needle| path.contains(needle));
    credential_path
        || text.contains("-----begin private key-----")
        || text.contains("bearer ")
        || text.contains("api_key=")
        || text.contains("access_token=")
        || text.contains("client_secret=")
        || text.contains("sk-")
        || text.contains("ghp_")
}

fn path_or_text_looks_like_customer_data(path: &str, text: &str) -> bool {
    let customer_path = [
        "customer",
        "client",
        "recipient",
        "to",
        "cc",
        "bcc",
        "email",
        "phone",
        "contact",
        "account",
        "company",
    ]
    .iter()
    .any(|needle| path_contains_segment(path, needle));
    customer_path || contains_email_like(text) || contains_phone_like(text)
}

fn path_or_text_looks_like_financial(path: &str, text: &str) -> bool {
    let financial_path = [
        "invoice",
        "payment",
        "bank",
        "iban",
        "routing",
        "account_number",
        "card",
        "balance",
        "amount",
        "contract_value",
    ]
    .iter()
    .any(|needle| path.contains(needle));
    financial_path
        || text.contains("iban")
        || text.contains("routing number")
        || text.contains("account number")
}

fn contains_email_like(text: &str) -> bool {
    text.split(|ch: char| ch.is_whitespace() || matches!(ch, '<' | '>' | ',' | ';' | '"' | '\''))
        .any(|token| {
            let token = token.trim_matches(|ch: char| {
                matches!(ch, '.' | ':' | ')' | '(' | '[' | ']' | '{' | '}')
            });
            let Some((local, domain)) = token.split_once('@') else {
                return false;
            };
            !local.is_empty() && domain.contains('.') && domain.len() >= 3
        })
}

fn contains_phone_like(text: &str) -> bool {
    let digits = text.chars().filter(|ch| ch.is_ascii_digit()).count();
    digits >= 10 && (text.contains('+') || text.contains('-') || text.contains(' '))
}

fn redact_egress_text(text: &str) -> String {
    let mut out = Vec::new();
    for token in text.split_whitespace().take(48) {
        out.push(redact_egress_token(token));
    }
    let joined = out.join(" ");
    truncate_for_preview(&joined, EGRESS_PREVIEW_LIMIT)
}

fn redact_egress_token(token: &str) -> String {
    let trimmed = token.trim_matches(|ch: char| matches!(ch, ',' | ';' | ')' | '('));
    if let Some((local, domain)) = trimmed.split_once('@') {
        if !local.is_empty() && domain.contains('.') {
            let first = local.chars().next().unwrap_or('*');
            return format!("{first}***@{domain}");
        }
    }
    if token.chars().filter(|ch| ch.is_ascii_digit()).count() >= 8 {
        return "[redacted number]".to_string();
    }
    token.to_string()
}

fn build_egress_preview(
    tool: &str,
    risk_tier: ToolRiskTier,
    target: Option<&str>,
    findings: &[EgressFinding],
    data_classes: &[DataClass],
    inspection_truncated: bool,
) -> String {
    let mut preview = String::new();
    preview.push_str("### Egress Preflight\n\n");
    preview.push_str(&format!("- Tool: `{}`\n", sanitize_preview_inline(tool)));
    preview.push_str(&format!("- Risk tier: `{}`\n", risk_tier.as_str()));
    if let Some(target) = target {
        preview.push_str(&format!(
            "- Target: `{}`\n",
            sanitize_preview_inline(target)
        ));
    }
    if !data_classes.is_empty() {
        let classes = data_classes
            .iter()
            .map(|class| format!("{class:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        preview.push_str(&format!("- Detected data: {classes}\n"));
    }
    if inspection_truncated {
        preview.push_str("- Inspection: truncated payload blocked fail-closed.\n");
    }
    if findings.is_empty() {
        preview.push_str("- Payload preview: no sensitive outbound fields detected.\n");
    } else {
        preview.push_str("\n#### Safe Payload Preview\n");
        for finding in findings.iter().take(6) {
            preview.push_str(&format!(
                "- `{}`: {} ({})\n",
                sanitize_preview_inline(&finding.path),
                sanitize_preview_inline(&finding.preview),
                finding.kind
            ));
        }
        if findings.len() > 6 {
            preview.push_str(&format!(
                "- {} additional finding(s) omitted from preview.\n",
                findings.len() - 6
            ));
        }
    }
    truncate_for_preview(&preview, EGRESS_PREVIEW_LIMIT)
}

fn egress_findings_metadata(findings: &[EgressFinding]) -> Value {
    Value::Array(
        findings
            .iter()
            .take(20)
            .map(|finding| {
                serde_json::json!({
                    "path": finding.path,
                    "data_class": finding.data_class,
                    "kind": finding.kind,
                    "preview": finding.preview,
                    "evidence_hash": finding.evidence_hash,
                })
            })
            .collect(),
    )
}

fn egress_target(args: &Value) -> Option<String> {
    for pointer in [
        "/to",
        "/recipient",
        "/recipient_email",
        "/email",
        "/channel",
        "/channel_id",
        "/webhook_url",
        "/url",
        "/crm_record_id",
        "/customer_id",
        "/account_id",
        "/file_path",
        "/path",
    ] {
        if let Some(value) = args.pointer(pointer).and_then(Value::as_str) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(redact_egress_target(pointer, trimmed));
            }
        }
    }
    None
}

fn redact_egress_target(pointer: &str, value: &str) -> String {
    if matches!(pointer, "/webhook_url" | "/url") {
        return redact_egress_url_target(value);
    }
    redact_egress_text(value)
}

fn redact_egress_url_target(value: &str) -> String {
    let hash = crate::sha256_hex(&[value]);
    let short_hash = hash.chars().take(12).collect::<String>();
    let Some((scheme, rest)) = value.split_once("://") else {
        return format!("[redacted-url-target]#{short_hash}");
    };
    let scheme = scheme
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
        .collect::<String>()
        .to_ascii_lowercase();
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .trim();
    if scheme.is_empty() || authority.is_empty() {
        return format!("[redacted-url-target]#{short_hash}");
    }
    format!("{scheme}://{authority}/[redacted-url-target]#{short_hash}")
}

fn tool_name_looks_like_egress(tool: &str) -> bool {
    let compact = tool
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    [
        "send",
        "post",
        "publish",
        "webhook",
        "export",
        "crmupdate",
        "createcontact",
        "updatecontact",
        "upload",
    ]
    .iter()
    .any(|needle| compact.contains(needle))
}

fn path_contains_segment(path: &str, segment: &str) -> bool {
    path.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .any(|part| part == segment)
}

fn key_stores_sensitive_runtime_context(key: &str) -> bool {
    key.starts_with("__")
}

fn push_data_class(classes: &mut Vec<DataClass>, class: DataClass) {
    if !classes.contains(&class) {
        classes.push(class);
    }
}

fn sanitize_egress_path_segment(segment: &str) -> String {
    segment
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn sanitize_preview_inline(value: &str) -> String {
    value
        .replace('`', "'")
        .replace('\n', " ")
        .replace('\r', " ")
}

fn truncate_for_preview(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        value.to_string()
    } else {
        let mut out = value
            .chars()
            .take(limit.saturating_sub(3))
            .collect::<String>();
        out.push_str("...");
        out
    }
}
