use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::automation_v2::types::{
    normalize_automation_webhook_provider, AutomationWebhookSignatureScheme,
    AutomationWebhookTriggerRecord,
};

type HmacSha256 = Hmac<Sha256>;

const TANDEM_HMAC_SHA256_VERIFIER_ID: &str = "tandem_hmac_sha256_v1";
const GITHUB_HMAC_SHA256_VERIFIER_ID: &str = "github_hmac_sha256";
const SHARED_SECRET_HEADER_VERIFIER_ID: &str = "shared_secret_header_v1";
const UNSIGNED_DEV_MODE_VERIFIER_ID: &str = "unsigned_dev_mode";
const TANDEM_SIGNED_ALLOW_SELF_FEEDBACK_HEADER: &str = "x-tandem-allow-self-feedback";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AutomationWebhookVerificationError {
    UnknownTrigger,
    DisabledTrigger,
    MissingSignature,
    MalformedSignature,
    StaleTimestamp,
    BadSignature,
    MissingSecretMaterial,
    ReplayDetected,
    UnsignedDevModeDisabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutomationWebhookVerificationDecision {
    pub provider: String,
    pub scheme: AutomationWebhookSignatureScheme,
    pub verifier_id: &'static str,
    pub reason_code: String,
}

impl AutomationWebhookVerificationDecision {
    pub(crate) fn rejected_for_trigger(
        trigger: &AutomationWebhookTriggerRecord,
        reason_code: impl Into<String>,
    ) -> Self {
        let provider = canonical_provider(&trigger.provider);
        let verifier =
            automation_webhook_signature_verifier_for(&provider, &trigger.signature_scheme);
        Self {
            provider,
            scheme: trigger.signature_scheme.clone(),
            verifier_id: verifier.verifier_id(),
            reason_code: reason_code.into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AutomationWebhookSignatureHeaders {
    tandem_hmac_sha256: Option<String>,
    legacy_tandem_hmac_sha256: Option<String>,
    github_hmac_sha256: Option<String>,
    shared_secret: Option<String>,
    tandem_signed_allow_self_feedback: Option<String>,
}

impl AutomationWebhookSignatureHeaders {
    pub(crate) fn from_headers(
        tandem_hmac_sha256: Option<&str>,
        legacy_tandem_hmac_sha256: Option<&str>,
        github_hmac_sha256: Option<&str>,
        shared_secret: Option<&str>,
    ) -> Self {
        Self {
            tandem_hmac_sha256: clean_header(tandem_hmac_sha256),
            legacy_tandem_hmac_sha256: clean_header(legacy_tandem_hmac_sha256),
            github_hmac_sha256: clean_header(github_hmac_sha256),
            shared_secret: clean_header(shared_secret),
            tandem_signed_allow_self_feedback: None,
        }
    }

    pub(crate) fn tandem(signature_header: Option<&str>) -> Self {
        Self::from_headers(signature_header, None, None, None)
    }

    pub(crate) fn with_tandem_signed_allow_self_feedback(mut self, value: Option<&str>) -> Self {
        self.tandem_signed_allow_self_feedback = clean_header(value);
        self
    }

    fn tandem_hmac_sha256(&self) -> Option<&str> {
        self.tandem_hmac_sha256
            .as_deref()
            .or(self.legacy_tandem_hmac_sha256.as_deref())
    }

    fn github_hmac_sha256(&self) -> Option<&str> {
        self.github_hmac_sha256.as_deref()
    }

    fn shared_secret(&self) -> Option<&str> {
        self.shared_secret.as_deref()
    }

    fn tandem_signed_allow_self_feedback(&self) -> Option<&str> {
        self.tandem_signed_allow_self_feedback.as_deref()
    }
}

fn clean_header(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) struct AutomationWebhookSignatureVerificationContext<'a> {
    pub provider: &'a str,
    pub scheme: &'a AutomationWebhookSignatureScheme,
    pub headers: &'a AutomationWebhookSignatureHeaders,
    pub secret: Option<&'a str>,
    pub body: &'a [u8],
    pub request_now_ms: u64,
    pub signature_tolerance_ms: u64,
}

pub(crate) trait AutomationWebhookSignatureVerifier: Sync {
    fn verifier_id(&self) -> &'static str;

    fn verify(
        &self,
        context: &AutomationWebhookSignatureVerificationContext<'_>,
    ) -> Result<&'static str, AutomationWebhookVerificationError>;
}

struct TandemHmacSha256Verifier;
struct GithubHmacSha256Verifier;
struct SharedSecretHeaderVerifier;
struct UnsignedDevModeVerifier;

static TANDEM_HMAC_SHA256_VERIFIER: TandemHmacSha256Verifier = TandemHmacSha256Verifier;
static GITHUB_HMAC_SHA256_VERIFIER: GithubHmacSha256Verifier = GithubHmacSha256Verifier;
static SHARED_SECRET_HEADER_VERIFIER: SharedSecretHeaderVerifier = SharedSecretHeaderVerifier;
static UNSIGNED_DEV_MODE_VERIFIER: UnsignedDevModeVerifier = UnsignedDevModeVerifier;

impl AutomationWebhookSignatureVerifier for TandemHmacSha256Verifier {
    fn verifier_id(&self) -> &'static str {
        TANDEM_HMAC_SHA256_VERIFIER_ID
    }

    fn verify(
        &self,
        context: &AutomationWebhookSignatureVerificationContext<'_>,
    ) -> Result<&'static str, AutomationWebhookVerificationError> {
        let secret = context
            .secret
            .ok_or(AutomationWebhookVerificationError::MissingSecretMaterial)?;
        let signature_header = context
            .headers
            .tandem_hmac_sha256()
            .ok_or(AutomationWebhookVerificationError::MissingSignature)?;
        let (timestamp_ms, signature) = parse_tandem_signature_header(signature_header)?;
        if webhook_timestamp_is_stale(
            timestamp_ms,
            context.request_now_ms,
            context.signature_tolerance_ms,
        ) {
            return Err(AutomationWebhookVerificationError::StaleTimestamp);
        }
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .expect("HMAC-SHA256 accepts secrets of any length");
        mac.update(&automation_webhook_signature_payload(
            timestamp_ms,
            context.body,
            context.headers.tandem_signed_allow_self_feedback(),
        ));
        mac.verify_slice(&signature)
            .map_err(|_| AutomationWebhookVerificationError::BadSignature)?;
        Ok("verified")
    }
}

impl AutomationWebhookSignatureVerifier for GithubHmacSha256Verifier {
    fn verifier_id(&self) -> &'static str {
        GITHUB_HMAC_SHA256_VERIFIER_ID
    }

    fn verify(
        &self,
        context: &AutomationWebhookSignatureVerificationContext<'_>,
    ) -> Result<&'static str, AutomationWebhookVerificationError> {
        let secret = context
            .secret
            .ok_or(AutomationWebhookVerificationError::MissingSecretMaterial)?;
        let signature_header = context
            .headers
            .github_hmac_sha256()
            .ok_or(AutomationWebhookVerificationError::MissingSignature)?;
        let signature = parse_prefixed_hex_signature(signature_header, "sha256=")?;
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .expect("HMAC-SHA256 accepts secrets of any length");
        mac.update(context.body);
        mac.verify_slice(&signature)
            .map_err(|_| AutomationWebhookVerificationError::BadSignature)?;
        Ok("verified")
    }
}

impl AutomationWebhookSignatureVerifier for SharedSecretHeaderVerifier {
    fn verifier_id(&self) -> &'static str {
        SHARED_SECRET_HEADER_VERIFIER_ID
    }

    fn verify(
        &self,
        context: &AutomationWebhookSignatureVerificationContext<'_>,
    ) -> Result<&'static str, AutomationWebhookVerificationError> {
        let secret = context
            .secret
            .ok_or(AutomationWebhookVerificationError::MissingSecretMaterial)?;
        let provided = context
            .headers
            .shared_secret()
            .ok_or(AutomationWebhookVerificationError::MissingSignature)?;
        if !constant_time_token_eq(provided, secret) {
            return Err(AutomationWebhookVerificationError::BadSignature);
        }
        Ok("verified")
    }
}

impl AutomationWebhookSignatureVerifier for UnsignedDevModeVerifier {
    fn verifier_id(&self) -> &'static str {
        UNSIGNED_DEV_MODE_VERIFIER_ID
    }

    fn verify(
        &self,
        _context: &AutomationWebhookSignatureVerificationContext<'_>,
    ) -> Result<&'static str, AutomationWebhookVerificationError> {
        Ok("unsigned_dev_mode")
    }
}

pub(crate) fn automation_webhook_signature_verifier_for(
    _provider: &str,
    scheme: &AutomationWebhookSignatureScheme,
) -> &'static dyn AutomationWebhookSignatureVerifier {
    match scheme {
        AutomationWebhookSignatureScheme::HmacSha256V1 => &TANDEM_HMAC_SHA256_VERIFIER,
        AutomationWebhookSignatureScheme::GithubHmacSha256 => &GITHUB_HMAC_SHA256_VERIFIER,
        AutomationWebhookSignatureScheme::SharedSecretHeaderV1 => &SHARED_SECRET_HEADER_VERIFIER,
        AutomationWebhookSignatureScheme::UnsignedDevMode => &UNSIGNED_DEV_MODE_VERIFIER,
    }
}

pub(crate) fn verify_automation_webhook_signature(
    context: AutomationWebhookSignatureVerificationContext<'_>,
) -> Result<AutomationWebhookVerificationDecision, AutomationWebhookVerificationError> {
    let provider = canonical_provider(context.provider);
    let verifier = automation_webhook_signature_verifier_for(&provider, context.scheme);
    let reason_code = verifier.verify(&context)?;
    Ok(AutomationWebhookVerificationDecision {
        provider,
        scheme: context.scheme.clone(),
        verifier_id: verifier.verifier_id(),
        reason_code: reason_code.to_string(),
    })
}

pub(crate) fn automation_webhook_signature_header(
    secret: &str,
    timestamp_ms: u64,
    body: &[u8],
) -> String {
    let signature = automation_webhook_signature(secret, timestamp_ms, body, None);
    format!("t={timestamp_ms},v1={signature}")
}

pub(crate) fn automation_webhook_signature_header_with_signed_allow_self_feedback(
    secret: &str,
    timestamp_ms: u64,
    body: &[u8],
    allow_self_feedback: &str,
) -> String {
    let signature =
        automation_webhook_signature(secret, timestamp_ms, body, Some(allow_self_feedback.trim()));
    format!("t={timestamp_ms},v1={signature}")
}

pub(crate) fn github_automation_webhook_signature_header(secret: &str, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 accepts secrets of any length");
    mac.update(body);
    let signature = mac.finalize().into_bytes();
    format!("sha256={}", hex_encode(&signature))
}

fn automation_webhook_signature(
    secret: &str,
    timestamp_ms: u64,
    body: &[u8],
    allow_self_feedback: Option<&str>,
) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 accepts secrets of any length");
    mac.update(&automation_webhook_signature_payload(
        timestamp_ms,
        body,
        allow_self_feedback,
    ));
    let signature = mac.finalize().into_bytes();
    hex_encode(&signature)
}

fn automation_webhook_signature_payload(
    timestamp_ms: u64,
    body: &[u8],
    allow_self_feedback: Option<&str>,
) -> Vec<u8> {
    let mut payload = timestamp_ms.to_string().into_bytes();
    payload.push(b'.');
    payload.extend_from_slice(body);
    if let Some(allow_self_feedback) = allow_self_feedback {
        payload.extend_from_slice(b"\n");
        payload.extend_from_slice(TANDEM_SIGNED_ALLOW_SELF_FEEDBACK_HEADER.as_bytes());
        payload.push(b':');
        payload.extend_from_slice(allow_self_feedback.as_bytes());
    }
    payload
}

fn parse_tandem_signature_header(
    header: &str,
) -> Result<(u64, Vec<u8>), AutomationWebhookVerificationError> {
    let mut timestamp_ms = None;
    let mut signature = None;
    for part in header.split(',') {
        let Some((key, value)) = part.trim().split_once('=') else {
            return Err(AutomationWebhookVerificationError::MalformedSignature);
        };
        match key.trim() {
            "t" => {
                timestamp_ms = value.trim().parse::<u64>().ok();
            }
            "v1" => {
                signature = hex_decode(value.trim());
            }
            _ => {}
        }
    }
    let timestamp_ms =
        timestamp_ms.ok_or(AutomationWebhookVerificationError::MalformedSignature)?;
    let signature = signature.ok_or(AutomationWebhookVerificationError::MalformedSignature)?;
    if signature.is_empty() {
        return Err(AutomationWebhookVerificationError::MalformedSignature);
    }
    Ok((timestamp_ms, signature))
}

fn parse_prefixed_hex_signature(
    header: &str,
    prefix: &str,
) -> Result<Vec<u8>, AutomationWebhookVerificationError> {
    let Some(signature) = header.trim().strip_prefix(prefix) else {
        return Err(AutomationWebhookVerificationError::MalformedSignature);
    };
    let signature = hex_decode(signature.trim())
        .ok_or(AutomationWebhookVerificationError::MalformedSignature)?;
    if signature.is_empty() {
        return Err(AutomationWebhookVerificationError::MalformedSignature);
    }
    Ok(signature)
}

fn webhook_timestamp_is_stale(timestamp_ms: u64, now_ms: u64, tolerance_ms: u64) -> bool {
    timestamp_ms.abs_diff(now_ms) > tolerance_ms
}

fn canonical_provider(provider: &str) -> String {
    normalize_automation_webhook_provider(provider).unwrap_or_else(|| "generic".to_string())
}

fn constant_time_token_eq(provided: &str, expected: &str) -> bool {
    let provided_hash = Sha256::digest(provided.as_bytes());
    let expected_hash = Sha256::digest(expected.as_bytes());
    let mut diff = 0u8;
    for (left, right) in provided_hash.iter().zip(expected_hash.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_decode(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 || !value.is_ascii() {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|idx| u8::from_str_radix(&value[idx..idx + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifier_registry_maps_signature_schemes() {
        assert_eq!(
            automation_webhook_signature_verifier_for(
                "github",
                &AutomationWebhookSignatureScheme::HmacSha256V1,
            )
            .verifier_id(),
            TANDEM_HMAC_SHA256_VERIFIER_ID
        );
        assert_eq!(
            automation_webhook_signature_verifier_for(
                "github",
                &AutomationWebhookSignatureScheme::GithubHmacSha256,
            )
            .verifier_id(),
            GITHUB_HMAC_SHA256_VERIFIER_ID
        );
        assert_eq!(
            automation_webhook_signature_verifier_for(
                "generic",
                &AutomationWebhookSignatureScheme::SharedSecretHeaderV1,
            )
            .verifier_id(),
            SHARED_SECRET_HEADER_VERIFIER_ID
        );
        assert_eq!(
            automation_webhook_signature_verifier_for(
                "generic",
                &AutomationWebhookSignatureScheme::UnsignedDevMode,
            )
            .verifier_id(),
            UNSIGNED_DEV_MODE_VERIFIER_ID
        );
    }

    #[test]
    fn verifier_records_canonical_provider_and_reason() {
        let body = br#"{"ok":true}"#;
        let now = 1_000;
        let secret = "whsec_test";
        let header = automation_webhook_signature_header(secret, now, body);
        let headers = AutomationWebhookSignatureHeaders::tandem(Some(&header));

        let decision =
            verify_automation_webhook_signature(AutomationWebhookSignatureVerificationContext {
                provider: " GitHub.com ",
                scheme: &AutomationWebhookSignatureScheme::HmacSha256V1,
                headers: &headers,
                secret: Some(secret),
                body,
                request_now_ms: now,
                signature_tolerance_ms: 300_000,
            })
            .expect("valid signature");

        assert_eq!(decision.provider, "github");
        assert_eq!(
            decision.scheme,
            AutomationWebhookSignatureScheme::HmacSha256V1
        );
        assert_eq!(decision.reason_code, "verified");
    }

    #[test]
    fn github_verifier_accepts_github_signature_header() {
        let body = br#"{"action":"opened"}"#;
        let secret = "github-secret";
        let header = github_automation_webhook_signature_header(secret, body);
        let headers =
            AutomationWebhookSignatureHeaders::from_headers(None, None, Some(&header), None);

        verify_automation_webhook_signature(AutomationWebhookSignatureVerificationContext {
            provider: "github",
            scheme: &AutomationWebhookSignatureScheme::GithubHmacSha256,
            headers: &headers,
            secret: Some(secret),
            body,
            request_now_ms: 1_000,
            signature_tolerance_ms: 300_000,
        })
        .expect("valid github signature");
    }

    #[test]
    fn shared_secret_verifier_checks_secret_header() {
        let body = br#"{"ok":true}"#;
        let headers =
            AutomationWebhookSignatureHeaders::from_headers(None, None, None, Some("shared"));

        verify_automation_webhook_signature(AutomationWebhookSignatureVerificationContext {
            provider: "generic",
            scheme: &AutomationWebhookSignatureScheme::SharedSecretHeaderV1,
            headers: &headers,
            secret: Some("shared"),
            body,
            request_now_ms: 1_000,
            signature_tolerance_ms: 300_000,
        })
        .expect("valid shared secret");

        assert_eq!(
            verify_automation_webhook_signature(AutomationWebhookSignatureVerificationContext {
                provider: "generic",
                scheme: &AutomationWebhookSignatureScheme::SharedSecretHeaderV1,
                headers: &headers,
                secret: Some("different"),
                body,
                request_now_ms: 1_000,
                signature_tolerance_ms: 300_000,
            })
            .expect_err("bad shared secret"),
            AutomationWebhookVerificationError::BadSignature
        );
    }

    #[test]
    fn unsigned_dev_mode_verifier_records_dev_reason() {
        let headers = AutomationWebhookSignatureHeaders::default();

        let decision =
            verify_automation_webhook_signature(AutomationWebhookSignatureVerificationContext {
                provider: "generic",
                scheme: &AutomationWebhookSignatureScheme::UnsignedDevMode,
                headers: &headers,
                secret: None,
                body: br#"{}"#,
                request_now_ms: 1_000,
                signature_tolerance_ms: 300_000,
            })
            .expect("unsigned dev mode");

        assert_eq!(decision.reason_code, "unsigned_dev_mode");
    }
}
