// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Startup-cached hosted context-assertion security configuration.

use base64::Engine as _;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use tandem_enterprise_contract::{
    ContextAssertionError, ContextAssertionPolicy, ContextAssertionReplayMode,
    ContextAssertionReplayStore, ContextAssertionVerifier, KeyStatus, SigningKeyPurpose,
    VerifiedTenantContext, VerifierKeyEntry, VerifierKeyring, DEFAULT_CONTEXT_ASSERTION_ISSUER,
    DEFAULT_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS, DEFAULT_CONTEXT_ASSERTION_MAX_LIFETIME_MS,
    HARD_CONTEXT_ASSERTION_MAX_LIFETIME_MS,
};
use tandem_types::RuntimeAuthMode;

const MAX_CONTEXT_ASSERTION_FUTURE_SKEW_MS: u64 = 60_000;

#[derive(Debug, Clone)]
pub(crate) struct RuntimeContextAssertionSecurity {
    verifier: ContextAssertionVerifier,
    replay_store: ContextAssertionReplayStore,
    replay_mode: ContextAssertionReplayMode,
    key_count: usize,
    keyring_fingerprint: String,
}

impl RuntimeContextAssertionSecurity {
    pub(crate) fn load_from_env(mode: RuntimeAuthMode) -> Result<Option<Self>, String> {
        let hosted = mode != RuntimeAuthMode::LocalSingleTenant
            || crate::config::env::hosted_control_plane_configured();
        let raw_keyring = read_optional_material(
            "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS",
            "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS_FILE",
            hosted,
        )?;
        let legacy = read_optional_material(
            "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY",
            "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY_FILE",
            hosted,
        )?;
        if hosted && legacy.is_some() {
            return Err(
                "hosted/enterprise mode rejects legacy TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY; configure a metadata keyring with TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS"
                    .to_string(),
            );
        }

        let keyring = match raw_keyring {
            Some(raw) => parse_runtime_keyring(&raw, hosted)?,
            None if legacy.is_some() && !hosted => {
                tracing::warn!(
                    target: "tandem_server::context_assertion",
                    "legacy anonymous context assertion key is enabled for local migration only"
                );
                legacy_keyring(legacy.as_deref().unwrap_or_default())?
            }
            None if hosted => {
                return Err("hosted/enterprise context assertion keyring is not configured".into())
            }
            None => return Ok(None),
        };
        if keyring.is_empty() {
            return Err("context assertion keyring must not be empty".into());
        }

        let issuer = nonempty_env("TANDEM_CONTEXT_ASSERTION_ISSUER")
            .unwrap_or_else(|| DEFAULT_CONTEXT_ASSERTION_ISSUER.to_string());
        let audience = nonempty_env("TANDEM_CONTEXT_ASSERTION_AUDIENCE")
            .unwrap_or_else(|| "tandem-runtime".to_string());
        let max_future_skew_ms = strict_u64_env(
            "TANDEM_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS",
            DEFAULT_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS,
            MAX_CONTEXT_ASSERTION_FUTURE_SKEW_MS,
        )?;
        let max_lifetime_ms = strict_u64_env(
            "TANDEM_CONTEXT_ASSERTION_MAX_LIFETIME_MS",
            DEFAULT_CONTEXT_ASSERTION_MAX_LIFETIME_MS,
            HARD_CONTEXT_ASSERTION_MAX_LIFETIME_MS,
        )?;
        let replay_mode = ContextAssertionReplayMode::parse(
            nonempty_env("TANDEM_CONTEXT_ASSERTION_REPLAY_MODE")
                .as_deref()
                .unwrap_or("bound"),
        )
        .map_err(|error| error.to_string())?;
        if hosted && replay_mode == ContextAssertionReplayMode::Off {
            return Err(
                "hosted/enterprise mode cannot disable context assertion replay protection"
                    .to_string(),
            );
        }
        let replay_store = match nonempty_env("TANDEM_CONTEXT_ASSERTION_REPLAY_STORE_FILE") {
            Some(path) => ContextAssertionReplayStore::persistent(path)
                .map_err(|error| error.to_string())?,
            None if hosted => {
                return Err(
                    "hosted/enterprise mode requires TANDEM_CONTEXT_ASSERTION_REPLAY_STORE_FILE pointing to shared durable storage"
                        .to_string(),
                )
            }
            None => ContextAssertionReplayStore::in_memory(),
        };
        let policy =
            ContextAssertionPolicy::new(issuer, audience, max_future_skew_ms, max_lifetime_ms)
                .map_err(|error| error.to_string())?;
        let keyring_fingerprint = keyring_fingerprint(&keyring)?;
        let key_count = keyring.len();
        let verifier =
            ContextAssertionVerifier::new(keyring, policy).map_err(|error| error.to_string())?;
        replay_store
            .readiness_check()
            .map_err(|error| error.to_string())?;
        Ok(Some(Self {
            verifier,
            replay_store,
            replay_mode,
            key_count,
            keyring_fingerprint,
        }))
    }

    pub(crate) fn verify(
        &self,
        assertion: &str,
    ) -> Result<VerifiedTenantContext, ContextAssertionError> {
        self.verify_at(assertion, crate::now_ms())
    }

    #[cfg(test)]
    pub(crate) fn verify_at(
        &self,
        assertion: &str,
        now_ms: u64,
    ) -> Result<VerifiedTenantContext, ContextAssertionError> {
        self.verify_at_inner(assertion, now_ms)
    }

    #[cfg(not(test))]
    fn verify_at(
        &self,
        assertion: &str,
        now_ms: u64,
    ) -> Result<VerifiedTenantContext, ContextAssertionError> {
        self.verify_at_inner(assertion, now_ms)
    }

    fn verify_at_inner(
        &self,
        assertion: &str,
        now_ms: u64,
    ) -> Result<VerifiedTenantContext, ContextAssertionError> {
        let verified = self
            .verifier
            .verify_at(assertion, now_ms)
            .map_err(|error| {
                tandem_observability::record_context_assertion_rejection(error.as_str());
                if matches!(
                    error,
                    ContextAssertionError::LifetimeExceeded | ContextAssertionError::TimeOverflow
                ) {
                    tracing::warn!(
                        target: "tandem_server::context_assertion",
                        reason = error.as_str(),
                        max_lifetime_ms = self.max_lifetime_ms(),
                        "rejected anomalous context assertion lifetime"
                    );
                }
                error
            })?;
        self.replay_store
            .check_and_record(self.replay_mode, &verified, now_ms)
            .map_err(|error| {
                tandem_observability::record_context_assertion_rejection(error.as_str());
                error
            })?;
        Ok(VerifiedTenantContext::from(verified.claims).with_assertion_key_id(verified.key_id))
    }

    pub(crate) fn key_count(&self) -> usize {
        self.key_count
    }

    pub(crate) fn replay_mode(&self) -> ContextAssertionReplayMode {
        self.replay_mode
    }

    pub(crate) fn max_lifetime_ms(&self) -> u64 {
        self.verifier.policy().max_lifetime_ms
    }

    pub(crate) fn keyring_fingerprint(&self) -> &str {
        &self.keyring_fingerprint
    }
}

fn keyring_fingerprint(keyring: &VerifierKeyring) -> Result<String, String> {
    let canonical = keyring
        .to_json()
        .map_err(|error| format!("failed to fingerprint context assertion keyring: {error}"))?;
    Ok(format!("{:x}", Sha256::digest(canonical.as_bytes())))
}

pub(crate) struct ContextAssertionSecurityReload {
    pub(crate) previous: Option<std::sync::Arc<RuntimeContextAssertionSecurity>>,
    pub(crate) current: Option<std::sync::Arc<RuntimeContextAssertionSecurity>>,
}

impl crate::AppState {
    /// Atomically publish a complete snapshot after all loading and readiness
    /// checks pass. Failed reloads leave the last-known-good generation live.
    pub(crate) fn reload_context_assertion_security(
        &self,
        mode: RuntimeAuthMode,
    ) -> Result<ContextAssertionSecurityReload, String> {
        let next = RuntimeContextAssertionSecurity::load_from_env(mode)?.map(std::sync::Arc::new);
        let mut current = self
            .context_assertion_security
            .write()
            .map_err(|_| "context assertion verifier cache lock is poisoned".to_string())?;
        let previous = current.clone();
        *current = next.clone();
        if let Some(snapshot) = next.as_ref() {
            tracing::info!(
                target: "tandem_server::context_assertion",
                previous_fingerprint = previous.as_ref().map(|value| value.keyring_fingerprint()),
                current_fingerprint = snapshot.keyring_fingerprint(),
                key_count = snapshot.key_count(),
                replay_mode = ?snapshot.replay_mode(),
                max_lifetime_ms = snapshot.max_lifetime_ms(),
                "published context assertion verifier snapshot"
            );
        }
        Ok(ContextAssertionSecurityReload {
            previous,
            current: next,
        })
    }

    pub(crate) fn context_assertion_security_snapshot(
        &self,
    ) -> Result<std::sync::Arc<RuntimeContextAssertionSecurity>, ContextAssertionError> {
        self.context_assertion_security
            .read()
            .map_err(|_| ContextAssertionError::KeyNotConfigured)?
            .clone()
            .ok_or(ContextAssertionError::KeyNotConfigured)
    }
}

fn parse_runtime_keyring(raw: &str, require_metadata: bool) -> Result<VerifierKeyring, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("context assertion keyring is empty".into());
    }
    if !trimmed.starts_with('{') {
        if require_metadata {
            return Err(
                "hosted/enterprise context assertion keyring must use JSON metadata entries".into(),
            );
        }
        return parse_local_delimited_keyring(trimmed);
    }
    let entries = serde_json::from_str::<BTreeMap<String, Value>>(trimmed)
        .map_err(|error| format!("invalid context assertion keyring JSON: {error}"))?;
    let mut keyring = VerifierKeyring::new();
    for (kid, value) in entries {
        let kid = kid.trim().to_string();
        if kid.is_empty() {
            return Err("context assertion keyring contains an empty kid".into());
        }
        let entry = match value {
            Value::String(public_key) if !require_metadata => {
                VerifierKeyEntry::new(&kid, SigningKeyPurpose::ContextAssertion, public_key)
            }
            Value::String(_) => {
                return Err(
                    "hosted/enterprise context assertion keys require purpose/status metadata"
                        .into(),
                )
            }
            Value::Object(object) => parse_metadata_entry(&kid, object, require_metadata)?,
            _ => {
                return Err(format!(
                    "context assertion key `{kid}` has invalid entry type"
                ))
            }
        };
        entry
            .verifying_key()
            .map_err(|_| format!("context assertion key `{kid}` is malformed"))?;
        keyring.insert(entry);
    }
    Ok(keyring)
}

fn parse_metadata_entry(
    kid: &str,
    mut object: serde_json::Map<String, Value>,
    require_metadata: bool,
) -> Result<VerifierKeyEntry, String> {
    normalize_alias(&mut object, "publicKey", "public_key");
    normalize_alias(&mut object, "organizationId", "organization_id");
    normalize_alias(&mut object, "orgId", "organization_id");
    normalize_alias(&mut object, "deploymentId", "deployment_id");
    normalize_alias(&mut object, "allowedAudiences", "allowed_audiences");
    normalize_alias(
        &mut object,
        "allowedResourceScopePrefixes",
        "allowed_resource_scope_prefixes",
    );
    normalize_alias(&mut object, "notBeforeMs", "not_before_ms");
    normalize_alias(&mut object, "notAfterMs", "not_after_ms");
    if !object.contains_key("purpose") {
        if require_metadata {
            return Err(format!(
                "context assertion key `{kid}` is missing purpose metadata"
            ));
        }
        object.insert(
            "purpose".to_string(),
            Value::String("context_assertion".to_string()),
        );
    }
    if !object.contains_key("status") {
        if require_metadata {
            return Err(format!(
                "context assertion key `{kid}` is missing status metadata"
            ));
        }
        object.insert("status".to_string(), Value::String("active".to_string()));
    }
    let mut entry: VerifierKeyEntry = serde_json::from_value(Value::Object(object))
        .map_err(|error| format!("context assertion key `{kid}` metadata is invalid: {error}"))?;
    entry.kid = kid.to_string();
    if entry.purpose != SigningKeyPurpose::ContextAssertion {
        return Err(format!(
            "context assertion key `{kid}` has the wrong purpose"
        ));
    }
    if require_metadata && entry.status != KeyStatus::Active {
        tracing::info!(
            target: "tandem_server::context_assertion",
            kid,
            status = entry.status.as_str(),
            "loaded non-active context assertion key metadata"
        );
    }
    Ok(entry)
}

fn normalize_alias(object: &mut serde_json::Map<String, Value>, alias: &str, canonical: &str) {
    if !object.contains_key(canonical) {
        if let Some(value) = object.remove(alias) {
            object.insert(canonical.to_string(), value);
        }
    }
}

fn parse_local_delimited_keyring(raw: &str) -> Result<VerifierKeyring, String> {
    let mut keyring = VerifierKeyring::new();
    for item in raw.split([',', ';', '\n']) {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let (kid, public_key) = item
            .split_once('=')
            .or_else(|| item.split_once(':'))
            .ok_or_else(|| "invalid local context assertion keyring entry".to_string())?;
        let entry = VerifierKeyEntry::new(
            kid.trim(),
            SigningKeyPurpose::ContextAssertion,
            public_key.trim(),
        );
        entry
            .verifying_key()
            .map_err(|_| format!("context assertion key `{}` is malformed", kid.trim()))?;
        keyring.insert(entry);
    }
    Ok(keyring)
}

fn legacy_keyring(raw: &str) -> Result<VerifierKeyring, String> {
    let decoded = decode_public_key(raw)
        .ok_or_else(|| "legacy context assertion public key is malformed".to_string())?;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(decoded);
    Ok(VerifierKeyring::from_entries([VerifierKeyEntry::new(
        "legacy",
        SigningKeyPurpose::ContextAssertion,
        encoded,
    )]))
}

fn decode_public_key(raw: &str) -> Option<[u8; 32]> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw.trim())
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(raw.trim()))
        .ok()?
        .try_into()
        .ok()
}

fn read_optional_material(
    env_name: &str,
    file_env_name: &str,
    strict_file_permissions: bool,
) -> Result<Option<String>, String> {
    if let Some(value) = nonempty_env(env_name) {
        return Ok(Some(value));
    }
    let Some(path) = nonempty_env(file_env_name) else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    let mut file = open_keyring_file(&path, strict_file_permissions)?;
    let mut value = String::new();
    file.read_to_string(&mut value)
        .map_err(|error| format!("failed to read `{}`: {error}", path.display()))?;
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(format!("keyring file `{}` is empty", path.display()));
    }
    Ok(Some(value))
}

fn open_keyring_file(path: &Path, strict: bool) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    let file = options
        .open(path)
        .map_err(|error| format!("failed to open keyring file `{}`: {error}", path.display()))?;
    let metadata = file.metadata().map_err(|error| {
        format!(
            "failed to inspect keyring file `{}`: {error}",
            path.display()
        )
    })?;
    if !metadata.file_type().is_file() {
        return Err(format!(
            "keyring path `{}` must be a regular file",
            path.display()
        ));
    }
    #[cfg(unix)]
    if strict {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let mode = metadata.permissions().mode() & 0o777;
        let effective_uid = rustix::process::geteuid().as_raw();
        if metadata.uid() != effective_uid {
            return Err(format!(
                "keyring file `{}` is not owned by the runtime user",
                path.display()
            ));
        }
        if mode & 0o077 != 0 {
            return Err(format!(
                "keyring file `{}` has insecure mode {:04o}; expected 0600 or stricter",
                path.display(),
                mode
            ));
        }
    }
    Ok(file)
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn strict_u64_env(name: &str, default: u64, maximum: u64) -> Result<u64, String> {
    let Some(raw) = nonempty_env(name) else {
        return Ok(default);
    };
    raw.parse::<u64>()
        .ok()
        .filter(|value| *value > 0 && *value <= maximum)
        .ok_or_else(|| format!("{name} must be between 1 and {maximum}"))
}

#[cfg(test)]
#[path = "context_assertion_security_runtime_tests.rs"]
mod tests;
