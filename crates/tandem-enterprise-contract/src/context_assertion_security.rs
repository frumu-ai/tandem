//! Shared, fail-closed verification and replay protection for hosted context assertions.
//!
//! Runtime and ACA deliberately use this module so their trust decisions cannot drift.

use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::{
    KeyUsageContext, KeyringDenial, ResourceKind, ResourceRef, SigningKeyPurpose,
    TenantContextAssertionClaims, TenantContextAssertionHeader, TenantSource, VerifierKeyEntry,
    VerifierKeyring,
};

pub const DEFAULT_CONTEXT_ASSERTION_ISSUER: &str = "tandem-web";
pub const DEFAULT_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS: u64 = 10_000;
pub const DEFAULT_CONTEXT_ASSERTION_MAX_LIFETIME_MS: u64 = 15 * 60 * 1_000;
pub const HARD_CONTEXT_ASSERTION_MAX_LIFETIME_MS: u64 = 60 * 60 * 1_000;
pub const DEFAULT_CONTEXT_ASSERTION_REPLAY_GRACE_MS: u64 = 60_000;
pub const DEFAULT_CONTEXT_ASSERTION_REPLAY_MAX_ENTRIES: usize = 100_000;
pub const DEFAULT_CONTEXT_ASSERTION_REPLAY_MAX_NAMESPACE_ENTRIES: usize = 10_000;
#[cfg(test)]
const REPLAY_DATABASE_INITIALIZATION_WAIT: Duration = Duration::from_millis(500);
#[cfg(not(test))]
const REPLAY_DATABASE_INITIALIZATION_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextAssertionError {
    MalformedAssertion,
    MalformedHeader,
    BadSignature,
    UnsupportedVersion,
    BadIssuer,
    BadAudience,
    InvalidIdentity,
    Expired,
    NotYetValid,
    LifetimeExceeded,
    TimeOverflow,
    KeyNotConfigured,
    UnknownKey,
    KeyringDenied(KeyringDenial),
    Replayed,
    ReplayBackendUnavailable,
    ReplayCapacityExceeded,
    InvalidPolicy,
}

impl ContextAssertionError {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MalformedAssertion => "context_assertion_malformed",
            Self::MalformedHeader => "context_assertion_malformed_header",
            Self::BadSignature => "context_assertion_bad_signature",
            Self::UnsupportedVersion => "context_assertion_unsupported_version",
            Self::BadIssuer => "context_assertion_bad_issuer",
            Self::BadAudience => "context_assertion_bad_audience",
            Self::InvalidIdentity => "context_assertion_invalid_identity",
            Self::Expired => "context_assertion_expired",
            Self::NotYetValid => "context_assertion_not_yet_valid",
            Self::LifetimeExceeded => "context_assertion_lifetime_exceeded",
            Self::TimeOverflow => "context_assertion_time_overflow",
            Self::KeyNotConfigured => "context_assertion_key_not_configured",
            Self::UnknownKey => "context_assertion_unknown_key",
            Self::KeyringDenied(_) => "context_assertion_keyring_denied",
            Self::Replayed => "context_assertion_replayed",
            Self::ReplayBackendUnavailable => "context_assertion_replay_backend_unavailable",
            Self::ReplayCapacityExceeded => "context_assertion_replay_capacity_exceeded",
            Self::InvalidPolicy => "context_assertion_invalid_policy",
        }
    }
}

impl core::fmt::Display for ContextAssertionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::error::Error for ContextAssertionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextAssertionPolicy {
    pub expected_issuer: String,
    pub expected_audience: String,
    pub max_future_skew_ms: u64,
    pub max_lifetime_ms: u64,
}

impl ContextAssertionPolicy {
    pub fn new(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        max_future_skew_ms: u64,
        max_lifetime_ms: u64,
    ) -> Result<Self, ContextAssertionError> {
        let policy = Self {
            expected_issuer: issuer.into().trim().to_string(),
            expected_audience: audience.into().trim().to_string(),
            max_future_skew_ms,
            max_lifetime_ms,
        };
        if policy.expected_issuer.is_empty()
            || policy.expected_audience.is_empty()
            || policy.max_future_skew_ms == 0
            || policy.max_lifetime_ms == 0
            || policy.max_lifetime_ms > HARD_CONTEXT_ASSERTION_MAX_LIFETIME_MS
        {
            return Err(ContextAssertionError::InvalidPolicy);
        }
        Ok(policy)
    }
}

pub fn parse_context_assertion_metadata_keyring(
    raw: &str,
) -> Result<VerifierKeyring, ContextAssertionError> {
    let entries = serde_json::from_str::<BTreeMap<String, serde_json::Value>>(raw.trim())
        .map_err(|_| ContextAssertionError::InvalidPolicy)?;
    if entries.is_empty() {
        return Err(ContextAssertionError::KeyNotConfigured);
    }
    let mut keyring = VerifierKeyring::new();
    for (kid, value) in entries {
        let kid = kid.trim().to_string();
        if kid.is_empty() {
            return Err(ContextAssertionError::InvalidPolicy);
        }
        let serde_json::Value::Object(mut object) = value else {
            return Err(ContextAssertionError::InvalidPolicy);
        };
        normalize_keyring_alias(&mut object, "publicKey", "public_key");
        normalize_keyring_alias(&mut object, "organizationId", "organization_id");
        normalize_keyring_alias(&mut object, "orgId", "organization_id");
        normalize_keyring_alias(&mut object, "deploymentId", "deployment_id");
        normalize_keyring_alias(&mut object, "allowedAudiences", "allowed_audiences");
        normalize_keyring_alias(
            &mut object,
            "allowedResourceScopePrefixes",
            "allowed_resource_scope_prefixes",
        );
        normalize_keyring_alias(&mut object, "notBeforeMs", "not_before_ms");
        normalize_keyring_alias(&mut object, "notAfterMs", "not_after_ms");
        if !object.contains_key("public_key")
            || !object.contains_key("purpose")
            || !object.contains_key("status")
        {
            return Err(ContextAssertionError::InvalidPolicy);
        }
        let mut entry: VerifierKeyEntry = serde_json::from_value(serde_json::Value::Object(object))
            .map_err(|_| ContextAssertionError::InvalidPolicy)?;
        entry.kid = kid;
        if entry.purpose != SigningKeyPurpose::ContextAssertion {
            return Err(ContextAssertionError::InvalidPolicy);
        }
        entry
            .verifying_key()
            .map_err(ContextAssertionError::KeyringDenied)?;
        keyring.insert(entry);
    }
    Ok(keyring)
}

fn normalize_keyring_alias(
    object: &mut serde_json::Map<String, serde_json::Value>,
    alias: &str,
    canonical: &str,
) {
    if !object.contains_key(canonical) {
        if let Some(value) = object.remove(alias) {
            object.insert(canonical.to_string(), value);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedContextAssertion {
    pub claims: TenantContextAssertionClaims,
    pub key_id: String,
    pub fingerprint: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct ContextAssertionVerifier {
    keyring: VerifierKeyring,
    policy: ContextAssertionPolicy,
}

impl ContextAssertionVerifier {
    pub fn new(
        keyring: VerifierKeyring,
        policy: ContextAssertionPolicy,
    ) -> Result<Self, ContextAssertionError> {
        if keyring.is_empty() {
            return Err(ContextAssertionError::KeyNotConfigured);
        }
        Ok(Self { keyring, policy })
    }

    pub(crate) fn new_allow_empty(
        keyring: VerifierKeyring,
        policy: ContextAssertionPolicy,
    ) -> Self {
        Self { keyring, policy }
    }

    pub fn policy(&self) -> &ContextAssertionPolicy {
        &self.policy
    }

    pub fn keyring(&self) -> &VerifierKeyring {
        &self.keyring
    }

    pub fn verify(
        &self,
        assertion: &str,
    ) -> Result<VerifiedContextAssertion, ContextAssertionError> {
        self.verify_at(assertion, current_unix_ms())
    }

    pub fn verify_at(
        &self,
        assertion: &str,
        now_ms: u64,
    ) -> Result<VerifiedContextAssertion, ContextAssertionError> {
        if self.keyring.is_empty() {
            return Err(ContextAssertionError::KeyNotConfigured);
        }
        let assertion = assertion.trim();
        let mut parts = assertion.split('.');
        let encoded_header = required_part(parts.next())?;
        let encoded_claims = required_part(parts.next())?;
        let encoded_signature = required_part(parts.next())?;
        if parts.next().is_some() {
            return Err(ContextAssertionError::MalformedAssertion);
        }

        let header_bytes = decode_base64url(encoded_header)?;
        let claims_bytes = decode_base64url(encoded_claims)?;
        let signature_bytes: [u8; 64] = decode_base64url(encoded_signature)?
            .try_into()
            .map_err(|_| ContextAssertionError::MalformedAssertion)?;
        let header: TenantContextAssertionHeader = serde_json::from_slice(&header_bytes)
            .map_err(|_| ContextAssertionError::MalformedHeader)?;
        if header.alg != "EdDSA"
            || header.typ != "tandem-tenant-context+jws"
            || header.kid.trim().is_empty()
        {
            return Err(ContextAssertionError::MalformedHeader);
        }
        let key = self
            .keyring
            .get(&header.kid)
            .ok_or(ContextAssertionError::UnknownKey)?;
        let verifying_key = key
            .verifying_key()
            .map_err(ContextAssertionError::KeyringDenied)?;

        let signing_input = format!("{encoded_header}.{encoded_claims}");
        verifying_key
            .verify(
                signing_input.as_bytes(),
                &Signature::from_bytes(&signature_bytes),
            )
            .map_err(|_| ContextAssertionError::BadSignature)?;
        let claims: TenantContextAssertionClaims = serde_json::from_slice(&claims_bytes)
            .map_err(|_| ContextAssertionError::MalformedAssertion)?;
        self.validate_claims(&claims, now_ms)?;
        let usage = key_usage_for_claims(&claims);
        key.authorize(SigningKeyPurpose::ContextAssertion, &usage, now_ms)
            .map_err(ContextAssertionError::KeyringDenied)?;
        validate_all_key_resource_scopes(&self.keyring, &header.kid, &claims)?;

        Ok(VerifiedContextAssertion {
            claims,
            key_id: header.kid,
            fingerprint: Sha256::digest(assertion.as_bytes()).into(),
        })
    }

    fn validate_claims(
        &self,
        claims: &TenantContextAssertionClaims,
        now_ms: u64,
    ) -> Result<(), ContextAssertionError> {
        if claims.version != "v1" {
            return Err(ContextAssertionError::UnsupportedVersion);
        }
        if claims.issuer != self.policy.expected_issuer {
            return Err(ContextAssertionError::BadIssuer);
        }
        if claims.audience != self.policy.expected_audience {
            return Err(ContextAssertionError::BadAudience);
        }
        validate_claim_identity(claims)?;
        if claims.expires_at_ms <= claims.issued_at_ms {
            return Err(ContextAssertionError::Expired);
        }
        let lifetime = claims
            .expires_at_ms
            .checked_sub(claims.issued_at_ms)
            .ok_or(ContextAssertionError::TimeOverflow)?;
        if lifetime > self.policy.max_lifetime_ms {
            return Err(ContextAssertionError::LifetimeExceeded);
        }
        let latest_issued_at = now_ms
            .checked_add(self.policy.max_future_skew_ms)
            .ok_or(ContextAssertionError::TimeOverflow)?;
        if claims.issued_at_ms > latest_issued_at {
            return Err(ContextAssertionError::NotYetValid);
        }
        if claims.is_expired_at(now_ms) {
            return Err(ContextAssertionError::Expired);
        }
        Ok(())
    }
}

fn validate_claim_identity(
    claims: &TenantContextAssertionClaims,
) -> Result<(), ContextAssertionError> {
    let tenant = &claims.tenant_context;
    let deployment_id = tenant
        .deployment_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if claims.assertion_id.trim().is_empty()
        || claims.human_actor.actor_id.trim().is_empty()
        || tenant.org_id.trim().is_empty()
        || tenant.workspace_id.trim().is_empty()
        || tenant.source != TenantSource::Explicit
        || deployment_id.is_none()
        || tenant.actor_id.as_deref() != Some(claims.human_actor.actor_id.as_str())
        || claims.authority_chain.initiated_by.actor_id.as_deref()
            != Some(claims.human_actor.actor_id.as_str())
    {
        return Err(ContextAssertionError::InvalidIdentity);
    }
    if let Some(principal) = claims.principal.as_ref() {
        if principal.id.trim().is_empty()
            || principal
                .tenant_actor_id
                .as_deref()
                .is_some_and(|actor| actor != claims.human_actor.actor_id)
        {
            return Err(ContextAssertionError::InvalidIdentity);
        }
    }
    for resource in assertion_resources(claims) {
        if resource.organization_id != tenant.org_id || resource.workspace_id != tenant.workspace_id
        {
            return Err(ContextAssertionError::InvalidIdentity);
        }
    }
    Ok(())
}

fn key_usage_for_claims(claims: &TenantContextAssertionClaims) -> KeyUsageContext {
    let mut usage = KeyUsageContext::new()
        .with_audience(claims.audience.clone())
        .with_organization_id(claims.tenant_context.org_id.clone())
        .with_deployment_id(
            claims
                .tenant_context
                .deployment_id
                .clone()
                .unwrap_or_default(),
        );
    usage.resource_scope = assertion_resources(claims)
        .first()
        .map(|resource| resource_scope_path(resource))
        .or_else(|| {
            Some(format!(
                "org/{}/workspace/{}",
                claims.tenant_context.org_id, claims.tenant_context.workspace_id
            ))
        });
    usage
}

fn validate_all_key_resource_scopes(
    keyring: &VerifierKeyring,
    kid: &str,
    claims: &TenantContextAssertionClaims,
) -> Result<(), ContextAssertionError> {
    let Some(entry) = keyring.get(kid) else {
        return Err(ContextAssertionError::UnknownKey);
    };
    if entry.allowed_resource_scope_prefixes.is_empty() {
        return Ok(());
    }
    let mut actual = assertion_resources(claims)
        .into_iter()
        .map(resource_scope_path)
        .collect::<Vec<_>>();
    if actual.is_empty() {
        actual.push(format!(
            "org/{}/workspace/{}",
            claims.tenant_context.org_id, claims.tenant_context.workspace_id
        ));
    }
    let all_allowed = actual.iter().all(|scope| {
        entry.allowed_resource_scope_prefixes.iter().any(|prefix| {
            let prefix = prefix.trim().trim_matches('/');
            !prefix.is_empty()
                && (scope == prefix
                    || scope
                        .strip_prefix(prefix)
                        .is_some_and(|suffix| suffix.starts_with('/')))
        })
    });
    if all_allowed {
        Ok(())
    } else {
        Err(ContextAssertionError::KeyringDenied(
            KeyringDenial::ResourceScopeNotAllowed,
        ))
    }
}

fn assertion_resources(claims: &TenantContextAssertionClaims) -> Vec<&ResourceRef> {
    let mut resources = Vec::new();
    if let Some(scope) = claims.resource_scope.as_ref() {
        resources.push(&scope.root);
        resources.extend(scope.allowed_resources.iter());
        resources.extend(scope.denied_resources.iter());
    }
    resources.extend(claims.grants.iter().map(|grant| &grant.resource));
    resources
}

fn resource_scope_path(resource: &ResourceRef) -> String {
    let project_id = resource.project_id.as_deref().or_else(|| {
        (resource.resource_kind == ResourceKind::Project).then_some(resource.resource_id.as_str())
    });
    if let Some(project_id) = project_id {
        format!(
            "org/{}/workspace/{}/project/{}",
            resource.organization_id, resource.workspace_id, project_id
        )
    } else {
        format!(
            "org/{}/workspace/{}",
            resource.organization_id, resource.workspace_id
        )
    }
}

fn required_part(part: Option<&str>) -> Result<&str, ContextAssertionError> {
    part.filter(|value| !value.is_empty())
        .ok_or(ContextAssertionError::MalformedAssertion)
}

fn decode_base64url(raw: &str) -> Result<Vec<u8>, ContextAssertionError> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(raw))
        .map_err(|_| ContextAssertionError::MalformedAssertion)
}

fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextAssertionReplayMode {
    Bound,
    OneShot,
    Off,
}

impl ContextAssertionReplayMode {
    pub fn parse(value: &str) -> Result<Self, ContextAssertionError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "bound" | "" => Ok(Self::Bound),
            "one_shot" | "one-shot" | "oneshot" => Ok(Self::OneShot),
            "off" => Ok(Self::Off),
            _ => Err(ContextAssertionError::InvalidPolicy),
        }
    }
}

#[derive(Debug, Clone)]
struct ReplayEntry {
    namespace_hash: String,
    fingerprint_hex: String,
    expires_at_ms: u64,
}

#[derive(Debug, Default)]
struct ReplayState {
    entries: BTreeMap<String, ReplayEntry>,
}

fn replay_state_version() -> i64 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReplayFileIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

#[derive(Debug)]
enum ReplayBackend {
    Memory(Mutex<ReplayState>),
    Sqlite {
        path: PathBuf,
        identity: ReplayFileIdentity,
    },
}

#[derive(Debug, Clone)]
pub struct ContextAssertionReplayStore {
    backend: Arc<ReplayBackend>,
    max_entries: usize,
    max_namespace_entries: usize,
    retention_grace_ms: u64,
}

impl ContextAssertionReplayStore {
    pub fn in_memory() -> Self {
        Self {
            backend: Arc::new(ReplayBackend::Memory(Mutex::new(ReplayState::default()))),
            max_entries: DEFAULT_CONTEXT_ASSERTION_REPLAY_MAX_ENTRIES,
            max_namespace_entries: DEFAULT_CONTEXT_ASSERTION_REPLAY_MAX_NAMESPACE_ENTRIES,
            retention_grace_ms: DEFAULT_CONTEXT_ASSERTION_REPLAY_GRACE_MS,
        }
    }

    pub fn persistent(path: impl Into<PathBuf>) -> Result<Self, ContextAssertionError> {
        let path = path.into();
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .ok_or(ContextAssertionError::ReplayBackendUnavailable)?;
        if !parent.is_dir() {
            return Err(ContextAssertionError::ReplayBackendUnavailable);
        }
        let (created, identity) = prepare_replay_database_file(&path)?;
        let store = Self {
            backend: Arc::new(ReplayBackend::Sqlite { path, identity }),
            max_entries: DEFAULT_CONTEXT_ASSERTION_REPLAY_MAX_ENTRIES,
            max_namespace_entries: DEFAULT_CONTEXT_ASSERTION_REPLAY_MAX_NAMESPACE_ENTRIES,
            retention_grace_ms: DEFAULT_CONTEXT_ASSERTION_REPLAY_GRACE_MS,
        };
        if created {
            store.initialize_database()?;
            store.readiness_check()?;
        } else {
            store.wait_for_database_readiness()?;
        }
        Ok(store)
    }

    fn wait_for_database_readiness(&self) -> Result<(), ContextAssertionError> {
        let deadline = Instant::now() + REPLAY_DATABASE_INITIALIZATION_WAIT;
        loop {
            match self.readiness_check() {
                Ok(()) => return Ok(()),
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub fn with_limits(mut self, max_entries: usize, max_namespace_entries: usize) -> Self {
        self.max_entries = max_entries.max(1);
        self.max_namespace_entries = max_namespace_entries.max(1);
        self
    }

    pub fn readiness_check(&self) -> Result<(), ContextAssertionError> {
        match self.backend.as_ref() {
            ReplayBackend::Memory(state) => {
                let _state = state
                    .lock()
                    .map_err(|_| ContextAssertionError::ReplayBackendUnavailable)?;
                Ok(())
            }
            ReplayBackend::Sqlite { path, identity } => {
                let connection = open_replay_database(path, *identity)?;
                let quick_check: String = connection
                    .query_row("PRAGMA quick_check", [], |row| row.get(0))
                    .map_err(replay_database_error)?;
                if quick_check != "ok" {
                    return Err(ContextAssertionError::ReplayBackendUnavailable);
                }
                validate_replay_database(&connection, self.max_entries, self.max_namespace_entries)
            }
        }
    }

    pub fn check_and_record(
        &self,
        mode: ContextAssertionReplayMode,
        verified: &VerifiedContextAssertion,
        now_ms: u64,
    ) -> Result<(), ContextAssertionError> {
        if mode == ContextAssertionReplayMode::Off {
            return Ok(());
        }
        let claims = &verified.claims;
        let replay_key = replay_key(&claims.issuer, &claims.audience, &claims.assertion_id);
        let namespace_hash = replay_namespace_key(&claims.issuer, &claims.audience);
        let fingerprint_hex = hex(&verified.fingerprint);
        match self.backend.as_ref() {
            ReplayBackend::Memory(state) => {
                let mut state = state
                    .lock()
                    .map_err(|_| ContextAssertionError::ReplayBackendUnavailable)?;
                check_memory_replay(
                    &mut state,
                    mode,
                    replay_key,
                    namespace_hash,
                    fingerprint_hex,
                    claims.expires_at_ms,
                    now_ms,
                    self.retention_grace_ms,
                    self.max_entries,
                    self.max_namespace_entries,
                )
            }
            ReplayBackend::Sqlite { path, identity } => self.check_sqlite_replay(
                path,
                *identity,
                mode,
                &replay_key,
                &namespace_hash,
                &fingerprint_hex,
                claims.expires_at_ms,
                now_ms,
            ),
        }
    }

    fn initialize_database(&self) -> Result<(), ContextAssertionError> {
        let ReplayBackend::Sqlite { path, identity } = self.backend.as_ref() else {
            return Ok(());
        };
        let mut connection = open_replay_database(path, *identity)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(replay_database_error)?;
        transaction
            .execute_batch(
                "CREATE TABLE replay_metadata (\n                    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),\n                    version INTEGER NOT NULL\n                 );\n                 CREATE TABLE replay_entries (\n                    replay_key TEXT PRIMARY KEY CHECK (length(replay_key) = 64),\n                    namespace_hash TEXT NOT NULL CHECK (length(namespace_hash) = 64),\n                    fingerprint_hex TEXT NOT NULL CHECK (length(fingerprint_hex) = 64),\n                    expires_at_ms INTEGER NOT NULL CHECK (expires_at_ms >= 0)\n                 );\n                 CREATE INDEX replay_entries_namespace_idx\n                    ON replay_entries(namespace_hash);",
            )
            .map_err(replay_database_error)?;
        transaction
            .execute(
                "INSERT INTO replay_metadata(singleton, version) VALUES (1, ?1)",
                [replay_state_version()],
            )
            .map_err(replay_database_error)?;
        transaction.commit().map_err(replay_database_error)
    }

    #[allow(clippy::too_many_arguments)]
    fn check_sqlite_replay(
        &self,
        path: &Path,
        identity: ReplayFileIdentity,
        mode: ContextAssertionReplayMode,
        replay_key: &str,
        namespace_hash: &str,
        fingerprint_hex: &str,
        expires_at_ms: u64,
        now_ms: u64,
    ) -> Result<(), ContextAssertionError> {
        let expires_at_ms = i64::try_from(expires_at_ms)
            .map_err(|_| ContextAssertionError::ReplayBackendUnavailable)?;
        let cleanup_cutoff =
            i64::try_from(now_ms.saturating_sub(self.retention_grace_ms)).unwrap_or(i64::MAX);
        let mut connection = open_replay_database(path, identity)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(replay_database_error)?;
        validate_replay_database(&transaction, self.max_entries, self.max_namespace_entries)?;
        transaction
            .execute(
                "DELETE FROM replay_entries WHERE expires_at_ms <= ?1",
                [cleanup_cutoff],
            )
            .map_err(replay_database_error)?;
        let existing = transaction
            .query_row(
                "SELECT fingerprint_hex FROM replay_entries WHERE replay_key = ?1",
                [replay_key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(replay_database_error)?;
        if let Some(existing_fingerprint) = existing {
            return match mode {
                ContextAssertionReplayMode::Bound if existing_fingerprint == fingerprint_hex => {
                    transaction.commit().map_err(replay_database_error)
                }
                ContextAssertionReplayMode::Bound | ContextAssertionReplayMode::OneShot => {
                    Err(ContextAssertionError::Replayed)
                }
                ContextAssertionReplayMode::Off => {
                    transaction.commit().map_err(replay_database_error)
                }
            };
        }
        let total: i64 = transaction
            .query_row("SELECT COUNT(*) FROM replay_entries", [], |row| row.get(0))
            .map_err(replay_database_error)?;
        let namespace_total: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM replay_entries WHERE namespace_hash = ?1",
                [namespace_hash],
                |row| row.get(0),
            )
            .map_err(replay_database_error)?;
        if total >= usize_to_i64(self.max_entries)
            || namespace_total >= usize_to_i64(self.max_namespace_entries)
        {
            return Err(ContextAssertionError::ReplayCapacityExceeded);
        }
        transaction
            .execute(
                "INSERT INTO replay_entries(\n                    replay_key, namespace_hash, fingerprint_hex, expires_at_ms\n                 ) VALUES (?1, ?2, ?3, ?4)",
                params![replay_key, namespace_hash, fingerprint_hex, expires_at_ms],
            )
            .map_err(replay_database_error)?;
        transaction.commit().map_err(replay_database_error)
    }
}

#[allow(clippy::too_many_arguments)]
fn check_memory_replay(
    state: &mut ReplayState,
    mode: ContextAssertionReplayMode,
    replay_key: String,
    namespace_hash: String,
    fingerprint_hex: String,
    expires_at_ms: u64,
    now_ms: u64,
    retention_grace_ms: u64,
    max_entries: usize,
    max_namespace_entries: usize,
) -> Result<(), ContextAssertionError> {
    state
        .entries
        .retain(|_, entry| entry.expires_at_ms.saturating_add(retention_grace_ms) > now_ms);
    if let Some(entry) = state.entries.get(&replay_key) {
        return match mode {
            ContextAssertionReplayMode::Bound if entry.fingerprint_hex == fingerprint_hex => Ok(()),
            ContextAssertionReplayMode::Bound | ContextAssertionReplayMode::OneShot => {
                Err(ContextAssertionError::Replayed)
            }
            ContextAssertionReplayMode::Off => Ok(()),
        };
    }
    let namespace_count = state
        .entries
        .values()
        .filter(|entry| entry.namespace_hash == namespace_hash)
        .count();
    if state.entries.len() >= max_entries || namespace_count >= max_namespace_entries {
        return Err(ContextAssertionError::ReplayCapacityExceeded);
    }
    state.entries.insert(
        replay_key,
        ReplayEntry {
            namespace_hash,
            fingerprint_hex,
            expires_at_ms,
        },
    );
    Ok(())
}

fn prepare_replay_database_file(
    path: &Path,
) -> Result<(bool, ReplayFileIdentity), ContextAssertionError> {
    match secure_create_new_file(path) {
        Ok(file) => {
            drop(file);
            Ok((true, replay_file_identity(path)?))
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            wait_for_initialized_replay_file(path)?;
            Ok((false, replay_file_identity(path)?))
        }
        Err(_) => Err(ContextAssertionError::ReplayBackendUnavailable),
    }
}

fn secure_create_new_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    options.open(path)
}

fn wait_for_initialized_replay_file(path: &Path) -> Result<(), ContextAssertionError> {
    let deadline = Instant::now() + REPLAY_DATABASE_INITIALIZATION_WAIT;
    loop {
        let file = secure_open_existing_file(path)?;
        if file
            .metadata()
            .map_err(|_| ContextAssertionError::ReplayBackendUnavailable)?
            .len()
            > 0
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(ContextAssertionError::ReplayBackendUnavailable);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn secure_open_existing_file(path: &Path) -> Result<File, ContextAssertionError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    let file = options
        .open(path)
        .map_err(|_| ContextAssertionError::ReplayBackendUnavailable)?;
    validate_secure_file(&file)?;
    Ok(file)
}

pub(crate) fn read_owner_only_regular_text_file(
    path: &Path,
) -> Result<String, ContextAssertionError> {
    let mut file = secure_open_existing_file(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|_| ContextAssertionError::ReplayBackendUnavailable)?;
    let contents = contents.trim().to_string();
    if contents.is_empty() {
        return Err(ContextAssertionError::ReplayBackendUnavailable);
    }
    Ok(contents)
}

fn validate_secure_file(file: &File) -> Result<(), ContextAssertionError> {
    let metadata = file
        .metadata()
        .map_err(|_| ContextAssertionError::ReplayBackendUnavailable)?;
    if !metadata.file_type().is_file() {
        return Err(ContextAssertionError::ReplayBackendUnavailable);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let mode = metadata.permissions().mode() & 0o777;
        if metadata.uid() != rustix::process::geteuid().as_raw() || mode & 0o077 != 0 {
            return Err(ContextAssertionError::ReplayBackendUnavailable);
        }
    }
    Ok(())
}

fn replay_file_identity(path: &Path) -> Result<ReplayFileIdentity, ContextAssertionError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| ContextAssertionError::ReplayBackendUnavailable)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(ContextAssertionError::ReplayBackendUnavailable);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let mode = metadata.permissions().mode() & 0o777;
        if metadata.uid() != rustix::process::geteuid().as_raw() || mode & 0o077 != 0 {
            return Err(ContextAssertionError::ReplayBackendUnavailable);
        }
        Ok(ReplayFileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
    #[cfg(not(unix))]
    Ok(ReplayFileIdentity {})
}

fn validate_replay_file_identity(
    path: &Path,
    expected: ReplayFileIdentity,
) -> Result<(), ContextAssertionError> {
    if replay_file_identity(path)? == expected {
        Ok(())
    } else {
        Err(ContextAssertionError::ReplayBackendUnavailable)
    }
}

fn open_replay_database(
    path: &Path,
    identity: ReplayFileIdentity,
) -> Result<Connection, ContextAssertionError> {
    validate_replay_file_identity(path, identity)?;
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let connection = Connection::open_with_flags(path, flags).map_err(replay_database_error)?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(replay_database_error)?;
    connection
        .execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")
        .map_err(replay_database_error)?;
    validate_replay_file_identity(path, identity)?;
    Ok(connection)
}

fn validate_replay_database(
    connection: &Connection,
    max_entries: usize,
    max_namespace_entries: usize,
) -> Result<(), ContextAssertionError> {
    let version: i64 = connection
        .query_row(
            "SELECT version FROM replay_metadata WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(replay_database_error)?;
    if version != replay_state_version() {
        return Err(ContextAssertionError::ReplayBackendUnavailable);
    }
    let metadata_rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM replay_metadata", [], |row| row.get(0))
        .map_err(replay_database_error)?;
    let invalid_entries: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM replay_entries\n             WHERE length(replay_key) != 64\n                OR length(namespace_hash) != 64\n                OR length(fingerprint_hex) != 64\n                OR expires_at_ms < 0",
            [],
            |row| row.get(0),
        )
        .map_err(replay_database_error)?;
    let total: i64 = connection
        .query_row("SELECT COUNT(*) FROM replay_entries", [], |row| row.get(0))
        .map_err(replay_database_error)?;
    let max_namespace_total: i64 = connection
        .query_row(
            "SELECT COALESCE(MAX(namespace_total), 0)\n             FROM (\n                SELECT COUNT(*) AS namespace_total\n                FROM replay_entries\n                GROUP BY namespace_hash\n             )",
            [],
            |row| row.get(0),
        )
        .map_err(replay_database_error)?;
    if metadata_rows != 1
        || invalid_entries != 0
        || total > usize_to_i64(max_entries)
        || max_namespace_total > usize_to_i64(max_namespace_entries)
    {
        return Err(ContextAssertionError::ReplayBackendUnavailable);
    }
    Ok(())
}

fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn replay_database_error(_: rusqlite::Error) -> ContextAssertionError {
    ContextAssertionError::ReplayBackendUnavailable
}

fn replay_key(issuer: &str, audience: &str, assertion_id: &str) -> String {
    let mut hasher = Sha256::new();
    for value in [issuer, audience, assertion_id] {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value.as_bytes());
    }
    hex(&hasher.finalize().into())
}

fn replay_namespace_key(issuer: &str, audience: &str) -> String {
    let mut hasher = Sha256::new();
    for value in [issuer, audience] {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value.as_bytes());
    }
    hex(&hasher.finalize().into())
}

fn hex(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
#[path = "context_assertion_security_tests.rs"]
mod tests;
