// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

pub(crate) async fn write_json_records_file(
    path: &Path,
    records: &[ProtectedJsonRecord],
    store: &ProtectedStoreContext,
) -> anyhow::Result<()> {
    let lock = path_lock_for(path).await;
    let _guard = lock.lock().await;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let crypto = crypto();
    let _process_guard = ProcessWriteLock::acquire(path).await?;
    if crypto.provider.is_plaintext() {
        ensure_legacy_store_not_anchored(path, store).await?;
        ensure_integrity_sidecars_absent(path, "plaintext protected JSON write").await?;
        return atomic_replace(
            path,
            ProtectedFileCrypto::plaintext_json(records)?.as_bytes(),
        )
        .await;
    }

    let old_data = read_optional_file(path).await?;
    let head_path = integrity_head_path(path);
    let state_path = initialized_state_path(path);
    let old_head = read_optional_file(&head_path).await?;
    let old_state = read_optional_file(&state_path).await?;
    let (generation, previous_digest) = match old_data.as_deref() {
        Some(bytes) => {
            let stored = std::str::from_utf8(bytes).context("protected JSON store is not UTF-8")?;
            if stored.starts_with(AUTHENTICATED_COLLECTION_PREFIX) {
                let current = crypto.decrypt_json_collection(stored, store)?;
                let head = read_committed_head(&crypto, path, store).await?;
                anyhow::ensure!(
                    current.generation == head.generation
                        && current.digest == head.digest
                        && current.integrity_key_id == head.integrity_key_id,
                    "protected JSON collection integrity head mismatch before write"
                );
                validate_cached_head(path, &head).await?;
                (current.generation.saturating_add(1), Some(current.digest))
            } else {
                ensure_legacy_store_not_anchored(path, store).await?;
                anyhow::ensure!(
                    old_head.is_none() && old_state.is_none(),
                    "legacy protected JSON document conflicts with integrity state"
                );
                crypto.decrypt_legacy_json_document(stored)?;
                (1, None)
            }
        }
        None => {
            ensure_legacy_store_not_anchored(path, store).await?;
            anyhow::ensure!(
                old_head.is_none() && old_state.is_none(),
                "protected JSON collection data is missing from an initialized store"
            );
            (1, None)
        }
    };
    let (stored, head) =
        crypto.encrypt_json_collection(records, store, generation, previous_digest)?;
    let previous_cached_head = cached_head(path).await;
    let store_anchor_snapshot =
        snapshot_protected_store_anchor(path, store, &head, previous_cached_head.as_ref()).await?;
    validate_cached_head(path, &head).await?;
    let state = authenticated_state(&head);
    let encoded_head = encode_authenticated_head(&crypto, &head, store)?;
    let encoded_state = encode_authenticated_state(&crypto, &state, store)?;

    // Advance the persistent witness before the sealed head and data. A crash
    // before all three renames agree leaves the store unavailable.
    let write_result = async {
        atomic_replace(&state_path, encoded_state.as_bytes())
            .await
            .context("write protected collection initialized state")?;
        atomic_replace(&head_path, encoded_head.as_bytes())
            .await
            .context("write protected collection integrity head")?;
        atomic_replace(path, stored.as_bytes())
            .await
            .context("write protected collection data")?;
        write_protected_store_anchor(path, store, &head, store_anchor_snapshot.as_ref()).await?;
        anyhow::Ok(())
    }
    .await;
    if let Err(error) = write_result {
        let anchor_rollback = rollback_protected_store_anchor(
            path,
            store,
            &head,
            previous_cached_head.as_ref(),
            store_anchor_snapshot.as_ref(),
        )
        .await;
        let file_rollback = restore_failed_collection_write(
            path,
            old_data.as_deref(),
            &head_path,
            old_head.as_deref(),
            &state_path,
            old_state.as_deref(),
            &head,
            previous_cached_head.as_ref(),
        )
        .await;
        return finish_failed_transaction(error, [anchor_rollback, file_rollback]);
    }
    Ok(())
}
