use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::automation_v2::types::AutomationWebhookSignatureScheme;

use super::automation_webhook_store::AutomationWebhookVerificationError;

type HmacSha256 = Hmac<Sha256>;

pub(crate) const AUTOMATION_WEBHOOK_SIGNATURE_HEADER: &str = "x-tandem-webhook-signature";
pub(crate) const AUTOMATION_WEBHOOK_LEGACY_SIGNATURE_HEADER: &str = "x-tandem-signature";
pub(crate) const AUTOMATION_WEBHOOK_GITHUB_SIGNATURE_HEADER: &str = "x-hub-signature-256";
pub(crate) const AUTOMATION_WEBHOOK_SHARED_SECRET_HEADER: &str = "x-webhook-secret";
pub(crate) const AUTOMATION_WEBHOOK_GITLAB_TOKEN_HEADER: &str = "x-gitlab-token";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AutomationWebhookSignatureHeaders {
    pub tandem_signature: Option<String>,
    pub legacy_tandem_signature: Option<String>,
    pub github_sha256_signature: Option<String>,
    pub shared_secret: Option<String>,
}

impl AutomationWebhookSignatureHeaders {
    pub(crate) fn from_tandem_header(header: Option<&str>) -> Self {
        Self {
            tandem_signature: header.map(ToOwned::to_owned),
            ..Self::default()
        }
    }

    fn tandem_signature(&self) -> Option<&str> {
        self.tandem_signature
            .as_deref()
            .or(self.legacy_tandem_signature.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    fn github_sha256_signature(&self) -> Option<&str> {
        self.github_sha256_signature
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    fn shared_secret(&self) -> Option<&str> {
        self.shared_secret
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutomationWebhookSignatureVerification {
    pub scheme: AutomationWebhookSignatureScheme,
    pub provider: String,
    pub timestamp_ms: Option<u64>,
    pub result_code: &'static str,
}

pub(crate) struct AutomationWebhookSignatureVerificationInput<'a> {
    pub scheme: &'a AutomationWebhookSignatureScheme,
    pub provider: &'a str,
    pub headers: &'a AutomationWebhookSignatureHeaders,
    pub secret: &'a str,
    pub body: &'a [u8],
    pub request_now_ms: u64,
    pub signature_tolerance_ms: u64,
    pub allow_unsigned_dev: bool,
}

pub(crate) trait AutomationWebhookSignatureVerifier {
    fn verify(
        &self,
        input: &AutomationWebhookSignatureVerificationInput<'_>,
    ) -> Result<AutomationWebhookSignatureVerification, AutomationWebhookVerificationError>;
}

#[derive(Default)]
pub(crate) struct AutomationWebhookSignatureVerifierRegistry {
    tandem_hmac_sha256_v1: TandemHmacSha256V1Verifier,
    github_sha256: GithubSha256Verifier,
    shared_secret_header: SharedSecretHeaderVerifier,
    unsigned_dev: UnsignedDevVerifier,
}

impl AutomationWebhookSignatureVerifierRegistry {
    pub(crate) fn verify(
        &self,
        input: &AutomationWebhookSignatureVerificationInput<'_>,
    ) -> Result<AutomationWebhookSignatureVerification, AutomationWebhookVerificationError> {
        match input.scheme {
            AutomationWebhookSignatureScheme::HmacSha256V1 => {
                self.tandem_hmac_sha256_v1.verify(input)
            }
            AutomationWebhookSignatureScheme::GithubSha256 => self.github_sha256.verify(input),
            AutomationWebhookSignatureScheme::SharedSecretHeader => {
                self.shared_secret_header.verify(input)
            }
            AutomationWebhookSignatureScheme::UnsignedDev => self.unsigned_dev.verify(input),
        }
    }
}

pub(crate) fn verify_automation_webhook_signature(
    input: &AutomationWebhookSignatureVerificationInput<'_>,
) -> Result<AutomationWebhookSignatureVerification, AutomationWebhookVerificationError> {
    AutomationWebhookSignatureVerifierRegistry::default().verify(input)
}

#[derive(Default)]
struct TandemHmacSha256V1Verifier;

impl AutomationWebhookSignatureVerifier for TandemHmacSha256V1Verifier {
    fn verify(
        &self,
        input: &AutomationWebhookSignatureVerificationInput<'_>,
    ) -> Result<AutomationWebhookSignatureVerification, AutomationWebhookVerificationError> {
        let signature_header = input
            .headers
            .tandem_signature()
            .ok_or(AutomationWebhookVerificationError::MissingSignature)?;
        let (timestamp_ms, signature) = parse_tandem_signature_header(signature_header)?;
        if webhook_timestamp_is_stale(
            timestamp_ms,
            input.request_now_ms,
            input.signature_tolerance_ms,
        ) {
            return Err(AutomationWebhookVerificationError::StaleTimestamp);
        }
        let expected = automation_webhook_hmac_sha256_signature(
            input.secret,
            &automation_webhook_signature_payload(timestamp_ms, input.body),
        );
        if !timing_safe_eq(&expected, &signature) {
            return Err(AutomationWebhookVerificationError::BadSignature);
        }
        Ok(verification(
            input,
            Some(timestamp_ms),
            "tandem_hmac_sha256_v1_verified",
        ))
    }
}

#[derive(Default)]
struct GithubSha256Verifier;

impl AutomationWebhookSignatureVerifier for GithubSha256Verifier {
    fn verify(
        &self,
        input: &AutomationWebhookSignatureVerificationInput<'_>,
    ) -> Result<AutomationWebhookSignatureVerification, AutomationWebhookVerificationError> {
        let signature_header = input
            .headers
            .github_sha256_signature()
            .ok_or(AutomationWebhookVerificationError::MissingSignature)?;
        let signature = parse_github_sha256_signature_header(signature_header)?;
        let expected = automation_webhook_hmac_sha256_signature(input.secret, input.body);
        if !timing_safe_eq(&expected, &signature) {
            return Err(AutomationWebhookVerificationError::BadSignature);
        }
        Ok(verification(input, None, "github_sha256_verified"))
    }
}

#[derive(Default)]
struct SharedSecretHeaderVerifier;

impl AutomationWebhookSignatureVerifier for SharedSecretHeaderVerifier {
    fn verify(
        &self,
        input: &AutomationWebhookSignatureVerificationInput<'_>,
    ) -> Result<AutomationWebhookSignatureVerification, AutomationWebhookVerificationError> {
        let secret = input
            .headers
            .shared_secret()
            .ok_or(AutomationWebhookVerificationError::MissingSignature)?;
        if !timing_safe_eq(secret.as_bytes(), input.secret.as_bytes()) {
            return Err(AutomationWebhookVerificationError::BadSignature);
        }
        Ok(verification(input, None, "shared_secret_header_verified"))
    }
}

#[derive(Default)]
struct UnsignedDevVerifier;

impl AutomationWebhookSignatureVerifier for UnsignedDevVerifier {
    fn verify(
        &self,
        input: &AutomationWebhookSignatureVerificationInput<'_>,
    ) -> Result<AutomationWebhookSignatureVerification, AutomationWebhookVerificationError> {
        if !input.allow_unsigned_dev {
            return Err(AutomationWebhookVerificationError::MissingSignature);
        }
        Ok(verification(input, None, "unsigned_dev_mode_verified"))
    }
}

fn verification(
    input: &AutomationWebhookSignatureVerificationInput<'_>,
    timestamp_ms: Option<u64>,
    result_code: &'static str,
) -> AutomationWebhookSignatureVerification {
    AutomationWebhookSignatureVerification {
        scheme: input.scheme.clone(),
        provider: input.provider.to_string(),
        timestamp_ms,
        result_code,
    }
}

pub(crate) fn automation_webhook_signature_header(
    secret: &str,
    timestamp_ms: u64,
    body: &[u8],
) -> String {
    let signature = automation_webhook_signature(secret, timestamp_ms, body);
    format!("t={timestamp_ms},v1={signature}")
}

pub(crate) fn automation_webhook_github_sha256_signature_header(
    secret: &str,
    body: &[u8],
) -> String {
    let signature = automation_webhook_hmac_sha256_signature(secret, body);
    format!("sha256={}", hex_encode(&signature))
}

fn automation_webhook_signature(secret: &str, timestamp_ms: u64, body: &[u8]) -> String {
    let signature = automation_webhook_hmac_sha256_signature(
        secret,
        &automation_webhook_signature_payload(timestamp_ms, body),
    );
    hex_encode(&signature)
}

fn automation_webhook_hmac_sha256_signature(secret: &str, payload: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 accepts secrets of any length");
    mac.update(payload);
    mac.finalize().into_bytes().to_vec()
}

fn automation_webhook_signature_payload(timestamp_ms: u64, body: &[u8]) -> Vec<u8> {
    let mut payload = timestamp_ms.to_string().into_bytes();
    payload.push(b'.');
    payload.extend_from_slice(body);
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

fn parse_github_sha256_signature_header(
    header: &str,
) -> Result<Vec<u8>, AutomationWebhookVerificationError> {
    let Some(signature) = header.trim().strip_prefix("sha256=") else {
        return Err(AutomationWebhookVerificationError::MalformedSignature);
    };
    let Some(signature) = hex_decode(signature.trim()) else {
        return Err(AutomationWebhookVerificationError::MalformedSignature);
    };
    if signature.is_empty() {
        return Err(AutomationWebhookVerificationError::MalformedSignature);
    }
    Ok(signature)
}

fn webhook_timestamp_is_stale(timestamp_ms: u64, now_ms: u64, tolerance_ms: u64) -> bool {
    timestamp_ms.abs_diff(now_ms) > tolerance_ms
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

fn timing_safe_eq(left: &[u8], right: &[u8]) -> bool {
    let max_len = left.len().max(right.len());
    let mut diff = left.len() ^ right.len();
    for index in 0..max_len {
        let left_byte = left.get(index).copied().unwrap_or(0);
        let right_byte = right.get(index).copied().unwrap_or(0);
        diff |= usize::from(left_byte ^ right_byte);
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(
        scheme: &'a AutomationWebhookSignatureScheme,
        headers: &'a AutomationWebhookSignatureHeaders,
        body: &'a [u8],
        now: u64,
    ) -> AutomationWebhookSignatureVerificationInput<'a> {
        AutomationWebhookSignatureVerificationInput {
            scheme,
            provider: "generic",
            headers,
            secret: "whsec_test",
            body,
            request_now_ms: now,
            signature_tolerance_ms: 300_000,
            allow_unsigned_dev: false,
        }
    }

    #[test]
    fn registry_verifies_tandem_hmac_sha256_v1() {
        let body = br#"{"ok":true}"#;
        let now = 10_000;
        let headers = AutomationWebhookSignatureHeaders::from_tandem_header(Some(
            &automation_webhook_signature_header("whsec_test", now, body),
        ));
        let verified = verify_automation_webhook_signature(&input(
            &AutomationWebhookSignatureScheme::HmacSha256V1,
            &headers,
            body,
            now,
        ))
        .expect("verify");

        assert_eq!(
            verified.scheme,
            AutomationWebhookSignatureScheme::HmacSha256V1
        );
        assert_eq!(verified.provider, "generic");
        assert_eq!(verified.timestamp_ms, Some(now));
        assert_eq!(verified.result_code, "tandem_hmac_sha256_v1_verified");
    }

    #[test]
    fn registry_rejects_stale_malformed_and_bad_tandem_hmac() {
        let body = br#"{"ok":true}"#;
        let stale = AutomationWebhookSignatureHeaders::from_tandem_header(Some(
            &automation_webhook_signature_header("whsec_test", 1_000, body),
        ));
        assert_eq!(
            verify_automation_webhook_signature(&input(
                &AutomationWebhookSignatureScheme::HmacSha256V1,
                &stale,
                body,
                600_000,
            ))
            .expect_err("stale"),
            AutomationWebhookVerificationError::StaleTimestamp
        );

        let malformed = AutomationWebhookSignatureHeaders::from_tandem_header(Some("wat"));
        assert_eq!(
            verify_automation_webhook_signature(&input(
                &AutomationWebhookSignatureScheme::HmacSha256V1,
                &malformed,
                body,
                1_000,
            ))
            .expect_err("malformed"),
            AutomationWebhookVerificationError::MalformedSignature
        );

        let bad = AutomationWebhookSignatureHeaders::from_tandem_header(Some(
            &automation_webhook_signature_header("wrong", 1_000, body),
        ));
        assert_eq!(
            verify_automation_webhook_signature(&input(
                &AutomationWebhookSignatureScheme::HmacSha256V1,
                &bad,
                body,
                1_000,
            ))
            .expect_err("bad"),
            AutomationWebhookVerificationError::BadSignature
        );
    }

    #[test]
    fn registry_verifies_github_sha256_signature() {
        let body = br#"{"action":"opened"}"#;
        let signature = hex_encode(&automation_webhook_hmac_sha256_signature(
            "whsec_test",
            body,
        ));
        let headers = AutomationWebhookSignatureHeaders {
            github_sha256_signature: Some(format!("sha256={signature}")),
            ..AutomationWebhookSignatureHeaders::default()
        };
        let verified = verify_automation_webhook_signature(&input(
            &AutomationWebhookSignatureScheme::GithubSha256,
            &headers,
            body,
            1_000,
        ))
        .expect("verify");

        assert_eq!(
            verified.scheme,
            AutomationWebhookSignatureScheme::GithubSha256
        );
        assert_eq!(verified.result_code, "github_sha256_verified");
    }

    #[test]
    fn registry_verifies_shared_secret_header() {
        let body = br#"{"ok":true}"#;
        let headers = AutomationWebhookSignatureHeaders {
            shared_secret: Some("whsec_test".to_string()),
            ..AutomationWebhookSignatureHeaders::default()
        };
        let verified = verify_automation_webhook_signature(&input(
            &AutomationWebhookSignatureScheme::SharedSecretHeader,
            &headers,
            body,
            1_000,
        ))
        .expect("verify");

        assert_eq!(
            verified.scheme,
            AutomationWebhookSignatureScheme::SharedSecretHeader
        );
        assert_eq!(verified.result_code, "shared_secret_header_verified");
    }

    #[test]
    fn unsigned_dev_mode_requires_explicit_allowance() {
        let body = br#"{"ok":true}"#;
        let headers = AutomationWebhookSignatureHeaders::default();
        let denied = input(
            &AutomationWebhookSignatureScheme::UnsignedDev,
            &headers,
            body,
            1_000,
        );
        assert_eq!(
            verify_automation_webhook_signature(&denied).expect_err("denied"),
            AutomationWebhookVerificationError::MissingSignature
        );

        let allowed = AutomationWebhookSignatureVerificationInput {
            allow_unsigned_dev: true,
            ..input(
                &AutomationWebhookSignatureScheme::UnsignedDev,
                &headers,
                body,
                1_000,
            )
        };
        let verified = verify_automation_webhook_signature(&allowed).expect("verify");
        assert_eq!(verified.result_code, "unsigned_dev_mode_verified");
    }
}
