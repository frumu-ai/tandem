// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use std::future::Future;
use crate::app::state::tests::encrypted_file_stores::with_hosted_candidate_crypto;

#[path = "derived_lineage_audit_admission_tests.rs"]
mod audit_admission_tests;

fn candidate(id: &str, summary: &str) -> WorkflowLearningCandidate {
    WorkflowLearningCandidate {
        candidate_id: id.into(), workflow_id: "session:commit-candidate-session".into(),
        project_id: "memory-commit-boundary".into(), source_run_id: "candidate-commit-run".into(),
        source_binding: None, kind: WorkflowLearningCandidateKind::MemoryFact,
        status: WorkflowLearningCandidateStatus::Proposed, confidence: 0.8, summary: summary.into(),
        fingerprint: "candidate-commit-fingerprint".into(), node_id: None, node_kind: None,
        validator_family: None, evidence_refs: Vec::new(), artifact_refs: Vec::new(),
        proposed_memory_payload: Some(json!({"content":summary,"kind":"fact","classification":"internal"})),
        proposed_revision_prompt: None, source_memory_id: None, promoted_memory_id: None,
        needs_plan_bundle: false, baseline_before: None, latest_observed_metrics: None,
        last_revision_session_id: None, run_ids: vec!["candidate-commit-run".into()],
        created_at_ms: 0, updated_at_ms: 0,
    }
}

async fn seed_candidate(state: &AppState) -> (Value, Vec<u8>) {
    with_hosted_candidate_crypto(state.upsert_workflow_learning_candidate(
        candidate("original-candidate", "Original synthetic fact"),
    )).await.expect("ordinary candidate compatibility control");
    (
        serde_json::to_value(&*state.workflow_learning_candidates.read().await).unwrap(),
        tokio::fs::read(&state.workflow_learning_candidates_path).await.unwrap(),
    )
}

async fn assert_sealed_durable_matches_cache(state: &AppState) {
    let raw = tokio::fs::read_to_string(&state.workflow_learning_candidates_path).await.unwrap();
    assert!(raw.starts_with(crate::encrypted_file_store::SCOPED_RECORD_PREFIX),
        "candidate snapshot must be sealed, not a readable JSON map");
    let mut cold = crate::test_support::test_state().await;
    cold.workflow_learning_candidates_path = state.workflow_learning_candidates_path.clone();
    with_hosted_candidate_crypto(cold.load_workflow_learning_candidates()).await
        .expect("fresh state opens the complete hosted candidate snapshot");
    assert_eq!(
        serde_json::to_value(&*state.workflow_learning_candidates.read().await).unwrap(),
        serde_json::to_value(&*cold.workflow_learning_candidates.read().await).unwrap(),
        "durable candidates must match the current cache after cold decrypt",
    );
}

async fn assert_candidates_unchanged(state: &AppState, before: &(Value, Vec<u8>)) {
    assert_eq!(serde_json::to_value(&*state.workflow_learning_candidates.read().await).unwrap(), before.0);
    assert_eq!(tokio::fs::read(&state.workflow_learning_candidates_path).await.unwrap(), before.1);
}

async fn finish_candidate(
    task: tokio::task::JoinHandle<anyhow::Result<WorkflowLearningCandidate>>,
) -> anyhow::Result<WorkflowLearningCandidate> {
    tokio::time::timeout(Duration::from_secs(4), task).await.unwrap().unwrap()
}

// Tokio's RwLock queues writers before later readers. Poll a reader while the
// test holds the writer, then require the candidate to own/precede that reader
// after release. This witnesses a queued candidate before authority changes.
async fn queue_reader_behind_candidate<T>(
    state: &AppState,
    task: &tokio::task::JoinHandle<T>,
) -> tokio::task::JoinHandle<()> {
    let rows = state.workflow_learning_candidates.clone();
    let mut reader = Box::pin(async move {
        let _guard = rows.read().await;
    });
    std::future::poll_fn(|cx| {
        assert!(reader.as_mut().poll(cx).is_pending(), "test writer must hold the cache");
        std::task::Poll::Ready(())
    }).await;
    assert!(!task.is_finished(), "candidate must remain pending with the cache writer held");
    tokio::spawn(reader)
}

async fn witness_candidate_before_reader(state: &AppState, reader: &tokio::task::JoinHandle<()>) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            assert!(!reader.is_finished(), "later queued reader overtook the candidate writer");
            if state.workflow_learning_candidates.try_read().is_err() {
                break;
            }
            tokio::task::yield_now().await;
        }
    }).await.expect("candidate writer must be queued ahead of the later reader");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_candidate_commit_rechecks_original_identity_after_real_cache_writer_wait() {
    for expired in [false, true] {
        let fixture = CommitFixture::new().await;
        let before = seed_candidate(&fixture.state).await;
        let writer = fixture.state.workflow_learning_candidates.write().await;
        let original = fixture.identity(if expired {1_500} else {60_000});
        fixture.state.enterprise.hosted_policy.authorize(Some(&original)).unwrap();
        let state = fixture.state.clone();
        let authority = derived_memory_commit_authority(&fixture.state, &original.tenant_context, Some(&original));
        let (queued, receiver) = oneshot::channel();
        let task = tokio::spawn(async move {
            with_hosted_candidate_crypto(state.upsert_workflow_learning_candidate_with_commit_authority_and_writer_wait(
                candidate("new-candidate", "Updated synthetic fact"), authority, queued,
            )).await
        });
        started(receiver).await;
        let reader = queue_reader_behind_candidate(&fixture.state, &task).await;
        assert!(!fixture.state.enterprise.hosted_policy.publication_mutex_locked_for_test(),
            "candidate wait must not prevent current policy publication");
        let publication = fixture.state.enterprise.hosted_policy.lock_publication_owned().await;
        assert_eq!(serde_json::to_value(&*writer).unwrap(), before.0);
        assert_eq!(tokio::fs::read(&fixture.state.workflow_learning_candidates_path).await.unwrap(), before.1);
        if expired { expire_while_writer_is_held(&fixture, &original).await; }
        drop(writer);
        witness_candidate_before_reader(&fixture.state, &reader).await;
        drop(publication);
        let result = finish_candidate(task).await;
        reader.await.unwrap();
        if expired {
            assert!(result.unwrap_err().to_string().contains("hosted_policy_or_identity_expired"));
            assert_candidates_unchanged(&fixture.state, &before).await;
        } else {
            assert_eq!(result.unwrap().candidate_id, "original-candidate", "reuse the ordinary merge semantics");
            let cache = serde_json::to_value(&*fixture.state.workflow_learning_candidates.read().await).unwrap();
            assert_sealed_durable_matches_cache(&fixture.state).await;
            assert_eq!(cache["original-candidate"]["summary"], "Updated synthetic fact");
        }
    }
    let state = crate::test_support::test_state().await;
    let row = state.upsert_workflow_learning_candidate_with_current_policy(candidate("standalone-candidate", "Standalone fact"), None)
        .await.expect("genuine standalone candidate publication");
    assert_eq!(state.get_workflow_learning_candidate(&row.candidate_id).await.unwrap().summary, "Standalone fact");
    let raw = tokio::fs::read_to_string(&state.workflow_learning_candidates_path).await.unwrap();
    assert!(raw.starts_with(crate::encrypted_file_store::SCOPED_RECORD_PREFIX));
    assert!(!raw.contains("Standalone fact"));
    let mut cold = crate::test_support::test_state().await;
    cold.workflow_learning_candidates_path = state.workflow_learning_candidates_path.clone();
    cold.load_workflow_learning_candidates().await.expect("standalone candidate cold reload");
    assert_eq!(cold.get_workflow_learning_candidate(&row.candidate_id).await.unwrap().summary, "Standalone fact");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_candidate_commit_rechecks_snapshot_expiry_after_real_cache_writer_wait() {
    let fixture = CommitFixture::new_with_policy_remaining(Some(3_000)).await;
    let before = seed_candidate(&fixture.state).await;
    let writer = fixture.state.workflow_learning_candidates.write().await;
    let original = fixture.identity(60_000);
    let expiry = fixture.state.enterprise.hosted_policy.current().unwrap().unwrap().expires_at_ms();
    let state = fixture.state.clone();
    let authority = derived_memory_commit_authority(&fixture.state, &original.tenant_context, Some(&original));
    let (queued, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        with_hosted_candidate_crypto(state.upsert_workflow_learning_candidate_with_commit_authority_and_writer_wait(
            candidate("aged-candidate", "Expired snapshot fact"), authority, queued,
        )).await
    });
    started(receiver).await;
    let reader = queue_reader_behind_candidate(&fixture.state, &task).await;
    let publication = fixture.state.enterprise.hosted_policy.lock_publication_owned().await;
    assert!(crate::now_ms() < expiry);
    tokio::time::timeout(Duration::from_secs(4), async {
        while crate::now_ms() < expiry { tokio::time::sleep(Duration::from_millis(20)).await; }
    }).await.unwrap();
    assert!(!original.is_expired_at(crate::now_ms()));
    assert!(!task.is_finished());
    drop(writer);
    witness_candidate_before_reader(&fixture.state, &reader).await;
    drop(publication);
    assert!(finish_candidate(task).await.unwrap_err().to_string().contains("hosted_policy_or_identity_expired"));
    reader.await.unwrap();
    assert_candidates_unchanged(&fixture.state, &before).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_candidate_commit_denies_actual_policy_removal_during_cache_writer_wait() {
    let fixture = CommitFixture::new().await;
    let before = seed_candidate(&fixture.state).await;
    let writer = fixture.state.workflow_learning_candidates.write().await;
    let original = fixture.identity(60_000);
    let state = fixture.state.clone();
    let authority = derived_memory_commit_authority(&fixture.state, &original.tenant_context, Some(&original));
    let (queued, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        with_hosted_candidate_crypto(state.upsert_workflow_learning_candidate_with_commit_authority_and_writer_wait(
            candidate("revoked-candidate", "Revoked fact"), authority, queued,
        )).await
    });
    started(receiver).await;
    let reader = queue_reader_behind_candidate(&fixture.state, &task).await;
    let path = fixture.directory.path().join("policy.json");
    write_policy(&path, 5);
    let mut bundle: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    bundle["users"][0]["is_active"] = json!(false);
    std::fs::write(&path, serde_json::to_vec(&bundle).unwrap()).unwrap();
    fixture.state.reload_hosted_policy().await.expect("real disabled-subject policy published while candidate writer held");
    assert_eq!(fixture.state.enterprise.hosted_policy.current().unwrap().unwrap().revision().version, 5);
    assert!(fixture.state.enterprise.hosted_policy.authorize(Some(&original)).is_err());
    assert_eq!(serde_json::to_value(&*writer).unwrap(), before.0);
    assert_eq!(tokio::fs::read(&fixture.state.workflow_learning_candidates_path).await.unwrap(), before.1);
    let publication = fixture.state.enterprise.hosted_policy.lock_publication_owned().await;
    drop(writer);
    witness_candidate_before_reader(&fixture.state, &reader).await;
    drop(publication);
    assert!(finish_candidate(task).await.unwrap_err().to_string()
        .contains("hosted_identity_policy_revision_changed"));
    reader.await.unwrap();
    assert_candidates_unchanged(&fixture.state, &before).await;
    assert!(with_hosted_candidate_crypto(fixture.state.upsert_workflow_learning_candidate_with_current_policy(
        candidate("forged-local", "Must not publish"), None,
    )).await.is_err(), "configured hosted authority cannot be bypassed with absent identity");
    assert_candidates_unchanged(&fixture.state, &before).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_candidate_commit_rechecks_canonical_source_ttl_after_real_cache_writer_wait() {
    for expired in [false, true] {
        let fixture = CommitFixture::new().await;
        let original = fixture.identity(60_000);
        let source_expiry = crate::now_ms() + if expired { 5_000 } else { 60_000 };
        let (_, lineage) = promotion_record_with_source_expiry(
            &fixture, &original, Some(source_expiry),
        ).await;
        let authority = source_bound_authority(&fixture, &original, lineage.clone(), None, None).await;
        authority().expect("source and original identity live at admission");
        let probe = authority.clone();
        let before = seed_candidate(&fixture.state).await;
        let mut proposal = candidate("source-bound-candidate", "Current canonical source fact");
        proposal.proposed_memory_payload.as_mut().unwrap()["metadata"] =
            metadata_with_derived_lineage(None, &lineage).unwrap().unwrap();
        let writer = fixture.state.workflow_learning_candidates.write().await;
        let state = fixture.state.clone();
        let (queued, receiver) = oneshot::channel();
        let task = tokio::spawn(async move {
            with_hosted_candidate_crypto(state.upsert_workflow_learning_candidate_with_commit_authority_and_writer_wait(
                proposal, authority, queued,
            )).await
        });
        started(receiver).await;
        let reader = queue_reader_behind_candidate(&fixture.state, &task).await;
        let publication = fixture.state.enterprise.hosted_policy.lock_publication_owned().await;
        assert!(crate::now_ms() < source_expiry, "real contributor was live at writer admission");
        if expired {
            tokio::time::timeout(Duration::from_secs(6), async {
                while crate::now_ms() < source_expiry {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }).await.expect("canonical source TTL naturally expired while cache writer held");
            let denied = probe().unwrap_err();
            assert_eq!(denied.kind, MemoryStoreErrorKind::ScopeViolation);
            assert_eq!(denied.message, "derived_memory_source_authority_expired_or_denied");
        } else {
            probe().expect("current source remains authorized");
        }
        assert!(!original.is_expired_at(crate::now_ms()));
        fixture.state.enterprise.hosted_policy.authorize(Some(&original))
            .expect("original identity and policy remain live across contributor expiry");
        assert_eq!(serde_json::to_value(&*writer).unwrap(), before.0);
        drop(writer);
        witness_candidate_before_reader(&fixture.state, &reader).await;
        drop(publication);
        let result = finish_candidate(task).await;
        reader.await.unwrap();
        if expired {
            assert!(result.unwrap_err().to_string().contains("derived_memory_source_authority_expired_or_denied"));
            assert_candidates_unchanged(&fixture.state, &before).await;
        } else {
            assert_eq!(result.unwrap().candidate_id, "source-bound-candidate");
            let rows = serde_json::to_value(&*fixture.state.workflow_learning_candidates.read().await).unwrap();
            assert_sealed_durable_matches_cache(&fixture.state).await;
            assert!(rows.get("source-bound-candidate").is_some());
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_cancelled_candidate_commit_retains_real_writer_until_authorized_publication() {
    for expired in [false, true] {
        let fixture = CommitFixture::new().await;
        let before = seed_candidate(&fixture.state).await;
        let publication = fixture.state.enterprise.hosted_policy.lock_publication_owned().await;
        let original = fixture.identity(if expired {1_500} else {60_000});
        let state = fixture.state.clone(); let verified = original.clone();
        let (entered, receiver) = oneshot::channel();
        let task = tokio::spawn(async move {
            entered.send(()).unwrap();
            with_hosted_candidate_crypto(state.upsert_workflow_learning_candidate_with_current_policy(
                candidate("cancelled-candidate", "Owned completed fact"), Some(verified),
            )).await
        });
        started(receiver).await;
        tokio::time::timeout(Duration::from_secs(3), async {
            while fixture.state.workflow_learning_candidates.try_write().is_ok() { tokio::task::yield_now().await; }
        }).await.expect("owned task acquired actual candidate writer before its publication wait");
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(fixture.state.workflow_learning_candidates.try_write().is_err(), "outer cancellation cannot release the owned writer");
        if expired { expire_while_writer_is_held(&fixture, &original).await; }
        assert_eq!(tokio::fs::read(&fixture.state.workflow_learning_candidates_path).await.unwrap(), before.1);
        drop(publication);
        let rows = tokio::time::timeout(Duration::from_secs(4), fixture.state.workflow_learning_candidates.read()).await.unwrap();
        let cache = serde_json::to_value(&*rows).unwrap();
        drop(rows);
        assert_sealed_durable_matches_cache(&fixture.state).await;
        if expired { assert_candidates_unchanged(&fixture.state, &before).await; }
        else { assert_eq!(cache["original-candidate"]["summary"], "Owned completed fact"); }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_candidate_atomic_publication_failure_preserves_cache_and_cleans_prepared_file() {
    let fixture = CommitFixture::new().await;
    let before = seed_candidate(&fixture.state).await;
    let original_path = fixture.state.workflow_learning_candidates_path.clone();
    let backup = original_path.with_extension("candidate-original-backup");
    let candidate_directory = original_path.parent().unwrap();
    let original = fixture.identity(60_000);
    let authority = derived_memory_commit_authority(
        &fixture.state, &original.tenant_context, Some(&original),
    );
    let (prepared, seen_prepared) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let gate = WorkflowLearningPreparedFileGateForTest { prepared, release: released };
    let state = fixture.state.clone();
    let task = tokio::spawn(async move {
        with_hosted_candidate_crypto(state.upsert_workflow_learning_candidate_with_commit_authority_and_prepared_file_gate_for_test(
            candidate("failed-candidate", "Must not enter cache"), authority, gate,
        )).await
    });
    started(seen_prepared).await;
    assert_eq!(std::fs::read(&original_path).unwrap(), before.1,
        "encrypted preparation must not publish before its actual rename");
    let prepared_paths = std::fs::read_dir(candidate_directory).unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "tmp"))
        .collect::<Vec<_>>();
    assert_eq!(prepared_paths.len(), 1, "test must reach a written and synced prepared file");
    assert!(std::fs::read(&prepared_paths[0]).unwrap().starts_with(b"tgs1:"));
    std::fs::rename(&original_path, &backup).unwrap();
    std::fs::create_dir(&original_path).unwrap();
    std::fs::write(original_path.join("marker"), b"unchanged durable directory").unwrap();
    release.send(()).unwrap();
    let error = finish_candidate(task).await.expect_err("atomic rename must reject a nonempty directory");
    assert!(format!("{error:?}").contains("publish workflow-learning candidate store"));
    assert_eq!(serde_json::to_value(&*fixture.state.workflow_learning_candidates.read().await).unwrap(), before.0);
    assert_eq!(std::fs::read(&backup).unwrap(), before.1);
    assert_eq!(std::fs::read(original_path.join("marker")).unwrap(), b"unchanged durable directory");
    assert!(!prepared_paths[0].exists(), "failed rename must remove its prepared file");
    assert!(std::fs::read_dir(candidate_directory).unwrap().all(|entry|
        !entry.unwrap().path().extension().is_some_and(|extension| extension == "tmp")),
        "failed prepared files must be removed");
    std::fs::remove_file(original_path.join("marker")).unwrap();
    std::fs::remove_dir(&original_path).unwrap();
    std::fs::rename(backup, &original_path).unwrap();
    assert_candidates_unchanged(&fixture.state, &before).await;
    assert_sealed_durable_matches_cache(&fixture.state).await;
    for (fault, expected) in [
        (WorkflowLearningPreparationFaultForTest::Write, "injected candidate write failure"),
        (WorkflowLearningPreparationFaultForTest::Sync, "injected candidate sync failure"),
    ] {
        let authority = derived_memory_commit_authority(
            &fixture.state, &original.tenant_context, Some(&original),
        );
        let error = with_hosted_candidate_crypto(
            fixture.state.upsert_workflow_learning_candidate_with_commit_authority_and_preparation_fault_for_test(
                candidate("failed-preparation", "Must not enter cache"), authority, fault,
            ),
        ).await.expect_err("guarded candidate prepublication fault must propagate");
        assert!(format!("{error:?}").contains(expected), "unexpected guarded fault: {error:?}");
        assert_candidates_unchanged(&fixture.state, &before).await;
        assert!(std::fs::read_dir(candidate_directory).unwrap().all(|entry|
            !entry.unwrap().path().extension().is_some_and(|extension| extension == "tmp")),
            "guarded preparation fault left a temporary candidate file");
    }
}

async fn promotion_record_with_source_controls(
    fixture: &CommitFixture,
    original: &VerifiedTenantContext,
    source_expiry: Option<u64>,
    knowledge_scope: Option<tandem_memory::KnowledgeScopePolicy>,
) -> (String, DerivedMemoryLineage) {
    let store = open_global_memory_store_for_state(&fixture.state).await.unwrap();
    let mut source = record(&original.tenant_context, "promotion-canonical-source");
    source.metadata.as_mut().unwrap()["owner_org_unit_id"] = json!("unit-memory-commit");
    source.expires_at_ms = source_expiry;
    if let Some(policy) = &knowledge_scope {
        source.metadata = tandem_memory::metadata_with_knowledge_scope(source.metadata, policy);
    }
    let tenant = MemoryTenantScope {org_id:original.tenant_context.org_id.clone(),
        workspace_id:original.tenant_context.workspace_id.clone(),deployment_id:original.tenant_context.deployment_id.clone()};
    let write_scope = tandem_memory::MemoryWriteScope {tenant:tenant.clone(),org_unit:Some("unit-memory-commit".into()),subject:Some("alice".into())};
    with_verified_memory_decrypt_principal(Some(original), store.write(tandem_memory::MemoryStoreWriteRequest::GlobalRecord {
        scope:write_scope.clone(),record:source.clone(),
    })).await.unwrap();
    let restriction = tandem_memory::CanonicalMemoryRestriction::from_global_record(&source, &tenant).unwrap();
    let lineage = DerivedMemoryLineage::new(Some("alice".into()),Some("unit-memory-commit".into()),vec![restriction.clone()],
        vec![tandem_memory::CanonicalInputReference::Memory {source:restriction.source_reference()}]).unwrap();
    let mut derived = record(&original.tenant_context, "promotion-derived-source");
    derived.metadata.as_mut().unwrap()["owner_org_unit_id"] = json!("unit-memory-commit");
    derived.metadata = metadata_with_derived_lineage(derived.metadata,&lineage).unwrap();
    derived.provenance.as_mut().unwrap()["partition"] = json!({"tier":"session"});
    let id = derived.id.clone();
    with_verified_memory_decrypt_principal(Some(original), store.write(tandem_memory::MemoryStoreWriteRequest::GlobalRecord {
        scope:write_scope,record:derived,
    })).await.unwrap();
    (id, lineage)
}

async fn promotion_record_with_source_expiry(
    fixture: &CommitFixture,
    original: &VerifiedTenantContext,
    source_expiry: Option<u64>,
) -> (String, DerivedMemoryLineage) {
    promotion_record_with_source_controls(fixture, original, source_expiry, None).await
}

async fn promotion_record(fixture: &CommitFixture, original: &VerifiedTenantContext) -> String {
    promotion_record_with_source_expiry(fixture, original, None).await.0
}

fn knowledge_resource(original: &VerifiedTenantContext) -> tandem_types::ResourceRef {
    tandem_types::ResourceRef::new(
        original.tenant_context.org_id.clone(),
        original.tenant_context.workspace_id.clone(),
        tandem_types::ResourceKind::KnowledgeSpace,
        "memory-commit-knowledge",
    ).with_project_id("memory-commit-boundary")
}

fn identity_with_knowledge_grant(
    fixture: &CommitFixture,
    grant_expires_at_ms: u64,
) -> VerifiedTenantContext {
    use tandem_types::{AccessPermission, DataBoundary, DataClass, GrantSource, PrincipalRef, ScopedGrant};
    let mut original = fixture.identity(60_000);
    // Trusted test projection after real hosted member projection. This tests
    // the native scoped-grant clock, not operator grant issuance over HTTP.
    let mut strict = original.strict_projection.take().expect("real hosted projection");
    strict.grants.push(ScopedGrant::new(
        "memory-commit-knowledge-read", PrincipalRef::human_user("alice"),
        knowledge_resource(&original), GrantSource::Direct,
    ).with_permissions(vec![AccessPermission::Read])
        .with_data_classes(vec![DataClass::Internal])
        .with_expires_at_ms(grant_expires_at_ms));
    original.strict_projection = Some(strict.with_data_boundary(DataBoundary::allow(vec![DataClass::Internal])));
    original
}

fn source_knowledge_policy(
    original: &VerifiedTenantContext,
    retention_expires_at_ms: Option<u64>,
) -> tandem_memory::KnowledgeScopePolicy {
    tandem_memory::KnowledgeScopePolicy {
        registry_id: "memory-commit-knowledge-registry".into(),
        resource_ref: knowledge_resource(original),
        data_class: tandem_types::DataClass::Internal,
        collection_id: None, source_binding_id: None, source_object_id: None,
        owner_org_unit_id: Some("unit-memory-commit".into()), risk_tier: None,
        allowed_workflow_phases: Vec::new(),
        allowed_write_tiers: vec![tandem_memory::GovernedMemoryTier::Session],
        allowed_promotion_tiers: vec![tandem_memory::GovernedMemoryTier::Project],
        retention_expires_at_ms, required_trust_label: None,
        promotion_requires_approval: false,
    }
}

async fn source_bound_authority(
    fixture: &CommitFixture,
    original: &VerifiedTenantContext,
    lineage: DerivedMemoryLineage,
    target: Option<GlobalMemoryRecord>,
    operation_deadline: Option<u64>,
) -> tandem_memory::MemoryCommitAuthority {
    let store = open_global_memory_store_for_state(&fixture.state).await.unwrap();
    let mut scope = tandem_memory::MemoryReadScope::tenant(MemoryTenantScope {
        org_id: original.tenant_context.org_id.clone(),
        workspace_id: original.tenant_context.workspace_id.clone(),
        deployment_id: original.tenant_context.deployment_id.clone(),
    });
    scope.org_unit = Some("unit-memory-commit".into());
    scope.subject = Some("alice".into());
    let filter = with_verified_memory_decrypt_principal(Some(original),
        crate::memory::derived_lineage::resolved_filter_for_lineage(
            &fixture.state, &original.tenant_context, store.as_ref(), &scope, &lineage,
            distillation_access_filter(Some(original), "alice"),
        ),
    ).await.expect("current canonical contributor resolved before writer wait");
    assert_eq!(filter.mode, tandem_memory::types::GovernedReadMode::GovernedStrict,
        "a local-noop filter cannot prove expiry of a scoped source grant");
    derived_memory_commit_authority_with_lineage(
        &fixture.state, &original.tenant_context, Some(original), lineage, filter, target,
        move |now| operation_deadline.is_none_or(|deadline| now < deadline),
    )
}

struct SqliteWriterWaitWitness {
    _observer: tandem_memory::MemorySqliteWriterWaitGuard,
    reached: Option<std::sync::mpsc::Receiver<()>>,
    release: std::sync::mpsc::SyncSender<()>,
}

impl SqliteWriterWaitWitness {
    async fn install(fixture: &CommitFixture) -> Self {
        // The handler obtains this exact cached Arc<dyn MemoryStore>. A hook on
        // a separately opened database would not prove its real SQL wait.
        let store = fixture.state.memory_store().await.expect("cached native memory store");
        let (reached_tx, reached_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let release_rx = std::sync::Mutex::new(release_rx);
        let observer: tandem_memory::MemorySqliteWriterWaitObserver = Arc::new(move |attempt| {
            if attempt == 0 {
                reached_tx.try_send(()).is_ok()
                    && release_rx.lock().unwrap().recv_timeout(Duration::from_secs(10)).is_ok()
            } else {
                attempt < 20
            }
        });
        let guard = store.observe_sqlite_writer_wait_for_test(observer)
            .expect("per-store actual SQLite guarded writer observer");
        Self { _observer: guard, reached: Some(reached_rx), release: release_tx }
    }

    async fn reached_actual_busy_callback(&mut self) {
        let reached = self.reached.take().expect("observe one native guarded writer wait");
        tokio::time::timeout(Duration::from_secs(5), tokio::task::spawn_blocking(move || {
            reached.recv_timeout(Duration::from_secs(5))
        })).await.expect("native SQL reached busy callback before deadline")
            .expect("busy observer wait task")
            .expect("native SQLite BEGIN IMMEDIATE was blocked by the independent writer");
    }

    fn release_after_writer_commit(&self) {
        self.release.send(()).expect("release blocked native SQLite busy callback");
    }
}

fn promotion_request(original: &VerifiedTenantContext, id: &str) -> (MemoryPromoteRequest, MemoryCapabilityToken) {
    let request: MemoryPromoteRequest = serde_json::from_value(json!({
        "run_id":"promotion-commit-run","source_memory_id":id,"from_tier":"session","to_tier":"project",
        "partition":{"org_id":original.tenant_context.org_id,"workspace_id":original.tenant_context.workspace_id,
            "project_id":"memory-commit-boundary","tier":"session"},
        "reason":"approved synthetic derived memory","review":{"required":true,"reviewer_id":"alice","approval_id":"current-review"},
        "source_outcome":{"status":"passed","approved":true},
    })).unwrap();
    let capability: MemoryCapabilityToken = serde_json::from_value(json!({
        "run_id":request.run_id,"subject":"alice","org_id":original.tenant_context.org_id,
        "workspace_id":original.tenant_context.workspace_id,"project_id":"memory-commit-boundary",
        "memory":{"read_tiers":["session","project"],"write_tiers":["session"],"promote_targets":["project"],
            "require_review_for_promote":true,"allow_auto_use_tiers":[]},"expires_at":crate::now_ms()+60_000,
    })).unwrap();
    (request,capability)
}

async fn wait_for_guarded_memory_writer<T>(
    fixture: &CommitFixture,
    task: &tokio::task::JoinHandle<T>,
    witness: &mut SqliteWriterWaitWitness,
) {
    witness.reached_actual_busy_callback().await;
    assert!(!task.is_finished(), "handler is waiting on the real SQLite BEGIN IMMEDIATE");
    assert!(fixture.state.enterprise.hosted_policy.publication_mutex_locked_for_test(),
        "current publication authority must cover the actual SQLite writer wait");
}

fn drain_target_promotion_events(
    events: &mut tokio::sync::broadcast::Receiver<tandem_types::EngineEvent>,
    id: &str,
) -> (usize, usize) {
    let mut promoted = 0;
    let mut updated = 0;
    loop {
        match events.try_recv() {
            Ok(event) if matches!(event.event_type.as_str(), "memory.promote" | "memory.updated") => {
                if event.properties["runID"] != "promotion-commit-run"
                    && event.properties["memoryID"] != id {
                    continue;
                }
                assert_eq!(event.properties["runID"], "promotion-commit-run");
                assert_eq!(event.properties["memoryID"], id);
                assert_eq!(event.properties["sourceMemoryID"], id);
                if event.event_type == "memory.promote" {
                    promoted += 1;
                } else {
                    assert_eq!(event.properties["action"], "promote");
                    updated += 1;
                }
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
            Err(error) => panic!("promotion event observer lost evidence: {error}"),
        }
    }
    (promoted, updated)
}

fn assert_original_identity_commit_denied(
    fixture: &CommitFixture,
    original: &VerifiedTenantContext,
) {
    let denied = derived_memory_commit_authority(
        &fixture.state, &original.tenant_context, Some(original),
    )().unwrap_err();
    assert_eq!(denied.kind, MemoryStoreErrorKind::ScopeViolation);
    assert_eq!(denied.message, "hosted_policy_or_identity_expired");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_derived_promotion_rechecks_original_identity_after_actual_sqlite_writer_wait() {
    for expired in [false,true] {
        let fixture = CommitFixture::new().await;
        let seed_identity = fixture.identity(60_000);
        let id = promotion_record(&fixture,&seed_identity).await;
        let writer = fixture.writer();
        let mut witness = SqliteWriterWaitWitness::install(&fixture).await;
        let before: (String,String,Option<String>,Option<String>) = writer.query_row(
            "SELECT visibility,content_hash,metadata,provenance FROM memory_records WHERE id=?1",[&id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
        let original = fixture.identity(if expired {1_500} else {60_000});
        let (request,capability) = promotion_request(&original,&id);
        let state = fixture.state.clone(); let verified = original.clone(); let tenant = original.tenant_context.clone();
        let task = tokio::spawn(async move {
            memory_promote_impl_with_verified(&state,&tenant,Some(&verified),request,Some(capability)).await
        });
        wait_for_guarded_memory_writer(&fixture,&task,&mut witness).await;
        assert!(!fixture.state.memory_audit_log.read().await.iter().any(|event|
            event.action == "memory_promote" && event.status == "ok"), "writer wait cannot publish success audit");
        if expired {
            expire_while_writer_is_held(&fixture,&original).await;
            assert_original_identity_commit_denied(&fixture,&original);
        }
        writer.execute_batch("COMMIT").unwrap();
        witness.release_after_writer_commit();
        let result = tokio::time::timeout(Duration::from_secs(4),task).await.unwrap().unwrap();
        let after: (String,String,Option<String>,Option<String>) = writer.query_row(
            "SELECT visibility,content_hash,metadata,provenance FROM memory_records WHERE id=?1",[&id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
        if expired {
            assert_eq!(result.unwrap_err(),StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(after,before,"expired promotion cannot mutate the real row");
            assert!(!fixture.state.memory_audit_log.read().await.iter().any(|event|
                event.action == "memory_promote" && event.status == "ok"));
        }
        else {
            assert!(result.unwrap().promoted); assert_eq!(after.0,"shared"); assert_eq!(after.1,before.1);
            let store = open_global_memory_store_for_state(&fixture.state).await.unwrap();
            let mut scope = tandem_memory::MemoryReadScope::tenant(MemoryTenantScope {org_id:original.tenant_context.org_id.clone(),
                workspace_id:original.tenant_context.workspace_id.clone(),deployment_id:original.tenant_context.deployment_id.clone()});
            scope.subject = Some("alice".into());
            let record = with_verified_memory_decrypt_principal(Some(&original), store.read(tandem_memory::MemoryStoreReadRequest::GlobalRecord {
                scope:scope.clone(),id:id.clone(),
            })).await.unwrap();
            let tandem_memory::MemoryStoreReadResult::GlobalRecord(Some(record)) = record else { panic!("current owner retains promoted row"); };
            assert_eq!(record.metadata.as_ref().unwrap()["owner_subject"],"alice");
            scope.subject = Some("bob".into());
            assert!(matches!(store.read(tandem_memory::MemoryStoreReadRequest::GlobalRecord {scope,id}).await.unwrap(),
                tandem_memory::MemoryStoreReadResult::GlobalRecord(None)),"promotion cannot erase original private ownership");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_derived_promotion_rechecks_source_and_capability_deadlines_after_actual_sqlite_writer_wait() {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Deadline { RecordTtl, KnowledgeRetention, ScopedGrant, CapabilityToken }
    for kind in [Deadline::RecordTtl, Deadline::KnowledgeRetention,
                 Deadline::ScopedGrant, Deadline::CapabilityToken] {
      for expired in [false, true] {
        let fixture = CommitFixture::new().await;
        let deadline = crate::now_ms() + if expired { 6_000 } else { 60_000 };
        let original = if matches!(kind, Deadline::RecordTtl | Deadline::CapabilityToken) {
            fixture.identity(60_000)
        } else {
            identity_with_knowledge_grant(&fixture,
                if kind == Deadline::ScopedGrant { deadline } else { crate::now_ms() + 60_000 })
        };
        let knowledge_scope = matches!(kind, Deadline::KnowledgeRetention | Deadline::ScopedGrant).then(||
            source_knowledge_policy(&original,
                (kind == Deadline::KnowledgeRetention).then_some(deadline)));
        let (id, lineage) = promotion_record_with_source_controls(
            &fixture, &original,
            (kind == Deadline::RecordTtl).then_some(deadline), knowledge_scope,
        ).await;
        match kind {
            Deadline::RecordTtl => assert_eq!(lineage.sources[0].expires_at_ms, Some(deadline)),
            Deadline::KnowledgeRetention => assert_eq!(lineage.sources[0]
                .knowledge_scope.as_ref().unwrap().retention_expires_at_ms, Some(deadline)),
            Deadline::ScopedGrant => assert_eq!(original.strict_projection.as_ref().unwrap()
                .grants.iter().find(|grant| grant.grant_id == "memory-commit-knowledge-read")
                .unwrap().expires_at_ms, Some(deadline)),
            Deadline::CapabilityToken => assert!(lineage.sources[0].expires_at_ms.is_none()),
        }
        let probe = source_bound_authority(&fixture, &original, lineage, None,
            (kind == Deadline::CapabilityToken).then_some(deadline)).await;
        probe().expect("canonical source and original identity live at admission");
        let writer = fixture.writer();
        let mut witness = SqliteWriterWaitWitness::install(&fixture).await;
        let before: (String, String, Option<String>, Option<String>) = writer.query_row(
            "SELECT visibility,content_hash,metadata,provenance FROM memory_records WHERE id=?1",
            [&id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).unwrap();
        let (request, mut capability) = promotion_request(&original, &id);
        if kind == Deadline::CapabilityToken {
            capability.expires_at = deadline;
            assert_eq!(capability.expires_at, deadline,
                "use the original run capability; do not renew it during writer wait");
        }
        let state = fixture.state.clone();
        let verified = original.clone();
        let tenant = original.tenant_context.clone();
        let mut events = fixture.state.event_bus.subscribe();
        let task = tokio::spawn(async move {
            memory_promote_impl_with_verified(&state, &tenant, Some(&verified), request, Some(capability)).await
        });
        wait_for_guarded_memory_writer(&fixture, &task, &mut witness).await;
        assert!(crate::now_ms() < deadline, "{kind:?} is current after real SQL busy entry");
        assert!(!fixture.state.memory_audit_log.read().await.iter().any(|event|
            event.action == "memory_promote" && event.status == "ok"));
        assert_eq!(drain_target_promotion_events(&mut events, &id), (0, 0),
            "held SQL writer cannot publish target promotion events");
        if expired {
            tokio::time::timeout(Duration::from_secs(7), async {
                while crate::now_ms() < deadline {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }).await.expect("canonical contributor deadline expires during real SQL writer wait");
            let denied = probe().unwrap_err();
            assert_eq!(denied.kind, MemoryStoreErrorKind::ScopeViolation);
            assert_eq!(denied.message, "derived_memory_source_authority_expired_or_denied");
        } else {
            probe().expect("healthy contributor remains current");
        }
        assert!(!original.is_expired_at(crate::now_ms()));
        fixture.state.enterprise.hosted_policy.authorize(Some(&original))
            .expect("original assertion and policy remain live");
        writer.execute_batch("COMMIT").unwrap();
        witness.release_after_writer_commit();
        let result = tokio::time::timeout(Duration::from_secs(4), task).await.unwrap().unwrap();
        let after: (String, String, Option<String>, Option<String>) = writer.query_row(
            "SELECT visibility,content_hash,metadata,provenance FROM memory_records WHERE id=?1",
            [&id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).unwrap();
        let published = drain_target_promotion_events(&mut events, &id);
        if expired {
            assert_eq!(result.unwrap_err(), StatusCode::FORBIDDEN);
            assert_eq!(after, before, "{kind:?} expiry cannot publish promotion");
            assert!(!fixture.state.memory_audit_log.read().await.iter().any(|event|
                event.action == "memory_promote" && event.status == "ok"));
            assert_eq!(published, (0, 0),
                "denied writer cannot publish target promotion or update events");
        } else {
            assert!(result.unwrap().promoted);
            assert_eq!(after.0, "shared");
            assert_eq!(after.1, before.1);
            assert_eq!(published, (1, 1),
                "healthy promotion publishes one target promotion and one update event");
        }
      }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_derived_promotion_rechecks_snapshot_expiry_after_actual_sqlite_writer_wait() {
    let fixture = CommitFixture::new_with_policy_remaining(Some(3_000)).await;
    let original = fixture.identity(60_000);
    let id = promotion_record(&fixture,&original).await;
    let writer = fixture.writer();
    let mut witness = SqliteWriterWaitWitness::install(&fixture).await;
    let before: (String,Option<String>,Option<String>) = writer.query_row(
        "SELECT visibility,metadata,provenance FROM memory_records WHERE id=?1",[&id],|row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
    let expiry = fixture.state.enterprise.hosted_policy.current().unwrap().unwrap().expires_at_ms();
    let (request,capability) = promotion_request(&original,&id);
    let state = fixture.state.clone(); let verified = original.clone(); let tenant = original.tenant_context.clone();
    let task = tokio::spawn(async move {
        memory_promote_impl_with_verified(&state,&tenant,Some(&verified),request,Some(capability)).await
    });
    wait_for_guarded_memory_writer(&fixture,&task,&mut witness).await;
    assert!(crate::now_ms()<expiry);
    tokio::time::timeout(Duration::from_secs(4),async {
        while crate::now_ms()<expiry {tokio::time::sleep(Duration::from_millis(20)).await;}
    }).await.unwrap();
    assert!(!original.is_expired_at(crate::now_ms()));
    assert_original_identity_commit_denied(&fixture,&original);
    writer.execute_batch("COMMIT").unwrap();
    witness.release_after_writer_commit();
    assert_eq!(tokio::time::timeout(Duration::from_secs(4),task).await.unwrap().unwrap().unwrap_err(),
        StatusCode::INTERNAL_SERVER_ERROR);
    let after: (String,Option<String>,Option<String>) = writer.query_row(
        "SELECT visibility,metadata,provenance FROM memory_records WHERE id=?1",[&id],|row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
    assert_eq!(after,before,"policy expiry during a real writer wait cannot publish promotion");
    assert!(!fixture.state.memory_audit_log.read().await.iter().any(|event|
        event.action == "memory_promote" && event.status == "ok"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_derived_demotion_rechecks_original_identity_after_actual_sqlite_writer_wait() {
    for expired in [false,true] {
        let fixture = CommitFixture::new().await;
        let seed_identity = fixture.identity(60_000);
        let id = promotion_record(&fixture,&seed_identity).await;
        let writer = fixture.writer();
        let mut witness = SqliteWriterWaitWitness::install(&fixture).await;
        let before: (String,bool,Option<String>,Option<String>) = writer.query_row(
            "SELECT visibility,demoted,metadata,provenance FROM memory_records WHERE id=?1",[&id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
        let original = fixture.identity(if expired {1_500} else {60_000});
        let state = fixture.state.clone(); let verified = original.clone(); let tenant = original.tenant_context.clone();
        let input = MemoryDemoteInput {id:id.clone(),run_id:"demotion-commit-run".into()};
        let mut events = fixture.state.event_bus.subscribe();
        let task = tokio::spawn(async move {
            memory_demote(State(state),Extension(tenant),Some(Extension(verified)),Json(input)).await
        });
        wait_for_guarded_memory_writer(&fixture,&task,&mut witness).await;
        assert!(!fixture.state.memory_audit_log.read().await.iter().any(|event|
            event.action == "memory_demote" && event.status == "ok"));
        assert!(matches!(events.try_recv(),Err(tokio::sync::broadcast::error::TryRecvError::Empty)),
            "demotion cannot publish while the actual writer is held");
        if expired {
            expire_while_writer_is_held(&fixture,&original).await;
            assert_original_identity_commit_denied(&fixture,&original);
        }
        writer.execute_batch("COMMIT").unwrap();
        witness.release_after_writer_commit();
        let result = tokio::time::timeout(Duration::from_secs(4),task).await.unwrap().unwrap();
        let after: (String,bool,Option<String>,Option<String>) = writer.query_row(
            "SELECT visibility,demoted,metadata,provenance FROM memory_records WHERE id=?1",[&id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
        let successful_audits = fixture.state.memory_audit_log.read().await.iter().filter(|event|
            event.action == "memory_demote" && event.status == "ok").count();
        let mut successful_events = 0;
        loop {
            match events.try_recv() {
                Ok(event) => {
                    if event.event_type == "memory.updated" {
                        assert_eq!(event.properties["memoryID"],id);
                        assert_eq!(event.properties["action"],"demote");
                        successful_events += 1;
                    }
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                Err(error) => panic!("demotion evidence receiver failed: {error}"),
            }
        }
        if expired {
            assert_eq!(result.unwrap_err(),StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(after,before,"expired demotion cannot replace the real row");
            assert_eq!(successful_audits,0); assert_eq!(successful_events,0);
        } else {
            assert_eq!(result.unwrap().0["ok"],true);
            assert_eq!(after.0,"private"); assert!(after.1);
            assert_eq!(after.2,before.2); assert_eq!(after.3,before.3);
            assert_eq!(successful_audits,1); assert_eq!(successful_events,1);
        }
    }
}
