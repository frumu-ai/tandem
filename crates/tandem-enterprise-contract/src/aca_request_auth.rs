//! EAA-05 (TAN-30): ACA hosted context verification mode.
//!
//! ACA (Autonomous Coding Agent) connects to the Tandem runtime with a
//! deployment-local API token (`ACA_API_TOKEN` / `ACA_API_TOKEN_FILE`). In
//! hosted deployments it must ALSO present a valid Tandem-signed context
//! assertion whose signature is verified through [`VerifierKeyring`] from
//! EAA-04 (TAN-29).
//!
//! | Mode | Token | Assertion |
//! |---|---|---|
//! | `Local` (default) | required | ignored — local/dev bypass |
//! | `Hosted` | required | required — verified via keyring |
//!
//! The local bypass is available only through explicit `ACA_AUTH_MODE=local`
//! configuration; omitting the env-var defaults to local, which is the
//! backwards-compatible behavior.

#[cfg(test)]
use base64::Engine as _;
use sha2::{Digest, Sha256};

#[cfg(test)]
use crate::TenantContextAssertionClaims;

use crate::{
    parse_context_assertion_metadata_keyring, ContextAssertionError, ContextAssertionPolicy,
    ContextAssertionReplayMode, ContextAssertionReplayStore, ContextAssertionVerifier,
    VerifiedTenantContext, VerifierKeyring, DEFAULT_CONTEXT_ASSERTION_ISSUER,
    DEFAULT_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS, DEFAULT_CONTEXT_ASSERTION_MAX_LIFETIME_MS,
};

const MAX_ALLOWED_FUTURE_SKEW_MS: u64 = 60_000;
const DEFAULT_AUDIENCE: &str = "tandem-aca";

/// Operating mode for ACA request authentication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcaAuthMode {
    /// Local/dev mode: transport token is sufficient; context assertions are not required.
    Local,
    /// Hosted mode: both the transport token and a Tandem-signed context
    /// assertion are required. Missing or invalid assertions are rejected.
    Hosted,
}

impl AcaAuthMode {
    /// Read `ACA_AUTH_MODE` from the environment. Defaults to [`Self::Local`].
    pub fn from_env() -> Self {
        match std::env::var("ACA_AUTH_MODE")
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("hosted") | Some("hosted_context") | Some("hosted-context") => Self::Hosted,
            _ => Self::Local,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Hosted => "hosted",
        }
    }
}

/// Why ACA context-assertion verification failed. Every variant is a hard
/// rejection; there is no allow-on-ambiguity path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcaContextError {
    /// Hosted mode requires a context assertion, but none was presented.
    MissingAssertion,
    /// The JWS structure is invalid (not exactly three dot-separated parts, or
    /// a part could not be base64-decoded).
    MalformedAssertion,
    /// JWS header has an unrecognised `alg`/`typ` or an empty `kid`.
    MalformedHeader,
    /// Ed25519 signature does not match the signing input.
    BadSignature,
    /// The `audience` claim does not match the configured expected audience.
    BadAudience,
    /// Assertion is expired or issued so far in the future it exceeds the
    /// allowed clock-skew tolerance.
    Expired,
    /// No verifier keys are configured; hosted mode requires at least one key.
    KeyNotConfigured,
    /// The `kid` in the assertion header has no entry in the loaded keyring.
    UnknownKey,
    /// The key was found but the keyring denied it (wrong purpose, org,
    /// deployment, audience restriction, status, or validity window).
    KeyringDenied(crate::KeyringDenial),
    BadIssuer,
    InvalidClaims,
    LifetimeExceeded,
    Replayed,
    ReplayBackendUnavailable,
    InvalidPolicy,
}

impl AcaContextError {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MissingAssertion => "aca_context_missing_assertion",
            Self::MalformedAssertion => "aca_context_malformed_assertion",
            Self::MalformedHeader => "aca_context_malformed_header",
            Self::BadSignature => "aca_context_bad_signature",
            Self::BadAudience => "aca_context_bad_audience",
            Self::Expired => "aca_context_expired",
            Self::KeyNotConfigured => "aca_context_key_not_configured",
            Self::UnknownKey => "aca_context_unknown_key",
            Self::KeyringDenied(_) => "aca_context_keyring_denied",
            Self::BadIssuer => "aca_context_bad_issuer",
            Self::InvalidClaims => "aca_context_invalid_claims",
            Self::LifetimeExceeded => "aca_context_lifetime_exceeded",
            Self::Replayed => "aca_context_replayed",
            Self::ReplayBackendUnavailable => "aca_context_replay_backend_unavailable",
            Self::InvalidPolicy => "aca_context_invalid_policy",
        }
    }
}

impl core::fmt::Display for AcaContextError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::error::Error for AcaContextError {}

/// Why ACA transport-token verification failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcaTransportError {
    /// No token is configured; all transport-auth checks fail.
    TokenNotConfigured,
    /// The presented token does not match the configured token.
    TokenMismatch,
}

impl AcaTransportError {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TokenNotConfigured => "aca_transport_token_not_configured",
            Self::TokenMismatch => "aca_transport_token_mismatch",
        }
    }
}

impl core::fmt::Display for AcaTransportError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::error::Error for AcaTransportError {}

/// Verifies Tandem-signed context assertions for ACA hosted mode using the
/// [`VerifierKeyring`] from EAA-04 (TAN-29).
///
/// Verification is fail-closed: every step that can reject an assertion does
/// so before proceeding to the next. Specifically:
///
/// 1. Parse JWS structure (header.claims.signature).
/// 2. Validate header (`alg=EdDSA`, `typ=tandem-tenant-context+jws`, non-empty `kid`).
/// 3. Locate `kid` and decode its Ed25519 public key.
/// 4. Verify the signature before consuming untrusted claim semantics.
/// 5. Validate version, issuer/audience, explicit tenant/deployment, actor chain,
///    resource consistency, bounded lifetime, expiry, and future skew.
/// 6. Authorize key purpose, status, validity, audience, org, deployment, and scope.
/// 7. Atomically enforce the configured shared replay policy.
/// 8. Return only the canonical [`VerifiedTenantContext`] projection.
#[derive(Debug, Clone)]
pub struct AcaContextAssertionVerifier {
    verifier: ContextAssertionVerifier,
    replay_store: ContextAssertionReplayStore,
    replay_mode: ContextAssertionReplayMode,
}

impl AcaContextAssertionVerifier {
    /// Build directly from a keyring and audience (for tests and explicit wiring).
    pub fn new(keyring: VerifierKeyring, audience: impl Into<String>) -> Self {
        let policy = ContextAssertionPolicy::new(
            DEFAULT_CONTEXT_ASSERTION_ISSUER,
            audience,
            DEFAULT_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS,
            DEFAULT_CONTEXT_ASSERTION_MAX_LIFETIME_MS,
        )
        .expect("default ACA assertion policy is valid");
        Self {
            verifier: ContextAssertionVerifier::new_allow_empty(keyring, policy),
            replay_store: ContextAssertionReplayStore::in_memory(),
            replay_mode: ContextAssertionReplayMode::Bound,
        }
    }

    pub fn new_with_policy(
        keyring: VerifierKeyring,
        policy: ContextAssertionPolicy,
        replay_store: ContextAssertionReplayStore,
        replay_mode: ContextAssertionReplayMode,
    ) -> Result<Self, AcaContextError> {
        let verifier = ContextAssertionVerifier::new(keyring, policy).map_err(map_shared_error)?;
        replay_store.readiness_check().map_err(map_shared_error)?;
        Ok(Self {
            verifier,
            replay_store,
            replay_mode,
        })
    }

    /// Load from environment variables:
    ///
    /// - `ACA_CONTEXT_ASSERTION_KEYRING` / `ACA_CONTEXT_ASSERTION_KEYRING_FILE` —
    ///   JSON keyring in the distribution form from `ENTERPRISE_KEY_ROTATION.md`.
    /// - `ACA_CONTEXT_ASSERTION_AUDIENCE` — expected `audience` claim (default: `"tandem-aca"`).
    /// - `ACA_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS` — clock skew tolerance in ms
    ///   (default 10 000, maximum 60 000).
    /// - `ACA_CONTEXT_ASSERTION_MAX_LIFETIME_MS` — maximum validity window
    ///   (default 15 minutes, hard ceiling 1 hour).
    /// - `ACA_CONTEXT_ASSERTION_REPLAY_MODE` — `bound` (default) or `one_shot`; `off` is rejected.
    /// - `ACA_CONTEXT_ASSERTION_REPLAY_STORE_FILE` — shared durable replay ledger.
    pub fn from_env() -> Result<Self, AcaContextError> {
        let keyring_json = read_aca_keyring_from_env()?;
        let keyring =
            parse_context_assertion_metadata_keyring(&keyring_json).map_err(map_shared_error)?;
        if keyring.is_empty() {
            return Err(AcaContextError::KeyNotConfigured);
        }
        let expected_issuer = nonempty_env("ACA_CONTEXT_ASSERTION_ISSUER")
            .unwrap_or_else(|| DEFAULT_CONTEXT_ASSERTION_ISSUER.to_string());
        let expected_audience = nonempty_env("ACA_CONTEXT_ASSERTION_AUDIENCE")
            .unwrap_or_else(|| DEFAULT_AUDIENCE.to_string());
        let max_future_skew_ms = strict_u64_env(
            "ACA_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS",
            DEFAULT_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS,
            MAX_ALLOWED_FUTURE_SKEW_MS,
        )?;
        let max_lifetime_ms = strict_u64_env(
            "ACA_CONTEXT_ASSERTION_MAX_LIFETIME_MS",
            DEFAULT_CONTEXT_ASSERTION_MAX_LIFETIME_MS,
            crate::HARD_CONTEXT_ASSERTION_MAX_LIFETIME_MS,
        )?;
        let replay_mode = ContextAssertionReplayMode::parse(
            nonempty_env("ACA_CONTEXT_ASSERTION_REPLAY_MODE")
                .as_deref()
                .unwrap_or("bound"),
        )
        .map_err(map_shared_error)?;
        if replay_mode == ContextAssertionReplayMode::Off {
            return Err(AcaContextError::InvalidPolicy);
        }
        let replay_path = nonempty_env("ACA_CONTEXT_ASSERTION_REPLAY_STORE_FILE")
            .or_else(|| nonempty_env("TANDEM_CONTEXT_ASSERTION_REPLAY_STORE_FILE"))
            .ok_or(AcaContextError::ReplayBackendUnavailable)?;
        let replay_store =
            ContextAssertionReplayStore::persistent(replay_path).map_err(map_shared_error)?;
        let policy = ContextAssertionPolicy::new(
            expected_issuer,
            expected_audience,
            max_future_skew_ms,
            max_lifetime_ms,
        )
        .map_err(map_shared_error)?;
        Self::new_with_policy(keyring, policy, replay_store, replay_mode)
    }

    /// Verify against wall-clock time and atomically enforce replay policy.
    pub fn verify(&self, assertion: &str) -> Result<VerifiedTenantContext, AcaContextError> {
        let now_ms = current_unix_ms();
        let verified = self
            .verifier
            .verify_at(assertion, now_ms)
            .map_err(map_shared_error)?;
        self.replay_store
            .check_and_record(self.replay_mode, &verified, now_ms)
            .map_err(map_shared_error)?;
        Ok(VerifiedTenantContext::from(verified.claims).with_assertion_key_id(verified.key_id))
    }

    /// Verify at an explicit time, including replay retention boundaries.
    pub fn verify_at(
        &self,
        assertion: &str,
        now_ms: u64,
    ) -> Result<VerifiedTenantContext, AcaContextError> {
        let verified = self
            .verifier
            .verify_at(assertion, now_ms)
            .map_err(map_shared_error)?;
        self.replay_store
            .check_and_record(self.replay_mode, &verified, now_ms)
            .map_err(map_shared_error)?;
        Ok(VerifiedTenantContext::from(verified.claims).with_assertion_key_id(verified.key_id))
    }
}

fn map_shared_error(error: ContextAssertionError) -> AcaContextError {
    match error {
        ContextAssertionError::MalformedAssertion | ContextAssertionError::UnsupportedVersion => {
            AcaContextError::MalformedAssertion
        }
        ContextAssertionError::MalformedHeader => AcaContextError::MalformedHeader,
        ContextAssertionError::BadSignature => AcaContextError::BadSignature,
        ContextAssertionError::BadIssuer => AcaContextError::BadIssuer,
        ContextAssertionError::BadAudience => AcaContextError::BadAudience,
        ContextAssertionError::InvalidIdentity => AcaContextError::InvalidClaims,
        ContextAssertionError::Expired
        | ContextAssertionError::NotYetValid
        | ContextAssertionError::TimeOverflow => AcaContextError::Expired,
        ContextAssertionError::LifetimeExceeded => AcaContextError::LifetimeExceeded,
        ContextAssertionError::KeyNotConfigured => AcaContextError::KeyNotConfigured,
        ContextAssertionError::UnknownKey => AcaContextError::UnknownKey,
        ContextAssertionError::KeyringDenied(denial) => AcaContextError::KeyringDenied(denial),
        ContextAssertionError::Replayed => AcaContextError::Replayed,
        ContextAssertionError::ReplayBackendUnavailable
        | ContextAssertionError::ReplayCapacityExceeded => {
            AcaContextError::ReplayBackendUnavailable
        }
        ContextAssertionError::InvalidPolicy => AcaContextError::InvalidPolicy,
    }
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn strict_u64_env(name: &str, default: u64, maximum: u64) -> Result<u64, AcaContextError> {
    let Some(raw) = nonempty_env(name) else {
        return Ok(default);
    };
    raw.parse::<u64>()
        .ok()
        .filter(|value| *value > 0 && *value <= maximum)
        .ok_or(AcaContextError::InvalidPolicy)
}

/// Combined verifier for ACA requests: transport token + optional context assertion.
///
/// In `Local` mode only the API token is checked (backward-compatible with all
/// existing ACA deployments). In `Hosted` mode both the token and a signed
/// context assertion are required.
#[derive(Debug, Clone)]
pub struct AcaRequestVerifier {
    pub mode: AcaAuthMode,
    api_token: Option<String>,
    context_verifier: Option<AcaContextAssertionVerifier>,
}

impl AcaRequestVerifier {
    /// Build directly (useful in tests and explicit wiring).
    pub fn new(
        mode: AcaAuthMode,
        api_token: Option<String>,
        context_verifier: Option<AcaContextAssertionVerifier>,
    ) -> Self {
        Self {
            mode,
            api_token,
            context_verifier,
        }
    }

    /// Load from environment variables:
    ///
    /// - `ACA_AUTH_MODE` — `"local"` (default) or `"hosted"`.
    /// - `ACA_API_TOKEN` / `ACA_API_TOKEN_FILE` — transport token material.
    /// - `ACA_CONTEXT_ASSERTION_KEYRING` / `ACA_CONTEXT_ASSERTION_KEYRING_FILE` —
    ///   JSON keyring (only loaded in hosted mode).
    pub fn from_env() -> Result<Self, AcaContextError> {
        let mode = AcaAuthMode::from_env();
        let api_token = read_aca_api_token_from_env();
        let context_verifier = match mode {
            AcaAuthMode::Hosted => Some(AcaContextAssertionVerifier::from_env()?),
            AcaAuthMode::Local => None,
        };
        Ok(Self {
            mode,
            api_token,
            context_verifier,
        })
    }

    /// Verify the transport token using constant-time comparison.
    ///
    /// Returns `Ok(())` if the presented token matches the configured token.
    pub fn verify_token(&self, presented: &str) -> Result<(), AcaTransportError> {
        let expected = self
            .api_token
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or(AcaTransportError::TokenNotConfigured)?;
        if constant_time_token_eq(presented.trim(), expected) {
            Ok(())
        } else {
            Err(AcaTransportError::TokenMismatch)
        }
    }

    /// Verify the context assertion for the current mode.
    ///
    /// - `Local` mode: returns `Ok(None)` regardless of whether an assertion
    ///   is present (backward-compatible bypass).
    /// - `Hosted` mode: `assertion` must be `Some` non-empty string that passes
    ///   full keyring-based verification. Only the canonical verified tenant
    ///   projection is returned on success.
    pub fn verify_context_assertion(
        &self,
        assertion: Option<&str>,
    ) -> Result<Option<VerifiedTenantContext>, AcaContextError> {
        match self.mode {
            AcaAuthMode::Local => Ok(None),
            AcaAuthMode::Hosted => {
                let assertion = assertion
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or(AcaContextError::MissingAssertion)?;
                let verifier = self
                    .context_verifier
                    .as_ref()
                    .ok_or(AcaContextError::KeyNotConfigured)?;
                verifier.verify(assertion).map(Some)
            }
        }
    }
}

fn read_aca_keyring_from_env() -> Result<String, AcaContextError> {
    if let Ok(raw) = std::env::var("ACA_CONTEXT_ASSERTION_KEYRING") {
        let raw = raw.trim().to_string();
        if !raw.is_empty() {
            return Ok(raw);
        }
    }
    if let Ok(path) = std::env::var("ACA_CONTEXT_ASSERTION_KEYRING_FILE") {
        let path = path.trim().to_string();
        if !path.is_empty() {
            return crate::context_assertion_security::read_owner_only_regular_text_file(
                std::path::Path::new(&path),
            )
            .map_err(|_| AcaContextError::KeyNotConfigured);
        }
    }
    Err(AcaContextError::KeyNotConfigured)
}

fn read_aca_api_token_from_env() -> Option<String> {
    if let Ok(token) = std::env::var("ACA_API_TOKEN") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Some(token);
        }
    }
    if let Ok(path) = std::env::var("ACA_API_TOKEN_FILE") {
        let path = path.trim().to_string();
        if !path.is_empty() {
            if let Ok(contents) = std::fs::read_to_string(&path) {
                let token = contents.trim().to_string();
                if !token.is_empty() {
                    return Some(token);
                }
            }
        }
    }
    None
}

#[cfg(test)]
fn decode_base64url(raw: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(raw))
        .ok()
}

fn constant_time_token_eq(provided: &str, expected: &str) -> bool {
    let provided_hash = Sha256::digest(provided.as_bytes());
    let expected_hash = Sha256::digest(expected.as_bytes());
    let mut diff = 0u8;
    for (a, b) in provided_hash.iter().zip(expected_hash.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AuthorityChain, HumanActor, KeyStatus, RequestPrincipal, SigningKeyPurpose, TenantContext,
        TenantContextAssertionHeader, VerifierKeyEntry,
    };
    use ed25519_dalek::SigningKey;

    fn test_signing_key() -> SigningKey {
        SigningKey::from_bytes(&[42u8; 32])
    }

    fn test_keyring(signing_key: &SigningKey) -> VerifierKeyring {
        let public_key_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(signing_key.verifying_key().to_bytes());
        VerifierKeyring::from_entries([VerifierKeyEntry::new(
            "aca-key-1",
            SigningKeyPurpose::ContextAssertion,
            public_key_b64,
        )])
    }

    fn test_verifier(signing_key: &SigningKey) -> AcaContextAssertionVerifier {
        AcaContextAssertionVerifier::new(test_keyring(signing_key), DEFAULT_AUDIENCE)
    }

    fn sign_assertion(
        signing_key: &SigningKey,
        kid: &str,
        claims: &TenantContextAssertionClaims,
    ) -> String {
        use ed25519_dalek::Signer;

        let header = TenantContextAssertionHeader::ed25519(kid);
        let encoded_header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&header).expect("header"));
        let encoded_claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(claims).expect("claims"));
        let signing_input = format!("{encoded_header}.{encoded_claims}");
        let signature = signing_key.sign(signing_input.as_bytes());
        let encoded_sig =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.to_bytes());
        format!("{signing_input}.{encoded_sig}")
    }

    fn test_claims(issued_at_ms: u64, expires_at_ms: u64) -> TenantContextAssertionClaims {
        let tenant = TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("dep-a".to_string()),
            "user-a",
        );
        let principal = RequestPrincipal::authenticated_user("user-a", "tandem-web");
        TenantContextAssertionClaims::new_v1(
            "tandem-web",
            DEFAULT_AUDIENCE,
            issued_at_ms,
            expires_at_ms,
            "assertion-aca-1",
            tenant,
            HumanActor::tandem_user("user-a"),
            AuthorityChain::from_request(principal),
            vec!["workspace:user".to_string()],
        )
    }

    // ── Local mode ───────────────────────────────────────────────────────────

    #[test]
    fn local_mode_accepts_missing_context_assertion() {
        let verifier = AcaRequestVerifier::new(AcaAuthMode::Local, Some("tk".into()), None);
        assert_eq!(verifier.verify_context_assertion(None), Ok(None));
    }

    #[test]
    fn local_mode_ignores_provided_context_assertion() {
        let verifier = AcaRequestVerifier::new(AcaAuthMode::Local, Some("tk".into()), None);
        assert_eq!(
            verifier.verify_context_assertion(Some("dummy.payload.sig")),
            Ok(None)
        );
    }

    #[test]
    fn local_mode_transport_token_accepted() {
        let verifier = AcaRequestVerifier::new(AcaAuthMode::Local, Some("my-token".into()), None);
        assert!(verifier.verify_token("my-token").is_ok());
    }

    #[test]
    fn local_mode_transport_token_rejected_on_mismatch() {
        let verifier = AcaRequestVerifier::new(AcaAuthMode::Local, Some("my-token".into()), None);
        assert_eq!(
            verifier.verify_token("wrong-token"),
            Err(AcaTransportError::TokenMismatch)
        );
    }

    #[test]
    fn transport_token_not_configured_returns_error() {
        let verifier = AcaRequestVerifier::new(AcaAuthMode::Local, None, None);
        assert_eq!(
            verifier.verify_token("any-token"),
            Err(AcaTransportError::TokenNotConfigured)
        );
    }

    // ── Token-only compatibility (local mode with token) ─────────────────────

    #[test]
    fn token_only_compatibility_does_not_require_context_assertion() {
        let verifier = AcaRequestVerifier::new(AcaAuthMode::Local, Some("aca-token".into()), None);
        assert!(verifier.verify_token("aca-token").is_ok());
        assert_eq!(verifier.verify_context_assertion(None), Ok(None));
    }

    // ── Hosted mode: valid context assertion ─────────────────────────────────

    #[test]
    fn hosted_mode_accepts_valid_context_assertion() {
        let signing_key = test_signing_key();
        let cv = test_verifier(&signing_key);
        let claims = test_claims(1_000, 9_000);
        let assertion = sign_assertion(&signing_key, "aca-key-1", &claims);

        let result = cv.verify_at(&assertion, 5_000).expect("should verify");
        assert_eq!(result.audience, DEFAULT_AUDIENCE);
        assert_eq!(result.tenant_context.org_id, "org-a");
    }

    #[test]
    fn hosted_mode_full_verifier_accepts_valid_assertion() {
        let signing_key = test_signing_key();
        let cv = test_verifier(&signing_key);
        let verifier = AcaRequestVerifier::new(
            AcaAuthMode::Hosted,
            Some("aca-token".into()),
            Some(cv.clone()),
        );
        // Use timestamps well into the future so wall-clock verify() succeeds.
        let now_ms = current_unix_ms();
        let assertion = sign_assertion(
            &signing_key,
            "aca-key-1",
            &test_claims(now_ms, now_ms + 300_000),
        );

        assert!(verifier.verify_token("aca-token").is_ok());
        let claims = verifier
            .verify_context_assertion(Some(&assertion))
            .expect("should verify")
            .expect("verified context present in hosted mode");
        assert_eq!(claims.audience, DEFAULT_AUDIENCE);
        assert_eq!(claims.tenant_context.org_id, "org-a");
        assert_eq!(claims.assertion_key_id.as_deref(), Some("aca-key-1"));
    }

    // ── Hosted mode: expired assertion ────────────────────────────────────────

    #[test]
    fn hosted_mode_rejects_expired_assertion() {
        let signing_key = test_signing_key();
        let cv = test_verifier(&signing_key);
        let claims = test_claims(1_000, 2_000);
        let assertion = sign_assertion(&signing_key, "aca-key-1", &claims);

        // now_ms is after expires_at_ms
        let err = cv
            .verify_at(&assertion, 3_000)
            .expect_err("expired assertion must be rejected");
        assert_eq!(err, AcaContextError::Expired);
    }

    #[test]
    fn hosted_mode_rejects_far_future_assertion() {
        let signing_key = test_signing_key();
        let cv = test_verifier(&signing_key);
        // issued_at far in the future relative to now
        let claims = test_claims(100_000, 200_000);
        let assertion = sign_assertion(&signing_key, "aca-key-1", &claims);

        let err = cv
            .verify_at(&assertion, 1_000)
            .expect_err("future-dated assertion beyond skew must be rejected");
        assert_eq!(err, AcaContextError::Expired);
    }

    // ── Hosted mode: bad signature ────────────────────────────────────────────

    #[test]
    fn hosted_mode_rejects_bad_signature() {
        let signing_key = test_signing_key();
        let cv = test_verifier(&signing_key);
        let claims = test_claims(1_000, 9_000);
        let good_assertion = sign_assertion(&signing_key, "aca-key-1", &claims);

        // Flip the last byte of the signature to corrupt it.
        let parts: Vec<&str> = good_assertion.split('.').collect();
        let mut sig_bytes = decode_base64url(parts[2]).unwrap();
        *sig_bytes.last_mut().unwrap() ^= 0xFF;
        let bad_sig = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&sig_bytes);
        let bad_assertion = format!("{}.{}.{}", parts[0], parts[1], bad_sig);

        let err = cv
            .verify_at(&bad_assertion, 5_000)
            .expect_err("corrupted signature must be rejected");
        assert_eq!(err, AcaContextError::BadSignature);
    }

    #[test]
    fn hosted_mode_rejects_assertion_signed_with_wrong_key() {
        let signing_key = test_signing_key();
        let cv = test_verifier(&signing_key);

        // Sign with a different key — the keyring won't contain it but the
        // kid still resolves to the correct entry, so signature fails.
        let wrong_key = SigningKey::from_bytes(&[99u8; 32]);
        let claims = test_claims(1_000, 9_000);
        let assertion = sign_assertion(&wrong_key, "aca-key-1", &claims);

        let err = cv
            .verify_at(&assertion, 5_000)
            .expect_err("wrong-key signature must be rejected");
        assert_eq!(err, AcaContextError::BadSignature);
    }

    // ── Hosted mode: wrong audience ───────────────────────────────────────────

    #[test]
    fn hosted_mode_rejects_wrong_audience_in_claims() {
        let signing_key = test_signing_key();
        let cv = test_verifier(&signing_key);

        // Build claims with a different audience.
        let tenant = TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("dep-a".to_string()),
            "user-a",
        );
        let principal = RequestPrincipal::authenticated_user("user-a", "tandem-web");
        let wrong_audience_claims = TenantContextAssertionClaims::new_v1(
            "tandem-web",
            "wrong-audience",
            1_000,
            9_000,
            "assertion-bad-aud",
            tenant,
            HumanActor::tandem_user("user-a"),
            AuthorityChain::from_request(principal),
            vec![],
        );
        let assertion = sign_assertion(&signing_key, "aca-key-1", &wrong_audience_claims);

        let err = cv
            .verify_at(&assertion, 5_000)
            .expect_err("wrong-audience assertion must be rejected");
        assert_eq!(err, AcaContextError::BadAudience);
    }

    #[test]
    fn hosted_mode_rejects_identity_issuer_version_and_lifetime_drift() {
        let signing_key = test_signing_key();
        let verifier = test_verifier(&signing_key);

        let mut actor_mismatch = test_claims(1_000, 9_000);
        actor_mismatch.tenant_context.actor_id = Some("other-user".to_string());
        assert_eq!(
            verifier.verify_at(
                &sign_assertion(&signing_key, "aca-key-1", &actor_mismatch),
                5_000,
            ),
            Err(AcaContextError::InvalidClaims)
        );

        let mut wrong_issuer = test_claims(1_000, 9_000);
        wrong_issuer.issuer = "untrusted-issuer".to_string();
        assert_eq!(
            verifier.verify_at(
                &sign_assertion(&signing_key, "aca-key-1", &wrong_issuer),
                5_000,
            ),
            Err(AcaContextError::BadIssuer)
        );

        let mut wrong_version = test_claims(1_000, 9_000);
        wrong_version.version = "v2".to_string();
        assert_eq!(
            verifier.verify_at(
                &sign_assertion(&signing_key, "aca-key-1", &wrong_version),
                5_000,
            ),
            Err(AcaContextError::MalformedAssertion)
        );

        let overlong = test_claims(1_000, 1_000 + DEFAULT_CONTEXT_ASSERTION_MAX_LIFETIME_MS + 1);
        assert_eq!(
            verifier.verify_at(&sign_assertion(&signing_key, "aca-key-1", &overlong), 5_000,),
            Err(AcaContextError::LifetimeExceeded)
        );
    }

    #[test]
    fn hosted_mode_one_shot_replay_is_rejected() {
        let signing_key = test_signing_key();
        let verifier = AcaContextAssertionVerifier::new_with_policy(
            test_keyring(&signing_key),
            ContextAssertionPolicy::new(
                DEFAULT_CONTEXT_ASSERTION_ISSUER,
                DEFAULT_AUDIENCE,
                DEFAULT_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS,
                DEFAULT_CONTEXT_ASSERTION_MAX_LIFETIME_MS,
            )
            .expect("policy"),
            ContextAssertionReplayStore::in_memory(),
            ContextAssertionReplayMode::OneShot,
        )
        .expect("verifier");
        let assertion = sign_assertion(&signing_key, "aca-key-1", &test_claims(1_000, 9_000));
        verifier
            .verify_at(&assertion, 5_000)
            .expect("first presentation");
        assert_eq!(
            verifier.verify_at(&assertion, 5_000),
            Err(AcaContextError::Replayed)
        );
    }

    // ── Hosted mode: missing assertion ────────────────────────────────────────

    #[test]
    fn hosted_mode_rejects_missing_assertion() {
        let signing_key = test_signing_key();
        let cv = test_verifier(&signing_key);
        let verifier = AcaRequestVerifier::new(AcaAuthMode::Hosted, Some("tk".into()), Some(cv));

        let err = verifier
            .verify_context_assertion(None)
            .expect_err("hosted mode must reject missing assertion");
        assert_eq!(err, AcaContextError::MissingAssertion);
    }

    // ── Hosted mode: key not configured ──────────────────────────────────────

    #[test]
    fn hosted_mode_rejects_when_key_not_configured() {
        let verifier = AcaRequestVerifier::new(AcaAuthMode::Hosted, Some("tk".into()), None);

        let err = verifier
            .verify_context_assertion(Some("header.claims.sig"))
            .expect_err("hosted mode without key must fail closed");
        assert_eq!(err, AcaContextError::KeyNotConfigured);
    }

    #[test]
    fn direct_empty_keyring_constructor_fails_closed_without_panicking() {
        let verifier = AcaContextAssertionVerifier::new(VerifierKeyring::new(), DEFAULT_AUDIENCE);
        assert_eq!(
            verifier.verify_at("malformed", 5_000),
            Err(AcaContextError::KeyNotConfigured)
        );
    }

    // ── Keyring: key status checks ────────────────────────────────────────────

    #[test]
    fn hosted_mode_rejects_retired_key() {
        let signing_key = test_signing_key();
        let public_key_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(signing_key.verifying_key().to_bytes());
        let retired_entry = VerifierKeyEntry::new(
            "aca-key-retired",
            SigningKeyPurpose::ContextAssertion,
            public_key_b64,
        )
        .with_status(KeyStatus::Retired);
        let keyring = VerifierKeyring::from_entries([retired_entry]);
        let cv = AcaContextAssertionVerifier::new(keyring, DEFAULT_AUDIENCE);

        let claims = test_claims(1_000, 9_000);
        let assertion = sign_assertion(&signing_key, "aca-key-retired", &claims);

        let err = cv
            .verify_at(&assertion, 5_000)
            .expect_err("retired key must be rejected");
        assert!(matches!(err, AcaContextError::KeyringDenied(_)));
    }

    #[test]
    fn hosted_mode_rejects_unknown_kid() {
        let signing_key = test_signing_key();
        let cv = test_verifier(&signing_key);
        let claims = test_claims(1_000, 9_000);
        let assertion = sign_assertion(&signing_key, "nonexistent-kid", &claims);

        let err = cv
            .verify_at(&assertion, 5_000)
            .expect_err("unknown kid must be rejected");
        assert_eq!(err, AcaContextError::UnknownKey);
    }

    // ── AcaAuthMode::from_env ─────────────────────────────────────────────────

    #[test]
    fn auth_mode_defaults_to_local() {
        let prev = std::env::var("ACA_AUTH_MODE").ok();
        std::env::remove_var("ACA_AUTH_MODE");
        assert_eq!(AcaAuthMode::from_env(), AcaAuthMode::Local);
        if let Some(v) = prev {
            std::env::set_var("ACA_AUTH_MODE", v);
        }
    }

    #[test]
    fn auth_mode_parses_hosted_from_env() {
        let prev = std::env::var("ACA_AUTH_MODE").ok();
        std::env::set_var("ACA_AUTH_MODE", "hosted");
        assert_eq!(AcaAuthMode::from_env(), AcaAuthMode::Hosted);
        match prev {
            Some(v) => std::env::set_var("ACA_AUTH_MODE", v),
            None => std::env::remove_var("ACA_AUTH_MODE"),
        }
    }
}
