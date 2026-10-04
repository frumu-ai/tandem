//! Optional trusted admission at actual adapter sends. This is deliberately
//! separate from repeatable policy revalidation and never refunds on Drop.

use std::{future::Future, sync::Arc};

use anyhow::{ensure, Context};
use futures::future::BoxFuture;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProtocol {
    ChatCompletions,
    Responses,
    Anthropic,
    Cohere,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderAttempt {
    pub provider_id: String,
    pub model_id: String,
    pub protocol: ProviderProtocol,
    pub endpoint_sha256: String,
    pub credential_sha256: String,
    pub payload_sha256: String,
    pub request_bytes: usize,
    pub maximum_output_tokens: u32,
    pub streaming: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmedProviderUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderAttemptOutcome {
    Usage(ConfirmedProviderUsage),
    /// Authority changed while durable admission was pending. The adapter has
    /// not called send; this is the only automatic zero-charge reconciliation.
    NotDispatched,
}

#[derive(Clone)]
pub struct ProviderAttemptReceipt {
    confirm:
        Arc<dyn Fn(ProviderAttemptOutcome) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
}

impl ProviderAttemptReceipt {
    pub fn new<F, Fut>(confirm: F) -> Self
    where
        F: Fn(ProviderAttemptOutcome) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        Self {
            confirm: Arc::new(move |usage| Box::pin(confirm(usage))),
        }
    }

    pub(crate) async fn confirm(&self, usage: ConfirmedProviderUsage) -> anyhow::Result<()> {
        (self.confirm)(ProviderAttemptOutcome::Usage(usage)).await
    }
}

#[derive(Clone)]
pub struct ProviderAttemptPolicy {
    max_output_tokens: u32,
    max_request_bytes: usize,
    admit: Arc<
        dyn Fn(ProviderAttempt) -> BoxFuture<'static, anyhow::Result<ProviderAttemptReceipt>>
            + Send
            + Sync,
    >,
}

tokio::task_local! {
    static ATTEMPT_POLICY: ProviderAttemptPolicy;
}

impl ProviderAttemptPolicy {
    /// Host/runtime-owned caps and admission callback, never deserialized from
    /// a browser request. Callback success must represent one durable new claim.
    pub fn new<F, Fut>(
        max_output_tokens: u32,
        max_request_bytes: usize,
        admit: F,
    ) -> anyhow::Result<Self>
    where
        F: Fn(ProviderAttempt) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<ProviderAttemptReceipt>> + Send + 'static,
    {
        ensure!(
            max_output_tokens > 0 && max_request_bytes > 0,
            "provider request limits must be finite and positive"
        );
        Ok(Self {
            max_output_tokens,
            max_request_bytes,
            admit: Arc::new(move |attempt| Box::pin(admit(attempt))),
        })
    }

    pub fn scope<F: Future>(self, future: F) -> impl Future<Output = F::Output> {
        ATTEMPT_POLICY.scope(self, Box::pin(future))
    }
}

pub(crate) fn ensure_supported(provider: &dyn crate::Provider) -> anyhow::Result<()> {
    ensure!(
        ATTEMPT_POLICY.try_with(|_| ()).is_err() || provider.supports_attempt_accounting(),
        "provider does not support bounded attempt accounting"
    );
    Ok(())
}

fn output_field(protocol: ProviderProtocol) -> &'static str {
    match protocol {
        ProviderProtocol::Responses => "max_output_tokens",
        _ => "max_tokens",
    }
}

pub(crate) fn bound_request(body: &mut Value, protocol: ProviderProtocol) -> anyhow::Result<()> {
    let Ok(policy) = ATTEMPT_POLICY.try_with(Clone::clone) else {
        return Ok(());
    };
    let field = output_field(protocol);
    let limit = match body.get(field) {
        None => u64::from(policy.max_output_tokens),
        Some(value) => value
            .as_u64()
            .filter(|value| *value > 0)
            .context("invalid provider output token limit")?
            .min(u64::from(policy.max_output_tokens)),
    };
    body[field] = Value::from(limit);
    Ok(())
}

fn digest(parts: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    format!("{:x}", digest.finalize())
}

pub(crate) struct RequestFingerprints {
    pub endpoint_sha256: String,
    pub credential_sha256: String,
}

pub(crate) fn request_fingerprints(request: &reqwest::Request) -> RequestFingerprints {
    let headers = request.headers();
    let authorization = headers
        .get("authorization")
        .map(|value| value.as_bytes())
        .unwrap_or_default();
    let api_key = headers
        .get("x-api-key")
        .map(|value| value.as_bytes())
        .unwrap_or_default();
    let account = headers
        .get("chatgpt-account-id")
        .map(|value| value.as_bytes())
        .unwrap_or_default();
    RequestFingerprints {
        endpoint_sha256: digest(&[b"provider-endpoint-v1", request.url().as_str().as_bytes()]),
        credential_sha256: digest(&[b"provider-credential-v1", authorization, api_key, account]),
    }
}

pub(crate) async fn before_send(
    request: &reqwest::RequestBuilder,
    provider_id: &str,
    model_id: &str,
    protocol: ProviderProtocol,
) -> anyhow::Result<Option<ProviderAttemptReceipt>> {
    let Ok(policy) = ATTEMPT_POLICY.try_with(Clone::clone) else {
        return Ok(None);
    };
    // Inspect the bytes and headers of the already-built request, after auth
    // overrides and sampling. Neither raw prompt nor key leaves this boundary.
    let built = request
        .try_clone()
        .context("uninspectable provider request")?
        .build()?;
    let bytes = built
        .body()
        .and_then(|body| body.as_bytes())
        .context("unbounded provider request body")?;
    ensure!(
        bytes.len() <= policy.max_request_bytes,
        "provider request exceeds reviewed byte limit"
    );
    let body: Value = serde_json::from_slice(bytes)?;
    ensure!(
        body.get("model").and_then(Value::as_str) == Some(model_id),
        "provider model binding changed"
    );
    let maximum_output_tokens = body
        .get(output_field(protocol))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .context("provider output is not bounded")?;
    ensure!(
        maximum_output_tokens <= policy.max_output_tokens,
        "provider output exceeds reviewed limit"
    );
    let fingerprints = request_fingerprints(&built);
    let attempt = ProviderAttempt {
        provider_id: provider_id.into(),
        model_id: model_id.into(),
        protocol,
        endpoint_sha256: fingerprints.endpoint_sha256,
        credential_sha256: fingerprints.credential_sha256,
        payload_sha256: digest(&[b"provider-payload-v1", bytes]),
        request_bytes: bytes.len(),
        maximum_output_tokens,
        streaming: body.get("stream").and_then(Value::as_bool).unwrap_or(false),
    };
    let receipt = (policy.admit)(attempt).await?;
    if let Err(error) = crate::dispatch_authority::revalidate().await {
        (receipt.confirm)(ProviderAttemptOutcome::NotDispatched).await?;
        return Err(error);
    }
    Ok(Some(receipt))
}

/// Legacy telemetry parsers use missing=>zero defaults; billing must not. Missing,
/// malformed or overflowing counters leave the attempt unresolved, never free.
pub(crate) fn confirmed_usage(
    value: &Value,
    protocol: ProviderProtocol,
) -> Option<ConfirmedProviderUsage> {
    let usage = value.get("usage")?;
    if let Some(extra) = usage.get("server_tool_use") {
        let fields = extra.as_object()?;
        if fields
            .values()
            .any(|value| !value.is_null() && value.as_u64() != Some(0))
        {
            return None;
        }
    }
    let (input, output) = match protocol {
        ProviderProtocol::ChatCompletions => {
            (usage.get("prompt_tokens")?, usage.get("completion_tokens")?)
        }
        ProviderProtocol::Cohere => {
            let billed = usage.get("billed_units")?;
            (billed.get("input_tokens")?, billed.get("output_tokens")?)
        }
        _ => (usage.get("input_tokens")?, usage.get("output_tokens")?),
    };
    let mut input_tokens = input.as_u64()?;
    let output_tokens = output.as_u64()?;
    if protocol == ProviderProtocol::Anthropic {
        for field in ["cache_creation_input_tokens", "cache_read_input_tokens"] {
            if let Some(value) = usage.get(field) {
                input_tokens = input_tokens.checked_add(value.as_u64()?)?;
            }
        }
    }
    let sum = input_tokens.checked_add(output_tokens)?;
    let total_tokens = if protocol == ProviderProtocol::Cohere {
        let billed = usage.get("billed_units")?.as_object()?;
        // Non-token billing requires its own approved tariff and receipt type.
        if billed.iter().any(|(name, value)| {
            !matches!(name.as_str(), "input_tokens" | "output_tokens")
                && !value.is_null()
                && value.as_u64() != Some(0)
        }) {
            return None;
        }
        let tokens = usage.get("tokens")?;
        tokens
            .get("input_tokens")?
            .as_u64()?
            .checked_add(tokens.get("output_tokens")?.as_u64()?)?
            .max(sum)
    } else {
        match usage.get("total_tokens") {
            None => sum,
            // A larger total contains tokens with no supported input/output
            // tariff classification. Do not release their reserved cost.
            Some(total) => total.as_u64().filter(|total| *total == sum)?,
        }
    };
    Some(ConfirmedProviderUsage {
        input_tokens,
        output_tokens,
        total_tokens,
    })
}

#[derive(Default)]
pub(crate) struct StreamingUsage {
    latest: Option<ConfirmedProviderUsage>,
    high_water: Option<ConfirmedProviderUsage>,
    conflicting: bool,
}

impl StreamingUsage {
    pub(crate) fn observe(&mut self, value: &Value, protocol: ProviderProtocol) {
        if value.get("usage").is_none_or(Value::is_null) {
            return;
        }
        let next = confirmed_usage(value, protocol);
        if let (Some(previous), Some(next)) = (&self.high_water, &next) {
            self.conflicting |= next.input_tokens < previous.input_tokens
                || next.output_tokens < previous.output_tokens
                || next.total_tokens < previous.total_tokens;
        }
        if next.is_some() {
            self.high_water = next.clone();
        }
        self.latest = next;
    }

    pub(crate) async fn confirm(
        &mut self,
        receipt: &Option<ProviderAttemptReceipt>,
    ) -> anyhow::Result<()> {
        if !self.conflicting {
            if let (Some(receipt), Some(usage)) = (receipt, self.latest.take()) {
                receipt.confirm(usage).await?;
            }
        }
        Ok(())
    }
}

pub(crate) async fn confirm_responses_sse(
    receipt: &Option<ProviderAttemptReceipt>,
    text: &str,
) -> anyhow::Result<()> {
    if let Some(receipt) = receipt {
        let mut terminal = None;
        for line in text.lines() {
            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if value.get("type").and_then(Value::as_str) == Some("response.completed") {
                let usage = confirmed_usage(&value["response"], ProviderProtocol::Responses);
                if let Some(previous) = &terminal {
                    ensure!(previous == &usage, "conflicting provider usage receipts");
                }
                terminal = Some(usage);
            }
        }
        if let Some(Some(usage)) = terminal {
            receipt.confirm(usage).await?;
        }
    }
    Ok(())
}

pub(crate) async fn confirm_json(
    receipt: &Option<ProviderAttemptReceipt>,
    value: &Value,
    protocol: ProviderProtocol,
) -> anyhow::Result<()> {
    if let (Some(receipt), Some(usage)) = (receipt, confirmed_usage(value, protocol)) {
        receipt.confirm(usage).await?;
    }
    Ok(())
}
