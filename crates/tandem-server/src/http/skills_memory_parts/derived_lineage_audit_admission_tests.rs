// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

async fn cold_record(
    fixture: &CommitFixture,
    original: &VerifiedTenantContext,
    id: &str,
) -> Value {
    let store = tandem_memory::db::MemoryDatabase::new(&fixture.state.memory_db_path)
        .await
        .expect("independent cold SQLite memory store");
    let mut scope = tandem_memory::MemoryReadScope::tenant(MemoryTenantScope {
        org_id: original.tenant_context.org_id.clone(),
        workspace_id: original.tenant_context.workspace_id.clone(),
        deployment_id: original.tenant_context.deployment_id.clone(),
    });
    scope.org_unit = Some("unit-memory-commit".into());
    scope.subject = Some("alice".into());
    let result = with_verified_memory_decrypt_principal(
        Some(original),
        store.read(tandem_memory::MemoryStoreReadRequest::GlobalRecord {
            scope,
            id: id.to_owned(),
        }),
    )
    .await
    .expect("cold canonical target read");
    let tandem_memory::MemoryStoreReadResult::GlobalRecord(Some(record)) = result else {
        panic!("persisted canonical target must remain readable");
    };
    serde_json::to_value(record).unwrap()
}

async fn make_audit_unwritable(state: &AppState) {
    match tokio::fs::remove_file(&state.memory_audit_path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("remove prior protected audit file: {error}"),
    }
    tokio::fs::create_dir_all(&state.memory_audit_path)
        .await
        .expect("actual protected audit destination is an unwritable directory");
}

async fn run_mutation(
    state: AppState,
    original: VerifiedTenantContext,
    id: String,
    promote: bool,
) -> Result<String, StatusCode> {
    let tenant = original.tenant_context.clone();
    if promote {
        let (request, capability) = promotion_request(&original, &id);
        memory_promote_impl_with_verified(
            &state,
            &tenant,
            Some(&original),
            request,
            Some(capability),
        )
        .await
        .map(|response| response.audit_id)
    } else {
        let result = memory_demote(
            State(state),
            Extension(tenant),
            Some(Extension(original)),
            Json(MemoryDemoteInput {
                id,
                run_id: "demotion-commit-run".into(),
            }),
        )
        .await?;
        Ok(result.0["audit_id"]
            .as_str()
            .expect("successful demotion audit ID")
            .to_owned())
    }
}

fn mutation_events(
    events: &mut tokio::sync::broadcast::Receiver<tandem_types::EngineEvent>,
    id: &str,
    promote: bool,
) -> usize {
    if promote {
        let (promoted, updated) = drain_target_promotion_events(events, id);
        assert_eq!(promoted, updated, "promotion and update publish together");
        return promoted + updated;
    }
    let mut count = 0;
    loop {
        match events.try_recv() {
            Ok(event) if event.event_type == "memory.updated" => {
                assert_eq!(event.properties["memoryID"], id);
                assert_eq!(event.properties["action"], "demote");
                count += 1;
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
            Err(error) => panic!("audit admission event observer lost evidence: {error}"),
        }
    }
    count
}

async fn assert_unwritable_admission_preserves_target(promote: bool) {
    let fixture = CommitFixture::new().await;
    let original = fixture.identity(60_000);
    let id = promotion_record(&fixture, &original).await;
    let before = cold_record(&fixture, &original, &id).await;
    let audits_before = serde_json::to_value(&*fixture.state.memory_audit_log.read().await).unwrap();
    let mut events = fixture.state.event_bus.subscribe();
    make_audit_unwritable(&fixture.state).await;

    assert_eq!(
        run_mutation(fixture.state.clone(), original.clone(), id.clone(), promote)
            .await
            .unwrap_err(),
        StatusCode::INTERNAL_SERVER_ERROR,
    );
    assert_eq!(
        cold_record(&fixture, &original, &id).await,
        before,
        "initial protected audit failure must preserve the entire cold canonical row",
    );
    assert_eq!(
        serde_json::to_value(&*fixture.state.memory_audit_log.read().await).unwrap(),
        audits_before,
        "failed protected admission cannot publish an intent or a success to the cache",
    );
    assert_eq!(mutation_events(&mut events, &id, promote), 0);
    assert!(fixture.state.memory_audit_path.is_dir());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_derived_promotion_audit_admission_failure_preserves_cold_record_and_no_success() {
    assert_unwritable_admission_preserves_target(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_derived_demotion_audit_admission_failure_preserves_cold_record_and_no_success() {
    assert_unwritable_admission_preserves_target(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_derived_mutation_admission_is_pending_until_actual_writer_commit() {
    for promote in [true, false] {
        let fixture = CommitFixture::new().await;
        let original = fixture.identity(60_000);
        let id = promotion_record(&fixture, &original).await;
        let before = cold_record(&fixture, &original, &id).await;
        let action = if promote { "memory_promote" } else { "memory_demote" };
        let admission_action = format!("{action}_admission");
        let writer = fixture.writer();
        let mut witness = SqliteWriterWaitWitness::install(&fixture).await;
        let mut events = fixture.state.event_bus.subscribe();
        let task = tokio::spawn(run_mutation(
            fixture.state.clone(), original.clone(), id.clone(), promote,
        ));
        wait_for_guarded_memory_writer(&fixture, &task, &mut witness).await;

        let held = fixture.state.memory_audit_log.read().await.clone();
        let admissions: Vec<_> = held.iter()
            .filter(|event| event.action == admission_action && event.memory_id.as_deref() == Some(id.as_str()))
            .collect();
        assert_eq!(admissions.len(), 1, "one real protected attempt precedes the actual SQL wait");
        let admission = admissions[0];
        assert_eq!(admission.status, "pending");
        let detail: Value = serde_json::from_str(admission.detail.as_deref().unwrap()).unwrap();
        let success_id = detail["success_audit_id"].as_str().unwrap().to_owned();
        assert_eq!(detail, json!({"success_audit_id":success_id}));
        assert_ne!(admission.audit_id, success_id);
        assert_eq!(Uuid::parse_str(&success_id).unwrap().get_version_num(), 4);
        assert!(!held.iter().any(|event| event.action == action && event.status == "ok"));
        assert_eq!(mutation_events(&mut events, &id, promote), 0,
            "a pending attempt is not a mutation success or an engine success event");
        let durable_held = crate::http::memory_audit_store::load_memory_audit_events_strict(
            &fixture.state,
        ).await.expect("read real protected admission chain while SQL writer is held");
        assert_eq!(serde_json::to_value(&durable_held).unwrap(), serde_json::to_value(&held).unwrap());

        writer.execute_batch("COMMIT").unwrap();
        witness.release_after_writer_commit();
        assert_eq!(tokio::time::timeout(Duration::from_secs(4), task)
            .await.unwrap().unwrap().unwrap(), success_id);
        let after = cold_record(&fixture, &original, &id).await;
        assert_eq!(after["content"], before["content"]);
        assert_eq!(after["content_hash"], before["content_hash"]);
        assert_eq!(after["metadata"]["owner_subject"], "alice");
        if promote {
            assert_eq!(after["visibility"], "shared");
            assert_eq!(after["demoted"], false);
        } else {
            assert_eq!(after["visibility"], "private");
            assert_eq!(after["demoted"], true);
        }
        let completed = fixture.state.memory_audit_log.read().await.clone();
        let successes: Vec<_> = completed.iter()
            .filter(|event| event.action == action && event.status == "ok")
            .collect();
        assert_eq!(successes.len(), 1);
        assert_eq!(successes[0].audit_id, success_id);
        assert!(successes[0].created_at_ms >= admission.created_at_ms);
        assert_eq!(completed.iter().filter(|event| event.action == admission_action).count(), 1);
        assert_eq!(mutation_events(&mut events, &id, promote), if promote { 2 } else { 1 });
        let durable_completed = crate::http::memory_audit_store::load_memory_audit_events_strict(
            &fixture.state,
        ).await.expect("protected pending and committed success audit records");
        assert_eq!(serde_json::to_value(&durable_completed).unwrap(), serde_json::to_value(&completed).unwrap());
    }
}
