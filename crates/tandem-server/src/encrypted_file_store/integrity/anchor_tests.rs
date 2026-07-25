// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::time::Duration;

use tandem_memory::MemoryCryptoProvider;

use super::tests::{assert_collection_value, json_record, record_context, store_context};
use super::*;
use crate::encrypted_file_store::with_test_crypto_provider;

async fn committed_triplet(path: &Path) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    (
        fs::read(path).await.expect("committed data"),
        fs::read(integrity_head_path(path))
            .await
            .expect("committed head"),
        fs::read(initialized_state_path(path))
            .await
            .expect("committed initialized state"),
    )
}

async fn assert_triplet(path: &Path, expected: &(Vec<u8>, Vec<u8>, Vec<u8>)) {
    assert_eq!(fs::read(path).await.unwrap(), expected.0);
    assert_eq!(
        fs::read(integrity_head_path(path)).await.unwrap(),
        expected.1
    );
    assert_eq!(
        fs::read(initialized_state_path(path)).await.unwrap(),
        expected.2
    );
}

#[tokio::test]
async fn anchor_failure_restores_prior_jsonl_and_collection_generations() {
    let root = tempfile::tempdir().expect("temporary root");
    let jsonl_path = root.path().join("state").join("anchored.jsonl");
    let collection_path = root.path().join("state").join("anchored.json");
    let anchors = root.path().join("anchors");
    let jsonl_store = store_context("anchored-jsonl");
    let collection_store = store_context("anchored-collection");
    let authority = crate::audit_integrity::test_keyring(
        "active",
        "normal-write-anchor-rollback-secret-32-bytes",
        &[],
    );

    with_test_crypto_provider(MemoryCryptoProvider::local_key([0x65; 32]), None, async {
        crate::audit_integrity::with_test_keyring(
            Some(authority),
            crate::audit_integrity::with_test_anchor_dir(anchors, async {
                append_jsonl_record_file(
                    &jsonl_path,
                    "first",
                    &record_context("first"),
                    &jsonl_store,
                    true,
                )
                .await
                .expect("first anchored JSONL append");
                let jsonl_files = committed_triplet(&jsonl_path).await;
                let jsonl_cache = cached_head(&jsonl_path).await;
                let jsonl_identity =
                    protected_store_anchor_identity(&jsonl_path, &jsonl_store).unwrap();
                let jsonl_anchor = crate::audit_integrity::read_test_anchor_bytes(
                    "protected-store",
                    &jsonl_identity,
                    &jsonl_path,
                )
                .await
                .unwrap()
                .unwrap();

                tokio::time::sleep(Duration::from_millis(5)).await;
                crate::audit_integrity::with_test_anchor_write_failure(append_jsonl_record_file(
                    &jsonl_path,
                    "failed",
                    &record_context("failed"),
                    &jsonl_store,
                    true,
                ))
                .await
                .expect_err("JSONL anchor publication must fail");
                assert_triplet(&jsonl_path, &jsonl_files).await;
                assert_eq!(cached_head(&jsonl_path).await, jsonl_cache);
                assert_eq!(
                    read_jsonl_records_file(&jsonl_path, &jsonl_store)
                        .await
                        .expect("read restored JSONL"),
                    vec!["first".to_string()]
                );
                assert_eq!(
                    crate::audit_integrity::read_test_anchor_bytes(
                        "protected-store",
                        &jsonl_identity,
                        &jsonl_path,
                    )
                    .await
                    .unwrap()
                    .unwrap(),
                    jsonl_anchor
                );

                append_jsonl_record_file(
                    &jsonl_path,
                    "retry",
                    &record_context("retry"),
                    &jsonl_store,
                    true,
                )
                .await
                .expect("retry anchored JSONL append");
                assert_eq!(
                    read_jsonl_records_file(&jsonl_path, &jsonl_store)
                        .await
                        .expect("read retried JSONL"),
                    vec!["first".to_string(), "retry".to_string()]
                );

                write_json_records_file(&collection_path, &json_record(1), &collection_store)
                    .await
                    .expect("first anchored collection write");
                let collection_files = committed_triplet(&collection_path).await;
                let collection_cache = cached_head(&collection_path).await;
                let collection_identity =
                    protected_store_anchor_identity(&collection_path, &collection_store).unwrap();
                let collection_anchor = crate::audit_integrity::read_test_anchor_bytes(
                    "protected-store",
                    &collection_identity,
                    &collection_path,
                )
                .await
                .unwrap()
                .unwrap();

                tokio::time::sleep(Duration::from_millis(5)).await;
                crate::audit_integrity::with_test_anchor_write_failure(write_json_records_file(
                    &collection_path,
                    &json_record(2),
                    &collection_store,
                ))
                .await
                .expect_err("collection anchor publication must fail");
                assert_triplet(&collection_path, &collection_files).await;
                assert_eq!(cached_head(&collection_path).await, collection_cache);
                assert_collection_value(&collection_path, &collection_store, 1).await;
                assert_eq!(
                    crate::audit_integrity::read_test_anchor_bytes(
                        "protected-store",
                        &collection_identity,
                        &collection_path,
                    )
                    .await
                    .unwrap()
                    .unwrap(),
                    collection_anchor
                );

                write_json_records_file(&collection_path, &json_record(3), &collection_store)
                    .await
                    .expect("retry anchored collection write");
                assert_collection_value(&collection_path, &collection_store, 3).await;
            }),
        )
        .await;
    })
    .await;
}

#[tokio::test]
async fn anchor_snapshot_failure_does_not_advance_trusted_head_cache() {
    let root = tempfile::tempdir().expect("temporary root");
    let path = root.path().join("state").join("snapshot-failure.jsonl");
    let anchors = root.path().join("anchors");
    let store = store_context("snapshot-failure");
    let authority = crate::audit_integrity::test_keyring(
        "active",
        "snapshot-failure-cache-secret-material-32-bytes",
        &[],
    );
    let identity = "snapshot-failure-companion";

    with_test_crypto_provider(MemoryCryptoProvider::local_key([0x67; 32]), None, async {
        crate::audit_integrity::with_test_keyring(
            Some(authority),
            crate::audit_integrity::with_test_anchor_dir(anchors, async {
                append_jsonl_record_file_with_anchor(
                    &path,
                    "first",
                    &record_context("first"),
                    &store,
                    true,
                    Some(crate::audit_integrity::ExternalAnchorUpdate {
                        scope: "snapshot-failure-logical",
                        identity,
                        generation: 1,
                        digest: "logical-one",
                        previous: None,
                    }),
                )
                .await
                .expect("first anchored append");
                let trusted = cached_head(&path).await;

                let error = append_jsonl_record_file_with_anchor(
                    &path,
                    "failed",
                    &record_context("failed"),
                    &store,
                    true,
                    Some(crate::audit_integrity::ExternalAnchorUpdate {
                        scope: "snapshot-failure-logical",
                        identity,
                        generation: 2,
                        digest: "logical-two",
                        previous: None,
                    }),
                )
                .await
                .expect_err("unexpected companion anchor must reject before cache advance");
                assert!(format!("{error:#}").contains("already exists"));
                assert_eq!(cached_head(&path).await, trusted);

                append_jsonl_record_file_with_anchor(
                    &path,
                    "retry",
                    &record_context("retry"),
                    &store,
                    true,
                    Some(crate::audit_integrity::ExternalAnchorUpdate {
                        scope: "snapshot-failure-logical",
                        identity,
                        generation: 2,
                        digest: "logical-two",
                        previous: Some((1, "logical-one")),
                    }),
                )
                .await
                .expect("retry after snapshot rejection");
                assert_eq!(
                    read_jsonl_records_file(&path, &store).await.unwrap(),
                    vec!["first".to_string(), "retry".to_string()]
                );
            }),
        )
        .await;
    })
    .await;
}
