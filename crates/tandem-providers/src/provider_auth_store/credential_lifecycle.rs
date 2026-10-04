//! Revisions in the existing credential indexes, under the existing file lock.
//! These are current local facts, not an account grant or an external rollback anchor.
use super::*;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCredentialKind {
    ApiKey,
    Credential,
}

impl ProviderCredentialKind {
    fn index(self, dir: &Path) -> PathBuf {
        dir.join(match self {
            Self::ApiKey => "provider_auth_index.json",
            Self::Credential => "provider_credentials_index.json",
        })
    }

    fn fallback(self, dir: &Path) -> PathBuf {
        dir.join(match self {
            Self::ApiKey => "provider_auth_fallback.json",
            Self::Credential => "provider_credentials_fallback.json",
        })
    }
}

/// Non-secret snapshot. Callers must re-read it at admission and separately prove
/// account ownership/sharing. Material revision changes even on same-account refresh.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCredentialRevision {
    pub authorization_revision: String,
    pub material_revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum State {
    Pending,
    Active,
    Absent,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema_version: u32,
    revision: ProviderCredentialRevision,
    state: State,
    material_sha256: Option<String>,
    keychain: bool,
}

fn strict_json(path: &Path) -> anyhow::Result<Value> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|_| anyhow::anyhow!("invalid credential store JSON"))?;
            anyhow::ensure!(value.is_object(), "invalid credential store object");
            Ok(value)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn save_index(path: &Path, ids: &HashSet<String>) -> anyhow::Result<()> {
    let mut value = strict_json(path)?;
    let mut sorted: Vec<_> = ids.iter().filter(|id| !id.trim().is_empty()).collect();
    sorted.sort();
    value["providers"] = serde_json::to_value(sorted)?;
    write_secure_json(&path.to_path_buf(), &value)
}

fn record(index: &Value, id: &str) -> anyhow::Result<Option<Record>> {
    let Some(bindings) = index.get("credential_bindings") else {
        return Ok(None);
    };
    anyhow::ensure!(bindings.is_object(), "invalid credential bindings");
    let Some(value) = bindings.get(id) else {
        return Ok(None);
    };
    let record: Record = serde_json::from_value(value.clone())
        .map_err(|_| anyhow::anyhow!("invalid credential revision"))?;
    anyhow::ensure!(
        record.schema_version == 1,
        "unsupported credential revision schema"
    );
    for revision in [
        &record.revision.authorization_revision,
        &record.revision.material_revision,
    ] {
        uuid::Uuid::parse_str(revision)
            .map_err(|_| anyhow::anyhow!("invalid credential revision ID"))?;
    }
    Ok(Some(record))
}

fn set_record(index: &mut Value, id: &str, record: &Record) -> anyhow::Result<()> {
    if index.get("credential_bindings").is_none() {
        index["credential_bindings"] = json!({});
    }
    anyhow::ensure!(
        index["credential_bindings"].is_object(),
        "invalid credential bindings"
    );
    index["credential_bindings"][id] = serde_json::to_value(record)?;
    Ok(())
}

fn material_hash(kind: ProviderCredentialKind, material: &Value) -> anyhow::Result<String> {
    let mut hash = Sha256::new();
    hash.update(b"tandem.credential-material.v1\0");
    hash.update(serde_json::to_vec(&(kind, material))?);
    Ok(format!("{:x}", hash.finalize()))
}

fn normalize_material(
    kind: ProviderCredentialKind,
    id: &str,
    value: Value,
) -> anyhow::Result<Value> {
    match kind {
        ProviderCredentialKind::ApiKey => {
            let token = value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("invalid credential material"))?
                .trim();
            anyhow::ensure!(!token.is_empty(), "empty credential material");
            Ok(json!(token))
        }
        ProviderCredentialKind::Credential => {
            let credential: ProviderCredential = serde_json::from_value(value)
                .map_err(|_| anyhow::anyhow!("invalid credential material"))?;
            Ok(serde_json::to_value(normalize_provider_credential(
                credential_with_provider_id(credential, id.to_string()),
            )?)?)
        }
    }
}

// Match the existing reader's keychain precedence. A recorded keychain binding
// cannot fall back to a file merely because its keychain became unavailable.
fn current_material(
    dir: &Path,
    kind: ProviderCredentialKind,
    id: &str,
    consult_keychain: bool,
    require_keychain: bool,
) -> anyhow::Result<(Option<Value>, bool)> {
    if consult_keychain {
        let entry = match kind {
            ProviderCredentialKind::ApiKey => keyring_entry(id),
            ProviderCredentialKind::Credential => credential_keyring_entry(id),
        };
        match entry.map(|entry| entry.get_password()) {
            Some(Ok(secret)) => {
                let value = match kind {
                    ProviderCredentialKind::ApiKey => json!(secret),
                    ProviderCredentialKind::Credential => serde_json::from_str(&secret)
                        .map_err(|_| anyhow::anyhow!("invalid keychain credential"))?,
                };
                return normalize_material(kind, id, value).map(|value| (Some(value), true));
            }
            Some(Err(keyring::Error::NoEntry)) => {}
            _ if require_keychain => anyhow::bail!("credential keychain unavailable"),
            _ => {}
        }
    }
    strict_json(&kind.fallback(dir))?
        .get(id)
        .cloned()
        .map(|value| normalize_material(kind, id, value))
        .transpose()
        .map(|value| (value, false))
}

pub(super) struct Mutation {
    dir: PathBuf,
    kind: ProviderCredentialKind,
    id: String,
    record: Record,
    consult_keychain: bool,
}

impl Mutation {
    // Caller holds ProviderCredentialMutationFileLock for the entire mutation.
    pub(super) fn begin(
        dir: &Path,
        kind: ProviderCredentialKind,
        id: &str,
        expected: Option<Value>,
        refresh: bool,
        consult_keychain: bool,
    ) -> anyhow::Result<Self> {
        let id = normalize_provider_id(id);
        let mut index = strict_json(&kind.index(dir))?;
        // Legacy compatibility readers tolerate corrupt JSON. A mutation must
        // never turn that tolerance into loss of another account's credentials.
        strict_json(&kind.fallback(dir))?;
        let previous = record(&index, &id)?;
        let keychain = previous.as_ref().is_some_and(|record| record.keychain);
        let mut authorization_revision = uuid::Uuid::new_v4().to_string();
        // Only an explicit refresh of a current, tracked, same nonempty account
        // can retain authorization. Untracked credentials acquire a new revision.
        if refresh {
            if let Some(previous) = &previous {
                let (current, current_keychain) =
                    current_material(dir, kind, &id, consult_keychain, keychain)?;
                let current_hash = current
                    .as_ref()
                    .map(|value| material_hash(kind, value))
                    .transpose()?;
                if previous.state == State::Active
                    && (!previous.keychain || current_keychain)
                    && index
                        .get("providers")
                        .and_then(Value::as_array)
                        .is_some_and(|providers| {
                            providers
                                .iter()
                                .any(|value| value.as_str() == Some(id.as_str()))
                        })
                    && current_hash == previous.material_sha256
                    && same_oauth_account(current.as_ref(), expected.as_ref())
                {
                    authorization_revision = previous.revision.authorization_revision.clone();
                }
            }
        }
        let material_sha256 = expected
            .as_ref()
            .map(|value| material_hash(kind, value))
            .transpose()?;
        let record = Record {
            schema_version: 1,
            revision: ProviderCredentialRevision {
                authorization_revision,
                material_revision: uuid::Uuid::new_v4().to_string(),
            },
            state: State::Pending,
            material_sha256,
            keychain,
        };
        set_record(&mut index, &id, &record)?;
        write_secure_json(&kind.index(dir), &index)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            kind,
            id,
            record,
            consult_keychain,
        })
    }

    pub(super) fn finish(mut self, backend: Option<ProviderAuthBackend>) -> anyhow::Result<()> {
        let keychain = backend
            .map(|backend| backend == ProviderAuthBackend::Keychain)
            .unwrap_or(self.record.keychain);
        let (current, current_keychain) = current_material(
            &self.dir,
            self.kind,
            &self.id,
            self.consult_keychain,
            keychain,
        )?;
        anyhow::ensure!(
            !keychain || current.is_none() || current_keychain,
            "credential keychain material missing"
        );
        let actual = current
            .as_ref()
            .map(|value| material_hash(self.kind, value))
            .transpose()?;
        anyhow::ensure!(
            actual == self.record.material_sha256,
            "credential persistence verification failed"
        );
        let mut index = strict_json(&self.kind.index(&self.dir))?;
        let pending = record(&index, &self.id)?
            .ok_or_else(|| anyhow::anyhow!("credential revision disappeared"))?;
        anyhow::ensure!(
            pending.state == State::Pending && pending.revision == self.record.revision,
            "credential revision changed during persistence"
        );
        self.record.state = if actual.is_some() {
            State::Active
        } else {
            State::Absent
        };
        self.record.keychain = keychain;
        set_record(&mut index, &self.id, &self.record)?;
        write_secure_json(&self.kind.index(&self.dir), &index)
    }
}

fn same_oauth_account(left: Option<&Value>, right: Option<&Value>) -> bool {
    let oauth = |value: Option<&Value>| {
        value
            .cloned()
            .and_then(|value| serde_json::from_value::<ProviderCredential>(value).ok())
            .and_then(|value| match value {
                ProviderCredential::OAuth(oauth) => Some(oauth),
                _ => None,
            })
    };
    match (oauth(left), oauth(right)) {
        (Some(left), Some(right)) => {
            left.provider_id == right.provider_id
                && left.managed_by == right.managed_by
                && left
                    .account_id
                    .as_deref()
                    .is_some_and(|id| !id.trim().is_empty())
                && left.account_id == right.account_id
        }
        _ => false,
    }
}

fn active_material(
    dir: &Path,
    kind: ProviderCredentialKind,
    id: &str,
    consult_keychain: bool,
) -> anyhow::Result<(ProviderCredentialRevision, Value)> {
    let _guard = ProviderCredentialMutationFileLock::acquire_blocking(dir)?;
    let id = normalize_provider_id(id);
    let index = strict_json(&kind.index(dir))?;
    let record =
        record(&index, &id)?.ok_or_else(|| anyhow::anyhow!("credential revision is untracked"))?;
    anyhow::ensure!(
        record.state == State::Active,
        "credential revision is not active"
    );
    anyhow::ensure!(
        index
            .get("providers")
            .and_then(Value::as_array)
            .is_some_and(|providers| providers.iter().any(|value| value.as_str() == Some(&id))),
        "credential index does not contain active binding"
    );
    let (current, current_keychain) =
        current_material(dir, kind, &id, consult_keychain, record.keychain)?;
    anyhow::ensure!(
        !record.keychain || current_keychain,
        "credential keychain material missing"
    );
    let actual = current
        .as_ref()
        .map(|value| material_hash(kind, value))
        .transpose()?;
    anyhow::ensure!(
        actual.is_some() && actual == record.material_sha256,
        "credential revision material mismatch"
    );
    Ok((
        record.revision,
        current.expect("active material was checked"),
    ))
}

fn active_revision(
    dir: &Path,
    kind: ProviderCredentialKind,
    id: &str,
    consult_keychain: bool,
) -> anyhow::Result<ProviderCredentialRevision> {
    active_material(dir, kind, id, consult_keychain).map(|(revision, _)| revision)
}

pub(crate) fn match_runtime_credential_revision(
    dir: &Path,
    runtime: &crate::ProviderRuntimeBinding,
    kind: ProviderCredentialKind,
    location: crate::ProviderCredentialLocation,
) -> anyhow::Result<ProviderCredentialRevision> {
    let (id, consult_keychain) = match location {
        crate::ProviderCredentialLocation::HostService => (runtime.provider_id.clone(), true),
        crate::ProviderCredentialLocation::TenantService => (
            tenant_scoped_provider_id(&runtime.tenant_context, &runtime.provider_id),
            kind == ProviderCredentialKind::Credential,
        ),
    };
    let (revision, material) = active_material(dir, kind, &id, consult_keychain)?;
    let token = match kind {
        ProviderCredentialKind::ApiKey => material
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("invalid API-key material"))?
            .to_string(),
        ProviderCredentialKind::Credential => {
            let credential: ProviderCredential = serde_json::from_value(material)
                .map_err(|_| anyhow::anyhow!("invalid typed credential material"))?;
            credential
                .runtime_bearer_token()
                .ok_or_else(|| anyhow::anyhow!("credential has no runtime token"))?
                .to_string()
        }
    };
    // Reuse the real request-header fingerprint implementation. The dummy URL
    // is only used to construct a Request; there is no client, DNS or network.
    let (bearer, api_key) = if runtime.protocol == crate::ProviderProtocol::Anthropic {
        (None, Some(token.as_str()))
    } else {
        (Some(token.as_str()), None)
    };
    let stored = crate::runtime_binding::transport(
        "https://credential-binding.invalid/",
        runtime.protocol,
        bearer,
        api_key,
        runtime.credential_source,
    )?;
    anyhow::ensure!(
        stored.credential_sha256 == runtime.credential_sha256,
        "loaded runtime credential differs from selected persisted revision"
    );
    Ok(revision)
}

pub fn provider_credential_revision(
    kind: ProviderCredentialKind,
    provider_id: &str,
) -> anyhow::Result<ProviderCredentialRevision> {
    active_revision(&provider_auth_security_dir(), kind, provider_id, true)
}

pub fn provider_credential_revision_for_tenant(
    tenant: &TenantContext,
    kind: ProviderCredentialKind,
    provider_id: &str,
) -> anyhow::Result<ProviderCredentialRevision> {
    provider_credential_revision(kind, &tenant_scoped_provider_id(tenant, provider_id))
}

pub fn provider_credential_revision_for_tenant_in_dir(
    dir: &Path,
    tenant: &TenantContext,
    kind: ProviderCredentialKind,
    provider_id: &str,
) -> anyhow::Result<ProviderCredentialRevision> {
    active_revision(
        dir,
        kind,
        &tenant_scoped_provider_id(tenant, provider_id),
        kind == ProviderCredentialKind::Credential,
    )
}

#[cfg(test)]
#[path = "credential_lifecycle_tests.rs"]
mod tests;
