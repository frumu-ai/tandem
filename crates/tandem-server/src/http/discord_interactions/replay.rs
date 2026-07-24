// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Durable replay claims for signed Discord interactions.
//!
//! A single cross-process advisory file lock makes claim creation atomic across
//! multiple server processes sharing the same state directory. Claims are
//! tenant/application/interaction bound, expire with Discord's retry window,
//! and are capped so the durable directory cannot grow without bound.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::Context;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tandem_types::TenantContext;
use tokio::io::AsyncWriteExt;

use crate::AppState;

const CLAIM_SCHEMA_VERSION: u32 = 1;
const CLAIM_TTL_MS: u64 = 5 * 60 * 1000;
const MAX_CLAIMS_PER_TENANT_APPLICATION: usize = 10_000;
const MAX_RECORD_BYTES: u64 = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiscordReplayClaimDecision {
    Claimed,
    Duplicate,
    Conflict,
}

pub(super) enum DiscordReplayClaimPreparation {
    Pending(PendingDiscordReplayClaim),
    Duplicate,
    Conflict,
}

pub(super) struct PendingDiscordReplayClaim {
    _lock: ClaimsDirectoryLock,
    scope_root: PathBuf,
    path: PathBuf,
    record: DiscordReplayClaimRecord,
    replace_existing: bool,
    max_claims: usize,
}

impl PendingDiscordReplayClaim {
    pub(super) async fn commit(self) -> anyhow::Result<()> {
        if !self.replace_existing {
            let active_claims =
                prune_expired_and_count(&self.scope_root, self.record.claimed_at_ms).await?;
            if active_claims >= self.max_claims {
                anyhow::bail!(
                    "Discord interaction replay claim quota exhausted for tenant/application"
                );
            }
        }
        write_record(&self.path, &self.record).await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiscordReplayClaimRecord {
    schema_version: u32,
    tenant_context: TenantContext,
    application_id: String,
    interaction_id: String,
    body_digest: String,
    claimed_at_ms: u64,
    expires_at_ms: u64,
}

pub(super) async fn claim_discord_interaction(
    state: &AppState,
    tenant_context: &TenantContext,
    application_id: &str,
    interaction_id: &str,
    body: &[u8],
    now_ms: u64,
) -> anyhow::Result<DiscordReplayClaimDecision> {
    claim_discord_interaction_with_limit(
        state,
        tenant_context,
        application_id,
        interaction_id,
        body,
        now_ms,
        MAX_CLAIMS_PER_TENANT_APPLICATION,
    )
    .await
}

pub(super) async fn prepare_discord_interaction(
    state: &AppState,
    tenant_context: &TenantContext,
    application_id: &str,
    interaction_id: &str,
    body: &[u8],
    now_ms: u64,
) -> anyhow::Result<DiscordReplayClaimPreparation> {
    prepare_discord_interaction_with_limit(
        state,
        tenant_context,
        application_id,
        interaction_id,
        body,
        now_ms,
        MAX_CLAIMS_PER_TENANT_APPLICATION,
    )
    .await
}

async fn claim_discord_interaction_with_limit(
    state: &AppState,
    tenant_context: &TenantContext,
    application_id: &str,
    interaction_id: &str,
    body: &[u8],
    now_ms: u64,
    max_claims: usize,
) -> anyhow::Result<DiscordReplayClaimDecision> {
    match prepare_discord_interaction_with_limit(
        state,
        tenant_context,
        application_id,
        interaction_id,
        body,
        now_ms,
        max_claims,
    )
    .await?
    {
        DiscordReplayClaimPreparation::Pending(pending) => {
            pending.commit().await?;
            Ok(DiscordReplayClaimDecision::Claimed)
        }
        DiscordReplayClaimPreparation::Duplicate => Ok(DiscordReplayClaimDecision::Duplicate),
        DiscordReplayClaimPreparation::Conflict => Ok(DiscordReplayClaimDecision::Conflict),
    }
}

async fn prepare_discord_interaction_with_limit(
    state: &AppState,
    tenant_context: &TenantContext,
    application_id: &str,
    interaction_id: &str,
    body: &[u8],
    now_ms: u64,
    max_claims: usize,
) -> anyhow::Result<DiscordReplayClaimPreparation> {
    validate_identifier(application_id, "application_id")?;
    validate_identifier(interaction_id, "interaction_id")?;
    let scope_root = claims_scope_root(state, tenant_context, application_id);
    let lock = ClaimsDirectoryLock::acquire(&scope_root).await?;
    tokio::fs::create_dir_all(&scope_root).await?;
    let path = claim_path(&scope_root, interaction_id);
    let digest = body_digest(body);

    if let Some(existing) = read_record(&path).await? {
        validate_record_identity(&existing, tenant_context, application_id, interaction_id)?;
        if existing.expires_at_ms > now_ms {
            return Ok(if existing.body_digest == digest {
                DiscordReplayClaimPreparation::Duplicate
            } else {
                DiscordReplayClaimPreparation::Conflict
            });
        }
        return Ok(DiscordReplayClaimPreparation::Pending(
            PendingDiscordReplayClaim {
                _lock: lock,
                scope_root,
                path,
                record: new_record(
                    tenant_context,
                    application_id,
                    interaction_id,
                    digest,
                    now_ms,
                ),
                replace_existing: true,
                max_claims,
            },
        ));
    }

    Ok(DiscordReplayClaimPreparation::Pending(
        PendingDiscordReplayClaim {
            _lock: lock,
            scope_root,
            path,
            record: new_record(
                tenant_context,
                application_id,
                interaction_id,
                digest,
                now_ms,
            ),
            replace_existing: false,
            max_claims,
        },
    ))
}
fn new_record(
    tenant_context: &TenantContext,
    application_id: &str,
    interaction_id: &str,
    body_digest: String,
    now_ms: u64,
) -> DiscordReplayClaimRecord {
    DiscordReplayClaimRecord {
        schema_version: CLAIM_SCHEMA_VERSION,
        tenant_context: tenant_context.clone(),
        application_id: application_id.to_string(),
        interaction_id: interaction_id.to_string(),
        body_digest,
        claimed_at_ms: now_ms,
        expires_at_ms: now_ms.saturating_add(CLAIM_TTL_MS),
    }
}

fn validate_identifier(value: &str, label: &str) -> anyhow::Result<()> {
    if value.is_empty() || value.len() > 256 || !value.is_ascii() {
        anyhow::bail!("Discord {label} is missing or invalid");
    }
    Ok(())
}

fn validate_record_identity(
    record: &DiscordReplayClaimRecord,
    tenant_context: &TenantContext,
    application_id: &str,
    interaction_id: &str,
) -> anyhow::Result<()> {
    if &record.tenant_context != tenant_context
        || record.application_id != application_id
        || record.interaction_id != interaction_id
    {
        anyhow::bail!("Discord replay claim identity binding mismatch");
    }
    Ok(())
}

fn claims_root(state: &AppState) -> PathBuf {
    state
        .idempotency_keys_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("discord_interaction_claims")
}

fn claims_scope_root(
    state: &AppState,
    tenant_context: &TenantContext,
    application_id: &str,
) -> PathBuf {
    let deployment_id = tenant_context.deployment_id.as_deref().unwrap_or_default();
    let scope_digest = crate::sha256_hex(&[
        &tenant_context.org_id,
        &tenant_context.workspace_id,
        deployment_id,
        application_id,
    ]);
    claims_root(state).join(scope_digest)
}

fn claim_path(scope_root: &Path, interaction_id: &str) -> PathBuf {
    let interaction_digest = crate::sha256_hex(&[interaction_id]);
    scope_root.join(format!("{interaction_digest}.json"))
}

fn body_digest(body: &[u8]) -> String {
    Sha256::digest(body)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

async fn read_record(path: &Path) -> anyhow::Result<Option<DiscordReplayClaimRecord>> {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("stat {}", path.display())),
    };
    if metadata.len() > MAX_RECORD_BYTES {
        anyhow::bail!("Discord replay claim exceeds its record-size limit");
    }
    let raw = tokio::fs::read(path)
        .await
        .with_context(|| format!("read {}", path.display()))?;
    let record = serde_json::from_slice::<DiscordReplayClaimRecord>(&raw)
        .with_context(|| format!("parse {}", path.display()))?;
    if record.schema_version != CLAIM_SCHEMA_VERSION {
        anyhow::bail!(
            "unsupported Discord replay claim schema {}",
            record.schema_version
        );
    }
    Ok(Some(record))
}

async fn prune_expired_and_count(root: &Path, now_ms: u64) -> anyhow::Result<usize> {
    let mut directory = match tokio::fs::read_dir(root).await {
        Ok(directory) => directory,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error).with_context(|| format!("read {}", root.display())),
    };
    let mut active = 0usize;
    while let Some(entry) = directory.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        match read_record(&path).await {
            Ok(Some(record)) if record.expires_at_ms <= now_ms => {
                tokio::fs::remove_file(&path).await?;
            }
            Ok(Some(_)) | Ok(None) | Err(_) => {
                // Corrupt records consume quota and fail closed rather than
                // being attacker-controllable deletion primitives.
                active = active.saturating_add(1);
            }
        }
    }
    Ok(active)
}

async fn write_record(path: &Path, record: &DiscordReplayClaimRecord) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .context("Discord replay claim path has no parent")?;
    tokio::fs::create_dir_all(parent).await?;
    let temporary = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let payload = serde_json::to_vec(record)?;
    if payload.len() as u64 > MAX_RECORD_BYTES {
        anyhow::bail!("Discord replay claim exceeds its record-size limit");
    }
    let mut options = tokio::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).await?;
    if let Err(error) = async {
        file.write_all(&payload).await?;
        file.flush().await?;
        file.sync_all().await?;
        tokio::fs::rename(&temporary, path).await?;
        sync_directory(parent).await
    }
    .await
    {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error);
    }
    Ok(())
}

async fn sync_directory(path: &Path) -> anyhow::Result<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || std::fs::File::open(path)?.sync_all()).await??;
    Ok(())
}

struct ClaimsDirectoryLock {
    file: std::fs::File,
}

impl ClaimsDirectoryLock {
    async fn acquire(claims_root: &Path) -> anyhow::Result<Self> {
        let parent = claims_root
            .parent()
            .context("Discord replay claims root has no parent")?;
        tokio::fs::create_dir_all(parent).await?;
        let lock_path = claims_root.with_extension("lock");
        tokio::task::spawn_blocking(move || {
            let file = {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    std::fs::OpenOptions::new()
                        .create(true)
                        .truncate(false)
                        .read(true)
                        .write(true)
                        .mode(0o600)
                        .open(&lock_path)?
                }
                #[cfg(not(unix))]
                {
                    std::fs::OpenOptions::new()
                        .create(true)
                        .truncate(false)
                        .read(true)
                        .write(true)
                        .open(&lock_path)?
                }
            };
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&lock_path, std::fs::Permissions::from_mode(0o600))?;
            }
            file.lock_exclusive()
                .with_context(|| format!("lock {}", lock_path.display()))?;
            Ok(Self { file })
        })
        .await
        .context("join Discord replay claim lock acquisition")?
    }
}

impl Drop for ClaimsDirectoryLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(org: &str) -> TenantContext {
        TenantContext::explicit_user_workspace(org, "hq", None, "discord-test")
    }

    #[tokio::test]
    async fn replay_claim_survives_restart_and_detects_conflict() {
        let first = crate::test_support::test_state().await;
        let path = first.idempotency_keys_path.clone();
        let scope = tenant("acme");
        assert_eq!(
            claim_discord_interaction(&first, &scope, "app-1", "interaction-1", b"one", 1_000)
                .await
                .unwrap(),
            DiscordReplayClaimDecision::Claimed
        );

        let mut restarted = crate::test_support::test_state().await;
        restarted.idempotency_keys_path = path;
        assert_eq!(
            claim_discord_interaction(&restarted, &scope, "app-1", "interaction-1", b"one", 2_000,)
                .await
                .unwrap(),
            DiscordReplayClaimDecision::Duplicate
        );
        assert_eq!(
            claim_discord_interaction(
                &restarted,
                &scope,
                "app-1",
                "interaction-1",
                b"different",
                2_000,
            )
            .await
            .unwrap(),
            DiscordReplayClaimDecision::Conflict
        );
    }

    #[tokio::test]
    async fn competing_instances_have_one_claim_owner() {
        let first = crate::test_support::test_state().await;
        let mut second = crate::test_support::test_state().await;
        second.idempotency_keys_path = first.idempotency_keys_path.clone();
        let scope = tenant("acme");
        let (left, right) = tokio::join!(
            claim_discord_interaction(&first, &scope, "app-2", "interaction-2", b"same", 1_000),
            claim_discord_interaction(&second, &scope, "app-2", "interaction-2", b"same", 1_000),
        );
        let decisions = [left.unwrap(), right.unwrap()];
        assert_eq!(
            decisions
                .iter()
                .filter(|decision| **decision == DiscordReplayClaimDecision::Claimed)
                .count(),
            1
        );
        assert_eq!(
            decisions
                .iter()
                .filter(|decision| **decision == DiscordReplayClaimDecision::Duplicate)
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn dropped_preparation_does_not_create_a_claim() {
        let state = crate::test_support::test_state().await;
        let scope = tenant("acme");
        let preparation = prepare_discord_interaction(
            &state,
            &scope,
            "app-rate-denied",
            "interaction-rate-denied",
            b"body",
            1_000,
        )
        .await
        .unwrap();
        assert!(matches!(
            preparation,
            DiscordReplayClaimPreparation::Pending(_)
        ));
        drop(preparation);

        assert_eq!(
            claim_discord_interaction(
                &state,
                &scope,
                "app-rate-denied",
                "interaction-rate-denied",
                b"body",
                1_001,
            )
            .await
            .unwrap(),
            DiscordReplayClaimDecision::Claimed
        );
    }

    #[tokio::test]
    async fn claim_key_is_tenant_bound_and_expires() {
        let state = crate::test_support::test_state().await;
        assert_eq!(
            claim_discord_interaction(&state, &tenant("a"), "app", "same", b"body", 1_000)
                .await
                .unwrap(),
            DiscordReplayClaimDecision::Claimed
        );
        assert_eq!(
            claim_discord_interaction(&state, &tenant("b"), "app", "same", b"body", 1_000)
                .await
                .unwrap(),
            DiscordReplayClaimDecision::Claimed
        );
        assert_eq!(
            claim_discord_interaction(
                &state,
                &tenant("a"),
                "app",
                "same",
                b"new-body",
                1_000 + CLAIM_TTL_MS,
            )
            .await
            .unwrap(),
            DiscordReplayClaimDecision::Claimed
        );
    }

    #[tokio::test]
    async fn quota_exhaustion_is_isolated_per_tenant_and_application() {
        let state = crate::test_support::test_state().await;
        let tenant_a = tenant("a");
        let tenant_b = tenant("b");

        assert_eq!(
            claim_discord_interaction_with_limit(
                &state,
                &tenant_a,
                "app-1",
                "interaction-1",
                b"one",
                1_000,
                1,
            )
            .await
            .unwrap(),
            DiscordReplayClaimDecision::Claimed
        );
        assert!(claim_discord_interaction_with_limit(
            &state,
            &tenant_a,
            "app-1",
            "interaction-2",
            b"two",
            1_000,
            1,
        )
        .await
        .is_err());
        assert_eq!(
            claim_discord_interaction_with_limit(
                &state,
                &tenant_b,
                "app-1",
                "interaction-2",
                b"two",
                1_000,
                1,
            )
            .await
            .unwrap(),
            DiscordReplayClaimDecision::Claimed
        );
        assert_eq!(
            claim_discord_interaction_with_limit(
                &state,
                &tenant_a,
                "app-2",
                "interaction-2",
                b"two",
                1_000,
                1,
            )
            .await
            .unwrap(),
            DiscordReplayClaimDecision::Claimed
        );
    }
}
