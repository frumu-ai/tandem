// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

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
    state.upsert_workflow_learning_candidate(candidate("original-candidate", "Original synthetic fact"))
        .await.expect("ordinary candidate compatibility control");
    (
        serde_json::to_value(&*state.workflow_learning_candidates.read().await).unwrap(),
        tokio::fs::read(&state.workflow_learning_candidates_path).await.unwrap(),
    )
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_candidate_commit_rechecks_original_identity_after_real_cache_writer_wait() {
    for expired in [false, true] {
        let fixture = CommitFixture::new().await;
        let before = seed_candidate(&fixture.state).await;
        let writer = fixture.state.workflow_learning_candidates.write().await;
        let original = fixture.identity(if expired {1_500} else {60_000});
        fixture.state.enterprise.hosted_policy.authorize(Some(&original)).unwrap();
        let state = fixture.state.clone(); let verified = original.clone();
        let (entered, receiver) = oneshot::channel();
        let task = tokio::spawn(async move {
            entered.send(()).unwrap();
            state.upsert_workflow_learning_candidate_with_current_policy(
                candidate("new-candidate", "Updated synthetic fact"), Some(verified),
            ).await
        });
        started(receiver).await;
        tokio::task::yield_now().await;
        assert!(!task.is_finished(), "actual candidate writer remains held");
        assert!(!fixture.state.enterprise.hosted_policy.publication_mutex_locked_for_test(),
            "candidate wait must not prevent current policy publication");
        assert_eq!(serde_json::to_value(&*writer).unwrap(), before.0);
        assert_eq!(tokio::fs::read(&fixture.state.workflow_learning_candidates_path).await.unwrap(), before.1);
        if expired { expire_while_writer_is_held(&fixture, &original).await; }
        drop(writer);
        let result = finish_candidate(task).await;
        if expired {
            assert!(result.unwrap_err().to_string().contains("hosted_policy_or_identity_expired"));
            assert_candidates_unchanged(&fixture.state, &before).await;
        } else {
            assert_eq!(result.unwrap().candidate_id, "original-candidate", "reuse the ordinary merge semantics");
            let cache = serde_json::to_value(&*fixture.state.workflow_learning_candidates.read().await).unwrap();
            let durable: Value = serde_json::from_slice(&tokio::fs::read(&fixture.state.workflow_learning_candidates_path).await.unwrap()).unwrap();
            assert_eq!(cache, durable);
            assert_eq!(cache["original-candidate"]["summary"], "Updated synthetic fact");
        }
    }
    let state = crate::test_support::test_state().await;
    let row = state.upsert_workflow_learning_candidate_with_current_policy(candidate("standalone-candidate", "Standalone fact"), None)
        .await.expect("genuine standalone candidate publication");
    assert_eq!(state.get_workflow_learning_candidate(&row.candidate_id).await.unwrap().summary, "Standalone fact");
    let durable: Value = serde_json::from_slice(&tokio::fs::read(&state.workflow_learning_candidates_path).await.unwrap()).unwrap();
    assert_eq!(durable[&row.candidate_id]["summary"], "Standalone fact");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_candidate_commit_rechecks_snapshot_expiry_after_real_cache_writer_wait() {
    let fixture = CommitFixture::new_with_policy_remaining(Some(3_000)).await;
    let before = seed_candidate(&fixture.state).await;
    let writer = fixture.state.workflow_learning_candidates.write().await;
    let original = fixture.identity(60_000);
    let expiry = fixture.state.enterprise.hosted_policy.current().unwrap().unwrap().expires_at_ms();
    let state = fixture.state.clone(); let verified = original.clone();
    let (entered, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        entered.send(()).unwrap();
        state.upsert_workflow_learning_candidate_with_current_policy(candidate("aged-candidate", "Expired snapshot fact"), Some(verified)).await
    });
    started(receiver).await;
    assert!(crate::now_ms() < expiry);
    tokio::time::timeout(Duration::from_secs(4), async {
        while crate::now_ms() < expiry { tokio::time::sleep(Duration::from_millis(20)).await; }
    }).await.unwrap();
    assert!(!original.is_expired_at(crate::now_ms()));
    assert!(!task.is_finished());
    drop(writer);
    assert!(finish_candidate(task).await.unwrap_err().to_string().contains("hosted_policy_or_identity_expired"));
    assert_candidates_unchanged(&fixture.state, &before).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_candidate_commit_denies_actual_policy_removal_during_cache_writer_wait() {
    let fixture = CommitFixture::new().await;
    let before = seed_candidate(&fixture.state).await;
    let writer = fixture.state.workflow_learning_candidates.write().await;
    let original = fixture.identity(60_000);
    let state = fixture.state.clone(); let verified = original.clone();
    let (entered, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        entered.send(()).unwrap();
        state.upsert_workflow_learning_candidate_with_current_policy(candidate("revoked-candidate", "Revoked fact"), Some(verified)).await
    });
    started(receiver).await;
    assert!(!task.is_finished());
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
    drop(writer);
    assert!(finish_candidate(task).await.is_err());
    assert_candidates_unchanged(&fixture.state, &before).await;
    assert!(fixture.state.upsert_workflow_learning_candidate_with_current_policy(candidate("forged-local", "Must not publish"), None)
        .await.is_err(), "configured hosted authority cannot be bypassed with absent identity");
    assert_candidates_unchanged(&fixture.state, &before).await;
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
            state.upsert_workflow_learning_candidate_with_current_policy(candidate("cancelled-candidate", "Owned completed fact"), Some(verified)).await
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
        let durable: Value = serde_json::from_slice(&tokio::fs::read(&fixture.state.workflow_learning_candidates_path).await.unwrap()).unwrap();
        assert_eq!(cache, durable);
        if expired { assert_candidates_unchanged(&fixture.state, &before).await; }
        else { assert_eq!(cache["original-candidate"]["summary"], "Owned completed fact"); }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_candidate_atomic_publication_failure_preserves_cache_and_cleans_prepared_file() {
    let mut fixture = CommitFixture::new().await;
    let before = seed_candidate(&fixture.state).await;
    let original_path = fixture.state.workflow_learning_candidates_path.clone();
    let failed_destination = fixture.directory.path().join("existing-directory");
    std::fs::create_dir(&failed_destination).unwrap();
    std::fs::write(failed_destination.join("marker"), b"unchanged durable directory").unwrap();
    fixture.state.workflow_learning_candidates_path = failed_destination.clone();
    let original = fixture.identity(60_000);
    assert!(fixture.state.upsert_workflow_learning_candidate_with_current_policy(candidate("failed-candidate", "Must not enter cache"), Some(original))
        .await.is_err(), "atomic rename cannot replace an existing nonempty directory");
    assert_eq!(serde_json::to_value(&*fixture.state.workflow_learning_candidates.read().await).unwrap(), before.0);
    assert_eq!(std::fs::read(&original_path).unwrap(), before.1);
    assert_eq!(std::fs::read(failed_destination.join("marker")).unwrap(), b"unchanged durable directory");
    assert!(std::fs::read_dir(fixture.directory.path()).unwrap().all(|entry|
        !entry.unwrap().path().extension().is_some_and(|extension| extension == "tmp")),
        "failed prepared files must be removed");
}

async fn promotion_record(fixture: &CommitFixture, original: &VerifiedTenantContext) -> String {
    let store = open_global_memory_store_for_state(&fixture.state).await.unwrap();
    let mut source = record(&original.tenant_context, "promotion-canonical-source");
    source.metadata.as_mut().unwrap()["owner_org_unit_id"] = json!("unit-memory-commit");
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
    id
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

async fn wait_for_guarded_memory_writer<T>(fixture: &CommitFixture, task: &tokio::task::JoinHandle<T>) {
    tokio::time::timeout(Duration::from_secs(3),async {
        loop {
            assert!(!task.is_finished(), "real memory handler must reach guarded writer rather than fail preflight");
            if fixture.state.enterprise.hosted_policy.publication_mutex_locked_for_test() { break; }
            tokio::task::yield_now().await;
        }
    }).await.expect("actual memory handler has entered its guarded SQL writer wait");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_derived_promotion_rechecks_original_identity_after_actual_sqlite_writer_wait() {
    for expired in [false,true] {
        let fixture = CommitFixture::new().await;
        let seed_identity = fixture.identity(60_000);
        let id = promotion_record(&fixture,&seed_identity).await;
        let writer = fixture.writer();
        let before: (String,String,Option<String>,Option<String>) = writer.query_row(
            "SELECT visibility,content_hash,metadata,provenance FROM memory_records WHERE id=?1",[&id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
        let original = fixture.identity(if expired {1_500} else {60_000});
        let (request,capability) = promotion_request(&original,&id);
        let state = fixture.state.clone(); let verified = original.clone(); let tenant = original.tenant_context.clone();
        let task = tokio::spawn(async move {
            memory_promote_impl_with_verified(&state,&tenant,Some(&verified),request,Some(capability)).await
        });
        wait_for_guarded_memory_writer(&fixture,&task).await;
        assert!(!fixture.state.memory_audit_log.read().await.iter().any(|event|
            event.action == "memory_promote" && event.status == "ok"), "writer wait cannot publish success audit");
        if expired { expire_while_writer_is_held(&fixture,&original).await; }
        writer.execute_batch("COMMIT").unwrap();
        let result = tokio::time::timeout(Duration::from_secs(4),task).await.unwrap().unwrap();
        let after: (String,String,Option<String>,Option<String>) = writer.query_row(
            "SELECT visibility,content_hash,metadata,provenance FROM memory_records WHERE id=?1",[&id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
        if expired {
            assert!(result.is_err()); assert_eq!(after,before,"expired promotion cannot mutate the real row");
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
async fn tan_829_derived_promotion_rechecks_snapshot_expiry_after_actual_sqlite_writer_wait() {
    let fixture = CommitFixture::new_with_policy_remaining(Some(3_000)).await;
    let original = fixture.identity(60_000);
    let id = promotion_record(&fixture,&original).await;
    let writer = fixture.writer();
    let before: (String,Option<String>,Option<String>) = writer.query_row(
        "SELECT visibility,metadata,provenance FROM memory_records WHERE id=?1",[&id],|row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
    let expiry = fixture.state.enterprise.hosted_policy.current().unwrap().unwrap().expires_at_ms();
    let (request,capability) = promotion_request(&original,&id);
    let state = fixture.state.clone(); let verified = original.clone(); let tenant = original.tenant_context.clone();
    let task = tokio::spawn(async move {
        memory_promote_impl_with_verified(&state,&tenant,Some(&verified),request,Some(capability)).await
    });
    wait_for_guarded_memory_writer(&fixture,&task).await;
    assert!(crate::now_ms()<expiry);
    tokio::time::timeout(Duration::from_secs(4),async {
        while crate::now_ms()<expiry {tokio::time::sleep(Duration::from_millis(20)).await;}
    }).await.unwrap();
    assert!(!original.is_expired_at(crate::now_ms()));
    writer.execute_batch("COMMIT").unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(4),task).await.unwrap().unwrap().is_err());
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
        wait_for_guarded_memory_writer(&fixture,&task).await;
        assert!(!fixture.state.memory_audit_log.read().await.iter().any(|event|
            event.action == "memory_demote" && event.status == "ok"));
        assert!(matches!(events.try_recv(),Err(tokio::sync::broadcast::error::TryRecvError::Empty)),
            "demotion cannot publish while the actual writer is held");
        if expired { expire_while_writer_is_held(&fixture,&original).await; }
        writer.execute_batch("COMMIT").unwrap();
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
            assert!(result.is_err()); assert_eq!(after,before,"expired demotion cannot replace the real row");
            assert_eq!(successful_audits,0); assert_eq!(successful_events,0);
        } else {
            assert_eq!(result.unwrap().0["ok"],true);
            assert_eq!(after.0,"private"); assert!(after.1);
            assert_eq!(after.2,before.2); assert_eq!(after.3,before.3);
            assert_eq!(successful_audits,1); assert_eq!(successful_events,1);
        }
    }
}
