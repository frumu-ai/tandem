// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::path::Path;

use anyhow::Context;

use super::{AuthenticatedStoreHead, ProtectedStoreContext};

pub(super) async fn snapshot_protected_store_anchor(
    path: &Path,
    store: &ProtectedStoreContext,
    head: &AuthenticatedStoreHead,
    previous: Option<&AuthenticatedStoreHead>,
) -> anyhow::Result<Option<crate::audit_integrity::ExternalAnchorSnapshot>> {
    if head.integrity_key_id.is_none() {
        return Ok(None);
    }
    let identity = protected_store_anchor_identity(path, store)?;
    crate::audit_integrity::snapshot_external_anchor_update(
        crate::audit_integrity::ExternalAnchorUpdate {
            scope: "protected-store",
            identity: &identity,
            generation: head.generation,
            digest: &head.digest,
            previous: previous
                .filter(|head| head.integrity_key_id.is_some())
                .map(|head| (head.generation, head.digest.as_str())),
        },
        path,
    )
    .await
    .map(Some)
    .context("snapshot protected store anchor before write")
}

pub(super) fn protected_store_anchor_identity(
    path: &Path,
    store: &ProtectedStoreContext,
) -> anyhow::Result<String> {
    Ok(format!(
        "{}:{}",
        store.store_id,
        crate::audit_integrity::canonical_state_file_identity(path)?
    ))
}

pub(super) async fn ensure_legacy_store_not_anchored(
    path: &Path,
    store: &ProtectedStoreContext,
) -> anyhow::Result<()> {
    let identity = protected_store_anchor_identity(path, store)?;
    crate::audit_integrity::ensure_external_anchor_absent("protected-store", &identity, path)
        .await
        .context("reject legacy protected-store replacement against external anchor")
}

pub(super) async fn write_protected_store_anchor(
    path: &Path,
    store: &ProtectedStoreContext,
    head: &AuthenticatedStoreHead,
    snapshot: Option<&crate::audit_integrity::ExternalAnchorSnapshot>,
) -> anyhow::Result<()> {
    if head.integrity_key_id.is_some() {
        let snapshot = snapshot.context("protected store anchor snapshot is missing")?;
        let identity = protected_store_anchor_identity(path, store)?;
        crate::audit_integrity::write_external_anchor(
            "protected-store",
            &identity,
            head.generation,
            &head.digest,
            snapshot,
            path,
        )
        .await
        .context("anchor protected store outside the state directory")?;
    }
    Ok(())
}

pub(super) async fn rollback_protected_store_anchor(
    path: &Path,
    store: &ProtectedStoreContext,
    head: &AuthenticatedStoreHead,
    previous: Option<&AuthenticatedStoreHead>,
    snapshot: Option<&crate::audit_integrity::ExternalAnchorSnapshot>,
) -> anyhow::Result<()> {
    if head.integrity_key_id.is_some() {
        let snapshot = snapshot.context("protected store anchor snapshot is missing")?;
        let identity = protected_store_anchor_identity(path, store)?;
        crate::audit_integrity::rollback_external_anchor_update(
            crate::audit_integrity::ExternalAnchorUpdate {
                scope: "protected-store",
                identity: &identity,
                generation: head.generation,
                digest: &head.digest,
                previous: previous
                    .filter(|head| head.integrity_key_id.is_some())
                    .map(|head| (head.generation, head.digest.as_str())),
            },
            snapshot,
            path,
        )
        .await
        .context("restore protected store anchor after failed write")?;
    }
    Ok(())
}

pub(super) async fn snapshot_additional_anchor(
    update: Option<crate::audit_integrity::ExternalAnchorUpdate<'_>>,
    path: &Path,
) -> anyhow::Result<Option<crate::audit_integrity::ExternalAnchorSnapshot>> {
    match update {
        Some(update) => crate::audit_integrity::snapshot_external_anchor_update(update, path)
            .await
            .map(Some)
            .context("snapshot companion external anchor before write"),
        None => Ok(None),
    }
}

pub(super) async fn write_additional_anchor(
    update: Option<crate::audit_integrity::ExternalAnchorUpdate<'_>>,
    snapshot: Option<&crate::audit_integrity::ExternalAnchorSnapshot>,
    path: &Path,
) -> anyhow::Result<()> {
    if let Some(update) = update {
        let snapshot = snapshot.context("companion external anchor snapshot is missing")?;
        crate::audit_integrity::write_external_anchor(
            update.scope,
            update.identity,
            update.generation,
            update.digest,
            snapshot,
            path,
        )
        .await
        .context("write companion external anchor")?;
    }
    Ok(())
}

pub(super) async fn rollback_additional_anchor(
    update: Option<crate::audit_integrity::ExternalAnchorUpdate<'_>>,
    snapshot: Option<&crate::audit_integrity::ExternalAnchorSnapshot>,
    path: &Path,
) -> anyhow::Result<()> {
    if let Some(update) = update {
        let snapshot = snapshot.context("companion external anchor snapshot is missing")?;
        crate::audit_integrity::rollback_external_anchor_update(update, snapshot, path)
            .await
            .context("restore companion external anchor after failed write")?;
    }
    Ok(())
}

pub(super) fn finish_failed_transaction<const N: usize>(
    error: anyhow::Error,
    rollbacks: [anyhow::Result<()>; N],
) -> anyhow::Result<()> {
    let failures = rollbacks
        .into_iter()
        .filter_map(Result::err)
        .map(|error| format!("{error:#}"))
        .collect::<Vec<_>>();
    if failures.is_empty() {
        Err(error)
    } else {
        Err(error.context(format!(
            "failed to roll back protected-store transaction: {}",
            failures.join("; ")
        )))
    }
}
