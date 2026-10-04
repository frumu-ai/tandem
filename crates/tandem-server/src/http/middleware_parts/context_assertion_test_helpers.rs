// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct TenantContextAssertionVerifier {
    public_keys_by_id: BTreeMap<String, ContextAssertionPublicKey>,
    legacy_public_key: Option<ContextAssertionPublicKey>,
    issuer: String,
    audience: String,
    max_future_skew_ms: u64,
}

#[cfg(test)]
struct SignedTenantContextAssertion {
    claims: TenantContextAssertionClaims,
    key_id: String,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct ContextAssertionPublicKey {
    public_key: [u8; 32],
    purpose: Option<SigningKeyPurpose>,
    organization_id: Option<String>,
    deployment_id: Option<String>,
    allowed_audiences: Vec<String>,
    allowed_resource_scope_prefixes: Vec<String>,
    not_before_ms: Option<u64>,
    not_after_ms: Option<u64>,
    status: Option<String>,
}

#[cfg(test)]
impl ContextAssertionPublicKey {
    fn legacy(public_key: [u8; 32]) -> Self {
        Self {
            public_key,
            purpose: None,
            organization_id: None,
            deployment_id: None,
            allowed_audiences: Vec::new(),
            allowed_resource_scope_prefixes: Vec::new(),
            not_before_ms: None,
            not_after_ms: None,
            status: None,
        }
    }
}

#[cfg(test)]
fn context_assertion_keyring_summary(
    verifier: &TenantContextAssertionVerifier,
) -> Vec<serde_json::Value> {
    let mut rows = verifier
        .public_keys_by_id
        .iter()
        .map(|(kid, key)| {
            json!({
                "kid": kid,
                "status": key.status.as_deref().unwrap_or("unspecified"),
                "purpose": key.purpose.map(|purpose| purpose.as_str()),
                "not_before_ms": key.not_before_ms,
                "not_after_ms": key.not_after_ms,
                "organization_id": key.organization_id.as_deref(),
                "deployment_id": key.deployment_id.as_deref(),
                "allowed_audiences": key.allowed_audiences.clone(),
                "allowed_resource_scope_prefixes": key.allowed_resource_scope_prefixes.clone(),
            })
        })
        .collect::<Vec<_>>();
    if let Some(key) = verifier.legacy_public_key.as_ref() {
        rows.push(json!({
            "kid": "legacy",
            "status": key.status.as_deref().unwrap_or("unspecified"),
            "purpose": key.purpose.map(|purpose| purpose.as_str()),
            "not_before_ms": key.not_before_ms,
            "not_after_ms": key.not_after_ms,
            "legacy": true,
        }));
    }
    rows
}

#[cfg(test)]
fn log_context_assertion_keyring_summary_once(verifier: &TenantContextAssertionVerifier) {
    static LOGGED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    let _ = LOGGED.get_or_init(|| {
        let keyring = context_assertion_keyring_summary(verifier);
        tracing::info!(
            key_count = keyring.len(),
            max_future_skew_ms = verifier.max_future_skew_ms,
            keys = ?keyring,
            "loaded context assertion verification keys"
        );
    });
}

#[cfg(test)]
impl TenantContextAssertionVerifier {
    fn from_env() -> Result<Self, TenantContextIngressError> {
        let public_keys_by_id = read_context_public_keyring_from_env()?;
        let legacy_public_key = read_legacy_context_public_key_from_env()?;
        if public_keys_by_id.is_empty() && legacy_public_key.is_none() {
            return Err(TenantContextIngressError::ContextAssertionKeyNotConfigured);
        }
        let issuer = std::env::var("TANDEM_CONTEXT_ASSERTION_ISSUER")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "tandem-web".to_string());
        let audience = std::env::var("TANDEM_CONTEXT_ASSERTION_AUDIENCE")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "tandem-runtime".to_string());

        let verifier = Self {
            public_keys_by_id,
            legacy_public_key,
            issuer,
            audience,
            max_future_skew_ms: resolve_context_assertion_max_future_skew_ms(),
        };
        log_context_assertion_keyring_summary_once(&verifier);
        Ok(verifier)
    }

    fn verify(&self, assertion: &str) -> Result<VerifiedTenantContext, TenantContextIngressError> {
        self.verify_at(assertion, current_unix_ms())
    }

    fn verify_at(
        &self,
        assertion: &str,
        now_ms: u64,
    ) -> Result<VerifiedTenantContext, TenantContextIngressError> {
        let signed = self.verify_signed_claims_at(assertion, now_ms)?;
        self.validate_claim_time(&signed.claims, now_ms)?;
        Ok(VerifiedTenantContext::from(signed.claims).with_assertion_key_id(signed.key_id))
    }

    fn denial_for_error(
        &self,
        assertion: &str,
        reason: TenantContextIngressError,
    ) -> TenantContextIngressDenial {
        if reason != TenantContextIngressError::ContextAssertionExpired {
            return TenantContextIngressDenial::from_assertion(reason, assertion);
        }
        self.verify_signed_claims_at(assertion, current_unix_ms())
            .map(|signed| {
                VerifiedTenantContext::from(signed.claims).with_assertion_key_id(signed.key_id)
            })
            .map(|verified| TenantContextIngressDenial::verified(reason, &verified))
            .unwrap_or_else(|_| TenantContextIngressDenial::untrusted(reason))
    }

    fn verify_signed_claims_at(
        &self,
        assertion: &str,
        now_ms: u64,
    ) -> Result<SignedTenantContextAssertion, TenantContextIngressError> {
        let assertion = assertion.trim();
        let mut parts = assertion.split('.');
        let encoded_header = parts
            .next()
            .filter(|part| !part.is_empty())
            .ok_or(TenantContextIngressError::ContextAssertionMalformed)?;
        let encoded_claims = parts
            .next()
            .filter(|part| !part.is_empty())
            .ok_or(TenantContextIngressError::ContextAssertionMalformed)?;
        let encoded_signature = parts
            .next()
            .filter(|part| !part.is_empty())
            .ok_or(TenantContextIngressError::ContextAssertionMalformed)?;
        if parts.next().is_some() {
            return Err(TenantContextIngressError::ContextAssertionMalformed);
        }

        let header_bytes = decode_base64url(encoded_header)
            .ok_or(TenantContextIngressError::ContextAssertionMalformed)?;
        let claims_bytes = decode_base64url(encoded_claims)
            .ok_or(TenantContextIngressError::ContextAssertionMalformed)?;
        let signature_bytes: [u8; 64] = decode_base64url(encoded_signature)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(TenantContextIngressError::ContextAssertionMalformed)?;

        let header: TenantContextAssertionHeader = serde_json::from_slice(&header_bytes)
            .map_err(|_| TenantContextIngressError::ContextAssertionMalformed)?;
        validate_context_assertion_header(&header)?;

        let (key, key_id) = self
            .key_for_header_kid(&header.kid)
            .ok_or(TenantContextIngressError::ContextAssertionUntrusted)?;
        let verifying_key = VerifyingKey::from_bytes(&key.public_key)
            .map_err(|_| TenantContextIngressError::ContextAssertionKeyNotConfigured)?;
        let signature = Signature::from_bytes(&signature_bytes);
        let signing_input = format!("{encoded_header}.{encoded_claims}");
        verifying_key
            .verify(signing_input.as_bytes(), &signature)
            .map_err(|_| TenantContextIngressError::ContextAssertionUntrusted)?;

        let claims: TenantContextAssertionClaims = serde_json::from_slice(&claims_bytes)
            .map_err(|_| TenantContextIngressError::ContextAssertionMalformed)?;
        self.validate_claim_identity(&claims)?;
        validate_context_assertion_key_metadata(key, &claims, now_ms)?;
        Ok(SignedTenantContextAssertion { claims, key_id })
    }

    fn key_for_header_kid(&self, kid: &str) -> Option<(&ContextAssertionPublicKey, String)> {
        if !self.public_keys_by_id.is_empty() {
            return self
                .public_keys_by_id
                .get(kid)
                .map(|key| (key, kid.to_string()));
        }
        self.legacy_public_key
            .as_ref()
            .map(|key| (key, "legacy".to_string()))
    }

    fn validate_claim_identity(
        &self,
        claims: &TenantContextAssertionClaims,
    ) -> Result<(), TenantContextIngressError> {
        if claims.version != "v1" {
            return Err(TenantContextIngressError::ContextAssertionMalformed);
        }
        if claims.issuer != self.issuer || claims.audience != self.audience {
            return Err(TenantContextIngressError::ContextAssertionUntrusted);
        }
        if claims.assertion_id.trim().is_empty()
            || claims.human_actor.actor_id.trim().is_empty()
            || claims.tenant_context.org_id.trim().is_empty()
            || claims.tenant_context.workspace_id.trim().is_empty()
        {
            return Err(TenantContextIngressError::ContextAssertionMalformed);
        }
        if claims.tenant_context.source != TenantSource::Explicit
            || claims
                .tenant_context
                .deployment_id
                .as_deref()
                .map(str::trim)
                .filter(|deployment_id| !deployment_id.is_empty())
                .is_none()
        {
            return Err(TenantContextIngressError::ContextAssertionMalformed);
        }
        if claims.tenant_context.actor_id.as_deref() != Some(claims.human_actor.actor_id.as_str()) {
            return Err(TenantContextIngressError::ContextAssertionUntrusted);
        }
        if claims.authority_chain.initiated_by.actor_id.as_deref()
            != Some(claims.human_actor.actor_id.as_str())
        {
            return Err(TenantContextIngressError::ContextAssertionUntrusted);
        }
        Ok(())
    }

    fn validate_claim_time(
        &self,
        claims: &TenantContextAssertionClaims,
        now_ms: u64,
    ) -> Result<(), TenantContextIngressError> {
        if claims.is_expired_at(now_ms) || claims.issued_at_ms > now_ms + self.max_future_skew_ms {
            return Err(TenantContextIngressError::ContextAssertionExpired);
        }
        Ok(())
    }
}

/// Replay handling for verified context assertions.
///
/// Assertions are bearer context that first-party clients legitimately reuse
/// across many requests within the expiry window (e.g. tandem-channels caches
/// one assertion per process), so the default cannot be one-shot:
///
/// - `bound` (default): the first use binds an `assertion_id` to the SHA-256
///   of the exact assertion bytes. Re-presenting the identical assertion is
///   allowed until expiry; a different assertion carrying the same
///   `assertion_id` is rejected as a replay/substitution.
/// - `one_shot`: an `assertion_id` is accepted exactly once. Requires the
///   issuing control plane to mint a fresh assertion per request.
/// - `off`: no replay tracking (unsafe; migration escape hatch only).
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssertionReplayMode {
    Bound,
    OneShot,
    Off,
}

#[cfg(test)]
fn resolve_assertion_replay_mode() -> AssertionReplayMode {
    match std::env::var("TANDEM_CONTEXT_ASSERTION_REPLAY_MODE")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("one_shot") | Some("one-shot") | Some("oneshot") => AssertionReplayMode::OneShot,
        Some("off") => AssertionReplayMode::Off,
        _ => AssertionReplayMode::Bound,
    }
}

#[cfg(test)]
struct AssertionReplayEntry {
    fingerprint: [u8; 32],
    expires_at_ms: u64,
}

#[cfg(test)]
struct AssertionReplayGuard {
    entries: std::sync::Mutex<std::collections::HashMap<String, AssertionReplayEntry>>,
}

/// Sweep expired entries once the map grows past this size, bounding memory
/// without a background task.
#[cfg(test)]
const ASSERTION_REPLAY_SWEEP_THRESHOLD: usize = 1024;

/// Entries are retained slightly past assertion expiry so a clock-skewed
/// replay near the expiry boundary still hits the cache instead of slipping
/// through between sweep and expiry validation.
#[cfg(test)]
const ASSERTION_REPLAY_RETENTION_GRACE_MS: u64 = 60_000;

#[cfg(test)]
impl AssertionReplayGuard {
    fn new() -> Self {
        Self {
            entries: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    fn global() -> &'static Self {
        static GUARD: std::sync::OnceLock<AssertionReplayGuard> = std::sync::OnceLock::new();
        GUARD.get_or_init(AssertionReplayGuard::new)
    }

    fn check_and_record(
        &self,
        mode: AssertionReplayMode,
        assertion_id: &str,
        fingerprint: [u8; 32],
        expires_at_ms: u64,
        now_ms: u64,
    ) -> Result<(), TenantContextIngressError> {
        if mode == AssertionReplayMode::Off {
            return Ok(());
        }
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if entries.len() >= ASSERTION_REPLAY_SWEEP_THRESHOLD {
            entries.retain(|_, entry| {
                entry
                    .expires_at_ms
                    .saturating_add(ASSERTION_REPLAY_RETENTION_GRACE_MS)
                    > now_ms
            });
        }
        match entries.get(assertion_id) {
            None => {
                entries.insert(
                    assertion_id.to_string(),
                    AssertionReplayEntry {
                        fingerprint,
                        expires_at_ms,
                    },
                );
                Ok(())
            }
            Some(entry)
                if entry
                    .expires_at_ms
                    .saturating_add(ASSERTION_REPLAY_RETENTION_GRACE_MS)
                    <= now_ms =>
            {
                entries.insert(
                    assertion_id.to_string(),
                    AssertionReplayEntry {
                        fingerprint,
                        expires_at_ms,
                    },
                );
                Ok(())
            }
            Some(entry) => match mode {
                AssertionReplayMode::OneShot => {
                    Err(TenantContextIngressError::ContextAssertionReplayed)
                }
                AssertionReplayMode::Bound if entry.fingerprint == fingerprint => Ok(()),
                AssertionReplayMode::Bound => {
                    Err(TenantContextIngressError::ContextAssertionReplayed)
                }
                AssertionReplayMode::Off => Ok(()),
            },
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

#[cfg(test)]
fn assertion_fingerprint(assertion: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(assertion.trim().as_bytes());
    hasher.finalize().into()
}

#[cfg(test)]
fn enforce_context_assertion_replay_policy(
    assertion: &str,
    verified: &VerifiedTenantContext,
) -> Result<(), TenantContextIngressError> {
    let mode = resolve_assertion_replay_mode();
    let result = AssertionReplayGuard::global().check_and_record(
        mode,
        &verified.assertion_id,
        assertion_fingerprint(assertion),
        verified.expires_at_ms,
        current_unix_ms(),
    );
    if let Err(error) = result {
        tracing::warn!(
            assertion_id = %verified.assertion_id,
            org_id = %verified.tenant_context.org_id,
            replay_mode = ?mode,
            "Authorization denied: context assertion rejected as replayed - reason={}",
            error.as_str()
        );
    }
    result
}

#[cfg(test)]
fn resolve_context_assertion_max_future_skew_ms() -> u64 {
    std::env::var("TANDEM_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS)
        .clamp(
            DEFAULT_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS,
            MAX_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS,
        )
}

#[cfg(test)]
fn read_context_public_keyring_from_env(
) -> Result<BTreeMap<String, ContextAssertionPublicKey>, TenantContextIngressError> {
    let Some(raw_keys) = std::env::var("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            let path = std::env::var("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS_FILE")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())?;
            std::fs::read_to_string(path)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
    else {
        return Ok(BTreeMap::new());
    };
    parse_context_public_keyring(&raw_keys)
        .ok_or(TenantContextIngressError::ContextAssertionKeyNotConfigured)
}

#[cfg(test)]
fn read_legacy_context_public_key_from_env(
) -> Result<Option<ContextAssertionPublicKey>, TenantContextIngressError> {
    let Some(raw_key) = std::env::var("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            let path = std::env::var("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY_FILE")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())?;
            std::fs::read_to_string(path)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
    else {
        return Ok(None);
    };
    decode_context_public_key(&raw_key)
        .map(ContextAssertionPublicKey::legacy)
        .map(Some)
        .ok_or(TenantContextIngressError::ContextAssertionKeyNotConfigured)
}

#[cfg(test)]
fn parse_context_public_keyring(raw: &str) -> Option<BTreeMap<String, ContextAssertionPublicKey>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Some(BTreeMap::new());
    }
    if trimmed.starts_with('{') {
        let parsed = serde_json::from_str::<BTreeMap<String, serde_json::Value>>(trimmed).ok()?;
        return parse_context_public_keyring_json_entries(parsed);
    }

    let mut entries = BTreeMap::new();
    for entry in trimmed.split([',', '\n', ';']) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (kid, key) = entry.split_once('=').or_else(|| entry.split_once(':'))?;
        entries.insert(kid.trim().to_string(), key.trim().to_string());
    }
    parse_context_public_keyring_entries(entries)
}

#[cfg(test)]
fn parse_context_public_keyring_entries(
    entries: BTreeMap<String, String>,
) -> Option<BTreeMap<String, ContextAssertionPublicKey>> {
    let mut decoded = BTreeMap::new();
    for (kid, raw_key) in entries {
        let kid = kid.trim();
        if kid.is_empty() {
            return None;
        }
        decoded.insert(
            kid.to_string(),
            ContextAssertionPublicKey::legacy(decode_context_public_key(&raw_key)?),
        );
    }
    Some(decoded)
}

#[cfg(test)]
fn parse_context_public_keyring_json_entries(
    entries: BTreeMap<String, serde_json::Value>,
) -> Option<BTreeMap<String, ContextAssertionPublicKey>> {
    let mut decoded = BTreeMap::new();
    for (kid, value) in entries {
        let kid = kid.trim();
        if kid.is_empty() {
            return None;
        }
        let key = match value {
            serde_json::Value::String(raw_key) => {
                ContextAssertionPublicKey::legacy(decode_context_public_key(&raw_key)?)
            }
            serde_json::Value::Object(mut object) => {
                let public_key = object
                    .remove("public_key")
                    .or_else(|| object.remove("publicKey"))
                    .and_then(|value| value.as_str().map(ToString::to_string))
                    .and_then(|raw_key| decode_context_public_key(&raw_key))?;
                let purpose = optional_string_field(&mut object, "purpose")
                    .map(|purpose| SigningKeyPurpose::parse(&purpose))
                    .transpose()
                    .ok()?;
                ContextAssertionPublicKey {
                    public_key,
                    purpose,
                    organization_id: optional_string_field(&mut object, "organization_id")
                        .or_else(|| optional_string_field(&mut object, "organizationId"))
                        .or_else(|| optional_string_field(&mut object, "org_id"))
                        .or_else(|| optional_string_field(&mut object, "orgId")),
                    deployment_id: optional_string_field(&mut object, "deployment_id")
                        .or_else(|| optional_string_field(&mut object, "deploymentId")),
                    allowed_audiences: string_vec_field(&mut object, "allowed_audiences")
                        .or_else(|| string_vec_field(&mut object, "allowedAudiences"))
                        .unwrap_or_default(),
                    allowed_resource_scope_prefixes: string_vec_field(
                        &mut object,
                        "allowed_resource_scope_prefixes",
                    )
                    .or_else(|| string_vec_field(&mut object, "allowedResourceScopePrefixes"))
                    .unwrap_or_default(),
                    not_before_ms: optional_u64_field(&mut object, "not_before_ms")
                        .or_else(|| optional_u64_field(&mut object, "notBeforeMs")),
                    not_after_ms: optional_u64_field(&mut object, "not_after_ms")
                        .or_else(|| optional_u64_field(&mut object, "notAfterMs")),
                    status: optional_string_field(&mut object, "status"),
                }
            }
            _ => return None,
        };
        decoded.insert(kid.to_string(), key);
    }
    Some(decoded)
}

#[cfg(test)]
fn optional_string_field(
    object: &mut serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<String> {
    object.remove(field).and_then(|value| match value {
        serde_json::Value::String(value) => {
            let value = value.trim().to_string();
            if value.is_empty() {
                None
            } else {
                Some(value)
            }
        }
        _ => None,
    })
}

#[cfg(test)]
fn optional_u64_field(
    object: &mut serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<u64> {
    object.remove(field).and_then(|value| value.as_u64())
}

#[cfg(test)]
fn string_vec_field(
    object: &mut serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<Vec<String>> {
    let value = object.remove(field)?;
    match value {
        serde_json::Value::Array(values) => Some(
            values
                .into_iter()
                .filter_map(|value| {
                    value
                        .as_str()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToString::to_string)
                })
                .collect(),
        ),
        serde_json::Value::String(value) => Some(
            value
                .split([',', ';', '\n'])
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect(),
        ),
        _ => None,
    }
}

#[cfg(test)]
fn validate_context_assertion_header(
    header: &TenantContextAssertionHeader,
) -> Result<(), TenantContextIngressError> {
    if header.alg != "EdDSA" || header.typ != "tandem-tenant-context+jws" || header.kid.is_empty() {
        return Err(TenantContextIngressError::ContextAssertionMalformed);
    }
    Ok(())
}

#[cfg(test)]
fn validate_context_assertion_key_metadata(
    key: &ContextAssertionPublicKey,
    claims: &TenantContextAssertionClaims,
    now_ms: u64,
) -> Result<(), TenantContextIngressError> {
    if let Some(status) = key.status.as_deref() {
        if !status.eq_ignore_ascii_case("active") {
            return Err(TenantContextIngressError::ContextAssertionUntrusted);
        }
    }
    if let Some(purpose) = key.purpose {
        if purpose != SigningKeyPurpose::ContextAssertion {
            return Err(TenantContextIngressError::ContextAssertionUntrusted);
        }
    }
    if key
        .not_before_ms
        .map(|not_before_ms| now_ms < not_before_ms)
        .unwrap_or(false)
        || key
            .not_after_ms
            .map(|not_after_ms| now_ms >= not_after_ms)
            .unwrap_or(false)
    {
        return Err(TenantContextIngressError::ContextAssertionExpired);
    }
    if !key.allowed_audiences.is_empty()
        && !key
            .allowed_audiences
            .iter()
            .any(|audience| audience == &claims.audience)
    {
        return Err(TenantContextIngressError::ContextAssertionUntrusted);
    }
    if key
        .organization_id
        .as_deref()
        .map(|organization_id| organization_id != claims.tenant_context.org_id)
        .unwrap_or(false)
    {
        return Err(TenantContextIngressError::ContextAssertionUntrusted);
    }
    if key
        .deployment_id
        .as_deref()
        .map(|deployment_id| claims.tenant_context.deployment_id.as_deref() != Some(deployment_id))
        .unwrap_or(false)
    {
        return Err(TenantContextIngressError::ContextAssertionUntrusted);
    }
    if !key.allowed_resource_scope_prefixes.is_empty()
        && !context_assertion_scope_allowed(
            &key.allowed_resource_scope_prefixes,
            &context_assertion_scope_prefixes(claims),
        )
    {
        return Err(TenantContextIngressError::ContextAssertionUntrusted);
    }

    Ok(())
}

#[cfg(test)]
fn context_assertion_scope_allowed(
    allowed_prefixes: &[String],
    actual_prefixes: &[String],
) -> bool {
    actual_prefixes.iter().any(|actual| {
        allowed_prefixes.iter().any(|allowed| {
            let allowed = allowed.trim().trim_matches('/');
            !allowed.is_empty()
                && (actual == allowed
                    || actual
                        .strip_prefix(allowed)
                        .map(|suffix| suffix.starts_with('/'))
                        .unwrap_or(false))
        })
    })
}

#[cfg(test)]
fn context_assertion_scope_prefixes(claims: &TenantContextAssertionClaims) -> Vec<String> {
    let mut prefixes = vec![
        format!("org/{}", claims.tenant_context.org_id),
        format!(
            "org/{}/workspace/{}",
            claims.tenant_context.org_id, claims.tenant_context.workspace_id
        ),
    ];
    if let Some(resource_scope) = claims.resource_scope.as_ref() {
        push_resource_ref_prefixes(&mut prefixes, &resource_scope.root);
        for resource in &resource_scope.allowed_resources {
            push_resource_ref_prefixes(&mut prefixes, resource);
        }
        for resource in &resource_scope.denied_resources {
            push_resource_ref_prefixes(&mut prefixes, resource);
        }
    }
    for grant in &claims.grants {
        push_resource_ref_prefixes(&mut prefixes, &grant.resource);
    }
    prefixes.sort();
    prefixes.dedup();
    prefixes
}

#[cfg(test)]
fn push_resource_ref_prefixes(prefixes: &mut Vec<String>, resource: &ResourceRef) {
    prefixes.push(format!("org/{}", resource.organization_id));
    prefixes.push(format!(
        "org/{}/workspace/{}",
        resource.organization_id, resource.workspace_id
    ));
    let project_id = resource.project_id.as_deref().or_else(|| {
        (resource.resource_kind == ResourceKind::Project).then_some(resource.resource_id.as_str())
    });
    if let Some(project_id) = project_id {
        prefixes.push(format!(
            "org/{}/workspace/{}/project/{}",
            resource.organization_id, resource.workspace_id, project_id
        ));
    }
    let mut resource_prefix = format!(
        "org/{}/workspace/{}/resource/{}/{}",
        resource.organization_id,
        resource.workspace_id,
        resource_kind_scope_label(resource.resource_kind),
        resource.resource_id
    );
    if let Some(project_id) = project_id {
        resource_prefix = format!(
            "org/{}/workspace/{}/project/{}/resource/{}/{}",
            resource.organization_id,
            resource.workspace_id,
            project_id,
            resource_kind_scope_label(resource.resource_kind),
            resource.resource_id
        );
    }
    prefixes.push(resource_prefix.clone());
    if let Some(branch_id) = resource.branch_id.as_deref() {
        prefixes.push(format!("{resource_prefix}/branch/{branch_id}"));
    }
    if let Some(path_prefix) = resource.path_prefix.as_deref() {
        let path_prefix = path_prefix.trim_matches('/');
        if !path_prefix.is_empty() {
            prefixes.push(format!("{resource_prefix}/path/{path_prefix}"));
        }
    }
}

#[cfg(test)]
fn resource_kind_scope_label(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::Organization => "organization",
        ResourceKind::Workspace => "workspace",
        ResourceKind::OrganizationUnit => "organization_unit",
        ResourceKind::Department => "department",
        ResourceKind::Group => "group",
        ResourceKind::Project => "project",
        ResourceKind::DataRoom => "data_room",
        ResourceKind::SharedDrive => "shared_drive",
        ResourceKind::DocumentCollection => "document_collection",
        ResourceKind::DataStore => "data_store",
        ResourceKind::Dataset => "dataset",
        ResourceKind::Document => "document",
        ResourceKind::Repository => "repository",
        ResourceKind::Directory => "directory",
        ResourceKind::File => "file",
        ResourceKind::Artifact => "artifact",
        ResourceKind::MemorySpace => "memory_space",
        ResourceKind::KnowledgeSpace => "knowledge_space",
        ResourceKind::SecretProviderCredential => "secret_provider_credential",
        ResourceKind::Automation => "automation",
        ResourceKind::Orchestration => "orchestration",
        ResourceKind::Run => "run",
        ResourceKind::Approval => "approval",
        ResourceKind::AuditExport => "audit_export",
        ResourceKind::McpServer => "mcp_server",
        ResourceKind::McpTool => "mcp_tool",
        ResourceKind::ConnectorInstance => "connector_instance",
        ResourceKind::SourceBinding => "source_binding",
        ResourceKind::SourceObject => "source_object",
        ResourceKind::IngestionJob => "ingestion_job",
        ResourceKind::ExternalIntegrationAccount => "external_integration_account",
        ResourceKind::HostedDeployment => "hosted_deployment",
    }
}

#[cfg(test)]
fn decode_context_public_key(raw: &str) -> Option<[u8; 32]> {
    decode_base64url(raw.trim())
        .or_else(|| {
            base64::engine::general_purpose::STANDARD
                .decode(raw.trim())
                .ok()
        })
        .and_then(|bytes| bytes.try_into().ok())
}
