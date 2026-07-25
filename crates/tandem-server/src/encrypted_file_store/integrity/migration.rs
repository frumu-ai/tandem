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

async fn rollback_protected_store_anchor(
    path: &Path,
    store: &ProtectedStoreContext,
    head: &AuthenticatedStoreHead,
) -> anyhow::Result<()> {
    if head.integrity_key_id.is_some() {
        let identity = protected_store_anchor_identity(path, store)?;
        crate::audit_integrity::rollback_external_anchor_write(
            "protected-store",
            &identity,
            head.generation,
            &head.digest,
            path,
        )
        .await
        .context("roll back newly published protected store anchor")?;
    }
    Ok(())
}

pub(crate) async fn migrate_jsonl_records_file(
    path: &Path,
    expected_legacy_lines: &[String],
    migrated_records: &[(String, ProtectedRecordContext)],
    store: &ProtectedStoreContext,
    additional_anchor: Option<(&str, &str, u64, &str)>,
) -> anyhow::Result<()> {
    let lock = path_lock_for(path).await;
    let _guard = lock.lock().await;
    fs::create_dir_all(path.parent().unwrap_or_else(|| Path::new("."))).await?;
    let crypto = crypto();
    let _process_guard = ProcessWriteLock::acquire(path).await?;
    let prior = decrypt_jsonl_state(&crypto, path, store).await?;
    anyhow::ensure!(
        !prior.authenticated
            && prior.generation == 0
            && prior.lines.as_slice() == expected_legacy_lines,
        "protected JSONL legacy rows changed before migration"
    );
    anyhow::ensure!(
        !migrated_records.is_empty(),
        "protected JSONL migration requires at least one output row"
    );

    if crypto.provider.is_plaintext() {
        return rewrite_plaintext_legacy_jsonl(path, migrated_records, store, additional_anchor)
            .await;
    }

    let authority = crate::audit_integrity::integrity_authority()?;
    commit_legacy_jsonl_migration(
        &crypto,
        path,
        migrated_records,
        store,
        authority.as_ref(),
        additional_anchor,
    )
    .await
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
    let mut records = legacy_lines
        .iter()
        .map(|line| (line.clone(), store.manifest.clone()))
        .collect::<Vec<_>>();
    records.push((plaintext.to_string(), context.clone()));
    commit_legacy_jsonl_migration(crypto, path, &records, store, Some(authority), None).await
}

async fn rewrite_plaintext_legacy_jsonl(
    path: &Path,
    migrated_records: &[(String, ProtectedRecordContext)],
    store: &ProtectedStoreContext,
    additional_anchor: Option<(&str, &str, u64, &str)>,
) -> anyhow::Result<()> {
    ensure_legacy_store_not_anchored(path, store).await?;
    ensure_integrity_sidecars_absent(path, "plaintext protected JSONL migration").await?;
    let previous_data = read_optional_file(path)
        .await?
        .context("legacy protected JSONL data disappeared during migration")?;
    let mut stored = String::new();
    for (record, _) in migrated_records {
        stored.push_str(record);
        stored.push('\n');
    }

    let write_result = async {
        atomic_replace(path, stored.as_bytes())
            .await
            .context("write migrated plaintext protected JSONL data")?;
        write_additional_anchor(additional_anchor, path).await
    }
    .await;
    if let Err(error) = write_result {
        let anchor_rollback = rollback_additional_anchor(additional_anchor, path).await;
        let data_rollback = restore_file(path, Some(&previous_data))
            .await
            .context("restore plaintext protected JSONL after failed migration");
        return finish_failed_migration(error, [anchor_rollback, data_rollback]);
    }
    Ok(())
}

async fn commit_legacy_jsonl_migration(
    crypto: &ProtectedFileCrypto,
    path: &Path,
    migrated_records: &[(String, ProtectedRecordContext)],
    store: &ProtectedStoreContext,
    authority: Option<&crate::audit_integrity::AuditIntegrityKeyring>,
    additional_anchor: Option<(&str, &str, u64, &str)>,
) -> anyhow::Result<()> {
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

    let mut stored = String::new();
    let mut previous_digest = None;
    for (index, (record, record_context)) in migrated_records.iter().enumerate() {
        let mut frame = AuthenticatedJsonlFrame {
            version: AUTHENTICATED_STORE_VERSION,
            store_id: store.store_id.clone(),
            sequence: index as u64 + 1,
            previous_digest,
            context: record_context.clone(),
            stored_record: crypto.encrypt_record(record, record_context)?,
            integrity_key_id: authority.map(|keys| keys.active_key_id().to_string()),
            digest: String::new(),
        };
        frame.digest = sign_jsonl_frame_digest(&frame, authority)?;
        previous_digest = Some(frame.digest.clone());
        let outer = crypto.encrypt_record(&serde_json::to_string(&frame)?, &store.manifest)?;
        stored.push_str(AUTHENTICATED_JSONL_PREFIX);
        stored.push_str(&outer);
        stored.push('\n');
    }

    let head = AuthenticatedStoreHead {
        version: AUTHENTICATED_STORE_VERSION,
        store_id: store.store_id.clone(),
        generation: migrated_records.len() as u64,
        digest: previous_digest.context("migrated protected JSONL head is missing")?,
        integrity_key_id: authority.map(|keys| keys.active_key_id().to_string()),
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
        write_protected_store_anchor(path, store, &head).await?;
        write_additional_anchor(additional_anchor, path).await
    }
    .await;
    if let Err(error) = write_result {
        let additional_rollback = rollback_additional_anchor(additional_anchor, path).await;
        let store_anchor_rollback = rollback_protected_store_anchor(path, store, &head).await;
        let file_rollback = restore_failed_collection_write(
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
        return finish_failed_migration(
            error,
            [additional_rollback, store_anchor_rollback, file_rollback],
        );
    }
    Ok(())
}

async fn write_additional_anchor(
    additional_anchor: Option<(&str, &str, u64, &str)>,
    path: &Path,
) -> anyhow::Result<()> {
    if let Some((scope, identity, generation, digest)) = additional_anchor {
        crate::audit_integrity::write_external_anchor(scope, identity, generation, digest, path)
            .await
            .context("write migration companion anchor")?;
    }
    Ok(())
}

async fn rollback_additional_anchor(
    additional_anchor: Option<(&str, &str, u64, &str)>,
    path: &Path,
) -> anyhow::Result<()> {
    if let Some((scope, identity, generation, digest)) = additional_anchor {
        crate::audit_integrity::rollback_external_anchor_write(
            scope, identity, generation, digest, path,
        )
        .await
        .context("roll back migration companion anchor")?;
    }
    Ok(())
}

fn finish_failed_migration<const N: usize>(
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
            "failed to roll back protected JSONL migration: {}",
            failures.join("; ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use tandem_memory::MemoryCryptoProvider;

    use super::*;
    use crate::encrypted_file_store::with_test_crypto_provider;

    fn migration_fixture(
        name: &str,
    ) -> (
        tempfile::TempDir,
        std::path::PathBuf,
        std::path::PathBuf,
        ProtectedStoreContext,
        ProtectedRecordContext,
        ProtectedRecordContext,
    ) {
        let root = tempfile::tempdir().expect("temporary root");
        let path = root.path().join("state").join(format!("{name}.jsonl"));
        let anchors = root.path().join("anchors");
        let store = super::super::tests::store_context(name);
        let first = super::super::tests::record_context("first");
        let second = super::super::tests::record_context("second");
        (root, path, anchors, store, first, second)
    }

    #[tokio::test]
    async fn configured_authority_migrates_legacy_rows_before_append() {
        let (_root, path, anchors, store, first_context, second_context) =
            migration_fixture("legacy-migration");
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

    #[tokio::test]
    async fn anchor_publication_failure_restores_legacy_store_and_anchor_absence() {
        let (_root, path, anchors, store, first_context, second_context) =
            migration_fixture("anchor-rollback");
        with_test_crypto_provider(MemoryCryptoProvider::local_key([0x64; 32]), None, async {
            fs::create_dir_all(path.parent().expect("state parent"))
                .await
                .expect("state directory");
            let legacy =
                encrypt_legacy_local_line(&crypto(), "first", &first_context).expect("legacy row");
            let original = format!("{legacy}\n");
            fs::write(&path, &original).await.expect("legacy store");
            let authority = crate::audit_integrity::test_keyring(
                "active",
                "rollback-audit-integrity-secret-32-bytes",
                &[],
            );

            crate::audit_integrity::with_test_keyring(
                Some(authority),
                crate::audit_integrity::with_test_anchor_dir(anchors, async {
                    let error = crate::audit_integrity::with_test_anchor_write_failure(
                        append_jsonl_record_file(&path, "second", &second_context, &store, true),
                    )
                    .await
                    .expect_err("injected anchor failure must fail migration");
                    assert!(format!("{error:#}").contains("injected external anchor failure"));
                    assert_eq!(fs::read_to_string(&path).await.unwrap(), original);
                    assert!(!integrity_head_path(&path).exists());
                    assert!(!initialized_state_path(&path).exists());
                    assert!(cached_head(&path).await.is_none());
                    assert_eq!(
                        read_jsonl_records_file(&path, &store).await.unwrap(),
                        vec!["first".to_string()]
                    );
                    let identity =
                        protected_store_anchor_identity(&path, &store).expect("anchor identity");
                    crate::audit_integrity::ensure_external_anchor_absent(
                        "protected-store",
                        &identity,
                        &path,
                    )
                    .await
                    .expect("failed migration must not retain its anchor");
                }),
            )
            .await;
        })
        .await;
    }
}
