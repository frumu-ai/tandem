// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;
use fs2::FileExt;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

const KEY_ENV: &str = "TANDEM_AUDIT_HMAC_KEY";
const KEY_FILE_ENV: &str = "TANDEM_AUDIT_HMAC_KEY_FILE";
const KEY_ID_ENV: &str = "TANDEM_AUDIT_HMAC_KEY_ID";
const KEYRING_FILE_ENV: &str = "TANDEM_AUDIT_HMAC_KEYRING_FILE";
const ANCHOR_DIR_ENV: &str = "TANDEM_AUDIT_ANCHOR_DIR";
const KEY_PURPOSE: &str = "audit_integrity";
const MAC_PREFIX: &str = "hmac-sha256:";
const ANCHOR_VERSION: u32 = 1;

#[cfg(test)]
tokio::task_local! {
    static TEST_ANCHOR_DIR: PathBuf;
    static TEST_KEYRING: Option<AuditIntegrityKeyring>;
    static TEST_FAIL_AFTER_ANCHOR_WRITES: std::cell::Cell<usize>;
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum KeyStatus {
    Active,
    VerifyOnly,
    Revoked,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyringDocument {
    active_key_id: String,
    keys: Vec<KeyDocument>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyDocument {
    id: String,
    purpose: String,
    status: KeyStatus,
    key: String,
}

#[derive(Debug, Clone)]
struct IntegrityKey {
    purpose: String,
    status: KeyStatus,
    material: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) struct AuditIntegrityKeyring {
    active_key_id: String,
    keys: BTreeMap<String, IntegrityKey>,
}

impl AuditIntegrityKeyring {
    pub(crate) fn active_key_id(&self) -> &str {
        &self.active_key_id
    }

    pub(crate) fn sign_active(&self, domain: &[u8], payload: &[u8]) -> anyhow::Result<String> {
        self.sign_with_key(&self.active_key_id, domain, payload)
    }

    pub(crate) fn sign_with_key(
        &self,
        key_id: &str,
        domain: &[u8],
        payload: &[u8],
    ) -> anyhow::Result<String> {
        let key = self.usable_key(key_id, true)?;
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&key.material)
            .map_err(|_| anyhow::anyhow!("audit integrity HMAC key is invalid"))?;
        mac.update(b"tandem-audit-integrity/v1\0");
        mac.update(domain);
        mac.update(b"\0");
        mac.update(payload);
        Ok(format!(
            "{MAC_PREFIX}{}",
            hex_bytes(&mac.finalize().into_bytes())
        ))
    }

    pub(crate) fn verify(
        &self,
        key_id: &str,
        domain: &[u8],
        payload: &[u8],
        expected: &str,
    ) -> anyhow::Result<()> {
        let key = self.usable_key(key_id, false)?;
        let expected = expected
            .strip_prefix(MAC_PREFIX)
            .context("audit integrity MAC algorithm is unsupported")?;
        let expected = decode_hex(expected).context("audit integrity MAC is malformed")?;
        anyhow::ensure!(
            expected.len() == 32,
            "audit integrity MAC has invalid length"
        );
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&key.material)
            .map_err(|_| anyhow::anyhow!("audit integrity HMAC key is invalid"))?;
        mac.update(b"tandem-audit-integrity/v1\0");
        mac.update(domain);
        mac.update(b"\0");
        mac.update(payload);
        mac.verify_slice(&expected)
            .map_err(|_| anyhow::anyhow!("audit integrity MAC verification failed"))
    }

    fn usable_key(&self, key_id: &str, signing: bool) -> anyhow::Result<&IntegrityKey> {
        let key = self
            .keys
            .get(key_id)
            .with_context(|| format!("audit integrity key `{key_id}` is missing"))?;
        anyhow::ensure!(
            key.purpose == KEY_PURPOSE,
            "audit integrity key `{key_id}` has wrong purpose `{}`",
            key.purpose
        );
        anyhow::ensure!(
            key.status != KeyStatus::Revoked,
            "audit integrity key `{key_id}` is revoked"
        );
        if signing {
            anyhow::ensure!(
                key.status == KeyStatus::Active && key_id == self.active_key_id,
                "audit integrity key `{key_id}` is not the active signing key"
            );
        }
        Ok(key)
    }
}

pub(crate) fn production_posture() -> bool {
    crate::config::env::resolve_runtime_auth_mode()
        != tandem_types::RuntimeAuthMode::LocalSingleTenant
        || crate::config::env::hosted_control_plane_configured()
}

fn configured_anchor_dir() -> Option<PathBuf> {
    #[cfg(test)]
    if let Ok(path) = TEST_ANCHOR_DIR.try_with(|path| path.clone()) {
        return Some(path);
    }
    configured_path(ANCHOR_DIR_ENV)
}

pub(crate) fn integrity_authority() -> anyhow::Result<Option<AuditIntegrityKeyring>> {
    let production = production_posture();
    let keyring = load_keyring(production)?;
    let anchor_dir = configured_anchor_dir();
    match (keyring, anchor_dir) {
        (Some(keyring), Some(_)) => Ok(Some(keyring)),
        (None, None) if !production => Ok(None),
        (Some(_), None) if !production => {
            tracing::warn!(
                "audit HMAC key is configured without TANDEM_AUDIT_ANCHOR_DIR; protected-store keyed integrity remains disabled in local posture"
            );
            Ok(None)
        }
        (None, Some(_)) if !production => anyhow::bail!(
            "TANDEM_AUDIT_ANCHOR_DIR requires an audit integrity HMAC key"
        ),
        (None, _) => anyhow::bail!(
            "hosted/enterprise audit integrity requires TANDEM_AUDIT_HMAC_KEY, TANDEM_AUDIT_HMAC_KEY_FILE, or TANDEM_AUDIT_HMAC_KEYRING_FILE"
        ),
        (Some(_), None) => anyhow::bail!(
            "hosted/enterprise audit integrity requires TANDEM_AUDIT_ANCHOR_DIR"
        ),
    }
}

pub(crate) fn verification_keyring() -> anyhow::Result<Option<AuditIntegrityKeyring>> {
    load_keyring(production_posture())
}

pub(crate) fn configured_active_key_material() -> anyhow::Result<Option<Vec<u8>>> {
    let Some(keyring) = load_keyring(production_posture())? else {
        return Ok(None);
    };
    Ok(Some(
        keyring
            .usable_key(keyring.active_key_id(), true)?
            .material
            .clone(),
    ))
}

pub(crate) fn validate_configuration(runtime_state_root: Option<&Path>) -> anyhow::Result<()> {
    let authority = integrity_authority()?;
    if let Some(authority) = authority {
        authority.usable_key(authority.active_key_id(), true)?;
        let anchor_dir =
            configured_anchor_dir().context("audit integrity anchor directory is missing")?;
        anyhow::ensure!(
            anchor_dir.is_absolute(),
            "TANDEM_AUDIT_ANCHOR_DIR must be an absolute path"
        );
        let canonical_anchor = prepare_anchor_dir(&anchor_dir)?;
        let mut state_roots = Vec::new();
        if let Some(root) = runtime_state_root {
            state_roots.push(std::fs::canonicalize(root).with_context(|| {
                format!(
                    "canonicalize resolved Tandem state root `{}`",
                    root.display()
                )
            })?);
        }
        if let Some(configured_root) = canonical_configured_state_root()? {
            if !state_roots.contains(&configured_root) {
                state_roots.push(configured_root);
            }
        }
        ensure_anchor_outside_state_roots(&canonical_anchor, &state_roots)?;
    }
    Ok(())
}

fn load_keyring(production: bool) -> anyhow::Result<Option<AuditIntegrityKeyring>> {
    #[cfg(test)]
    if let Ok(keyring) = TEST_KEYRING.try_with(Clone::clone) {
        return Ok(keyring);
    }
    let keyring_path = configured_path(KEYRING_FILE_ENV);
    let direct = std::env::var(KEY_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty());
    let single_file = configured_path(KEY_FILE_ENV);
    let configured_sources = usize::from(keyring_path.is_some())
        + usize::from(direct.is_some())
        + usize::from(single_file.is_some());
    anyhow::ensure!(
        configured_sources <= 1,
        "configure exactly one audit integrity key source"
    );

    let result = if let Some(path) = keyring_path {
        let bytes = read_secret_file(&path)?;
        let document = serde_json::from_slice::<KeyringDocument>(&bytes)
            .with_context(|| format!("parse audit integrity keyring `{}`", path.display()))?;
        Some(keyring_from_document(document, production)?)
    } else if let Some(value) = direct {
        Some(single_keyring(
            configured_key_id(),
            value.trim().as_bytes().to_vec(),
            production,
        )?)
    } else if let Some(path) = single_file {
        let bytes = read_secret_file(&path)?;
        let material = trim_ascii_whitespace(&bytes).to_vec();
        Some(single_keyring(configured_key_id(), material, production)?)
    } else {
        None
    };
    Ok(result)
}

fn keyring_from_document(
    document: KeyringDocument,
    production: bool,
) -> anyhow::Result<AuditIntegrityKeyring> {
    validate_key_id(&document.active_key_id)?;
    anyhow::ensure!(
        !document.keys.is_empty(),
        "audit integrity keyring is empty"
    );
    let mut keys = BTreeMap::new();
    let mut material_fingerprints = BTreeSet::new();
    let mut active_key_count = 0usize;
    for entry in document.keys {
        validate_key_id(&entry.id)?;
        let material = entry.key.as_bytes().to_vec();
        validate_key_material(&entry.id, &material, production)?;
        let fingerprint = hex_bytes(&Sha256::digest(&material));
        anyhow::ensure!(
            material_fingerprints.insert(fingerprint),
            "audit integrity key material is duplicated across key IDs"
        );
        if entry.status == KeyStatus::Active {
            active_key_count = active_key_count.saturating_add(1);
            anyhow::ensure!(
                entry.id == document.active_key_id,
                "only active_key_id may have active status"
            );
        }
        anyhow::ensure!(
            keys.insert(
                entry.id.clone(),
                IntegrityKey {
                    purpose: entry.purpose,
                    status: entry.status,
                    material,
                },
            )
            .is_none(),
            "duplicate audit integrity key ID `{}`",
            entry.id
        );
    }
    anyhow::ensure!(
        active_key_count == 1,
        "audit integrity keyring must contain exactly one active key"
    );
    let keyring = AuditIntegrityKeyring {
        active_key_id: document.active_key_id,
        keys,
    };
    keyring.usable_key(keyring.active_key_id(), true)?;
    Ok(keyring)
}

fn single_keyring(
    key_id: String,
    material: Vec<u8>,
    production: bool,
) -> anyhow::Result<AuditIntegrityKeyring> {
    validate_key_id(&key_id)?;
    validate_key_material(&key_id, &material, production)?;
    let mut keys = BTreeMap::new();
    keys.insert(
        key_id.clone(),
        IntegrityKey {
            purpose: KEY_PURPOSE.to_string(),
            status: KeyStatus::Active,
            material,
        },
    );
    Ok(AuditIntegrityKeyring {
        active_key_id: key_id,
        keys,
    })
}

fn configured_key_id() -> String {
    std::env::var(KEY_ID_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "primary".to_string())
}

fn validate_key_id(key_id: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !key_id.is_empty()
            && key_id.len() <= 128
            && key_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)),
        "audit integrity key ID is invalid"
    );
    Ok(())
}

fn validate_key_material(key_id: &str, material: &[u8], production: bool) -> anyhow::Result<()> {
    let minimum = if production { 32 } else { 16 };
    anyhow::ensure!(
        material.len() >= minimum,
        "audit integrity key `{key_id}` must contain at least {minimum} bytes"
    );
    Ok(())
}

fn configured_path(name: &str) -> Option<PathBuf> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(unix)]
fn read_secret_file(path: &Path) -> anyhow::Result<Vec<u8>> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .with_context(|| format!("open audit integrity key file `{}`", path.display()))?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.file_type().is_file() && metadata.nlink() == 1,
        "audit integrity key file must be a regular single-link file"
    );
    anyhow::ensure!(
        metadata.uid() == unsafe { libc::geteuid() },
        "audit integrity key file has the wrong owner"
    );
    anyhow::ensure!(
        metadata.mode() & 0o777 == 0o600,
        "audit integrity key file must have mode 0600"
    );
    anyhow::ensure!(
        metadata.len() <= 1024 * 1024,
        "audit integrity key file is too large"
    );
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(windows)]
fn read_secret_file(path: &Path) -> anyhow::Result<Vec<u8>> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };

    let parent = path_parent_or_current(path);
    let directory = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(parent)
        .with_context(|| format!("open audit integrity key parent `{}`", parent.display()))?;
    let parent_metadata = directory.metadata()?;
    anyhow::ensure!(
        parent_metadata.file_type().is_dir()
            && parent_metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "audit integrity key parent must be a non-reparse directory"
    );
    tandem_memory::windows_acl::validate_private_file_handle(
        &directory,
        "audit integrity key parent",
    )?;

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .with_context(|| format!("open audit integrity key file `{}`", path.display()))?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.file_type().is_file()
            && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "audit integrity key file must be a non-reparse regular file"
    );
    anyhow::ensure!(
        metadata.len() <= 1024 * 1024,
        "audit integrity key file is too large"
    );
    tandem_memory::windows_acl::validate_private_file_handle(&file, "audit integrity key file")?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(not(any(unix, windows)))]
fn read_secret_file(path: &Path) -> anyhow::Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
        "audit integrity key path must be a regular file"
    );
    anyhow::ensure!(
        metadata.len() <= 1024 * 1024,
        "audit integrity key file is too large"
    );
    std::fs::read(path).map_err(Into::into)
}

fn path_parent_or_current(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

pub(crate) fn canonical_state_file_identity(path: &Path) -> anyhow::Result<String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "protected state path must not be a symbolic link"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let file_name = path
                .file_name()
                .context("protected state path is missing a file name")?;
            let parent =
                std::fs::canonicalize(path_parent_or_current(path)).with_context(|| {
                    format!(
                        "canonicalize protected state directory `{}`",
                        path_parent_or_current(path).display()
                    )
                })?;
            return encode_canonical_path_identity(&parent.join(file_name));
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect protected state path `{}`", path.display()))
        }
    };
    let canonical = std::fs::canonicalize(path)
        .with_context(|| format!("canonicalize protected state path `{}`", path.display()))?;
    encode_canonical_path_identity(&canonical)
}

fn push_hex_byte(encoded: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    encoded.push(HEX[(byte >> 4) as usize] as char);
    encoded.push(HEX[(byte & 0x0f) as usize] as char);
}

#[cfg(unix)]
fn encode_canonical_path_identity(path: &Path) -> anyhow::Result<String> {
    use std::os::unix::ffi::OsStrExt;

    let bytes = path.as_os_str().as_bytes();
    let mut encoded = String::with_capacity(14 + bytes.len() * 2);
    encoded.push_str("unix-bytes-v1:");
    for byte in bytes {
        push_hex_byte(&mut encoded, *byte);
    }
    Ok(encoded)
}

#[cfg(windows)]
fn encode_canonical_path_identity(path: &Path) -> anyhow::Result<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Globalization::{
        LCMapStringEx, LCMAP_UPPERCASE, LOCALE_NAME_INVARIANT,
    };

    let source = path.as_os_str().encode_wide().collect::<Vec<_>>();
    let source_len = source
        .len()
        .try_into()
        .context("canonical path is too long")?;
    let required = unsafe {
        LCMapStringEx(
            LOCALE_NAME_INVARIANT,
            LCMAP_UPPERCASE,
            source.as_ptr(),
            source_len,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
            std::ptr::null(),
            0,
        )
    };
    anyhow::ensure!(
        required > 0,
        "normalize canonical Windows path identity: {}",
        std::io::Error::last_os_error()
    );
    let mut normalized = vec![0u16; required as usize];
    let written = unsafe {
        LCMapStringEx(
            LOCALE_NAME_INVARIANT,
            LCMAP_UPPERCASE,
            source.as_ptr(),
            source_len,
            normalized.as_mut_ptr(),
            required,
            std::ptr::null(),
            std::ptr::null(),
            0,
        )
    };
    anyhow::ensure!(
        written == required,
        "normalize canonical Windows path identity: {}",
        std::io::Error::last_os_error()
    );
    let mut encoded = String::from("windows-uppercase-utf16le-v1:");
    for unit in normalized {
        for byte in unit.to_le_bytes() {
            push_hex_byte(&mut encoded, byte);
        }
    }
    Ok(encoded)
}

#[cfg(not(any(unix, windows)))]
fn encode_canonical_path_identity(path: &Path) -> anyhow::Result<String> {
    let value = path
        .to_str()
        .context("canonical protected state path is not valid UTF-8")?;
    let mut encoded = String::with_capacity(8 + value.len() * 2);
    encoded.push_str("utf8-v1:");
    for byte in value.as_bytes() {
        push_hex_byte(&mut encoded, *byte);
    }
    Ok(encoded)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalAnchor {
    version: u32,
    scope: String,
    identity: String,
    generation: u64,
    digest: String,
    key_id: String,
    anchored_at_ms: u64,
    mac: String,
}

#[derive(Serialize)]
struct ExternalAnchorForMac<'a> {
    version: u32,
    scope: &'a str,
    identity: &'a str,
    generation: u64,
    digest: &'a str,
    key_id: &'a str,
    anchored_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct AnchorVerification {
    pub configured: bool,
    pub verified: bool,
    pub key_id: Option<String>,
    pub generation: Option<u64>,
    pub anchored_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ExternalAnchorUpdate<'a> {
    pub scope: &'a str,
    pub identity: &'a str,
    pub generation: u64,
    pub digest: &'a str,
    pub previous: Option<(u64, &'a str)>,
}

#[derive(Debug)]
pub(crate) struct ExternalAnchorSnapshot {
    anchor_path: PathBuf,
    bytes: Option<Vec<u8>>,
    _lock: std::fs::File,
}

impl AnchorVerification {
    fn disabled() -> Self {
        Self {
            configured: false,
            verified: false,
            key_id: None,
            generation: None,
            anchored_at_ms: None,
        }
    }

    fn missing() -> Self {
        Self {
            configured: true,
            verified: false,
            key_id: None,
            generation: None,
            anchored_at_ms: None,
        }
    }
}

#[cfg(unix)]
fn read_anchor_file(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("open external audit anchor at {}", path.display()))
        }
    };
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.file_type().is_file() && metadata.nlink() == 1,
        "external audit anchor must be a regular single-link file"
    );
    anyhow::ensure!(
        metadata.uid() == unsafe { libc::geteuid() },
        "external audit anchor has the wrong owner"
    );
    anyhow::ensure!(
        metadata.mode() & 0o777 == 0o600,
        "external audit anchor must have mode 0600"
    );
    anyhow::ensure!(
        metadata.len() <= 64 * 1024,
        "external audit anchor is too large"
    );
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

#[cfg(windows)]
fn read_anchor_file(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
    };

    validate_windows_anchor_directory(path.parent().unwrap_or_else(|| Path::new(".")))?;
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("open external audit anchor at {}", path.display()))
        }
    };
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.file_type().is_file()
            && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "external audit anchor must be a non-reparse regular file"
    );
    anyhow::ensure!(
        metadata.len() <= 64 * 1024,
        "external audit anchor is too large"
    );
    tandem_memory::windows_acl::validate_private_file_handle(&file, "external audit anchor")?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

#[cfg(not(any(unix, windows)))]
fn read_anchor_file(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("inspect external audit anchor"),
    };
    anyhow::ensure!(
        metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
        "external audit anchor path must be a regular file"
    );
    anyhow::ensure!(
        metadata.len() <= 64 * 1024,
        "external audit anchor is too large"
    );
    Ok(Some(std::fs::read(path)?))
}

pub(crate) async fn write_external_anchor(
    scope: &str,
    identity: &str,
    generation: u64,
    digest: &str,
    snapshot: &ExternalAnchorSnapshot,
    state_path: &Path,
) -> anyhow::Result<AnchorVerification> {
    let Some(keyring) = integrity_authority()? else {
        return Ok(AnchorVerification::disabled());
    };
    anyhow::ensure!(
        generation > 0 && !digest.is_empty(),
        "external anchor is incomplete"
    );
    let anchor_path = resolved_anchor_path(scope, identity, state_path)?;
    anyhow::ensure!(
        anchor_path == snapshot.anchor_path,
        "external integrity anchor snapshot does not match its update path"
    );
    let mut anchor = ExternalAnchor {
        version: ANCHOR_VERSION,
        scope: scope.to_string(),
        identity: identity.to_string(),
        generation,
        digest: digest.to_string(),
        key_id: keyring.active_key_id().to_string(),
        anchored_at_ms: crate::now_ms(),
        mac: String::new(),
    };
    anchor.mac = keyring.sign_active(b"external-anchor", &anchor_mac_payload(&anchor)?)?;
    let encoded = serde_json::to_vec_pretty(&anchor)?;
    let path_for_write = anchor_path.clone();
    tokio::task::spawn_blocking(move || atomic_write_anchor(&path_for_write, &encoded))
        .await
        .context("join external audit anchor write")??;
    #[cfg(test)]
    if TEST_FAIL_AFTER_ANCHOR_WRITES
        .try_with(|remaining| {
            let current = remaining.get();
            if current > 0 {
                remaining.set(current - 1);
            }
            current == 1
        })
        .unwrap_or(false)
    {
        anyhow::bail!("injected external anchor failure after publication");
    }
    Ok(AnchorVerification {
        configured: true,
        verified: true,
        key_id: Some(anchor.key_id),
        generation: Some(anchor.generation),
        anchored_at_ms: Some(anchor.anchored_at_ms),
    })
}

pub(crate) async fn snapshot_external_anchor_update(
    update: ExternalAnchorUpdate<'_>,
    state_path: &Path,
) -> anyhow::Result<ExternalAnchorSnapshot> {
    let anchor_path = resolved_anchor_path(update.scope, update.identity, state_path)?;
    let anchor_path_for_lock = anchor_path.clone();
    let lock =
        tokio::task::spawn_blocking(move || acquire_external_anchor_lock(&anchor_path_for_lock))
            .await
            .context("join external integrity anchor lock acquisition")??;
    let bytes = read_external_anchor_bytes(&anchor_path).await?;
    match (&bytes, update.previous) {
        (None, None) => {}
        (None, Some(_)) => anyhow::bail!("previous external integrity anchor is missing"),
        (Some(_), None) => {
            anyhow::bail!("unexpected external integrity anchor already exists")
        }
        (Some(bytes), Some((generation, digest))) => {
            let anchor = serde_json::from_slice::<ExternalAnchor>(bytes)
                .context("parse previous external integrity anchor")?;
            anyhow::ensure!(
                anchor.version == ANCHOR_VERSION
                    && anchor.scope == update.scope
                    && anchor.identity == update.identity,
                "previous external integrity anchor identity mismatch"
            );
            let keyring = verification_keyring()?
                .context("previous external integrity anchor requires its audit keyring")?;
            keyring.verify(
                &anchor.key_id,
                b"external-anchor",
                &anchor_mac_payload(&anchor)?,
                &anchor.mac,
            )?;
            validate_anchor_target(&anchor, generation, digest)?;
        }
    }
    Ok(ExternalAnchorSnapshot {
        anchor_path,
        bytes,
        _lock: lock,
    })
}

pub(crate) async fn rollback_external_anchor_update(
    update: ExternalAnchorUpdate<'_>,
    snapshot: &ExternalAnchorSnapshot,
    state_path: &Path,
) -> anyhow::Result<()> {
    let anchor_path = resolved_anchor_path(update.scope, update.identity, state_path)?;
    anyhow::ensure!(
        anchor_path == snapshot.anchor_path,
        "external integrity anchor snapshot does not match its rollback path"
    );
    let Some(bytes) = read_external_anchor_bytes(&anchor_path).await? else {
        if let Some(previous) = snapshot.bytes.as_ref() {
            let anchor_path_for_write = anchor_path.clone();
            let previous = previous.clone();
            tokio::task::spawn_blocking(move || {
                atomic_write_anchor(&anchor_path_for_write, &previous)
            })
            .await
            .context("join exact external audit anchor restore")??;
        }
        return Ok(());
    };
    if snapshot.bytes.as_deref() == Some(bytes.as_slice()) {
        return Ok(());
    }
    let anchor = serde_json::from_slice::<ExternalAnchor>(&bytes)
        .context("parse external integrity anchor during rollback")?;
    anyhow::ensure!(
        anchor.version == ANCHOR_VERSION
            && anchor.scope == update.scope
            && anchor.identity == update.identity,
        "refusing to restore a different external integrity anchor during rollback"
    );
    let keyring = verification_keyring()?
        .context("external integrity anchor rollback requires its audit keyring")?;
    keyring.verify(
        &anchor.key_id,
        b"external-anchor",
        &anchor_mac_payload(&anchor)?,
        &anchor.mac,
    )?;
    if anchor.generation == update.generation && anchor.digest == update.digest {
        return match snapshot.bytes.as_ref() {
            Some(previous) => {
                let anchor_path_for_write = anchor_path.clone();
                let previous = previous.clone();
                tokio::task::spawn_blocking(move || {
                    atomic_write_anchor(&anchor_path_for_write, &previous)
                })
                .await
                .context("join exact external audit anchor rollback")?
            }
            None => tokio::task::spawn_blocking(move || remove_anchor_file(&anchor_path))
                .await
                .context("join external audit anchor rollback")?,
        };
    }
    anyhow::bail!("external integrity anchor changed during rollback")
}

pub(crate) async fn verify_external_anchor(
    scope: &str,
    identity: &str,
    generation: u64,
    digest: &str,
    integrity_keyed: bool,
    state_path: &Path,
) -> anyhow::Result<AnchorVerification> {
    let Some(keyring) = verification_keyring()? else {
        anyhow::ensure!(
            !integrity_keyed,
            "keyed integrity data cannot be verified without its audit keyring"
        );
        return Ok(AnchorVerification::disabled());
    };
    let Some(_) = configured_anchor_dir() else {
        anyhow::ensure!(
            !integrity_keyed,
            "keyed integrity data cannot be verified without TANDEM_AUDIT_ANCHOR_DIR"
        );
        return Ok(AnchorVerification::disabled());
    };
    let anchor_path = resolved_anchor_path(scope, identity, state_path)?;
    let bytes = read_external_anchor_bytes(&anchor_path).await?;
    let Some(bytes) = bytes else {
        anyhow::ensure!(!integrity_keyed, "external integrity anchor is missing");
        return Ok(AnchorVerification::missing());
    };
    let anchor = serde_json::from_slice::<ExternalAnchor>(&bytes)
        .context("parse external integrity anchor")?;
    anyhow::ensure!(
        anchor.version == ANCHOR_VERSION && anchor.scope == scope && anchor.identity == identity,
        "external integrity anchor identity mismatch"
    );
    keyring.verify(
        &anchor.key_id,
        b"external-anchor",
        &anchor_mac_payload(&anchor)?,
        &anchor.mac,
    )?;
    validate_anchor_target(&anchor, generation, digest)?;
    Ok(AnchorVerification {
        configured: true,
        verified: true,
        key_id: Some(anchor.key_id),
        generation: Some(anchor.generation),
        anchored_at_ms: Some(anchor.anchored_at_ms),
    })
}

async fn read_external_anchor_bytes(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    let path_for_read = path.to_path_buf();
    tokio::task::spawn_blocking(move || read_anchor_file(&path_for_read))
        .await
        .context("join external audit anchor read")?
}

#[cfg(test)]
pub(crate) async fn read_test_anchor_bytes(
    scope: &str,
    identity: &str,
    state_path: &Path,
) -> anyhow::Result<Option<Vec<u8>>> {
    let anchor_path = resolved_anchor_path(scope, identity, state_path)?;
    read_external_anchor_bytes(&anchor_path).await
}

pub(crate) async fn ensure_external_anchor_absent(
    scope: &str,
    identity: &str,
    state_path: &Path,
) -> anyhow::Result<()> {
    let Some(_) = configured_anchor_dir() else {
        anyhow::ensure!(
            !production_posture(),
            "hosted/enterprise legacy integrity verification requires TANDEM_AUDIT_ANCHOR_DIR"
        );
        return Ok(());
    };
    let anchor_path = resolved_anchor_path(scope, identity, state_path)?;
    ensure_anchor_path_absent(&anchor_path).await
}

async fn ensure_anchor_path_absent(path: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        read_external_anchor_bytes(path).await?.is_none(),
        "external integrity anchor proves this store was previously keyed; refusing legacy replacement"
    );
    Ok(())
}

fn validate_anchor_target(
    anchor: &ExternalAnchor,
    generation: u64,
    digest: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        anchor.generation == generation && anchor.digest == digest,
        "external integrity anchor detects deletion, rollback, or head substitution"
    );
    Ok(())
}

fn anchor_mac_payload(anchor: &ExternalAnchor) -> anyhow::Result<Vec<u8>> {
    Ok(serde_json::to_vec(&ExternalAnchorForMac {
        version: anchor.version,
        scope: &anchor.scope,
        identity: &anchor.identity,
        generation: anchor.generation,
        digest: &anchor.digest,
        key_id: &anchor.key_id,
        anchored_at_ms: anchor.anchored_at_ms,
    })?)
}

fn prepare_anchor_dir(anchor_dir: &Path) -> anyhow::Result<PathBuf> {
    if !anchor_dir.exists() {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(anchor_dir).with_context(|| {
            format!(
                "create external audit anchor directory at {}",
                anchor_dir.display()
            )
        })?;
    }
    let metadata = std::fs::symlink_metadata(anchor_dir).with_context(|| {
        format!(
            "inspect external audit anchor directory at {}",
            anchor_dir.display()
        )
    })?;
    anyhow::ensure!(
        metadata.file_type().is_dir() && !metadata.file_type().is_symlink(),
        "external audit anchor path must be a real directory"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        anyhow::ensure!(
            metadata.uid() == unsafe { libc::geteuid() },
            "external audit anchor directory has the wrong owner"
        );
        anyhow::ensure!(
            metadata.mode() & 0o777 == 0o700,
            "external audit anchor directory must have mode 0700"
        );
    }
    #[cfg(windows)]
    validate_windows_anchor_directory(anchor_dir)?;
    std::fs::canonicalize(anchor_dir).with_context(|| {
        format!(
            "canonicalize external audit anchor directory at {}",
            anchor_dir.display()
        )
    })
}

#[cfg(windows)]
fn validate_windows_anchor_directory(path: &Path) -> anyhow::Result<()> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };

    let directory = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .with_context(|| format!("open external audit anchor directory at {}", path.display()))?;
    let metadata = directory.metadata()?;
    anyhow::ensure!(
        metadata.file_type().is_dir()
            && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "external audit anchor directory must be a non-reparse directory"
    );
    tandem_memory::windows_acl::validate_private_file_handle(
        &directory,
        "external audit anchor directory",
    )?;
    Ok(())
}

fn canonical_configured_state_root() -> anyhow::Result<Option<PathBuf>> {
    if let Some(root) = configured_path("TANDEM_STATE_DIR") {
        anyhow::ensure!(root.is_absolute(), "TANDEM_STATE_DIR must be absolute");
        return std::fs::canonicalize(&root)
            .with_context(|| format!("canonicalize TANDEM_STATE_DIR `{}`", root.display()))
            .map(Some);
    }

    let root = tandem_core::resolve_shared_paths()
        .ok()
        .map(|paths| paths.canonical_root)
        .or_else(|| {
            let data = crate::config::paths::default_state_dir();
            data.parent().map(Path::to_path_buf)
        });
    match root {
        Some(root) if root.exists() => std::fs::canonicalize(&root)
            .with_context(|| format!("canonicalize Tandem shared state root `{}`", root.display()))
            .map(Some),
        _ => Ok(None),
    }
}

fn ensure_anchor_outside_state_roots(anchor: &Path, state_roots: &[PathBuf]) -> anyhow::Result<()> {
    for root in state_roots {
        anyhow::ensure!(
            !anchor.starts_with(root) && !root.starts_with(anchor),
            "external audit anchor directory must be outside the configured Tandem state tree"
        );
    }
    Ok(())
}

fn resolved_anchor_path(scope: &str, identity: &str, state_path: &Path) -> anyhow::Result<PathBuf> {
    let anchor_dir =
        configured_anchor_dir().context("TANDEM_AUDIT_ANCHOR_DIR is not configured")?;
    anyhow::ensure!(
        anchor_dir.is_absolute(),
        "TANDEM_AUDIT_ANCHOR_DIR must be absolute"
    );
    let canonical_anchor = prepare_anchor_dir(&anchor_dir)?;
    let state_parent = path_parent_or_current(state_path);
    let canonical_state = std::fs::canonicalize(state_parent).with_context(|| {
        format!(
            "canonicalize protected state directory `{}`",
            state_parent.display()
        )
    })?;
    let mut state_roots = vec![canonical_state];
    if let Some(configured_root) = canonical_configured_state_root()? {
        if !state_roots.contains(&configured_root) {
            state_roots.push(configured_root);
        }
    }
    ensure_anchor_outside_state_roots(&canonical_anchor, &state_roots)?;
    let mut hasher = Sha256::new();
    hasher.update(scope.as_bytes());
    hasher.update(b"\0");
    hasher.update(identity.as_bytes());
    Ok(canonical_anchor.join(format!("{}.anchor.json", hex_bytes(&hasher.finalize()))))
}

fn acquire_external_anchor_lock(anchor_path: &Path) -> anyhow::Result<std::fs::File> {
    #[cfg(unix)]
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    #[cfg(windows)]
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

    let mut lock_name = anchor_path.as_os_str().to_os_string();
    lock_name.push(".lock");
    let lock_path = PathBuf::from(lock_name);
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    options
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    #[cfg(windows)]
    options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    let file = options.open(&lock_path).with_context(|| {
        format!(
            "open external integrity anchor lock at {}",
            lock_path.display()
        )
    })?;
    let metadata = file.metadata()?;
    #[cfg(unix)]
    anyhow::ensure!(
        metadata.file_type().is_file()
            && metadata.nlink() == 1
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o777 == 0o600,
        "external integrity anchor lock must be an owner-only regular single-link file"
    );
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        anyhow::ensure!(
            metadata.file_type().is_file()
                && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
            "external integrity anchor lock must be a non-reparse regular file"
        );
        tandem_memory::windows_acl::validate_private_file_handle(
            &file,
            "external integrity anchor lock",
        )?;
    }
    #[cfg(not(any(unix, windows)))]
    anyhow::ensure!(
        metadata.file_type().is_file(),
        "external integrity anchor lock must be a regular file"
    );
    FileExt::lock_exclusive(&file).context("lock external integrity anchor update path")?;
    Ok(file)
}

#[cfg(windows)]
fn replace_anchor_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_anchor_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(source, destination)
}

fn remove_anchor_file(path: &Path) -> anyhow::Result<()> {
    std::fs::remove_file(path)
        .with_context(|| format!("remove external audit anchor at {}", path.display()))?;
    #[cfg(unix)]
    std::fs::File::open(path_parent_or_current(path))?
        .sync_all()
        .context("sync external audit anchor directory after rollback")?;
    Ok(())
}

fn atomic_write_anchor(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    #[cfg(windows)]
    use std::os::windows::fs::OpenOptionsExt;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    #[cfg(windows)]
    validate_windows_anchor_directory(parent)?;
    let temporary = parent.join(format!(".anchor-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
        #[cfg(windows)]
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
        let mut file = options.open(&temporary)?;
        #[cfg(windows)]
        tandem_memory::windows_acl::validate_private_file_handle(
            &file,
            "external audit anchor temporary file",
        )?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        replace_anchor_file(&temporary, path)?;
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        anyhow::Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn trim_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|index| index + 1)
        .unwrap_or(start);
    &bytes[start..end]
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16)?;
            let low = (pair[1] as char).to_digit(16)?;
            Some(((high << 4) | low) as u8)
        })
        .collect()
}

#[cfg(test)]
pub(crate) async fn with_test_anchor_dir<F, T>(anchor_dir: PathBuf, future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    TEST_ANCHOR_DIR.scope(anchor_dir, future).await
}

#[cfg(test)]
pub(crate) async fn with_test_keyring<F, T>(keyring: Option<AuditIntegrityKeyring>, future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    TEST_KEYRING.scope(keyring, future).await
}

#[cfg(test)]
pub(crate) async fn with_test_anchor_write_failure<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    with_test_anchor_write_failure_after(1, future).await
}

#[cfg(test)]
pub(crate) async fn with_test_anchor_write_failure_after<F, T>(writes: usize, future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    TEST_FAIL_AFTER_ANCHOR_WRITES
        .scope(std::cell::Cell::new(writes), future)
        .await
}

#[cfg(test)]
pub(crate) fn write_test_anchor_marker(
    scope: &str,
    identity: &str,
    state_path: &Path,
) -> anyhow::Result<()> {
    let path = resolved_anchor_path(scope, identity, state_path)?;
    atomic_write_anchor(&path, b"test-anchor-marker")
}

#[cfg(test)]
pub(crate) fn test_keyring(
    active_key_id: &str,
    active_material: &str,
    verify_only: &[(&str, &str)],
) -> AuditIntegrityKeyring {
    let mut entries = verify_only
        .iter()
        .map(|(id, material)| KeyDocument {
            id: (*id).to_string(),
            purpose: KEY_PURPOSE.to_string(),
            status: KeyStatus::VerifyOnly,
            key: (*material).to_string(),
        })
        .collect::<Vec<_>>();
    entries.push(KeyDocument {
        id: active_key_id.to_string(),
        purpose: KEY_PURPOSE.to_string(),
        status: KeyStatus::Active,
        key: active_material.to_string(),
    });
    keyring_from_document(
        KeyringDocument {
            active_key_id: active_key_id.to_string(),
            keys: entries,
        },
        false,
    )
    .expect("valid test audit integrity keyring")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keyring(entries: Vec<KeyDocument>, active_key_id: &str) -> AuditIntegrityKeyring {
        keyring_from_document(
            KeyringDocument {
                active_key_id: active_key_id.to_string(),
                keys: entries,
            },
            false,
        )
        .expect("valid test keyring")
    }

    fn key(id: &str, purpose: &str, status: KeyStatus, material: &str) -> KeyDocument {
        KeyDocument {
            id: id.to_string(),
            purpose: purpose.to_string(),
            status,
            key: material.to_string(),
        }
    }

    #[test]
    fn hmac_rejects_recomputed_public_hash_and_wrong_key() {
        let keys = keyring(
            vec![
                key(
                    "old",
                    KEY_PURPOSE,
                    KeyStatus::VerifyOnly,
                    "old-audit-integrity-secret-32-bytes",
                ),
                key(
                    "new",
                    KEY_PURPOSE,
                    KeyStatus::Active,
                    "new-audit-integrity-secret-32-bytes",
                ),
            ],
            "new",
        );
        let payload = b"protected-record";
        let mac = keys.sign_active(b"record", payload).expect("sign");
        keys.verify("new", b"record", payload, &mac)
            .expect("verify");
        assert!(keys
            .verify("new", b"record", b"rewritten-record", &mac)
            .is_err());
        assert!(keys.verify("old", b"record", payload, &mac).is_err());
        assert!(keys
            .verify(
                "new",
                b"record",
                payload,
                &format!("{MAC_PREFIX}{}", "00".repeat(32))
            )
            .is_err());
    }

    #[test]
    fn external_anchor_comparison_detects_deletion_rollback_and_substitution() {
        let keys = keyring(
            vec![key(
                "active",
                KEY_PURPOSE,
                KeyStatus::Active,
                "active-audit-integrity-secret-32bytes",
            )],
            "active",
        );
        let mut anchor = ExternalAnchor {
            version: ANCHOR_VERSION,
            scope: "protected-audit-ledger".to_string(),
            identity: "ledger-a".to_string(),
            generation: 7,
            digest: "root-7".to_string(),
            key_id: "active".to_string(),
            anchored_at_ms: 42,
            mac: String::new(),
        };
        anchor.mac = keys
            .sign_active(
                b"external-anchor",
                &anchor_mac_payload(&anchor).expect("payload"),
            )
            .expect("sign anchor");
        keys.verify(
            &anchor.key_id,
            b"external-anchor",
            &anchor_mac_payload(&anchor).expect("payload"),
            &anchor.mac,
        )
        .expect("verify anchor");
        validate_anchor_target(&anchor, 7, "root-7").expect("matching root");
        assert!(validate_anchor_target(&anchor, 6, "root-6").is_err());
        assert!(validate_anchor_target(&anchor, 7, "substituted").is_err());

        anchor.generation = 6;
        assert!(keys
            .verify(
                &anchor.key_id,
                b"external-anchor",
                &anchor_mac_payload(&anchor).expect("mutated payload"),
                &anchor.mac,
            )
            .is_err());
    }

    #[test]
    fn rotation_verifies_old_segments_but_revoked_and_wrong_purpose_fail() {
        let rotating = keyring(
            vec![
                key(
                    "old",
                    KEY_PURPOSE,
                    KeyStatus::VerifyOnly,
                    "old-audit-integrity-secret-32-bytes",
                ),
                key(
                    "new",
                    KEY_PURPOSE,
                    KeyStatus::Active,
                    "new-audit-integrity-secret-32-bytes",
                ),
            ],
            "new",
        );
        let old_mac = rotating
            .sign_with_key("new", b"record", b"new")
            .expect("active sign");
        rotating
            .verify("new", b"record", b"new", &old_mac)
            .expect("new verify");

        let revoked = keyring(
            vec![
                key(
                    "old",
                    KEY_PURPOSE,
                    KeyStatus::Revoked,
                    "old-audit-integrity-secret-32-bytes",
                ),
                key(
                    "new",
                    KEY_PURPOSE,
                    KeyStatus::Active,
                    "new-audit-integrity-secret-32-bytes",
                ),
            ],
            "new",
        );
        assert!(revoked.verify("old", b"record", b"old", &old_mac).is_err());

        let wrong = keyring(
            vec![
                key(
                    "wrong",
                    "predicate_evidence",
                    KeyStatus::VerifyOnly,
                    "wrong-purpose-secret-material-32bytes",
                ),
                key(
                    "new",
                    KEY_PURPOSE,
                    KeyStatus::Active,
                    "new-audit-integrity-secret-32-bytes",
                ),
            ],
            "new",
        );
        assert!(wrong.verify("wrong", b"record", b"old", &old_mac).is_err());
    }

    #[test]
    fn keyring_rejects_duplicate_material_and_multiple_active_keys() {
        let duplicate_material = keyring_from_document(
            KeyringDocument {
                active_key_id: "new".to_string(),
                keys: vec![
                    key(
                        "old",
                        KEY_PURPOSE,
                        KeyStatus::VerifyOnly,
                        "shared-audit-integrity-secret-32-bytes",
                    ),
                    key(
                        "new",
                        KEY_PURPOSE,
                        KeyStatus::Active,
                        "shared-audit-integrity-secret-32-bytes",
                    ),
                ],
            },
            false,
        )
        .expect_err("duplicate material must fail");
        assert!(duplicate_material.to_string().contains("duplicated"));

        let multiple_active = keyring_from_document(
            KeyringDocument {
                active_key_id: "new".to_string(),
                keys: vec![
                    key(
                        "old",
                        KEY_PURPOSE,
                        KeyStatus::Active,
                        "old-audit-integrity-secret-32-bytes",
                    ),
                    key(
                        "new",
                        KEY_PURPOSE,
                        KeyStatus::Active,
                        "new-audit-integrity-secret-32-bytes",
                    ),
                ],
            },
            false,
        )
        .expect_err("multiple active keys must fail");
        assert!(multiple_active
            .to_string()
            .contains("only active_key_id may have active status"));
    }

    #[test]
    fn anchor_directory_is_compared_with_the_configured_state_root() {
        let state_root = PathBuf::from("state-root");
        let leaf_parent = state_root.join("data").join("audit");
        let sibling_anchor = state_root.join("anchors");
        assert!(ensure_anchor_outside_state_roots(
            &sibling_anchor,
            &[leaf_parent, state_root.clone()],
        )
        .is_err());
        assert!(ensure_anchor_outside_state_roots(
            &PathBuf::from("independent-anchors"),
            &[state_root],
        )
        .is_ok());
    }

    #[tokio::test]
    async fn startup_rejects_anchor_nested_under_configured_state_root() {
        let root = tempfile::tempdir().expect("temporary root");
        let state_root = root.path().join("state");
        std::fs::create_dir(&state_root).expect("state directory");
        let anchor_dir = state_root.join("anchors");
        let authority = test_keyring("active", "startup-audit-integrity-secret-32-bytes", &[]);

        with_test_keyring(
            Some(authority),
            with_test_anchor_dir(anchor_dir, async {
                let error = validate_configuration(Some(&state_root))
                    .expect_err("nested external anchor must fail startup validation");
                assert!(error.to_string().contains("outside"));
            }),
        )
        .await;
    }

    #[test]
    fn canonical_state_file_identity_normalizes_spelling_and_separates_roots() {
        let first = tempfile::tempdir().expect("first root");
        let second = tempfile::tempdir().expect("second root");
        std::fs::create_dir(first.path().join("child")).expect("first child");
        let direct = first.path().join("ledger.jsonl");
        let equivalent = first.path().join("child").join("..").join("ledger.jsonl");
        assert_eq!(
            canonical_state_file_identity(&direct).expect("direct identity"),
            canonical_state_file_identity(&equivalent).expect("equivalent identity")
        );
        assert_ne!(
            canonical_state_file_identity(&direct).expect("first identity"),
            canonical_state_file_identity(&second.path().join("ledger.jsonl"))
                .expect("second identity")
        );

        std::fs::write(&direct, b"state").expect("state file");
        let alternate_case = first.path().join("LEDGER.JSONL");
        if std::fs::metadata(&alternate_case).is_ok() {
            let stored_identity =
                canonical_state_file_identity(&direct).expect("stored-case identity");
            assert_eq!(
                stored_identity,
                canonical_state_file_identity(&alternate_case).expect("alternate-case identity")
            );
            #[cfg(windows)]
            {
                std::fs::remove_file(&direct).expect("remove aliased state file");
                assert_eq!(
                    stored_identity,
                    canonical_state_file_identity(&alternate_case)
                        .expect("deleted alternate-case identity")
                );
            }
        }

        #[cfg(unix)]
        {
            use std::ffi::OsString;
            use std::os::unix::ffi::OsStringExt;
            use std::os::unix::fs::symlink;

            let alias_root = tempfile::tempdir().expect("alias root");
            let directory_alias = alias_root.path().join("state-alias");
            symlink(first.path(), &directory_alias).expect("directory symlink");
            assert_eq!(
                canonical_state_file_identity(&direct).expect("direct identity"),
                canonical_state_file_identity(&directory_alias.join("ledger.jsonl"))
                    .expect("directory alias identity")
            );

            let file_alias = alias_root.path().join("state-file-alias");
            symlink(&direct, &file_alias).expect("state file symlink");
            assert!(canonical_state_file_identity(&file_alias).is_err());

            let invalid_one = alias_root.path().join(OsString::from_vec(vec![b'a', 0xff]));
            let invalid_two = alias_root.path().join(OsString::from_vec(vec![b'a', 0xfe]));
            std::fs::create_dir(&invalid_one).expect("first invalid UTF-8 directory");
            std::fs::create_dir(&invalid_two).expect("second invalid UTF-8 directory");
            assert_ne!(
                canonical_state_file_identity(&invalid_one.join("ledger.jsonl"))
                    .expect("first lossless identity"),
                canonical_state_file_identity(&invalid_two.join("ledger.jsonl"))
                    .expect("second lossless identity")
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn anchor_storage_requires_owner_only_real_files_and_directories() {
        use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};

        let root = tempfile::tempdir().expect("temporary root");
        let directory = root.path().join("anchors");
        let canonical = prepare_anchor_dir(&directory).expect("secure anchor directory");
        assert_eq!(
            std::fs::metadata(&canonical)
                .expect("directory metadata")
                .mode()
                & 0o777,
            0o700
        );

        let anchor = canonical.join("root.anchor.json");
        atomic_write_anchor(&anchor, b"authenticated-anchor").expect("write anchor");
        assert_eq!(
            read_anchor_file(&anchor).expect("read anchor"),
            Some(b"authenticated-anchor".to_vec())
        );

        std::fs::set_permissions(&anchor, std::fs::Permissions::from_mode(0o644))
            .expect("loosen anchor mode");
        assert!(read_anchor_file(&anchor).is_err());
        std::fs::set_permissions(&anchor, std::fs::Permissions::from_mode(0o600))
            .expect("restore anchor mode");

        let hardlink = canonical.join("anchor-hardlink.json");
        std::fs::hard_link(&anchor, &hardlink).expect("create hard link");
        assert!(read_anchor_file(&anchor).is_err());
        std::fs::remove_file(&hardlink).expect("remove hard link");

        let symlink_path = canonical.join("anchor-symlink.json");
        symlink(&anchor, &symlink_path).expect("create symlink");
        assert!(read_anchor_file(&symlink_path).is_err());

        let unsafe_directory = root.path().join("unsafe-anchors");
        std::fs::create_dir(&unsafe_directory).expect("create unsafe directory");
        std::fs::set_permissions(&unsafe_directory, std::fs::Permissions::from_mode(0o755))
            .expect("loosen directory mode");
        assert!(prepare_anchor_dir(&unsafe_directory).is_err());

        let directory_symlink = root.path().join("anchor-directory-symlink");
        symlink(&canonical, &directory_symlink).expect("create directory symlink");
        assert!(prepare_anchor_dir(&directory_symlink).is_err());
    }

    #[tokio::test]
    async fn anchor_replacement_advances_and_blocks_legacy_state() {
        let root = tempfile::tempdir().expect("temporary root");
        let path = root.path().join("replaceable.anchor.json");
        atomic_write_anchor(&path, b"first-head").expect("write first anchor");
        assert!(ensure_anchor_path_absent(&path).await.is_err());

        atomic_write_anchor(&path, b"second-head").expect("replace anchor");
        assert_eq!(
            read_anchor_file(&path).expect("read replaced anchor"),
            Some(b"second-head".to_vec())
        );
    }
}
