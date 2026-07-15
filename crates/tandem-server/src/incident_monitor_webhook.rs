use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use anyhow::Context;
use futures::StreamExt;
use reqwest::{redirect::Policy as RedirectPolicy, StatusCode, Url};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tandem_tools::Tool;
use tandem_types::{EngineEvent, ToolResult, ToolSchema, ToolSecurityDescriptor};
use url::Host;

use crate::{
    app::state::automation_webhook_signature_header, now_ms, sha256_hex, truncate_text, AppState,
    ExternalActionRecord, IncidentMonitorConfig, IncidentMonitorDestinationConfig,
    IncidentMonitorDestinationKind, IncidentMonitorDraftRecord, IncidentMonitorIncidentRecord,
    IncidentMonitorPostRecord,
};

pub use crate::incident_monitor_github::{PublishMode, PublishOutcome};

const INCIDENT_MONITOR_LABEL: &str = "incident-monitor";
const WEBHOOK_OPERATION: &str = "post_webhook";
const WEBHOOK_SIGNATURE_SCHEME: &str = "tandem_hmac_sha256_v1";
const DEFAULT_TIMEOUT_MS: u64 = 5_000;
const MIN_TIMEOUT_MS: u64 = 250;
const MAX_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_MAX_ATTEMPTS: u64 = 2;
const MAX_ATTEMPTS: u64 = 3;
/// Maximum number of publish-trigger attempts (recovery sweeps / auto-post) for
/// a webhook delivery before it is terminally suppressed. Webhook delivery is
/// idempotent (each request carries an idempotency key), so a transient failure
/// is retried on the next trigger rather than permanently suppressed after one.
const WEBHOOK_MAX_PUBLISH_ATTEMPTS: usize = 5;
const DEFAULT_PAYLOAD_BYTE_LIMIT: usize = 64 * 1024;
const MAX_PAYLOAD_BYTE_LIMIT: usize = 256 * 1024;
const DEFAULT_RESPONSE_BYTE_LIMIT: usize = 4 * 1024;
const MAX_RESPONSE_BYTE_LIMIT: usize = 16 * 1024;

pub(crate) const INCIDENT_MONITOR_WEBHOOK_TOOL: &str = "incident_monitor_webhook_send";

pub(crate) fn incident_monitor_webhook_dispatch_args(
    destination_id: &str,
    url: &Url,
    config: Option<&Value>,
    delivery_id: &str,
    idempotency_key: &str,
    body: &[u8],
) -> Value {
    json!({
        "destination_id": destination_id,
        "url": url.as_str(),
        "config": config,
        "delivery_id": delivery_id,
        "idempotency_key": idempotency_key,
        "body": String::from_utf8_lossy(body),
    })
}

#[derive(Clone)]
pub(crate) struct IncidentMonitorWebhookDispatchTool {
    state: AppState,
}

impl IncidentMonitorWebhookDispatchTool {
    pub(crate) fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[async_trait::async_trait]
impl Tool for IncidentMonitorWebhookDispatchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            INCIDENT_MONITOR_WEBHOOK_TOOL,
            "Send an Incident Monitor webhook through the governed server dispatcher",
            json!({"type":"object","additionalProperties":true}),
        )
        .with_security(ToolSecurityDescriptor {
            external_side_effect: true,
            ..Default::default()
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let destination_id = args
            .get("destination_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("webhook destination id is missing"))?;
        let url = Url::parse(
            args.get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("webhook URL is missing"))?,
        )?;
        let destination = self
            .state
            .incident_monitor_config()
            .await
            .effective_destinations()
            .into_iter()
            .find(|destination| destination.destination_id == destination_id)
            .ok_or_else(|| anyhow::anyhow!("webhook destination is no longer configured"))?;
        if !destination.enabled || destination.kind != IncidentMonitorDestinationKind::Webhook {
            anyhow::bail!("webhook destination is no longer enabled");
        }
        let configured_url = Url::parse(
            destination
                .webhook_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("webhook URL is missing"))?,
        )?;
        let requested_config = args.get("config").filter(|value| !value.is_null());
        if configured_url != url || requested_config != destination.config.as_ref() {
            anyhow::bail!("webhook destination changed after policy evaluation");
        }
        let policy = WebhookPolicy::from_config(destination.config.as_ref());
        let resolved_target = validate_webhook_url(&url, &policy).await?;
        let secret = resolve_webhook_secret(
            destination
                .webhook_secret_ref
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("webhook secret reference is missing"))?,
        )?;
        let delivery_id = args
            .get("delivery_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("webhook delivery id is missing"))?;
        let idempotency_key = args
            .get("idempotency_key")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("webhook idempotency key is missing"))?;
        let body = args
            .get("body")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("webhook body is missing"))?
            .as_bytes();
        let sent = send_webhook(
            &url,
            &resolved_target,
            &policy,
            &secret,
            delivery_id,
            idempotency_key,
            body,
        )
        .await
        .map_err(anyhow::Error::new)?;
        let attempts = sent
            .attempts
            .iter()
            .map(WebhookSendAttempt::receipt_value)
            .collect::<Vec<_>>();
        Ok(ToolResult {
            output: sent.response_excerpt.clone().unwrap_or_default(),
            metadata: json!({
                "status_code": sent.status_code,
                "response_excerpt": sent.response_excerpt,
                "response_truncated": sent.response_truncated,
                "attempts": attempts,
            }),
        })
    }
}

#[derive(Debug, Clone)]
pub struct WebhookDestinationContext {
    pub destination_id: String,
    pub route_id: Option<String>,
    pub route_match_reason: Option<String>,
    pub webhook_url: Option<String>,
    pub webhook_secret_ref: Option<String>,
    pub config: Option<Value>,
}

impl WebhookDestinationContext {
    fn route_match_reason(&self) -> Option<String> {
        self.route_match_reason
            .clone()
            .or_else(|| Some("destination_router".to_string()))
    }

    fn raw_url(&self) -> anyhow::Result<&str> {
        self.webhook_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Webhook URL is missing"))
    }

    fn secret_ref(&self) -> anyhow::Result<&str> {
        self.webhook_secret_ref
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Webhook secret reference is missing"))
    }
}

#[derive(Debug, Clone)]
struct WebhookPolicy {
    allow_private_networks: bool,
    allow_insecure_http: bool,
    allowed_hosts: Vec<String>,
    timeout_ms: u64,
    max_attempts: u64,
    payload_byte_limit: usize,
    response_byte_limit: usize,
}

impl WebhookPolicy {
    fn from_config(config: Option<&Value>) -> Self {
        Self {
            allow_private_networks: config_bool(config, "allow_private_networks").unwrap_or(false),
            allow_insecure_http: config_bool(config, "allow_insecure_http").unwrap_or(false),
            allowed_hosts: config_string_array(config, &["allowed_hosts", "host_allowlist"]),
            timeout_ms: config_u64(config, &["timeout_ms"])
                .unwrap_or(DEFAULT_TIMEOUT_MS)
                .clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS),
            max_attempts: config_u64(config, &["max_attempts", "retry_max_attempts"])
                .unwrap_or(DEFAULT_MAX_ATTEMPTS)
                .clamp(1, MAX_ATTEMPTS),
            payload_byte_limit: config_u64(config, &["payload_byte_limit", "max_payload_bytes"])
                .map(|value| value as usize)
                .unwrap_or(DEFAULT_PAYLOAD_BYTE_LIMIT)
                .clamp(1_024, MAX_PAYLOAD_BYTE_LIMIT),
            response_byte_limit: config_u64(config, &["response_byte_limit", "max_response_bytes"])
                .map(|value| value as usize)
                .unwrap_or(DEFAULT_RESPONSE_BYTE_LIMIT)
                .clamp(512, MAX_RESPONSE_BYTE_LIMIT),
        }
    }
}

#[derive(Debug, Clone)]
struct ResponseExcerpt {
    text: Option<String>,
    truncated: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct WebhookSendAttempt {
    attempt: u64,
    status_code: Option<u16>,
    retryable: bool,
    response_excerpt: Option<String>,
    response_truncated: bool,
    error: Option<String>,
}

impl WebhookSendAttempt {
    fn receipt_value(&self) -> Value {
        json!({
            "attempt": self.attempt,
            "status_code": self.status_code,
            "retryable": self.retryable,
            "response_excerpt": self.response_excerpt,
            "response_truncated": self.response_truncated,
            "error": self.error,
        })
    }
}

#[derive(Debug, Clone)]
struct WebhookSendSuccess {
    status_code: u16,
    response_excerpt: Option<String>,
    response_truncated: bool,
    attempts: Vec<WebhookSendAttempt>,
}

#[derive(Debug)]
struct WebhookSendFailure {
    detail: String,
    attempts: Vec<WebhookSendAttempt>,
}

impl std::fmt::Display for WebhookSendFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for WebhookSendFailure {}

#[derive(Debug, Clone, Default)]
struct WebhookResolvedTarget {
    dns_override_host: Option<String>,
    dns_override_addrs: Vec<SocketAddr>,
}

pub(crate) fn webhook_destination_readiness(
    destination: &IncidentMonitorDestinationConfig,
) -> (bool, Vec<String>, Option<String>) {
    let policy = WebhookPolicy::from_config(destination.config.as_ref());
    let mut missing = Vec::new();
    let mut detail = None;

    match destination.webhook_url.as_deref().map(str::trim) {
        Some(raw_url) if !raw_url.is_empty() => match Url::parse(raw_url) {
            Ok(url) => {
                if let Err(error) = validate_webhook_url_syntax(&url, &policy) {
                    missing.push(error.to_string());
                }
                if !policy.allow_private_networks {
                    if let Some(reason) = obvious_private_host_reason(url.host_str()) {
                        missing.push(reason);
                    }
                }
            }
            Err(_) => missing.push("Webhook URL is invalid".to_string()),
        },
        _ => missing.push("Webhook URL is missing".to_string()),
    }

    match destination.webhook_secret_ref.as_deref().map(str::trim) {
        Some(secret_ref) if !secret_ref.is_empty() => match webhook_secret_env_name(secret_ref) {
            Ok(env_name) => {
                if std::env::var(env_name)
                    .map(|value| value.trim().is_empty())
                    .unwrap_or(true)
                {
                    missing.push("Webhook secret reference is unavailable".to_string());
                }
            }
            Err(error) => missing.push(error.to_string()),
        },
        _ => missing.push("Webhook secret reference is missing".to_string()),
    }

    if !missing.is_empty() {
        detail = Some(
            "Webhook destination needs a valid public URL and env-backed signing secret"
                .to_string(),
        );
    }

    (missing.is_empty(), missing, detail)
}

pub async fn publish_draft(
    state: &AppState,
    draft_id: &str,
    incident_id: Option<&str>,
    mode: PublishMode,
    destination: WebhookDestinationContext,
) -> anyhow::Result<PublishOutcome> {
    let status = state.incident_monitor_status_snapshot().await;
    let config = status.config.clone();
    if !config.enabled {
        anyhow::bail!("Incident Monitor is disabled");
    }
    if config.paused && matches!(mode, PublishMode::Auto | PublishMode::Recovery) {
        anyhow::bail!("Incident Monitor is paused");
    }

    let mut draft = state
        .get_incident_monitor_draft(draft_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Incident Monitor draft not found"))?;
    if draft.status.eq_ignore_ascii_case("denied") {
        anyhow::bail!("Incident Monitor draft has been denied");
    }
    if mode == PublishMode::Auto
        && config.require_approval_for_new_issues
        && draft.status.eq_ignore_ascii_case("approval_required")
    {
        return Ok(PublishOutcome {
            action: "approval_required".to_string(),
            draft,
            post: None,
        });
    }
    if mode == PublishMode::RecheckOnly {
        return Ok(PublishOutcome {
            action: "no_match".to_string(),
            draft,
            post: None,
        });
    }

    let policy = WebhookPolicy::from_config(destination.config.as_ref());
    let parsed_url = Url::parse(destination.raw_url()?)
        .with_context(|| "parse Incident Monitor webhook destination URL")?;
    let target_ref = webhook_target_ref(&parsed_url);
    let incident = match incident_id {
        Some(id) => state.get_incident_monitor_incident(id).await,
        None => None,
    };
    let evidence_digest = compute_evidence_digest(&draft, incident.as_ref());
    draft.evidence_digest = Some(evidence_digest.clone());

    if let Some(existing) = successful_post_for_draft(
        state,
        &draft.draft_id,
        &destination.destination_id,
        &target_ref,
        Some(&evidence_digest),
    )
    .await
    {
        apply_existing_webhook_post_to_draft(&mut draft, &existing);
        mirror_webhook_post_as_external_action(state, &draft, &existing).await;
        let draft = state.put_incident_monitor_draft(draft).await?;
        return Ok(PublishOutcome {
            action: "skip_duplicate".to_string(),
            draft,
            post: Some(existing),
        });
    }

    if !matches!(mode, PublishMode::ManualPublish) {
        let failed_posts = failed_webhook_posts_for_draft(
            state,
            &draft,
            &destination.destination_id,
            &target_ref,
            &evidence_digest,
        )
        .await;
        if let Some(previous) = failed_posts.first() {
            // Webhook delivery is idempotent, so a transient failure should be
            // retried on the next publish trigger rather than permanently
            // suppressed. Give up only on a non-retryable failure (e.g. 4xx or a
            // configuration problem) or after a bounded number of attempts.
            let exhausted = failed_posts.len() >= WEBHOOK_MAX_PUBLISH_ATTEMPTS;
            if should_suppress_webhook_retry(&failed_posts) {
                let reason = if exhausted {
                    format!("exhausted {} publish attempts", failed_posts.len())
                } else {
                    "the last failure was not retryable".to_string()
                };
                let detail = format!(
                    "suppressed webhook publish for fingerprint {} because {reason} (last attempt {})",
                    draft.fingerprint, previous.post_id
                );
                draft.status = "webhook_post_failed".to_string();
                draft.github_status = Some("webhook_post_failed".to_string());
                draft.last_post_error = Some(truncate_text(&detail, 500));
                let previous = previous.clone();
                let draft = state.put_incident_monitor_draft(draft).await?;
                return Ok(PublishOutcome {
                    action: "webhook_retry_suppressed".to_string(),
                    draft,
                    post: Some(previous),
                });
            }
            // Otherwise fall through and retry the (idempotent) delivery.
        }
    }

    let issue_draft = if draft.triage_run_id.is_none() {
        if mode == PublishMode::ManualPublish {
            anyhow::bail!("Incident Monitor draft needs a triage run before webhook publish");
        }
        None
    } else if mode == PublishMode::ManualPublish {
        Some(
            crate::http::incident_monitor::ensure_incident_monitor_issue_draft(
                state.clone(),
                &draft.draft_id,
                false,
            )
            .await
            .context("generate Incident Monitor issue draft")?,
        )
    } else {
        match draft.triage_run_id.as_deref() {
            Some(run_id) => {
                crate::http::incident_monitor::load_incident_monitor_issue_draft_artifact(
                    state, run_id,
                )
                .await
            }
            None => None,
        }
    };

    let idempotency_key = build_idempotency_key(
        &destination.destination_id,
        &target_ref,
        &draft.fingerprint,
        WEBHOOK_OPERATION,
        &evidence_digest,
    );
    if let Some(existing) = successful_post_by_idempotency(state, &idempotency_key).await {
        apply_existing_webhook_post_to_draft(&mut draft, &existing);
        mirror_webhook_post_as_external_action(state, &draft, &existing).await;
        let draft = state.put_incident_monitor_draft(draft).await?;
        return Ok(PublishOutcome {
            action: "skip_duplicate".to_string(),
            draft,
            post: Some(existing),
        });
    }

    let delivery_id = format!("bmwh_{}", uuid::Uuid::new_v4().simple());
    let claim = pending_webhook_post(
        &draft,
        incident.as_ref(),
        &destination,
        &target_ref,
        &delivery_id,
        &idempotency_key,
        &evidence_digest,
    );
    let (claimed, existing_claim) = state
        .try_claim_incident_monitor_post_idempotency(claim)
        .await?;
    if !claimed {
        if existing_claim.status == "posted" {
            apply_existing_webhook_post_to_draft(&mut draft, &existing_claim);
            mirror_webhook_post_as_external_action(state, &draft, &existing_claim).await;
            let draft = state.put_incident_monitor_draft(draft).await?;
            return Ok(PublishOutcome {
                action: "skip_duplicate".to_string(),
                draft,
                post: Some(existing_claim),
            });
        }
        draft.github_status = Some("webhook_posting".to_string());
        draft.last_post_error = Some(
            "another Incident Monitor publisher already claimed this webhook idempotency key"
                .to_string(),
        );
        return Ok(PublishOutcome {
            action: "publish_in_progress".to_string(),
            draft,
            post: Some(existing_claim),
        });
    }

    let result = publish_claimed_webhook(
        state,
        &config,
        &draft,
        incident.as_ref(),
        issue_draft,
        &destination,
        &parsed_url,
        &target_ref,
        &delivery_id,
        &idempotency_key,
        &evidence_digest,
        &policy,
        existing_claim,
    )
    .await;

    match result {
        Ok(post) => {
            mirror_webhook_post_as_external_action(state, &draft, &post).await;
            draft.status = "webhook_posted".to_string();
            draft.github_status = Some("webhook_posted".to_string());
            draft.github_issue_url = post.external_url.clone();
            draft.github_posted_at_ms = Some(post.updated_at_ms);
            draft.last_post_error = None;
            let draft = state.put_incident_monitor_draft(draft).await?;
            state
                .update_incident_monitor_runtime_status(|runtime| {
                    runtime.last_post_result = Some(format!(
                        "posted webhook delivery {}",
                        post.external_id.as_deref().unwrap_or("unknown")
                    ));
                })
                .await;
            state.event_bus.publish(EngineEvent::new(
                "incident_monitor.webhook.posted",
                json!({
                    "draft_id": draft.draft_id,
                    "repo": draft.repo,
                    "target_ref": target_ref,
                    "destination_id": destination.destination_id,
                    "external_id": post.external_id,
                }),
            ));
            Ok(PublishOutcome {
                action: WEBHOOK_OPERATION.to_string(),
                draft,
                post: Some(post),
            })
        }
        Err((error, post)) => {
            let error_text = truncate_text(&error.to_string(), 500);
            draft.status = "webhook_post_failed".to_string();
            draft.github_status = Some("webhook_post_failed".to_string());
            draft.last_post_error = Some(error_text);
            let _ = state.put_incident_monitor_draft(draft).await;
            Err(error).with_context(|| {
                format!(
                    "post Incident Monitor webhook delivery {} for destination {}",
                    post.external_id.unwrap_or(delivery_id),
                    destination.destination_id
                )
            })
        }
    }
}

async fn publish_claimed_webhook(
    state: &AppState,
    _config: &IncidentMonitorConfig,
    draft: &IncidentMonitorDraftRecord,
    incident: Option<&IncidentMonitorIncidentRecord>,
    issue_draft: Option<Value>,
    destination: &WebhookDestinationContext,
    parsed_url: &Url,
    target_ref: &str,
    delivery_id: &str,
    idempotency_key: &str,
    evidence_digest: &str,
    policy: &WebhookPolicy,
    claim: IncidentMonitorPostRecord,
) -> Result<IncidentMonitorPostRecord, (anyhow::Error, IncidentMonitorPostRecord)> {
    let payload = build_webhook_payload(
        draft,
        incident,
        issue_draft.as_ref(),
        destination,
        target_ref,
        delivery_id,
        idempotency_key,
        evidence_digest,
    );
    let body = match serde_json::to_vec(&payload) {
        Ok(body) => body,
        Err(error) => {
            let post = record_claim_failure(
                state,
                claim,
                destination,
                target_ref,
                delivery_id,
                "failed",
                &format!("serialize webhook payload: {error}"),
                Vec::new(),
                None,
            )
            .await;
            return Err((error.into(), post));
        }
    };
    if body.len() > policy.payload_byte_limit {
        let detail = format!(
            "Webhook payload is {} bytes, exceeding configured limit of {} bytes",
            body.len(),
            policy.payload_byte_limit
        );
        let post = record_claim_failure(
            state,
            claim,
            destination,
            target_ref,
            delivery_id,
            "failed",
            &detail,
            Vec::new(),
            Some(body.len()),
        )
        .await;
        return Err((anyhow::anyhow!(detail), post));
    }

    let dispatch_context = state.tool_dispatch_context(
        tandem_tools::ToolDispatchSource::new("incident_monitor_webhook")
            .request(format!("{}:webhook_publish", draft.draft_id))
            .run(
                draft
                    .triage_run_id
                    .clone()
                    .unwrap_or_else(|| draft.draft_id.clone()),
            ),
        crate::incident_monitor::draft_tenant_context(draft),
        vec![INCIDENT_MONITOR_WEBHOOK_TOOL.to_string()],
    );
    let send_result = state
        .tool_dispatcher
        .dispatch(
            INCIDENT_MONITOR_WEBHOOK_TOOL,
            incident_monitor_webhook_dispatch_args(
                &destination.destination_id,
                parsed_url,
                destination.config.as_ref(),
                delivery_id,
                idempotency_key,
                &body,
            ),
            dispatch_context,
        )
        .await
        .map(|result| WebhookSendSuccess {
            status_code: result
                .metadata
                .get("status_code")
                .and_then(Value::as_u64)
                .unwrap_or(200) as u16,
            response_excerpt: result
                .metadata
                .get("response_excerpt")
                .and_then(Value::as_str)
                .map(str::to_string),
            response_truncated: result
                .metadata
                .get("response_truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            attempts: result
                .metadata
                .get("attempts")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or_default(),
        })
        .map_err(|error| match error.downcast::<WebhookSendFailure>() {
            Ok(failure) => failure,
            Err(error) => WebhookSendFailure {
                detail: error.to_string(),
                attempts: Vec::new(),
            },
        });
    match send_result {
        Ok(sent) => {
            let post = IncidentMonitorPostRecord {
                status: "posted".to_string(),
                external_id: Some(delivery_id.to_string()),
                external_url: None,
                external_title: Some("Webhook delivery".to_string()),
                receipt: Some(webhook_receipt(
                    destination,
                    target_ref,
                    delivery_id,
                    "posted",
                    Some(sent.status_code),
                    sent.attempts.len() as u64,
                    Some(sent.response_truncated),
                    sent.attempts,
                    Some(body.len()),
                )),
                response_excerpt: sent
                    .response_excerpt
                    .map(|text| truncate_text(&text, policy.response_byte_limit)),
                error: None,
                updated_at_ms: now_ms(),
                ..claim
            };
            state
                .put_incident_monitor_post(post)
                .await
                .context("record Incident Monitor webhook post")
                .map_err(|error| {
                    let fallback = IncidentMonitorPostRecord {
                        status: "failed".to_string(),
                        error: Some(error.to_string()),
                        updated_at_ms: now_ms(),
                        ..IncidentMonitorPostRecord::default()
                    };
                    (error, fallback)
                })
        }
        Err(error) => {
            let post = record_claim_failure(
                state,
                claim,
                destination,
                target_ref,
                delivery_id,
                "failed",
                &error.detail,
                error.attempts,
                Some(body.len()),
            )
            .await;
            Err((
                anyhow::anyhow!(post.error.clone().unwrap_or_default()),
                post,
            ))
        }
    }
}

async fn send_webhook(
    url: &Url,
    resolved_target: &WebhookResolvedTarget,
    policy: &WebhookPolicy,
    secret: &str,
    delivery_id: &str,
    idempotency_key: &str,
    body: &[u8],
) -> Result<WebhookSendSuccess, WebhookSendFailure> {
    let mut builder = reqwest::Client::builder()
        .redirect(RedirectPolicy::none())
        .timeout(Duration::from_millis(policy.timeout_ms));
    if let Some(host) = resolved_target.dns_override_host.as_deref() {
        builder = builder.resolve_to_addrs(host, &resolved_target.dns_override_addrs);
    }
    let client = builder.build().map_err(|error| WebhookSendFailure {
        detail: format!("build webhook HTTP client: {error}"),
        attempts: Vec::new(),
    })?;
    let mut attempts = Vec::new();

    for attempt_no in 1..=policy.max_attempts {
        if attempt_no > 1 {
            tokio::time::sleep(Duration::from_millis(25 * attempt_no)).await;
        }
        let timestamp_ms = now_ms();
        let signature = automation_webhook_signature_header(secret, timestamp_ms, body);
        let response = client
            .post(url.clone())
            .timeout(Duration::from_millis(policy.timeout_ms))
            .header("content-type", "application/json")
            .header(
                "user-agent",
                concat!("Tandem-Incident-Monitor/", env!("CARGO_PKG_VERSION")),
            )
            .header("x-tandem-event", "incident_monitor.incident")
            .header("x-tandem-delivery-id", delivery_id)
            .header("x-tandem-signature", signature)
            .header("x-tandem-signature-scheme", WEBHOOK_SIGNATURE_SCHEME)
            .header("idempotency-key", idempotency_key)
            .body(body.to_vec())
            .send()
            .await;

        match response {
            Ok(response) => {
                let status = response.status();
                let retryable = status_is_retryable(status);
                let excerpt = read_response_excerpt(response, policy.response_byte_limit)
                    .await
                    .unwrap_or_else(|error| ResponseExcerpt {
                        text: Some(truncate_text(&error.to_string(), 500)),
                        truncated: false,
                    });
                let attempt = WebhookSendAttempt {
                    attempt: attempt_no,
                    status_code: Some(status.as_u16()),
                    retryable,
                    response_excerpt: excerpt.text.clone(),
                    response_truncated: excerpt.truncated,
                    error: (!status.is_success()).then(|| format!("webhook returned {status}")),
                };
                attempts.push(attempt);
                if status.is_success() {
                    return Ok(WebhookSendSuccess {
                        status_code: status.as_u16(),
                        response_excerpt: excerpt.text,
                        response_truncated: excerpt.truncated,
                        attempts,
                    });
                }
                if retryable && attempt_no < policy.max_attempts {
                    continue;
                }
                return Err(WebhookSendFailure {
                    detail: format!("webhook returned HTTP status {}", status.as_u16()),
                    attempts,
                });
            }
            Err(error) => {
                let retryable = error.is_timeout() || error.is_connect() || error.is_request();
                let detail = if error.is_timeout() {
                    format!("webhook request timed out after {}ms", policy.timeout_ms)
                } else {
                    error.to_string()
                };
                attempts.push(WebhookSendAttempt {
                    attempt: attempt_no,
                    status_code: None,
                    retryable,
                    response_excerpt: None,
                    response_truncated: false,
                    error: Some(truncate_text(&detail, 500)),
                });
                if retryable && attempt_no < policy.max_attempts {
                    continue;
                }
                return Err(WebhookSendFailure { detail, attempts });
            }
        }
    }

    Err(WebhookSendFailure {
        detail: "webhook retry budget exhausted".to_string(),
        attempts,
    })
}

async fn read_response_excerpt(
    response: reqwest::Response,
    limit: usize,
) -> anyhow::Result<ResponseExcerpt> {
    let mut stream = response.bytes_stream();
    let mut out = Vec::new();
    let mut truncated = false;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let remaining = limit.saturating_sub(out.len());
        if chunk.len() > remaining {
            out.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }
        out.extend_from_slice(&chunk);
        if out.len() >= limit {
            truncated = true;
            break;
        }
    }
    let text = String::from_utf8_lossy(&out).trim().to_string();
    Ok(ResponseExcerpt {
        text: (!text.is_empty()).then_some(text),
        truncated,
    })
}

async fn validate_webhook_url(
    url: &Url,
    policy: &WebhookPolicy,
) -> anyhow::Result<WebhookResolvedTarget> {
    validate_webhook_url_syntax(url, policy)?;
    let host = url.host();
    let host_str = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Webhook URL host is missing"))?;
    if policy.allow_private_networks {
        return Ok(WebhookResolvedTarget::default());
    }
    match host {
        Some(Host::Ipv4(ip)) => {
            if !ipv4_is_publicly_routable(ip) {
                anyhow::bail!("Webhook URL resolves to a private or internal address");
            }
            return Ok(WebhookResolvedTarget::default());
        }
        Some(Host::Ipv6(ip)) => {
            if !ipv6_is_publicly_routable(ip) {
                anyhow::bail!("Webhook URL resolves to a private or internal address");
            }
            return Ok(WebhookResolvedTarget::default());
        }
        Some(Host::Domain(host)) => {
            if let Some(reason) = obvious_private_host_reason(Some(host)) {
                anyhow::bail!("{reason}");
            }
        }
        None => anyhow::bail!("Webhook URL host is missing"),
    }
    let port = url.port_or_known_default().unwrap_or(443);
    let addrs = tokio::net::lookup_host((host_str, port))
        .await
        .with_context(|| "resolve webhook destination host")?
        .collect::<Vec<_>>();
    if addrs.is_empty() {
        anyhow::bail!("Webhook destination host did not resolve");
    }
    if addrs.iter().any(|addr| !ip_is_publicly_routable(addr.ip())) {
        anyhow::bail!("Webhook URL resolves to a private or internal address");
    }
    Ok(WebhookResolvedTarget {
        dns_override_host: Some(host_str.to_string()),
        dns_override_addrs: addrs,
    })
}

fn validate_webhook_url_syntax(url: &Url, policy: &WebhookPolicy) -> anyhow::Result<()> {
    match url.scheme() {
        "https" => {}
        "http" if policy.allow_insecure_http => {}
        "http" => anyhow::bail!("Webhook URL must use https"),
        scheme => anyhow::bail!("Webhook URL uses unsupported scheme `{scheme}`"),
    }
    if url.host_str().is_none() {
        anyhow::bail!("Webhook URL host is missing");
    }
    if !url.username().is_empty() || url.password().is_some() {
        anyhow::bail!("Webhook URL must not include credentials");
    }
    if !policy.allowed_hosts.is_empty() {
        let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
        if !policy
            .allowed_hosts
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(&host))
        {
            anyhow::bail!("Webhook URL host is not in the destination allowlist");
        }
    }
    Ok(())
}

fn obvious_private_host_reason(host: Option<&str>) -> Option<String> {
    let host = host?.trim().trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return Some("Webhook URL points to localhost/private network".to_string());
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if !ip_is_publicly_routable(ip) {
            return Some("Webhook URL points to localhost/private network".to_string());
        }
    }
    None
}

fn ip_is_publicly_routable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ipv4_is_publicly_routable(ip),
        IpAddr::V6(ip) => ipv6_is_publicly_routable(ip),
    }
}

fn ipv4_is_publicly_routable(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || octets[0] == 0
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 169 && octets[1] == 254)
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
        || (octets[0] == 198 && (18..=19).contains(&octets[1]))
        || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
        || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113))
}

fn ipv6_is_publicly_routable(ip: Ipv6Addr) -> bool {
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return ipv4_is_publicly_routable(mapped);
    }
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ((ip.segments()[0] & 0xfe00) == 0xfc00)
        || ((ip.segments()[0] & 0xffc0) == 0xfe80))
}

fn resolve_webhook_secret(secret_ref: &str) -> anyhow::Result<String> {
    let env_name = webhook_secret_env_name(secret_ref)?;
    let value = std::env::var(env_name)
        .map_err(|_| anyhow::anyhow!("Webhook secret reference is unavailable"))?;
    let value = value.trim().to_string();
    if value.is_empty() {
        anyhow::bail!("Webhook secret reference is unavailable");
    }
    Ok(value)
}

fn webhook_secret_env_name(secret_ref: &str) -> anyhow::Result<&str> {
    let trimmed = secret_ref.trim();
    let env = trimmed
        .strip_prefix("${env:")
        .and_then(|rest| rest.strip_suffix('}'))
        .or_else(|| trimmed.strip_prefix("env://"))
        .or_else(|| trimmed.strip_prefix("env:"))
        .unwrap_or(trimmed)
        .trim();
    if env.is_empty()
        || env.contains('/')
        || env.contains('\\')
        || env.contains('=')
        || !env
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
    {
        anyhow::bail!(
            "Webhook secret reference must use an env-backed reference such as env:TANDEM_WEBHOOK_SECRET"
        );
    }
    Ok(env)
}

fn pending_webhook_post(
    draft: &IncidentMonitorDraftRecord,
    incident: Option<&IncidentMonitorIncidentRecord>,
    destination: &WebhookDestinationContext,
    target_ref: &str,
    delivery_id: &str,
    idempotency_key: &str,
    evidence_digest: &str,
) -> IncidentMonitorPostRecord {
    let now = now_ms();
    IncidentMonitorPostRecord {
        post_id: format!("failure-post-{}", uuid::Uuid::new_v4().simple()),
        draft_id: draft.draft_id.clone(),
        tenant_id: draft.tenant_id.clone(),
        workspace_id: draft.workspace_id.clone(),
        incident_id: incident.map(|row| row.incident_id.clone()),
        fingerprint: draft.fingerprint.clone(),
        repo: draft.repo.clone(),
        operation: WEBHOOK_OPERATION.to_string(),
        status: "pending".to_string(),
        issue_number: None,
        issue_url: None,
        comment_id: None,
        comment_url: None,
        destination_id: Some(destination.destination_id.clone()),
        destination_kind: Some(IncidentMonitorDestinationKind::Webhook),
        route_id: destination.route_id.clone(),
        route_match_reason: destination.route_match_reason(),
        external_id: Some(delivery_id.to_string()),
        external_url: None,
        external_title: Some("Webhook delivery".to_string()),
        target_ref: Some(target_ref.to_string()),
        receipt: Some(webhook_receipt(
            destination,
            target_ref,
            delivery_id,
            "pending",
            None,
            0,
            None,
            Vec::new(),
            None,
        )),
        evidence_digest: Some(evidence_digest.to_string()),
        confidence: draft.confidence.clone(),
        risk_level: draft.risk_level.clone(),
        expected_destination: draft.expected_destination.clone(),
        evidence_refs: draft.evidence_refs.clone(),
        quality_gate: draft.quality_gate.clone(),
        idempotency_key: idempotency_key.to_string(),
        response_excerpt: None,
        error: None,
        created_at_ms: now,
        updated_at_ms: now,
    }
}

async fn record_claim_failure(
    state: &AppState,
    claim: IncidentMonitorPostRecord,
    destination: &WebhookDestinationContext,
    target_ref: &str,
    delivery_id: &str,
    receipt_status: &str,
    detail: &str,
    attempts: Vec<WebhookSendAttempt>,
    payload_bytes: Option<usize>,
) -> IncidentMonitorPostRecord {
    let status_code = attempts
        .iter()
        .rev()
        .find_map(|attempt| attempt.status_code);
    let post = IncidentMonitorPostRecord {
        status: "failed".to_string(),
        receipt: Some(webhook_receipt(
            destination,
            target_ref,
            delivery_id,
            receipt_status,
            status_code,
            attempts.len() as u64,
            attempts.last().map(|attempt| attempt.response_truncated),
            attempts,
            payload_bytes,
        )),
        error: Some(truncate_text(detail, 500)),
        updated_at_ms: now_ms(),
        ..claim
    };
    match state.put_incident_monitor_post(post.clone()).await {
        Ok(recorded) => recorded,
        Err(error) => {
            tracing::warn!(
                draft_id = %post.draft_id,
                error = %error,
                "failed to record Incident Monitor webhook failure receipt",
            );
            post
        }
    }
}

fn webhook_receipt(
    destination: &WebhookDestinationContext,
    target_ref: &str,
    delivery_id: &str,
    status: &str,
    status_code: Option<u16>,
    attempt_count: u64,
    response_truncated: Option<bool>,
    attempts: Vec<WebhookSendAttempt>,
    payload_bytes: Option<usize>,
) -> Value {
    json!({
        "provider": "webhook",
        "destination_id": destination.destination_id,
        "operation": WEBHOOK_OPERATION,
        "status": status,
        "delivery_id": delivery_id,
        "target_ref": target_ref,
        "route_id": destination.route_id,
        "route_match_reason": destination.route_match_reason(),
        "signature_scheme": WEBHOOK_SIGNATURE_SCHEME,
        "status_code": status_code,
        "attempt_count": attempt_count,
        "response_truncated": response_truncated,
        "payload_bytes": payload_bytes,
        "attempts": attempts
            .iter()
            .map(WebhookSendAttempt::receipt_value)
            .collect::<Vec<_>>(),
    })
}

fn build_webhook_payload(
    draft: &IncidentMonitorDraftRecord,
    incident: Option<&IncidentMonitorIncidentRecord>,
    issue_draft: Option<&Value>,
    destination: &WebhookDestinationContext,
    target_ref: &str,
    delivery_id: &str,
    idempotency_key: &str,
    evidence_digest: &str,
) -> Value {
    json!({
        "event": "incident_monitor.incident",
        "schema_version": "2026-06-29",
        "delivery_id": delivery_id,
        "idempotency_key": idempotency_key,
        "created_at_ms": now_ms(),
        "destination": {
            "destination_id": destination.destination_id,
            "kind": "webhook",
            "route_id": destination.route_id,
            "route_match_reason": destination.route_match_reason(),
            "target_ref": target_ref,
        },
        "draft": {
            "draft_id": draft.draft_id,
            "fingerprint": draft.fingerprint,
            "repo": draft.repo,
            "project_id": draft.project_id,
            "log_source_id": draft.log_source_id,
            "source_kind": draft.source_kind,
            "tenant_id": draft.tenant_id,
            "workspace_id": draft.workspace_id,
            "event_schema_version": draft.event_schema_version,
            "status": draft.status,
            "title": draft.title,
            "detail": draft.detail.as_deref().map(|value| truncate_text(value, 4_000)),
            "risk_level": draft.risk_level,
            "risk_category": draft.risk_category,
            "actor": draft.actor,
            "model": draft.model,
            "tool_name": draft.tool_name,
            "action": draft.action,
            "policy": draft.policy,
            "approval_state": draft.approval_state,
            "blast_radius": draft.blast_radius,
            "external_correlation_ids": draft.external_correlation_ids,
            "confidence": draft.confidence,
            "expected_destination": draft.expected_destination,
            "route_tags": draft.route_tags,
            "evidence_digest": evidence_digest,
            "evidence_refs": draft.evidence_refs.iter().take(20).cloned().collect::<Vec<_>>(),
            "quality_gate": draft.quality_gate,
            "triage_run_id": draft.triage_run_id,
        },
        "incident": incident.map(webhook_incident_payload),
        "issue_draft": issue_draft.map(|value| sanitize_webhook_json(value, 0)),
    })
}

fn webhook_incident_payload(incident: &IncidentMonitorIncidentRecord) -> Value {
    json!({
        "incident_id": incident.incident_id,
        "fingerprint": incident.fingerprint,
        "event_type": incident.event_type,
        "status": incident.status,
        "repo": incident.repo,
        "workspace_root": incident.workspace_root,
        "title": incident.title,
        "project_id": incident.project_id,
        "log_source_id": incident.log_source_id,
        "source_kind": incident.source_kind,
        "detail": incident.detail.as_deref().map(|value| truncate_text(value, 4_000)),
        "excerpt": incident.excerpt.iter().take(20).map(|value| truncate_text(value, 500)).collect::<Vec<_>>(),
        "source": incident.source,
        "component": incident.component,
        "level": incident.level,
        "risk_level": incident.risk_level,
        "risk_category": incident.risk_category,
        "actor": incident.actor,
        "model": incident.model,
        "tool_name": incident.tool_name,
        "action": incident.action,
        "policy": incident.policy,
        "approval_state": incident.approval_state,
        "blast_radius": incident.blast_radius,
        "external_correlation_ids": incident.external_correlation_ids,
        "occurrence_count": incident.occurrence_count,
        "created_at_ms": incident.created_at_ms,
        "updated_at_ms": incident.updated_at_ms,
        "last_seen_at_ms": incident.last_seen_at_ms,
    })
}

fn sanitize_webhook_json(value: &Value, depth: usize) -> Value {
    if depth > 8 {
        return Value::String("<truncated>".to_string());
    }
    match value {
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, value) in map.iter().take(80) {
                if json_key_is_sensitive(key) {
                    out.insert(key.clone(), Value::String("<redacted>".to_string()));
                } else {
                    out.insert(key.clone(), sanitize_webhook_json(value, depth + 1));
                }
            }
            Value::Object(out)
        }
        Value::Array(rows) => Value::Array(
            rows.iter()
                .take(80)
                .map(|value| sanitize_webhook_json(value, depth + 1))
                .collect(),
        ),
        Value::String(text) => Value::String(truncate_text(text, 2_000)),
        other => other.clone(),
    }
}

fn json_key_is_sensitive(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("secret")
        || key.contains("token")
        || key.contains("password")
        || key.contains("authorization")
        || key.contains("api_key")
        || key.contains("apikey")
}

async fn successful_post_by_idempotency(
    state: &AppState,
    idempotency_key: &str,
) -> Option<IncidentMonitorPostRecord> {
    let mut rows = state
        .incident_monitor_posts
        .read()
        .await
        .values()
        .filter(|post| post.idempotency_key == idempotency_key && post.status == "posted")
        .cloned()
        .collect::<Vec<_>>();
    rows.sort_by_key(|post| std::cmp::Reverse(post.updated_at_ms));
    rows.into_iter().next()
}

async fn failed_webhook_posts_for_draft(
    state: &AppState,
    draft: &IncidentMonitorDraftRecord,
    destination_id: &str,
    target_ref: &str,
    evidence_digest: &str,
) -> Vec<IncidentMonitorPostRecord> {
    let mut rows = state
        .incident_monitor_posts
        .read()
        .await
        .values()
        .filter(|post| {
            post.draft_id == draft.draft_id
                && post.fingerprint == draft.fingerprint
                && post.operation == WEBHOOK_OPERATION
                && post.status == "failed"
                && post.destination_id.as_deref() == Some(destination_id)
                && post.target_ref.as_deref() == Some(target_ref)
                && post.evidence_digest.as_deref() == Some(evidence_digest)
        })
        .cloned()
        .collect::<Vec<_>>();
    rows.sort_by_key(|post| std::cmp::Reverse(post.updated_at_ms));
    rows
}

/// Whether the final send attempt of a failed webhook post was retryable
/// (transient). Unknown/missing metadata is treated as retryable — the retry is
/// still bounded by `WEBHOOK_MAX_PUBLISH_ATTEMPTS`.
fn webhook_post_last_attempt_retryable(post: &IncidentMonitorPostRecord) -> bool {
    post.receipt
        .as_ref()
        .and_then(|receipt| receipt.get("attempts"))
        .and_then(Value::as_array)
        .and_then(|attempts| attempts.last())
        .and_then(|attempt| attempt.get("retryable"))
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

/// Decide whether to terminally suppress a webhook redelivery given the prior
/// failed posts (most-recent first). No prior failure → do not suppress; a
/// non-retryable last failure or an exhausted attempt budget → suppress;
/// otherwise allow another idempotent retry.
fn should_suppress_webhook_retry(failed_posts: &[IncidentMonitorPostRecord]) -> bool {
    match failed_posts.first() {
        None => false,
        Some(previous) => {
            !webhook_post_last_attempt_retryable(previous)
                || failed_posts.len() >= WEBHOOK_MAX_PUBLISH_ATTEMPTS
        }
    }
}

async fn successful_post_for_draft(
    state: &AppState,
    draft_id: &str,
    destination_id: &str,
    target_ref: &str,
    evidence_digest: Option<&str>,
) -> Option<IncidentMonitorPostRecord> {
    let mut rows = state
        .incident_monitor_posts
        .read()
        .await
        .values()
        .filter(|post| post.draft_id == draft_id && post.status == "posted")
        .cloned()
        .collect::<Vec<_>>();
    rows.sort_by_key(|post| std::cmp::Reverse(post.updated_at_ms));
    rows.into_iter().find(|row| {
        row.destination_id.as_deref() == Some(destination_id)
            && row.target_ref.as_deref() == Some(target_ref)
            && match evidence_digest {
                Some(expected) => row.evidence_digest.as_deref() == Some(expected),
                None => true,
            }
    })
}

fn apply_existing_webhook_post_to_draft(
    draft: &mut IncidentMonitorDraftRecord,
    post: &IncidentMonitorPostRecord,
) {
    draft.status = "webhook_posted".to_string();
    draft.github_status = Some("webhook_posted".to_string());
    draft.github_issue_url = post.external_url.clone();
    draft.github_posted_at_ms = Some(post.updated_at_ms);
    draft.last_post_error = None;
}

fn compute_evidence_digest(
    draft: &IncidentMonitorDraftRecord,
    incident: Option<&IncidentMonitorIncidentRecord>,
) -> String {
    let _ = incident;
    sha256_hex(&[
        draft.repo.as_str(),
        draft.fingerprint.as_str(),
        draft.title.as_deref().unwrap_or(""),
        draft.detail.as_deref().unwrap_or(""),
    ])
}

fn build_idempotency_key(
    destination_id: &str,
    target_ref: &str,
    fingerprint: &str,
    operation: &str,
    digest: &str,
) -> String {
    sha256_hex(&[
        destination_id,
        "webhook",
        target_ref,
        fingerprint,
        operation,
        digest,
    ])
}

fn webhook_target_ref(url: &Url) -> String {
    let mut out = format!("{}://{}", url.scheme(), url.host_str().unwrap_or_default());
    if let Some(port) = url.port() {
        out.push(':');
        out.push_str(&port.to_string());
    }
    out.push_str(url.path());
    truncate_text(&out, 500)
}

fn status_is_retryable(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.as_u16() == 425
        || status.is_server_error()
}

fn config_bool(config: Option<&Value>, key: &str) -> Option<bool> {
    config
        .and_then(|value| value.get(key))
        .and_then(|value| match value {
            Value::Bool(value) => Some(*value),
            Value::String(value) if value.eq_ignore_ascii_case("true") => Some(true),
            Value::String(value) if value.eq_ignore_ascii_case("false") => Some(false),
            _ => None,
        })
}

fn config_u64(config: Option<&Value>, keys: &[&str]) -> Option<u64> {
    let config = config?;
    keys.iter().find_map(|key| {
        config.get(*key).and_then(|value| match value {
            Value::Number(value) => value.as_u64(),
            Value::String(value) => value.trim().parse::<u64>().ok(),
            _ => None,
        })
    })
}

fn config_string_array(config: Option<&Value>, keys: &[&str]) -> Vec<String> {
    let Some(config) = config else {
        return Vec::new();
    };
    keys.iter()
        .find_map(|key| config.get(*key))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| value.trim_end_matches('.').to_ascii_lowercase())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

async fn mirror_webhook_post_as_external_action(
    state: &AppState,
    draft: &IncidentMonitorDraftRecord,
    post: &IncidentMonitorPostRecord,
) {
    let action = ExternalActionRecord {
        action_id: post.post_id.clone(),
        operation: post.operation.clone(),
        status: post.status.clone(),
        source_kind: Some("incident_monitor".to_string()),
        source_id: Some(draft.draft_id.clone()),
        routine_run_id: None,
        context_run_id: draft.triage_run_id.clone(),
        capability_id: Some("webhook.post".to_string()),
        provider: Some(INCIDENT_MONITOR_LABEL.to_string()),
        target: post.target_ref.clone(),
        approval_state: Some(if draft.status.eq_ignore_ascii_case("approval_required") {
            "approval_required".to_string()
        } else {
            "executed".to_string()
        }),
        idempotency_key: Some(post.idempotency_key.clone()),
        receipt: Some(json!({
            "post_id": post.post_id,
            "draft_id": post.draft_id,
            "incident_id": post.incident_id,
            "destination_id": post.destination_id,
            "destination_kind": post.destination_kind,
            "route_id": post.route_id,
            "route_match_reason": post.route_match_reason,
            "external_id": post.external_id,
            "external_url": post.external_url,
            "external_title": post.external_title,
            "target_ref": post.target_ref,
            "response_excerpt": post.response_excerpt,
        })),
        error: post.error.clone(),
        metadata: Some(json!({
            "repo": post.repo,
            "destination_id": post.destination_id,
            "destination_kind": post.destination_kind,
            "route_id": post.route_id,
            "route_match_reason": post.route_match_reason,
            "target_ref": post.target_ref,
            "fingerprint": post.fingerprint,
            "evidence_digest": post.evidence_digest,
            "confidence": post.confidence,
            "risk_level": post.risk_level,
            "risk_category": draft.risk_category,
            "actor": draft.actor,
            "model": draft.model,
            "tool_name": draft.tool_name,
            "action": draft.action,
            "policy": draft.policy,
            "approval_state": draft.approval_state,
            "blast_radius": draft.blast_radius,
            "external_correlation_ids": draft.external_correlation_ids,
            "expected_destination": post.expected_destination,
            "evidence_refs": post.evidence_refs,
            "quality_gate": post.quality_gate,
            "incident_monitor_operation": post.operation,
        })),
        created_at_ms: post.created_at_ms,
        updated_at_ms: post.updated_at_ms,
    };
    if let Err(error) = AppState::record_external_action(state, action).await {
        tracing::warn!(
            "failed to persist external action mirror for incident monitor webhook post {}: {}",
            post.post_id,
            error
        );
    }
}

#[cfg(test)]
mod webhook_retry_tests {
    use super::*;

    fn failed_post(retryable: Option<bool>) -> IncidentMonitorPostRecord {
        let attempt = match retryable {
            Some(value) => json!({ "attempt": 1, "retryable": value }),
            None => json!({ "attempt": 1 }),
        };
        IncidentMonitorPostRecord {
            status: "failed".to_string(),
            operation: WEBHOOK_OPERATION.to_string(),
            receipt: Some(json!({ "attempts": [attempt] })),
            ..Default::default()
        }
    }

    #[test]
    fn no_prior_failures_does_not_suppress() {
        assert!(!should_suppress_webhook_retry(&[]));
    }

    #[test]
    fn single_retryable_failure_is_retried() {
        // TAN-550: one transient failure must not permanently suppress delivery.
        assert!(!should_suppress_webhook_retry(&[failed_post(Some(true))]));
    }

    #[test]
    fn unknown_retryability_defaults_to_retry() {
        assert!(!should_suppress_webhook_retry(&[failed_post(None)]));
    }

    #[test]
    fn non_retryable_failure_suppresses_immediately() {
        assert!(should_suppress_webhook_retry(&[failed_post(Some(false))]));
    }

    #[test]
    fn exhausted_attempt_budget_suppresses() {
        let posts: Vec<_> = (0..WEBHOOK_MAX_PUBLISH_ATTEMPTS)
            .map(|_| failed_post(Some(true)))
            .collect();
        assert!(should_suppress_webhook_retry(&posts));
    }
}
