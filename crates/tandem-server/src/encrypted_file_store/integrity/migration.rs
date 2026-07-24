// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::path::Path;

use anyhow::Context;

use super::*;

pub(super) fn encrypt_legacy_local_line(
    crypto: &ProtectedFileCrypto,
    plaintext: &str,
    context: &ProtectedRecordContext,
) -> anyhow::Result<String> {
    anyhow::ensure!(
        !crypto.provider.is_plaintext() && !crypto.provider.is_hosted(),
        "legacy local line encryption requires a local-key provider"
    );
    Ok(crypto
        .provider
        .encrypt_field_scoped(
            plaintext,
            &context.key_scope,
            &context.policy_decision_id,
            &context.audit_id,
        )?
        .0)
}

pub(super) async fn migrate_legacy_jsonl_and_append(
    crypto: &ProtectedFileCrypto,
    path: &Path,
    legacy_lines: &[String],
    plaintext: &str,
    context: &ProtectedRecordContext,
    store: &ProtectedStoreContext,
    authority: &crate::audit_integrity::AuditIntegrityKeyring,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !legacy_lines.is_empty(),
        "legacy protected JSONL migration requires existing rows"
    );
    let previous_data = read_optional_file(path)
        .await?
        .context("legacy protected JSONL data disappeared during migration")?;
    let head_path = integrity_head_path(path);
    let state_path = initialized_state_path(path);
    let previous_head = read_optional_file(&head_path).await?;
    let previous_state = read_optional_file(&state_path).await?;
    anyhow::ensure!(
        previous_head.is_none() && previous_state.is_none(),
        "legacy protected JSONL rows conflict with integrity state"
    );

    let mut records = legacy_lines
        .iter()
        .map(|line| (line.as_str(), &store.manifest))
        .collect::<Vec<_>>();
    records.push((plaintext, context));
    let mut stored = String::new();
    let mut previous_digest = None;
    for (index, (record, record_context)) in records.into_iter().enumerate() {
        let mut frame = AuthenticatedJsonlFrame {
            version: AUTHENTICATED_STORE_VERSION,
            store_id: store.store_id.clone(),
            sequence: index as u64 + 1,
            previous_digest,
            context: record_context.clone(),
            stored_record: crypto.encrypt_record(record, record_context)?,
            integrity_key_id: Some(authority.active_key_id().to_string()),
            digest: String::new(),
        };
        frame.digest = sign_jsonl_frame_digest(&frame, Some(authority))?;
        previous_digest = Some(frame.digest.clone());
        let outer = crypto.encrypt_record(&serde_json::to_string(&frame)?, &store.manifest)?;
        stored.push_str(AUTHENTICATED_JSONL_PREFIX);
        stored.push_str(&outer);
        stored.push('\n');
    }

    let head = AuthenticatedStoreHead {
        version: AUTHENTICATED_STORE_VERSION,
        store_id: store.store_id.clone(),
        generation: legacy_lines.len() as u64 + 1,
        digest: previous_digest.context("migrated protected JSONL head is missing")?,
        integrity_key_id: Some(authority.active_key_id().to_string()),
    };
    let previous_cached_head = cached_head(path).await;
    validate_cached_head(path, &head).await?;
    let encoded_head = encode_authenticated_head(crypto, &head, store)?;
    let encoded_state = encode_authenticated_state(crypto, &authenticated_state(&head), store)?;

    let write_result = async {
        atomic_replace(&state_path, encoded_state.as_bytes())
            .await
            .context("write migrated protected JSONL initialized state")?;
        atomic_replace(&head_path, encoded_head.as_bytes())
            .await
            .context("write migrated protected JSONL integrity head")?;
        atomic_replace(path, stored.as_bytes())
            .await
            .context("write migrated protected JSONL data")?;
        anyhow::Ok(())
    }
    .await;
    if let Err(error) = write_result {
        let rollback = restore_failed_collection_write(
            path,
            Some(&previous_data),
            &head_path,
            previous_head.as_deref(),
            &state_path,
            previous_state.as_deref(),
            &head,
            previous_cached_head.as_ref(),
        )
        .await;
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(error.context(format!(
                "failed to roll back protected JSONL migration: {rollback_error:#}"
            ))),
        };
    }
    write_protected_store_anchor(path, store, &head).await
}

#[cfg(test)]
mod tests {
    use tandem_memory::MemoryCryptoProvider;

    use super::*;
    use crate::encrypted_file_store::with_test_crypto_provider;

    #[tokio::test]
    async fn configured_authority_migrates_legacy_rows_before_append() {
        let root = tempfile::tempdir().expect("temporary root");
        let path = root.path().join("state").join("legacy.jsonl");
        let anchors = root.path().join("anchors");
        let store = super::super::tests::store_context("legacy-migration");
        let first_context = super::super::tests::record_context("first");
        let second_context = super::super::tests::record_context("second");
        let provider = MemoryCryptoProvider::local_key([0x63; 32]);

        with_test_crypto_provider(provider, None, async {
            fs::create_dir_all(path.parent().expect("state parent"))
                .await
                .expect("state directory");
            let legacy =
                encrypt_legacy_local_line(&crypto(), "first", &first_context).expect("legacy row");
            fs::write(&path, format!("{legacy}\n"))
                .await
                .expect("legacy store");
            let authority = crate::audit_integrity::test_keyring(
                "active",
                "migration-audit-integrity-secret-32-bytes",
                &[],
            );

            crate::audit_integrity::with_test_keyring(
                Some(authority),
                crate::audit_integrity::with_test_anchor_dir(anchors, async {
                    append_jsonl_record_file(&path, "second", &second_context, &store, true)
                        .await
                        .expect("migrate and append");

                    let encoded = fs::read_to_string(&path).await.expect("migrated store");
                    assert_eq!(encoded.lines().count(), 2);
                    assert!(encoded
                        .lines()
                        .all(|line| line.starts_with(AUTHENTICATED_JSONL_PREFIX)));
                    assert!(integrity_head_path(&path).exists());
                    assert!(initialized_state_path(&path).exists());
                    assert_eq!(
                        read_jsonl_records_file(&path, &store)
                            .await
                            .expect("read migrated store"),
                        vec!["first".to_string(), "second".to_string()]
                    );

                    let second = encoded.lines().nth(1).expect("second frame");
                    fs::write(&path, format!("{second}\n"))
                        .await
                        .expect("delete migrated legacy row");
                    forget_cached_head_for_test(&path).await;
                    assert!(read_jsonl_records_file(&path, &store).await.is_err());
                }),
            )
            .await;
        })
        .await;
    }
}
