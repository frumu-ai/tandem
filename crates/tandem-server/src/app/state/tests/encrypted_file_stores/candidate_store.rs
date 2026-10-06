// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use std::path::Path;
use tandem_automation::{
    WorkflowLearningCandidate, WorkflowLearningCandidateKind,
    WorkflowLearningCandidateSourceBinding, WorkflowLearningCandidateStatus,
};

fn candidate(id: &str, owner: &str) -> WorkflowLearningCandidate {
    let marker = format!("sealed-{id}-{owner}-synthetic");
    WorkflowLearningCandidate {
        candidate_id: id.into(),
        workflow_id: format!("session:{marker}"),
        project_id: format!("project:{marker}"),
        source_run_id: format!("run:{marker}"),
        source_binding: Some(WorkflowLearningCandidateSourceBinding::Session {
            tenant_context: TenantContext::explicit(
                "org-candidate",
                "workspace-candidate",
                Some(owner.into()),
            ),
            actor_id: owner.into(),
            subject: owner.into(),
            session_id: format!("session:{marker}"),
        }),
        kind: WorkflowLearningCandidateKind::MemoryFact,
        status: WorkflowLearningCandidateStatus::Proposed,
        confidence: 0.8,
        summary: format!("summary:{marker}"),
        fingerprint: format!("fingerprint:{marker}"),
        node_id: Some(format!("node:{marker}")),
        node_kind: Some("report_markdown".into()),
        validator_family: Some("candidate-fixture".into()),
        evidence_refs: vec![json!({"evidence":marker})],
        artifact_refs: vec![format!("artifact://{marker}")],
        proposed_memory_payload: Some(json!({"content":format!("memory:{marker}")})),
        proposed_revision_prompt: Some(format!("prompt:{marker}")),
        source_memory_id: Some(format!("source:{marker}")),
        promoted_memory_id: None,
        needs_plan_bundle: false,
        baseline_before: None,
        latest_observed_metrics: None,
        last_revision_session_id: None,
        run_ids: vec![format!("run:{marker}")],
        created_at_ms: 0,
        updated_at_ms: 0,
    }
}

fn install_hosted_policy(state: &AppState) {
    let policy = serde_json::to_vec(&json!({
        "schema_version": 1, "policy_version": 1,
        "organization_id": "acme", "deployment_id": "acme",
        "generated_at": chrono::DateTime::from_timestamp_millis(crate::now_ms() as i64).unwrap(),
        "users": [], "org_units": [], "org_unit_memberships": [], "deployment_grants": []
    }))
    .unwrap();
    state
        .install_hosted_policy_snapshot_for_test("acme", "acme", &policy)
        .unwrap();
}

async fn hosted_state(path: &Path) -> AppState {
    let mut state = ready_test_state().await;
    state.workflow_learning_candidates_path = path.to_path_buf();
    install_hosted_policy(&state);
    state
}

fn raw_files(root: &Path) -> Vec<Vec<u8>> {
    fn visit(path: &Path, output: &mut Vec<Vec<u8>>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, output);
            } else {
                output.push(std::fs::read(path).unwrap());
            }
        }
    }
    let mut output = Vec::new();
    visit(root, &mut output);
    output
}

async fn assert_cold_snapshot(path: &Path, expected: &WorkflowLearningCandidate) {
    let cold = hosted_state(path).await;
    with_hosted_candidate_crypto(cold.load_workflow_learning_candidates())
        .await
        .expect("fresh hosted state decrypts the complete candidate map");
    let loaded = cold
        .get_workflow_learning_candidate(&expected.candidate_id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(loaded).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

#[tokio::test]
#[serial]
async fn complete_hosted_snapshot_hides_all_markers_and_cold_reopens_private_binding() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("candidates.json");
    let state = hosted_state(&path).await;
    let owner = "owner-private-alice-9ac7cb2e";
    let proposed = candidate("complete-map", owner);
    let saved = with_hosted_candidate_crypto(state.put_workflow_learning_candidate(proposed))
        .await
        .unwrap();
    let raw = tokio::fs::read_to_string(&path).await.unwrap();
    assert!(raw.starts_with(crate::encrypted_file_store::SCOPED_RECORD_PREFIX));
    let markers = [
        "sealed-complete-map-owner-private-alice-9ac7cb2e-synthetic",
        "summary:sealed-complete-map-owner-private-alice-9ac7cb2e-synthetic",
        "memory:sealed-complete-map-owner-private-alice-9ac7cb2e-synthetic",
        "prompt:sealed-complete-map-owner-private-alice-9ac7cb2e-synthetic",
        "artifact://sealed-complete-map-owner-private-alice-9ac7cb2e-synthetic",
        owner,
        "org-candidate",
    ];
    for content in raw_files(root.path()) {
        for marker in markers {
            assert!(
                !content
                    .windows(marker.len())
                    .any(|window| window == marker.as_bytes()),
                "candidate data leaked into a persisted file"
            );
        }
    }
    assert_cold_snapshot(&path, &saved).await;
    assert!(
        matches!(saved.source_binding, Some(WorkflowLearningCandidateSourceBinding::Session { ref subject, .. }) if subject == owner)
    );
}

#[tokio::test]
#[serial]
async fn tamper_schema_and_wrong_context_load_fail_without_erasing_cache() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("candidates.json");
    let state = hosted_state(&path).await;
    let row = with_hosted_candidate_crypto(
        state.put_workflow_learning_candidate(candidate("sealed", "alice")),
    )
    .await
    .unwrap();
    let original = tokio::fs::read_to_string(&path).await.unwrap();
    let context = workflow_learning_candidate_storage_context();
    let altered = with_hosted_candidate_crypto(async {
        let plaintext =
            crate::encrypted_file_store::decrypt_text_required(&original, &context).unwrap();
        let mut snapshot: Value = serde_json::from_str(&plaintext).unwrap();
        snapshot["schema_version"] = json!(99);
        crate::encrypted_file_store::encrypt_text_required(&snapshot.to_string(), &context).unwrap()
    })
    .await;
    let mut wrong_context = context.clone();
    wrong_context.audit_id.push_str(":untrusted-replay");
    let rebound = with_hosted_candidate_crypto(async {
        let plaintext =
            crate::encrypted_file_store::decrypt_text_required(&original, &context).unwrap();
        crate::encrypted_file_store::encrypt_text_required(&plaintext, &wrong_context).unwrap()
    })
    .await;
    let mut corrupt: Value = serde_json::from_str(original.strip_prefix("tgs1:").unwrap()).unwrap();
    corrupt["ciphertext"] = json!("tce1:broken-synthetic-ciphertext");
    let corrupt = format!("tgs1:{corrupt}");
    let mut wrong_key: Value =
        serde_json::from_str(original.strip_prefix("tgs1:").unwrap()).unwrap();
    wrong_key["envelope"]["kek_id"] =
        json!("projects/test/locations/global/keyRings/tandem/cryptoKeys/other-key");
    let wrong_key = format!("tgs1:{wrong_key}");
    for invalid in [
        altered,
        rebound,
        corrupt,
        wrong_key,
        "tgs1:{malformed".into(),
    ] {
        tokio::fs::write(&path, &invalid).await.unwrap();
        assert!(
            with_hosted_candidate_crypto(state.load_workflow_learning_candidates())
                .await
                .is_err()
        );
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), invalid);
        assert_eq!(
            serde_json::to_value(
                state
                    .get_workflow_learning_candidate("sealed")
                    .await
                    .unwrap()
            )
            .unwrap(),
            serde_json::to_value(&row).unwrap()
        );
    }
    tokio::fs::write(&path, &original).await.unwrap();
    assert_cold_snapshot(&path, &row).await;
}

#[tokio::test]
#[serial]
async fn hosted_crypto_failure_and_legacy_plaintext_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("candidates.json");
    let state = hosted_state(&path).await;
    let saved = with_hosted_candidate_crypto(
        state.put_workflow_learning_candidate(candidate("baseline", "alice")),
    )
    .await
    .unwrap();
    let before = tokio::fs::read(&path).await.unwrap();
    for (label, provider, principal) in [
        (
            "KMS wrap unavailable",
            hosted_provider(true, false),
            Some(RUNTIME_PRINCIPAL),
        ),
        (
            "plaintext provider",
            MemoryCryptoProvider::plaintext(),
            None,
        ),
        (
            "local encrypted provider",
            MemoryCryptoProvider::from_mode(tandem_memory::MemoryCryptoMode::LocalEncrypted {
                provider: "local-file".into(),
            }),
            None,
        ),
        (
            "missing runtime principal",
            hosted_provider(false, false),
            None,
        ),
        (
            "wrong runtime principal",
            hosted_provider(false, false),
            Some("wrong-runtime-principal"),
        ),
    ] {
        let result = crate::encrypted_file_store::with_test_crypto_provider(
            provider,
            principal,
            state.put_workflow_learning_candidate(candidate("forbidden", "bob")),
        )
        .await;
        assert!(
            result.is_err(),
            "hosted publication accepted an unavailable or wrong crypto authority: {label}"
        );
        assert_eq!(tokio::fs::read(&path).await.unwrap(), before);
        assert!(state
            .get_workflow_learning_candidate("forbidden")
            .await
            .is_none());
    }
    assert_eq!(
        serde_json::to_value(
            state
                .get_workflow_learning_candidate("baseline")
                .await
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(&saved).unwrap()
    );
    let cold = hosted_state(&path).await;
    assert!(crate::encrypted_file_store::with_test_crypto_provider(
        hosted_provider(false, true),
        Some(RUNTIME_PRINCIPAL),
        cold.load_workflow_learning_candidates()
    )
    .await
    .is_err());
    assert!(cold
        .get_workflow_learning_candidate("baseline")
        .await
        .is_none());
    assert_eq!(tokio::fs::read(&path).await.unwrap(), before);
    assert!(crate::encrypted_file_store::with_test_crypto_provider(
        hosted_provider(false, false),
        Some("wrong-runtime-principal"),
        cold.load_workflow_learning_candidates()
    )
    .await
    .is_err());
    assert!(cold
        .get_workflow_learning_candidate("baseline")
        .await
        .is_none());
    assert_eq!(tokio::fs::read(&path).await.unwrap(), before);
    let legacy = serde_json::to_vec(&HashMap::from([("baseline".to_string(), saved)])).unwrap();
    tokio::fs::write(&path, &legacy).await.unwrap();
    assert!(
        with_hosted_candidate_crypto(cold.load_workflow_learning_candidates())
            .await
            .is_err()
    );
    assert!(cold
        .get_workflow_learning_candidate("baseline")
        .await
        .is_none());
    assert_eq!(tokio::fs::read(&path).await.unwrap(), legacy);
}

#[tokio::test]
#[serial]
async fn genuine_local_legacy_map_migrates_before_cache_replacement() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("candidates.json");
    let mut state = ready_test_state().await;
    state.workflow_learning_candidates_path = path.clone();
    let mut row = candidate("local-legacy", "alice");
    row.source_binding = None;
    let raw =
        serde_json::to_vec(&HashMap::from([(row.candidate_id.clone(), row.clone())])).unwrap();
    tokio::fs::write(&path, &raw).await.unwrap();
    assert!(state
        .get_workflow_learning_candidate("local-legacy")
        .await
        .is_none());
    state
        .load_workflow_learning_candidates()
        .await
        .expect("genuine local legacy map migrates");
    let migrated = tokio::fs::read_to_string(&path).await.unwrap();
    assert!(migrated.starts_with("tgs1:"));
    assert!(!migrated.contains("sealed-local-legacy-alice-synthetic"));
    let loaded = state
        .get_workflow_learning_candidate("local-legacy")
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(loaded).unwrap(),
        serde_json::to_value(&row).unwrap()
    );
    let mut cold = ready_test_state().await;
    cold.workflow_learning_candidates_path = path;
    cold.load_workflow_learning_candidates().await.unwrap();
    assert_eq!(
        serde_json::to_value(
            cold.get_workflow_learning_candidate("local-legacy")
                .await
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(&row).unwrap()
    );
}

#[tokio::test]
#[serial]
async fn ordinary_put_upsert_update_seal_and_cold_roundtrip() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("candidates.json");
    let state = hosted_state(&path).await;
    let first = with_hosted_candidate_crypto(
        state.put_workflow_learning_candidate(candidate("ordinary-operations", "alice")),
    )
    .await
    .unwrap();
    let first_raw = tokio::fs::read_to_string(&path).await.unwrap();
    assert!(first_raw.starts_with("tgs1:"));
    assert!(!first_raw.contains(&first.summary));

    let mut proposal = candidate("ordinary-operations", "alice");
    proposal.summary = "upserted-synthetic-private-summary".into();
    let merged = with_hosted_candidate_crypto(state.upsert_workflow_learning_candidate(proposal))
        .await
        .unwrap();
    assert_eq!(merged.summary, "upserted-synthetic-private-summary");
    let second_raw = tokio::fs::read_to_string(&path).await.unwrap();
    assert!(second_raw.starts_with("tgs1:"));
    assert!(!second_raw.contains(&merged.summary));

    let updated = with_hosted_candidate_crypto(
        state.update_workflow_learning_candidate("ordinary-operations", |row| {
            row.proposed_revision_prompt = Some("updated-synthetic-private-prompt".into())
        }),
    )
    .await
    .unwrap()
    .expect("existing candidate updated");
    let third_raw = tokio::fs::read_to_string(&path).await.unwrap();
    assert!(third_raw.starts_with("tgs1:"));
    assert!(!third_raw.contains("updated-synthetic-private-prompt"));
    assert_cold_snapshot(&path, &updated).await;

    let before_missing = tokio::fs::read(&path).await.unwrap();
    assert!(with_hosted_candidate_crypto(
        state.update_workflow_learning_candidate("missing-candidate", |_| panic!(
            "missing row must not invoke update"
        ),)
    )
    .await
    .unwrap()
    .is_none());
    assert_eq!(tokio::fs::read(&path).await.unwrap(), before_missing);
}

#[tokio::test]
#[serial]
async fn injected_write_and_sync_failures_preserve_cache_durable_bytes_and_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("candidates.json");
    let state = hosted_state(&path).await;
    let saved = with_hosted_candidate_crypto(
        state.put_workflow_learning_candidate(candidate("baseline", "alice")),
    )
    .await
    .unwrap();
    let original = tokio::fs::read(&path).await.unwrap();
    for (fault, expected) in [
        (
            WorkflowLearningPreparationFaultForTest::Write,
            "injected candidate write failure",
        ),
        (
            WorkflowLearningPreparationFaultForTest::Sync,
            "injected candidate sync failure",
        ),
    ] {
        for operation in ["put", "upsert", "update"] {
            let result: anyhow::Result<()> = with_hosted_candidate_crypto(async {
                match operation {
                    "put" => state
                        .put_workflow_learning_candidate_with_preparation_fault_for_test(
                            candidate("failed-preparation", "bob"),
                            fault,
                        )
                        .await
                        .map(|_| ()),
                    "upsert" => state
                        .upsert_workflow_learning_candidate_with_preparation_fault_for_test(
                            candidate("failed-preparation", "bob"),
                            fault,
                        )
                        .await
                        .map(|_| ()),
                    "update" => state
                        .update_workflow_learning_candidate_with_preparation_fault_for_test(
                            "baseline",
                            |row| row.summary = "must-not-publish-private-update".into(),
                            fault,
                        )
                        .await
                        .map(|_| ()),
                    _ => unreachable!(),
                }
            })
            .await;
            let error = result.expect_err("injected prepublication I/O failure must propagate");
            assert!(
                format!("{error:?}").contains(expected),
                "unexpected {operation} fault: {error:?}"
            );
            assert_eq!(tokio::fs::read(&path).await.unwrap(), original);
            assert!(state
                .get_workflow_learning_candidate("failed-preparation")
                .await
                .is_none());
            assert_eq!(
                serde_json::to_value(
                    state
                        .get_workflow_learning_candidate("baseline")
                        .await
                        .unwrap()
                )
                .unwrap(),
                serde_json::to_value(&saved).unwrap()
            );
            assert!(
                !std::fs::read_dir(root.path()).unwrap().any(|entry| entry
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "tmp")),
                "failed preparation left a candidate temp file"
            );
        }
    }
    assert_cold_snapshot(&path, &saved).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn prepared_file_real_rename_failure_preserves_original_and_cleans_temp() {
    for operation in ["put", "upsert", "update"] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("candidates.json");
        let state = hosted_state(&path).await;
        let saved = with_hosted_candidate_crypto(
            state.put_workflow_learning_candidate(candidate("baseline", "alice")),
        )
        .await
        .unwrap();
        let original = tokio::fs::read(&path).await.unwrap();
        let (prepared, seen_prepared) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let gate = WorkflowLearningPreparedFileGateForTest {
            prepared,
            release: released,
        };
        let owned = state.clone();
        let task = tokio::spawn(async move {
            with_hosted_candidate_crypto(async {
                match operation {
                    "put" => owned
                        .put_workflow_learning_candidate_with_prepared_file_gate_for_test(
                            candidate("must-not-publish", "bob"),
                            gate,
                        )
                        .await
                        .map(|_| ()),
                    "upsert" => owned
                        .upsert_workflow_learning_candidate_with_prepared_file_gate_for_test(
                            candidate("must-not-publish", "bob"),
                            gate,
                        )
                        .await
                        .map(|_| ()),
                    "update" => owned
                        .update_workflow_learning_candidate_with_prepared_file_gate_for_test(
                            "baseline",
                            |row| row.summary = "must-not-publish-private-update".into(),
                            gate,
                        )
                        .await
                        .map(|_| ()),
                    _ => unreachable!(),
                }
            })
            .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), seen_prepared)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            tokio::fs::read(&path).await.unwrap(),
            original,
            "prepared encrypted file must not publish before the gate"
        );
        let prepared_paths = std::fs::read_dir(root.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "tmp"))
            .collect::<Vec<_>>();
        assert_eq!(
            prepared_paths.len(),
            1,
            "gate must witness one fully written candidate temp file"
        );
        let temp_bytes = std::fs::read(&prepared_paths[0]).unwrap();
        assert!(temp_bytes.starts_with(b"tgs1:"));
        assert!(!temp_bytes
            .windows(b"sealed-must-not-publish-bob-synthetic".len())
            .any(|window| window == b"sealed-must-not-publish-bob-synthetic"));

        let saved_path = root.path().join("original-snapshot.backup");
        std::fs::rename(&path, &saved_path).unwrap();
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("marker"), b"unchanged target directory").unwrap();
        release.send(()).unwrap();
        let error = tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .expect_err("OS rename must reject replacing a nonempty directory");
        assert!(
            format!("{error:?}").contains("publish workflow-learning candidate store"),
            "{operation} failed before real rename: {error:?}"
        );
        assert_eq!(std::fs::read(&saved_path).unwrap(), original);
        assert_eq!(
            std::fs::read(path.join("marker")).unwrap(),
            b"unchanged target directory"
        );
        assert!(
            !prepared_paths[0].exists(),
            "failed publication must remove its prepared file"
        );
        assert!(state
            .get_workflow_learning_candidate("must-not-publish")
            .await
            .is_none());
        assert_eq!(
            serde_json::to_value(
                state
                    .get_workflow_learning_candidate("baseline")
                    .await
                    .unwrap()
            )
            .unwrap(),
            serde_json::to_value(&saved).unwrap()
        );
        std::fs::remove_file(path.join("marker")).unwrap();
        std::fs::remove_dir(&path).unwrap();
        std::fs::rename(saved_path, &path).unwrap();
        assert_cold_snapshot(&path, &saved).await;
    }
}

#[tokio::test]
#[serial]
async fn startup_rejects_malformed_candidate_store_without_erasing_source() {
    let _env_lock = crate::test_support::TEST_STATE_ENV_LOCK.lock().await;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("candidates.json");
    let source = br#"{"candidate-id":{"not":"a complete candidate"}}"#;
    tokio::fs::write(&path, source).await.unwrap();
    let (mut state, runtime) = starting_test_state_and_runtime().await;
    state.workflow_learning_candidates_path = path.clone();
    let error = state
        .mark_ready(runtime)
        .await
        .expect_err("startup must propagate candidate decode failure");
    assert!(
        format!("{error:?}").contains("workflow-learning candidate"),
        "wrong startup failure: {error:?}"
    );
    assert!(!state.is_ready());
    assert_eq!(tokio::fs::read(&path).await.unwrap().as_slice(), source);
    assert!(state.get_workflow_learning_candidate("any").await.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn warm_hosted_key_failure_after_prepared_file_refuses_publication() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("candidates.json");
    let state = hosted_state(&path).await;
    let fail_unwrap = Arc::new(AtomicBool::new(false));
    let decrypt_count = Arc::new(AtomicUsize::new(0));
    let provider = hosted_provider_with_decrypt_control(
        false,
        false,
        Some(Arc::clone(&decrypt_count)),
        Some(Arc::clone(&fail_unwrap)),
    );
    let saved = crate::encrypted_file_store::with_test_crypto_provider(
        provider.clone(),
        Some(RUNTIME_PRINCIPAL),
        state.put_workflow_learning_candidate(candidate("warm-baseline", "alice")),
    )
    .await
    .unwrap();
    let original = tokio::fs::read(&path).await.unwrap();
    let stored = std::str::from_utf8(&original).unwrap();
    let opened = crate::encrypted_file_store::with_test_crypto_provider(
        provider.clone(),
        Some(RUNTIME_PRINCIPAL),
        async {
            crate::encrypted_file_store::decrypt_text_required(
                stored,
                &workflow_learning_candidate_storage_context(),
            )
        },
    )
    .await
    .unwrap();
    assert!(opened.contains("tandem-workflow-learning-candidates"));
    let before_unwrap = decrypt_count.load(Ordering::SeqCst);
    assert!(
        before_unwrap > 0,
        "the real hosted KMS fixture must have unwrapped a DEK"
    );
    let reopened = crate::encrypted_file_store::with_test_crypto_provider(
        provider.clone(),
        Some(RUNTIME_PRINCIPAL),
        async {
            crate::encrypted_file_store::decrypt_text_required(
                stored,
                &workflow_learning_candidate_storage_context(),
            )
        },
    )
    .await
    .unwrap();
    assert_eq!(reopened, opened);
    assert_eq!(
        decrypt_count.load(Ordering::SeqCst),
        before_unwrap,
        "a second unseal must use the warm DEK cache before the simulated outage"
    );

    let (prepared, seen_prepared) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let gate = WorkflowLearningPreparedFileGateForTest {
        prepared,
        release: released,
    };
    let owned = state.clone();
    let task = tokio::spawn(async move {
        crate::encrypted_file_store::with_test_crypto_provider(
            provider,
            Some(RUNTIME_PRINCIPAL),
            owned.upsert_workflow_learning_candidate_with_prepared_file_gate_for_test(
                candidate("must-not-publish-after-kms-loss", "bob"),
                gate,
            ),
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), seen_prepared)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(tokio::fs::read(&path).await.unwrap(), original);
    let prepared_paths = std::fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "tmp"))
        .collect::<Vec<_>>();
    assert_eq!(
        prepared_paths.len(),
        1,
        "the real encrypted temp file must be prepared"
    );
    assert!(std::fs::read(&prepared_paths[0])
        .unwrap()
        .starts_with(b"tgs1:"));
    fail_unwrap.store(true, Ordering::SeqCst);
    release.send(()).unwrap();
    let error = tokio::time::timeout(std::time::Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .expect_err("the captured warm hosted provider must fail its fresh prepublish unwrap");
    assert!(
        format!("{error:?}").contains("fixture KMS decrypt unavailable"),
        "{error:?}"
    );
    assert!(decrypt_count.load(Ordering::SeqCst) > before_unwrap);
    assert_eq!(tokio::fs::read(&path).await.unwrap(), original);
    assert!(
        !prepared_paths[0].exists(),
        "failed publication must remove its prepared file"
    );
    assert!(state
        .get_workflow_learning_candidate("must-not-publish-after-kms-loss")
        .await
        .is_none());
    assert_eq!(
        serde_json::to_value(
            state
                .get_workflow_learning_candidate("warm-baseline")
                .await
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(&saved).unwrap(),
    );
    let recovered = with_hosted_candidate_crypto(
        state.upsert_workflow_learning_candidate(candidate("healthy-after-kms-recovery", "bob")),
    )
    .await
    .unwrap();
    let cold = hosted_state(&path).await;
    with_hosted_candidate_crypto(cold.load_workflow_learning_candidates())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(
            cold.get_workflow_learning_candidate("warm-baseline")
                .await
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(&saved).unwrap(),
    );
    assert_eq!(
        serde_json::to_value(
            cold.get_workflow_learning_candidate("healthy-after-kms-recovery")
                .await
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(&recovered).unwrap(),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn queued_local_capture_rejects_current_hosted_policy_after_writer_wait() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("candidates.json");
    let policy_path = root.path().join("hosted-policy.json");
    let mut state = ready_test_state().await;
    state.workflow_learning_candidates_path = path.clone();
    let _env_lock = crate::test_support::TEST_STATE_ENV_LOCK.lock().await;
    let _restore = enable_local_file_encryption(&root);
    let local_provider =
        MemoryCryptoProvider::from_mode(tandem_memory::MemoryCryptoMode::LocalEncrypted {
            provider: "local-file".into(),
        });
    assert!(local_provider.is_encrypted_ready());
    let writer = state.workflow_learning_candidates.write().await;
    let pending = crate::encrypted_file_store::with_test_crypto_provider(
        local_provider,
        None,
        state.put_workflow_learning_candidate(candidate("local-capture", "alice")),
    );
    tokio::pin!(pending);
    assert!(
        futures::poll!(pending.as_mut()).is_pending(),
        "the mutation must capture the local provider and await the held candidate writer"
    );

    state
        .enterprise
        .hosted_policy
        .configure_test_source("acme", "acme", policy_path.clone());
    std::fs::write(
        &policy_path,
        serde_json::to_vec(&json!({
            "schema_version": 1, "policy_version": 1,
            "organization_id": "acme", "deployment_id": "acme",
            "generated_at": chrono::Utc::now(), "users": [], "org_units": [],
            "org_unit_memberships": [], "deployment_grants": []
        }))
        .unwrap(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&policy_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    state.reload_hosted_policy().await.unwrap();
    drop(writer);
    let error = tokio::time::timeout(std::time::Duration::from_secs(5), pending)
        .await
        .unwrap()
        .expect_err("the captured local provider cannot publish after hosted policy installation");
    assert!(
        format!("{error:?}").contains("hosted candidate storage requires a hosted KMS provider"),
        "{error:?}"
    );
    assert!(!path.exists());
    assert!(state
        .get_workflow_learning_candidate("local-capture")
        .await
        .is_none());
    let current = with_hosted_candidate_crypto(
        state.put_workflow_learning_candidate(candidate("current-hosted", "alice")),
    )
    .await
    .unwrap();
    drop(_restore);
    drop(_env_lock);
    assert_cold_snapshot(&path, &current).await;
    // Pending on the caller proves capture before policy publication. It does
    // not, by itself, witness the owned task's first writer Poll::Pending.
}

#[tokio::test]
#[serial]
async fn absent_standalone_store_needs_no_key_until_first_required_write() {
    let root = tempfile::tempdir().unwrap();
    let standalone_path = root.path().join("standalone-candidates.json");
    let hosted_path = root.path().join("hosted-candidates.json");
    let mut standalone = ready_test_state().await;
    standalone.workflow_learning_candidates_path = standalone_path.clone();
    let hosted = hosted_state(&hosted_path).await;
    let _env_lock = crate::test_support::TEST_STATE_ENV_LOCK.lock().await;
    let _restore = EnvRestore::capture();
    std::env::remove_var("TANDEM_MEMORY_DECRYPT_PROVIDER");
    std::env::remove_var("TANDEM_MEMORY_ENCRYPTION_REQUIRED");
    std::env::remove_var("TANDEM_MEMORY_DECRYPT_PRINCIPAL_ID");
    let unavailable_key = root.path().join("unavailable-key-directory");
    std::fs::create_dir(&unavailable_key).unwrap();
    std::env::set_var("TANDEM_MEMORY_LOCAL_KEY_FILE", &unavailable_key);

    standalone
        .load_workflow_learning_candidates()
        .await
        .expect("an absent standalone feature store must not resolve an unavailable key");
    assert!(!standalone_path.exists());
    assert!(
        std::fs::read_dir(&unavailable_key)
            .unwrap()
            .next()
            .is_none(),
        "absent load must not create key material"
    );
    let error = standalone
        .put_workflow_learning_candidate(candidate("first-write", "alice"))
        .await
        .expect_err("first required standalone write must resolve and reject the unavailable key");
    assert!(
        format!("{error:?}")
            .contains("required local protected file-store encryption key is unavailable"),
        "{error:?}"
    );
    assert!(!standalone_path.exists());
    assert!(std::fs::read_dir(&unavailable_key)
        .unwrap()
        .next()
        .is_none());
    assert!(standalone
        .get_workflow_learning_candidate("first-write")
        .await
        .is_none());

    let error = crate::encrypted_file_store::with_test_crypto_provider(
        hosted_provider(false, true),
        Some(RUNTIME_PRINCIPAL),
        hosted.load_workflow_learning_candidates(),
    )
    .await
    .expect_err("an absent hosted store still requires a working KMS provider");
    assert!(
        format!("{error:?}").contains("fixture KMS decrypt unavailable"),
        "{error:?}"
    );
    assert!(!hosted_path.exists());
    assert!(hosted
        .get_workflow_learning_candidate("first-write")
        .await
        .is_none());
}

#[tokio::test]
#[serial]
async fn missing_initialized_candidate_store_refuses_load_and_ordinary_mutations() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("candidates.json");
    let state = hosted_state(&path).await;
    let saved = with_hosted_candidate_crypto(
        state.put_workflow_learning_candidate(candidate("retained", "alice")),
    )
    .await
    .unwrap();
    let backup = tokio::fs::read(&path).await.unwrap();
    tokio::fs::remove_file(&path).await.unwrap();

    let load_error = with_hosted_candidate_crypto(state.load_workflow_learning_candidates())
        .await
        .expect_err("missing durable bytes cannot be mistaken for a new empty store");
    assert!(
        format!("{load_error:?}").contains("missing after initialization"),
        "{load_error:?}"
    );
    for operation in ["put", "upsert", "update"] {
        let result: anyhow::Result<()> = with_hosted_candidate_crypto(async {
            match operation {
                "put" => state
                    .put_workflow_learning_candidate(candidate("resurrected", "bob"))
                    .await
                    .map(|_| ()),
                "upsert" => state
                    .upsert_workflow_learning_candidate(candidate("resurrected", "bob"))
                    .await
                    .map(|_| ()),
                "update" => state
                    .update_workflow_learning_candidate("retained", |row| {
                        row.summary = "must-not-resurrect-private-summary".into();
                    })
                    .await
                    .map(|_| ()),
                _ => unreachable!(),
            }
        })
        .await;
        let error =
            result.expect_err("ordinary mutation cannot recreate a missing initialized store");
        assert!(
            format!("{error:?}").contains("missing after initialization"),
            "wrong {operation} failure: {error:?}"
        );
        assert!(!path.exists());
        assert!(state
            .get_workflow_learning_candidate("resurrected")
            .await
            .is_none());
        assert_eq!(
            serde_json::to_value(
                state
                    .get_workflow_learning_candidate("retained")
                    .await
                    .unwrap()
            )
            .unwrap(),
            serde_json::to_value(&saved).unwrap(),
        );
    }
    tokio::fs::write(&path, &backup).await.unwrap();
    assert_cold_snapshot(&path, &saved).await;
}
