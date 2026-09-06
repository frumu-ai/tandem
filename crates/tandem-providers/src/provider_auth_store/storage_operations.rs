pub fn load_provider_auth() -> HashMap<String, String> {
    let fallback = load_fallback_map();
    let mut known = load_provider_index();
    known.extend(fallback.keys().cloned());
    let mut out = HashMap::new();

    for provider_id in known {
        if let Some(entry) = keyring_entry(&provider_id) {
            if let Ok(secret) = entry.get_password() {
                let trimmed = secret.trim();
                if !trimmed.is_empty() {
                    out.insert(provider_id.clone(), trimmed.to_string());
                    continue;
                }
            }
        }
        if let Some(secret) = fallback.get(&provider_id) {
            let trimmed = secret.trim();
            if !trimmed.is_empty() {
                out.insert(provider_id.clone(), trimmed.to_string());
            }
        }
    }

    out
}

pub fn set_provider_auth(provider_id: &str, token: &str) -> anyhow::Result<ProviderAuthBackend> {
    let _file_guard =
        ProviderCredentialMutationFileLock::acquire_blocking(&provider_auth_security_dir())?;
    let id = normalize_provider_id(provider_id);
    let secret = token.trim().to_string();
    if id.is_empty() {
        anyhow::bail!("provider id cannot be empty");
    }
    if secret.is_empty() {
        anyhow::bail!("provider token cannot be empty");
    }

    let mutation = Mutation::begin(
        &provider_auth_security_dir(),
        ProviderCredentialKind::ApiKey,
        &id,
        Some(json!(secret)),
        false,
        true,
    )?;
    let mut known = load_provider_index();
    known.insert(id.clone());

    if let Some(entry) = keyring_entry(&id) {
        if entry.set_password(&secret).is_ok() {
            let mut fallback = load_fallback_map();
            fallback.remove(&id);
            save_fallback_map(&fallback)?;
            save_provider_index(&known)?;
            mutation.finish(Some(ProviderAuthBackend::Keychain))?;
            return Ok(ProviderAuthBackend::Keychain);
        }
    }

    let mut fallback = load_fallback_map();
    fallback.insert(id.clone(), secret);
    save_fallback_map(&fallback)?;
    save_provider_index(&known)?;
    mutation.finish(Some(ProviderAuthBackend::File))?;
    Ok(ProviderAuthBackend::File)
}

pub fn delete_provider_auth(provider_id: &str) -> anyhow::Result<bool> {
    let _file_guard =
        ProviderCredentialMutationFileLock::acquire_blocking(&provider_auth_security_dir())?;
    let id = normalize_provider_id(provider_id);
    if id.is_empty() {
        return Ok(false);
    }

    let mutation = Mutation::begin(
        &provider_auth_security_dir(),
        ProviderCredentialKind::ApiKey,
        &id,
        None,
        false,
        true,
    )?;
    let mut removed = false;

    if let Some(entry) = keyring_entry(&id) {
        // Ignore unsupported backend errors; we still clear file fallback/index below.
        if entry.delete_password().is_ok() {
            removed = true;
        }
    }

    let mut fallback = load_fallback_map();
    if fallback.remove(&id).is_some() {
        removed = true;
    }
    save_fallback_map(&fallback)?;

    let mut known = load_provider_index();
    if known.remove(&id) {
        removed = true;
    }
    save_provider_index(&known)?;

    mutation.finish(None)?;
    Ok(removed)
}

pub fn load_provider_auth_for_tenant(tenant_context: &TenantContext) -> HashMap<String, String> {
    load_provider_auth()
        .into_iter()
        .filter_map(|(provider_id, token)| {
            strip_tenant_scoped_provider_id(tenant_context, &provider_id)
                .map(|stripped| (stripped, token))
        })
        .collect()
}

pub fn load_provider_auth_for_tenant_in_dir(
    security_dir: &Path,
    tenant_context: &TenantContext,
) -> HashMap<String, String> {
    let fallback = load_fallback_map_from_dir(security_dir);
    let mut known = load_provider_index_from_dir(security_dir);
    known.extend(fallback.keys().cloned());
    known
        .into_iter()
        .filter_map(|provider_id| {
            let token = fallback.get(&provider_id)?;
            strip_tenant_scoped_provider_id(tenant_context, &provider_id)
                .map(|stripped| (stripped, token.clone()))
        })
        .collect()
}

pub fn set_provider_auth_for_tenant(
    tenant_context: &TenantContext,
    provider_id: &str,
    token: &str,
) -> anyhow::Result<ProviderAuthBackend> {
    let scoped_provider_id = tenant_scoped_provider_id(tenant_context, provider_id);
    set_provider_auth(&scoped_provider_id, token)
}

fn set_provider_auth_for_tenant_in_dir_unlocked(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
    token: &str,
) -> anyhow::Result<ProviderAuthBackend> {
    let scoped_provider_id = tenant_scoped_provider_id(tenant_context, provider_id);
    let id = normalize_provider_id(&scoped_provider_id);
    let secret = token.trim().to_string();
    if id.is_empty() {
        anyhow::bail!("provider id cannot be empty");
    }
    if secret.is_empty() {
        anyhow::bail!("provider token cannot be empty");
    }
    let mutation = Mutation::begin(
        security_dir,
        ProviderCredentialKind::ApiKey,
        &id,
        Some(json!(secret)),
        false,
        false,
    )?;
    let mut fallback = load_fallback_map_from_dir(security_dir);
    fallback.insert(id.clone(), secret);
    save_fallback_map_to_dir(security_dir, &fallback)?;
    let mut known = load_provider_index_from_dir(security_dir);
    known.insert(id);
    save_provider_index_to_dir(security_dir, &known)?;
    mutation.finish(Some(ProviderAuthBackend::File))?;
    Ok(ProviderAuthBackend::File)
}

pub fn set_provider_auth_for_tenant_in_dir(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
    token: &str,
) -> anyhow::Result<ProviderAuthBackend> {
    let _file_guard = ProviderCredentialMutationFileLock::acquire_blocking(security_dir)?;
    set_provider_auth_for_tenant_in_dir_unlocked(security_dir, tenant_context, provider_id, token)
}

pub fn delete_provider_auth_for_tenant(
    tenant_context: &TenantContext,
    provider_id: &str,
) -> anyhow::Result<bool> {
    let scoped_provider_id = tenant_scoped_provider_id(tenant_context, provider_id);
    delete_provider_auth(&scoped_provider_id)
}

fn delete_provider_auth_for_tenant_in_dir_unlocked(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
) -> anyhow::Result<bool> {
    let scoped_provider_id =
        normalize_provider_id(&tenant_scoped_provider_id(tenant_context, provider_id));
    if scoped_provider_id.is_empty() {
        return Ok(false);
    }
    let mutation = Mutation::begin(
        security_dir,
        ProviderCredentialKind::ApiKey,
        &scoped_provider_id,
        None,
        false,
        false,
    )?;
    let mut removed = false;
    let mut fallback = load_fallback_map_from_dir(security_dir);
    if fallback.remove(&scoped_provider_id).is_some() {
        removed = true;
    }
    save_fallback_map_to_dir(security_dir, &fallback)?;
    let mut known = load_provider_index_from_dir(security_dir);
    if known.remove(&scoped_provider_id) {
        removed = true;
    }
    save_provider_index_to_dir(security_dir, &known)?;
    mutation.finish(None)?;
    Ok(removed)
}

pub fn delete_provider_auth_for_tenant_in_dir(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
) -> anyhow::Result<bool> {
    let _file_guard = ProviderCredentialMutationFileLock::acquire_blocking(security_dir)?;
    delete_provider_auth_for_tenant_in_dir_unlocked(security_dir, tenant_context, provider_id)
}

pub fn load_provider_credentials() -> HashMap<String, ProviderCredential> {
    let fallback = load_credential_fallback_map();
    let mut known = load_provider_credentials_index();
    known.extend(fallback.keys().cloned());
    let mut out = HashMap::new();

    for provider_id in known {
        if let Some(credential) = load_provider_credential_from_keyring(&provider_id) {
            out.insert(provider_id.clone(), credential);
            continue;
        }

        if let Some(credential) = fallback.get(&provider_id) {
            out.insert(provider_id.clone(), credential.clone());
        }
    }

    out
}

pub fn load_provider_credentials_for_tenant(
    tenant_context: &TenantContext,
) -> HashMap<String, ProviderCredential> {
    load_provider_credentials()
        .into_iter()
        .filter_map(|(provider_id, credential)| {
            strip_tenant_scoped_provider_id(tenant_context, &provider_id).map(|stripped| {
                (
                    stripped.clone(),
                    credential_with_provider_id(credential, stripped),
                )
            })
        })
        .collect()
}

pub fn load_provider_credentials_for_tenant_in_dir(
    security_dir: &Path,
    tenant_context: &TenantContext,
) -> HashMap<String, ProviderCredential> {
    let fallback = load_credential_fallback_map_from_dir(security_dir);
    let mut known = load_provider_credentials_index_from_dir(security_dir);
    known.extend(fallback.keys().cloned());
    known
        .into_iter()
        .filter_map(|provider_id| {
            let credential = load_provider_credential_from_keyring(&provider_id)
                .or_else(|| fallback.get(&provider_id).cloned())?;
            strip_tenant_scoped_provider_id(tenant_context, &provider_id).map(|stripped| {
                (
                    stripped.clone(),
                    credential_with_provider_id(credential, stripped),
                )
            })
        })
        .collect()
}

/// Discover the tenant scopes that own a persisted OAuth credential for one
/// provider. Hosted scopes are reconstructed from the encoded credential keys;
/// actor identity is intentionally absent because credentials are owned by the
/// tenant/deployment boundary rather than an individual actor.
pub fn list_provider_oauth_tenant_contexts(provider_id: &str) -> Vec<TenantContext> {
    provider_oauth_tenant_contexts_from_credentials(load_provider_credentials(), provider_id)
}

pub fn list_provider_oauth_tenant_contexts_in_dir(
    security_dir: &Path,
    provider_id: &str,
) -> Vec<TenantContext> {
    let fallback = load_credential_fallback_map_from_dir(security_dir);
    let mut known = load_provider_credentials_index_from_dir(security_dir);
    known.extend(fallback.keys().cloned());
    let credentials = known
        .into_iter()
        .filter_map(|provider_id| {
            let credential = load_provider_credential_from_keyring(&provider_id)
                .or_else(|| fallback.get(&provider_id).cloned())?;
            Some((provider_id, credential))
        })
        .collect();
    provider_oauth_tenant_contexts_from_credentials(credentials, provider_id)
}

pub fn load_provider_oauth_credential(provider_id: &str) -> Option<OAuthProviderCredential> {
    match load_provider_credentials().remove(&normalize_provider_id(provider_id)) {
        Some(ProviderCredential::OAuth(credential)) => Some(credential),
        Some(ProviderCredential::ApiKey(_)) | None => None,
    }
}

pub fn load_provider_oauth_credential_in_dir(
    security_dir: &Path,
    provider_id: &str,
) -> Option<OAuthProviderCredential> {
    match load_credential_fallback_map_from_dir(security_dir)
        .remove(&normalize_provider_id(provider_id))
    {
        Some(ProviderCredential::OAuth(credential)) => Some(credential),
        Some(ProviderCredential::ApiKey(_)) | None => None,
    }
}

pub fn load_provider_oauth_credential_for_tenant(
    tenant_context: &TenantContext,
    provider_id: &str,
) -> Option<OAuthProviderCredential> {
    match load_provider_credentials_for_tenant(tenant_context)
        .remove(&normalize_provider_id(provider_id))
    {
        Some(ProviderCredential::OAuth(credential)) => Some(credential),
        Some(ProviderCredential::ApiKey(_)) | None => None,
    }
}

pub fn load_provider_oauth_credential_for_tenant_in_dir(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
) -> Option<OAuthProviderCredential> {
    match load_provider_credentials_for_tenant_in_dir(security_dir, tenant_context)
        .remove(&normalize_provider_id(provider_id))
    {
        Some(ProviderCredential::OAuth(credential)) => Some(credential),
        Some(ProviderCredential::ApiKey(_)) | None => None,
    }
}

fn set_provider_credential_unlocked(
    credential: ProviderCredential,
) -> anyhow::Result<ProviderAuthBackend> {
    let normalized = normalize_provider_credential(credential)?;
    let provider_id = normalized.provider_id().to_string();
    let serialized = serde_json::to_string(&normalized)?;

    let mutation = Mutation::begin(
        &provider_auth_security_dir(),
        ProviderCredentialKind::Credential,
        &provider_id,
        Some(serde_json::to_value(&normalized)?),
        false,
        true,
    )?;
    let mut known = load_provider_credentials_index();
    known.insert(provider_id.clone());

    if let Some(entry) = credential_keyring_entry(&provider_id) {
        if entry.set_password(&serialized).is_ok() {
            let mut fallback = load_credential_fallback_map();
            fallback.remove(&provider_id);
            save_credential_fallback_map(&fallback)?;
            save_provider_credentials_index(&known)?;
            mutation.finish(Some(ProviderAuthBackend::Keychain))?;
            return Ok(ProviderAuthBackend::Keychain);
        }
    }

    let mut fallback = load_credential_fallback_map();
    delay_provider_credential_rmw_for_test();
    fallback.insert(provider_id.clone(), normalized);
    save_credential_fallback_map(&fallback)?;
    save_provider_credentials_index(&known)?;
    mutation.finish(Some(ProviderAuthBackend::File))?;
    Ok(ProviderAuthBackend::File)
}

pub fn set_provider_credential(
    credential: ProviderCredential,
) -> anyhow::Result<ProviderAuthBackend> {
    let _file_guard =
        ProviderCredentialMutationFileLock::acquire_blocking(&provider_auth_security_dir())?;
    set_provider_credential_unlocked(credential)
}

pub fn set_provider_credential_for_tenant(
    tenant_context: &TenantContext,
    credential: ProviderCredential,
) -> anyhow::Result<ProviderAuthBackend> {
    let normalized = normalize_provider_credential(credential)?;
    let scoped_provider_id = tenant_scoped_provider_id(tenant_context, normalized.provider_id());
    set_provider_credential(credential_with_provider_id(normalized, scoped_provider_id))
}

pub fn set_provider_oauth_credential(
    provider_id: &str,
    credential: OAuthProviderCredential,
) -> anyhow::Result<ProviderAuthBackend> {
    let mut credential = credential;
    credential.provider_id = normalize_provider_id(provider_id);
    set_provider_credential(ProviderCredential::OAuth(credential))
}

fn persist_typed_material_in_dir(
    security_dir: &Path,
    provider_id: &str,
    credential: ProviderCredential,
) -> anyhow::Result<ProviderAuthBackend> {
    let mut fallback = load_credential_fallback_map_from_dir(security_dir);
    delay_provider_credential_rmw_for_test();
    let existing_keychain =
        credential_keyring_entry(provider_id).filter(|entry| entry.get_password().is_ok());
    let backend = if let Some(entry) = existing_keychain {
        entry.set_password(&serde_json::to_string(&credential)?)?;
        fallback.remove(provider_id);
        ProviderAuthBackend::Keychain
    } else {
        fallback.insert(provider_id.to_string(), credential);
        ProviderAuthBackend::File
    };
    save_credential_fallback_map_to_dir(security_dir, &fallback)?;
    Ok(backend)
}

fn set_provider_oauth_credential_in_dir_unlocked(
    security_dir: &Path,
    provider_id: &str,
    credential: OAuthProviderCredential,
) -> anyhow::Result<ProviderAuthBackend> {
    let mut credential = credential;
    credential.provider_id = normalize_provider_id(provider_id);
    let normalized = normalize_provider_credential(ProviderCredential::OAuth(credential))?;
    let provider_id = normalized.provider_id().to_string();
    let mutation = Mutation::begin(
        security_dir,
        ProviderCredentialKind::Credential,
        &provider_id,
        Some(serde_json::to_value(&normalized)?),
        false,
        true,
    )?;
    let backend = persist_typed_material_in_dir(security_dir, &provider_id, normalized)?;
    let mut known = load_provider_credentials_index_from_dir(security_dir);
    known.insert(provider_id);
    save_provider_credentials_index_to_dir(security_dir, &known)?;
    mutation.finish(Some(backend))?;
    Ok(backend)
}

pub fn set_provider_oauth_credential_in_dir(
    security_dir: &Path,
    provider_id: &str,
    credential: OAuthProviderCredential,
) -> anyhow::Result<ProviderAuthBackend> {
    let _file_guard = ProviderCredentialMutationFileLock::acquire_blocking(security_dir)?;
    set_provider_oauth_credential_in_dir_unlocked(security_dir, provider_id, credential)
}

pub fn set_provider_oauth_credential_for_tenant(
    tenant_context: &TenantContext,
    provider_id: &str,
    credential: OAuthProviderCredential,
) -> anyhow::Result<ProviderAuthBackend> {
    let mut credential = credential;
    credential.provider_id = normalize_provider_id(provider_id);
    set_provider_credential_for_tenant(tenant_context, ProviderCredential::OAuth(credential))
}

pub async fn set_provider_oauth_credential_for_tenant_serialized(
    tenant_context: &TenantContext,
    provider_id: &str,
    credential: OAuthProviderCredential,
) -> anyhow::Result<ProviderAuthBackend> {
    let _guard = provider_credential_mutation_lock().lock().await;
    let _file_guard =
        ProviderCredentialMutationFileLock::acquire(&provider_auth_security_dir()).await?;
    let mut credential = credential;
    credential.provider_id = normalize_provider_id(provider_id);
    let normalized = normalize_provider_credential(ProviderCredential::OAuth(credential))?;
    let scoped_provider_id = tenant_scoped_provider_id(tenant_context, normalized.provider_id());
    set_provider_credential_unlocked(credential_with_provider_id(normalized, scoped_provider_id))
}

fn set_provider_oauth_credential_for_tenant_in_dir_unlocked(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
    credential: OAuthProviderCredential,
) -> anyhow::Result<ProviderAuthBackend> {
    let mut credential = credential;
    credential.provider_id = normalize_provider_id(provider_id);
    let normalized = normalize_provider_credential(ProviderCredential::OAuth(credential))?;
    let scoped_provider_id = tenant_scoped_provider_id(tenant_context, normalized.provider_id());
    let normalized = credential_with_provider_id(normalized, scoped_provider_id.clone());
    let mutation = Mutation::begin(
        security_dir,
        ProviderCredentialKind::Credential,
        &scoped_provider_id,
        Some(serde_json::to_value(&normalized)?),
        false,
        true,
    )?;
    let backend = persist_typed_material_in_dir(security_dir, &scoped_provider_id, normalized)?;
    let mut known = load_provider_credentials_index_from_dir(security_dir);
    known.insert(normalize_provider_id(&scoped_provider_id));
    save_provider_credentials_index_to_dir(security_dir, &known)?;
    mutation.finish(Some(backend))?;
    Ok(backend)
}

pub fn set_provider_oauth_credential_for_tenant_in_dir(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
    credential: OAuthProviderCredential,
) -> anyhow::Result<ProviderAuthBackend> {
    let _file_guard = ProviderCredentialMutationFileLock::acquire_blocking(security_dir)?;
    set_provider_oauth_credential_for_tenant_in_dir_unlocked(
        security_dir,
        tenant_context,
        provider_id,
        credential,
    )
}

pub async fn set_provider_oauth_credential_for_tenant_in_dir_serialized(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
    credential: OAuthProviderCredential,
) -> anyhow::Result<ProviderAuthBackend> {
    let _guard = provider_credential_mutation_lock().lock().await;
    let _file_guard = ProviderCredentialMutationFileLock::acquire(security_dir).await?;
    set_provider_oauth_credential_for_tenant_in_dir_unlocked(
        security_dir,
        tenant_context,
        provider_id,
        credential,
    )
}

fn compare_and_set_provider_oauth_credential_for_tenant_in_dir_unlocked(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
    expected: Option<&OAuthProviderCredential>,
    replacement: Option<OAuthProviderCredential>,
    refresh: bool,
) -> anyhow::Result<bool> {
    let provider_id = normalize_provider_id(provider_id);
    if provider_id.is_empty() {
        anyhow::bail!("provider id cannot be empty");
    }
    let scoped_provider_id = tenant_scoped_provider_id(tenant_context, &provider_id);
    let mut fallback = load_credential_fallback_map_from_dir(security_dir);
    let keyring_entry = credential_keyring_entry(&scoped_provider_id);
    let keyring_current = keyring_entry
        .as_ref()
        .and_then(|entry| entry.get_password().ok())
        .and_then(|secret| serde_json::from_str::<ProviderCredential>(&secret).ok())
        .and_then(|credential| normalize_provider_credential(credential).ok());
    let current_is_keyring_backed = keyring_current.is_some();
    let current = keyring_current
        .or_else(|| fallback.get(&scoped_provider_id).cloned())
        .map(|credential| credential_with_provider_id(credential, provider_id.clone()));
    let expected = expected
        .cloned()
        .map(|mut expected| {
            expected.provider_id = provider_id.clone();
            normalize_provider_credential(ProviderCredential::OAuth(expected))
        })
        .transpose()?;
    if current != expected {
        return Ok(false);
    }

    let expected_material = replacement
        .clone()
        .map(|mut credential| {
            credential.provider_id = scoped_provider_id.clone();
            normalize_provider_credential(ProviderCredential::OAuth(credential))
                .and_then(|credential| Ok(serde_json::to_value(credential)?))
        })
        .transpose()?;
    let mutation = Mutation::begin(
        security_dir,
        ProviderCredentialKind::Credential,
        &scoped_provider_id,
        expected_material,
        refresh,
        true,
    )?;
    let has_replacement = replacement.is_some();
    match replacement {
        Some(mut replacement) => {
            replacement.provider_id = provider_id;
            let replacement =
                normalize_provider_credential(ProviderCredential::OAuth(replacement))?;
            let replacement = credential_with_provider_id(replacement, scoped_provider_id.clone());
            if current_is_keyring_backed {
                let serialized = serde_json::to_string(&replacement)?;
                keyring_entry
                    .as_ref()
                    .expect("keyring-backed credential has an entry")
                    .set_password(&serialized)?;
                fallback.remove(&scoped_provider_id);
            } else {
                fallback.insert(scoped_provider_id.clone(), replacement);
            }
        }
        None => {
            if current_is_keyring_backed {
                keyring_entry
                    .as_ref()
                    .expect("keyring-backed credential has an entry")
                    .delete_password()?;
            }
            fallback.remove(&scoped_provider_id);
        }
    }
    save_credential_fallback_map_to_dir(security_dir, &fallback)?;

    let mut known = load_provider_credentials_index_from_dir(security_dir);
    if has_replacement {
        known.insert(scoped_provider_id);
    } else {
        known.remove(&scoped_provider_id);
    }
    save_provider_credentials_index_to_dir(security_dir, &known)?;
    mutation.finish(Some(if current_is_keyring_backed {
        ProviderAuthBackend::Keychain
    } else {
        ProviderAuthBackend::File
    }))?;
    Ok(true)
}

pub fn compare_and_set_provider_oauth_credential_for_tenant_in_dir(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
    expected: &OAuthProviderCredential,
    replacement: Option<OAuthProviderCredential>,
) -> anyhow::Result<bool> {
    let _file_guard = ProviderCredentialMutationFileLock::acquire_blocking(security_dir)?;
    compare_and_set_provider_oauth_credential_for_tenant_in_dir_unlocked(
        security_dir,
        tenant_context,
        provider_id,
        Some(expected),
        replacement,
        false,
    )
}

pub async fn compare_and_set_provider_oauth_credential_for_tenant_in_dir_serialized(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
    expected: &OAuthProviderCredential,
    replacement: Option<OAuthProviderCredential>,
) -> anyhow::Result<bool> {
    let _guard = provider_credential_mutation_lock().lock().await;
    let _file_guard = ProviderCredentialMutationFileLock::acquire(security_dir).await?;
    compare_and_set_provider_oauth_credential_for_tenant_in_dir_unlocked(
        security_dir,
        tenant_context,
        provider_id,
        Some(expected),
        replacement,
        false,
    )
}

pub async fn compare_and_set_optional_provider_oauth_credential_for_tenant_in_dir_serialized(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
    expected: Option<&OAuthProviderCredential>,
    replacement: Option<OAuthProviderCredential>,
) -> anyhow::Result<bool> {
    let _guard = provider_credential_mutation_lock().lock().await;
    let _file_guard = ProviderCredentialMutationFileLock::acquire(security_dir).await?;
    compare_and_set_provider_oauth_credential_for_tenant_in_dir_unlocked(
        security_dir,
        tenant_context,
        provider_id,
        expected,
        replacement,
        false,
    )
}

fn delete_provider_credential_unlocked(provider_id: &str) -> anyhow::Result<bool> {
    let id = normalize_provider_id(provider_id);
    if id.is_empty() {
        return Ok(false);
    }

    let mutation = Mutation::begin(
        &provider_auth_security_dir(),
        ProviderCredentialKind::Credential,
        &id,
        None,
        false,
        true,
    )?;
    let mut removed = false;

    if let Some(entry) = credential_keyring_entry(&id) {
        if entry.delete_password().is_ok() {
            removed = true;
        }
    }

    let mut fallback = load_credential_fallback_map();
    delay_provider_credential_rmw_for_test();
    if fallback.remove(&id).is_some() {
        removed = true;
    }
    save_credential_fallback_map(&fallback)?;

    let mut known = load_provider_credentials_index();
    if known.remove(&id) {
        removed = true;
    }
    save_provider_credentials_index(&known)?;

    mutation.finish(None)?;
    Ok(removed)
}

pub fn delete_provider_credential(provider_id: &str) -> anyhow::Result<bool> {
    let _file_guard =
        ProviderCredentialMutationFileLock::acquire_blocking(&provider_auth_security_dir())?;
    delete_provider_credential_unlocked(provider_id)
}

pub fn delete_provider_credential_for_tenant(
    tenant_context: &TenantContext,
    provider_id: &str,
) -> anyhow::Result<bool> {
    let scoped_provider_id = tenant_scoped_provider_id(tenant_context, provider_id);
    delete_provider_credential(&scoped_provider_id)
}

fn delete_provider_credential_for_tenant_in_dir_unlocked(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
) -> anyhow::Result<bool> {
    let scoped_provider_id =
        normalize_provider_id(&tenant_scoped_provider_id(tenant_context, provider_id));
    if scoped_provider_id.is_empty() {
        return Ok(false);
    }
    let mutation = Mutation::begin(
        security_dir,
        ProviderCredentialKind::Credential,
        &scoped_provider_id,
        None,
        false,
        true,
    )?;
    let mut removed = false;
    if let Some(entry) = credential_keyring_entry(&scoped_provider_id) {
        if entry.delete_password().is_ok() {
            removed = true;
        }
    }
    let mut fallback = load_credential_fallback_map_from_dir(security_dir);
    delay_provider_credential_rmw_for_test();
    if fallback.remove(&scoped_provider_id).is_some() {
        removed = true;
    }
    save_credential_fallback_map_to_dir(security_dir, &fallback)?;
    let mut known = load_provider_credentials_index_from_dir(security_dir);
    if known.remove(&scoped_provider_id) {
        removed = true;
    }
    save_provider_credentials_index_to_dir(security_dir, &known)?;
    mutation.finish(None)?;
    Ok(removed)
}

pub fn delete_provider_credential_for_tenant_in_dir(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
) -> anyhow::Result<bool> {
    let _file_guard = ProviderCredentialMutationFileLock::acquire_blocking(security_dir)?;
    delete_provider_credential_for_tenant_in_dir_unlocked(security_dir, tenant_context, provider_id)
}

pub async fn delete_provider_credential_for_tenant_in_dir_serialized(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
) -> anyhow::Result<bool> {
    let _guard = provider_credential_mutation_lock().lock().await;
    let _file_guard = ProviderCredentialMutationFileLock::acquire(security_dir).await?;
    delete_provider_credential_for_tenant_in_dir_unlocked(security_dir, tenant_context, provider_id)
}

/// Persist a refresh. Authorization is retained only for an already tracked,
/// current credential with the same nonempty account ID and management source.
/// Compensation and explicit reconnect must use the ordinary CAS operation.
pub async fn refresh_provider_oauth_credential_for_tenant_in_dir_serialized(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
    expected: &OAuthProviderCredential,
    replacement: OAuthProviderCredential,
) -> anyhow::Result<bool> {
    let _guard = provider_credential_mutation_lock().lock().await;
    let _file_guard = ProviderCredentialMutationFileLock::acquire(security_dir).await?;
    compare_and_set_provider_oauth_credential_for_tenant_in_dir_unlocked(
        security_dir,
        tenant_context,
        provider_id,
        Some(expected),
        Some(replacement),
        true,
    )
}

/// Holds the existing credential serialization and file lock without blocking a
/// Tokio worker during acquisition. Revalidate authority after awaiting this
/// guard, then snapshot/mutate through it. Drop before rollback or runtime I/O.
pub struct ProviderAuthMutation {
    security_dir: PathBuf,
    _file_guard: ProviderCredentialMutationFileLock,
    _guard: tokio::sync::MutexGuard<'static, ()>,
}

pub async fn provider_auth_mutation_in_dir(
    security_dir: &Path,
) -> anyhow::Result<ProviderAuthMutation> {
    let guard = provider_credential_mutation_lock().lock().await;
    let file_guard = ProviderCredentialMutationFileLock::acquire(security_dir).await?;
    Ok(ProviderAuthMutation {
        security_dir: security_dir.to_path_buf(),
        _file_guard: file_guard,
        _guard: guard,
    })
}

impl ProviderAuthMutation {
    pub fn set_for_tenant(
        &mut self,
        tenant: &TenantContext,
        provider_id: &str,
        token: &str,
    ) -> anyhow::Result<ProviderAuthBackend> {
        set_provider_auth_for_tenant_in_dir_unlocked(&self.security_dir, tenant, provider_id, token)
    }

    pub fn delete_for_tenant(
        &mut self,
        tenant: &TenantContext,
        provider_id: &str,
    ) -> anyhow::Result<bool> {
        delete_provider_auth_for_tenant_in_dir_unlocked(&self.security_dir, tenant, provider_id)
    }
}
