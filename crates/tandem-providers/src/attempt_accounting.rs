//! Optional trusted admission at actual adapter sends. This is deliberately
//! separate from repeatable policy revalidation. Only an owned, undispatched
//! receipt may clean up on Drop; unknown outcomes after send remain reserved.

use std::{
    future::Future,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex,
    },
};

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
    /// The adapter has not called send, so zero-charge reconciliation is safe.
    NotDispatched,
}

struct PresendCleanup {
    phase: AtomicU8,
    cleanup: Mutex<Option<Box<dyn FnOnce() + Send + 'static>>>,
}

const PENDING: u8 = 0;
const CANCELLED: u8 = 1;
const DISPATCHED: u8 = 2;

impl PresendCleanup {
    fn disarm(&self) {
        self.cleanup
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take();
    }
}

impl Drop for PresendCleanup {
    fn drop(&mut self) {
        // The shared state drops only after the final receipt owner. A receipt
        // clone being dropped must not release another owner's pending send.
        if let Some(cleanup) = self
            .cleanup
            .get_mut()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
        {
            cleanup();
        }
    }
}

#[derive(Clone)]
pub struct ProviderAttemptReceipt {
    confirm:
        Arc<dyn Fn(ProviderAttemptOutcome) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    presend: Option<Arc<PresendCleanup>>,
}

impl ProviderAttemptReceipt {
    pub fn new<F, Fut>(confirm: F) -> Self
    where
        F: Fn(ProviderAttemptOutcome) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        Self {
            confirm: Arc::new(move |usage| Box::pin(confirm(usage))),
            presend: None,
        }
    }

    /// The host must construct this while it still owns the new durable claim,
    /// before handing the blocking worker's output to an awaitable consumer.
    /// Cleanup must schedule nonblocking, idempotent zero settlement and retain
    /// the claim if settlement fails. It runs only for the last presend owner.
    pub fn with_presend_cleanup<F, Fut, Cleanup>(confirm: F, cleanup: Cleanup) -> Self
    where
        F: Fn(ProviderAttemptOutcome) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
        Cleanup: FnOnce() + Send + 'static,
    {
        let mut receipt = Self::new(confirm);
        receipt.presend = Some(Arc::new(PresendCleanup {
            phase: AtomicU8::new(PENDING),
            cleanup: Mutex::new(Some(Box::new(cleanup))),
        }));
        receipt
    }

    /// Explicit denial still waits for durable zero settlement. Cancellation
    /// during that await leaves the presend cleanup armed for an idempotent retry.
    pub async fn confirm_not_dispatched(&self) -> anyhow::Result<()> {
        if let Some(state) = &self.presend {
            // Claim cancellation before awaiting storage: another shared owner
            // must not dispatch after this zero settlement has started.
            let phase = state.phase.compare_exchange(
                PENDING,
                CANCELLED,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
            ensure!(
                matches!(phase, Ok(PENDING) | Err(CANCELLED)),
                "dispatched provider attempt cannot settle as undispatched"
            );
        }
        (self.confirm)(ProviderAttemptOutcome::NotDispatched).await?;
        if let Some(state) = &self.presend {
            state.disarm();
        }
        Ok(())
    }

    fn mark_dispatched(&self) -> anyhow::Result<()> {
        if let Some(state) = &self.presend {
            ensure!(
                state
                    .phase
                    .compare_exchange(PENDING, DISPATCHED, Ordering::SeqCst, Ordering::SeqCst,)
                    .is_ok(),
                "provider attempt is no longer pending dispatch"
            );
            state.disarm();
        }
        Ok(())
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
        receipt.confirm_not_dispatched().await?;
        return Err(error);
    }
    // Every caller immediately polls req.send(): no await or cancellation point
    // may intervene after this conservative transition to an unknown outcome.
    receipt.mark_dispatched()?;
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

#[cfg(test)]
mod presend_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn tracked(cleanups: &Arc<AtomicUsize>) -> ProviderAttemptReceipt {
        let cleanups = cleanups.clone();
        ProviderAttemptReceipt::with_presend_cleanup(
            |_| async { Ok(()) },
            move || {
                cleanups.fetch_add(1, Ordering::SeqCst);
            },
        )
    }

    #[test]
    fn presend_cleanup_waits_for_last_shared_receipt() {
        let cleanups = Arc::new(AtomicUsize::new(0));
        let receipt = tracked(&cleanups);
        let shared = receipt.clone();
        drop(receipt);
        assert_eq!(cleanups.load(Ordering::SeqCst), 0);
        drop(shared);
        assert_eq!(cleanups.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn presend_dispatched_receipt_never_zero_settles_or_cleans_up() {
        let cleanups = Arc::new(AtomicUsize::new(0));
        let receipt = tracked(&cleanups);
        let shared = receipt.clone();
        receipt.mark_dispatched().unwrap();
        assert!(shared.confirm_not_dispatched().await.is_err());
        assert!(
            shared.mark_dispatched().is_err(),
            "one claim cannot authorize two sends"
        );
        drop(receipt);
        drop(shared);
        assert_eq!(cleanups.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn presend_successful_zero_settlement_disarms_cleanup() {
        let cleanups = Arc::new(AtomicUsize::new(0));
        let receipt = tracked(&cleanups);
        receipt.confirm_not_dispatched().await.unwrap();
        receipt.confirm_not_dispatched().await.unwrap();
        assert!(receipt.mark_dispatched().is_err());
        drop(receipt);
        assert_eq!(cleanups.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn presend_cancelled_zero_settlement_blocks_shared_dispatch_and_cleans_up() {
        let cleanups = Arc::new(AtomicUsize::new(0));
        let cleanup_count = cleanups.clone();
        let started = Arc::new(tokio::sync::Notify::new());
        let signal = started.clone();
        let receipt = ProviderAttemptReceipt::with_presend_cleanup(
            move |_| {
                let signal = signal.clone();
                async move {
                    signal.notify_one();
                    std::future::pending::<anyhow::Result<()>>().await
                }
            },
            move || {
                cleanup_count.fetch_add(1, Ordering::SeqCst);
            },
        );
        let shared = receipt.clone();
        {
            let confirming = shared.confirm_not_dispatched();
            tokio::pin!(confirming);
            tokio::select! {
                result = &mut confirming => panic!("settlement returned: {result:?}"),
                _ = started.notified() => {},
            }
            assert!(
                receipt.mark_dispatched().is_err(),
                "zero settlement claims cancellation before awaiting"
            );
        }
        drop(shared);
        assert_eq!(cleanups.load(Ordering::SeqCst), 0);
        drop(receipt);
        assert_eq!(cleanups.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn presend_failed_zero_settlement_keeps_cleanup_armed() {
        let cleanups = Arc::new(AtomicUsize::new(0));
        let cleanup_count = cleanups.clone();
        let receipt = ProviderAttemptReceipt::with_presend_cleanup(
            |_| async { anyhow::bail!("synthetic storage failure") },
            move || {
                cleanup_count.fetch_add(1, Ordering::SeqCst);
            },
        );
        assert!(receipt.confirm_not_dispatched().await.is_err());
        assert!(receipt.mark_dispatched().is_err());
        drop(receipt);
        assert_eq!(cleanups.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn presend_plain_receipt_retains_legacy_drop_and_confirmation_semantics() {
        let confirmations = Arc::new(AtomicUsize::new(0));
        let count = confirmations.clone();
        let receipt = ProviderAttemptReceipt::new(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            async { Ok(()) }
        });
        receipt.confirm_not_dispatched().await.unwrap();
        receipt.mark_dispatched().unwrap();
        drop(receipt);
        assert_eq!(confirmations.load(Ordering::SeqCst), 1);
    }
}
