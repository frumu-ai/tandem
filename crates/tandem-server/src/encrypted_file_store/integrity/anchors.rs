// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::path::Path;

use anyhow::Context;

use super::{AuthenticatedStoreHead, ProtectedStoreContext};

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
) -> anyhow::Result<()> {
    if head.integrity_key_id.is_some() {
        let identity = protected_store_anchor_identity(path, store)?;
        crate::audit_integrity::write_external_anchor(
            "protected-store",
            &identity,
            head.generation,
            &head.digest,
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
) -> anyhow::Result<()> {
    if head.integrity_key_id.is_some() {
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
            path,
        )
        .await
        .context("restore protected store anchor after failed write")?;
    }
    Ok(())
}

pub(super) async fn write_additional_anchor(
    update: Option<crate::audit_integrity::ExternalAnchorUpdate<'_>>,
    path: &Path,
) -> anyhow::Result<()> {
    if let Some(update) = update {
        crate::audit_integrity::write_external_anchor(
            update.scope,
            update.identity,
            update.generation,
            update.digest,
            path,
        )
        .await
        .context("write companion external anchor")?;
    }
    Ok(())
}

pub(super) async fn rollback_additional_anchor(
    update: Option<crate::audit_integrity::ExternalAnchorUpdate<'_>>,
    path: &Path,
) -> anyhow::Result<()> {
    if let Some(update) = update {
        crate::audit_integrity::rollback_external_anchor_update(update, path)
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
