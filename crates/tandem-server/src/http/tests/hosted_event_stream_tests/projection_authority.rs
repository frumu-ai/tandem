// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use crate::http::context_types::{
    ContextBlackboardPatchOp, ContextBlackboardTask, ContextRunEventAppendInput, ContextRunState,
    ContextRunStatus,
};

fn pending_snapshot(fixture: &StreamFixture, run: &ContextRunState) {
    context::append_jsonl_line(
        &context::context_run_events_path(&fixture.state, &run.run_id),
        &json!({
            "event_id": "pending-snapshot", "run_id": run.run_id,
            "seq": 1, "ts_ms": 2, "type": "context.run.updated",
            "status": "queued", "revision": 2, "payload": {"run": run}
        }),
    )
    .unwrap();
}

fn event_input() -> ContextRunEventAppendInput {
    serde_json::from_value(json!({
        "type": "context.run.updated", "status": "queued", "payload": {}
    }))
    .unwrap()
}

fn task() -> ContextBlackboardTask {
    serde_json::from_value(json!({
        "id": "engine-task", "task_type": "inspection", "status": "pending",
        "created_ts": 1, "updated_ts": 1
    }))
    .unwrap()
}

#[tokio::test]
async fn buffered_context_projection_frame_publication_holds_the_actual_engine_token() {
    use std::future::Future;
    let fixture = group_fixture().await;
    let id = "projection-frame-boundary";
    fixture.context_run(id, "alice", "interactive").await;
    let run = context::load_context_run_state_sync(&fixture.state, id).unwrap();
    let ids = crate::http::context_run_authority::with_current_context_run_reads(
        &fixture.state, &stream_tenant("alice"), Some(&fixture.verified),
        &[id.into()], |ids| {
            let waker = futures::task::noop_waker();
            let mut cx = std::task::Context::from_waker(&waker);
            let mut snapshot_write = Box::pin(context::save_context_run_state(&fixture.state, &run));
            assert!(snapshot_write.as_mut().poll(&mut cx).is_pending(), "the actual projection snapshot writer must stay excluded through Event/JSON construction");
            let mut commit = Box::pin(context::context_run_engine().commit_run_event(&fixture.state, id, event_input(), None));
            assert!(commit.as_mut().poll(&mut cx).is_pending(), "the actual engine commit map must be held, not the separate task map");
            ids
        }, None,
    ).await;
    assert_eq!(ids, vec![id]);
}

#[tokio::test]
async fn buffered_context_projection_repair_waits_on_the_actual_engine_without_reentry() {
    let fixture = group_fixture().await;
    let id = "projection-repair-engine";
    fixture.context_run(id, "alice", "interactive").await;
    let mut replaced = context::load_context_run_state_sync(&fixture.state, id).unwrap();
    replaced.tenant_context = stream_tenant("bob");
    pending_snapshot(&fixture, &replaced);
    let held = context::context_run_projection_guard_for(id).await.unwrap();
    let load = context::load_context_run_state(&fixture.state, id);
    tokio::pin!(load);
    assert!(futures::poll!(load.as_mut()).is_pending());
    assert_eq!(
        context::load_context_run_state_sync(&fixture.state, id)
            .unwrap()
            .tenant_context,
        stream_tenant("alice")
    );
    drop(held);
    let repaired = tokio::time::timeout(Duration::from_secs(5), load)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(repaired.tenant_context, stream_tenant("bob"));
    assert_eq!(repaired.last_event_seq, 1);
    assert_eq!(
        context::load_context_run_state_sync(&fixture.state, id)
            .unwrap()
            .tenant_context,
        stream_tenant("bob")
    );
}

#[tokio::test]
async fn buffered_context_projection_all_four_engine_commits_repair_under_one_token() {
    let fixture = group_fixture().await;
    for kind in 0..4 {
        let id = format!("projection-commit-{kind}");
        fixture.context_run(&id, "alice", "interactive").await;
        let mut pending = context::load_context_run_state_sync(&fixture.state, &id).unwrap();
        pending.objective = "pending projection repaired".into();
        pending_snapshot(&fixture, &pending);
        let held = context::context_run_projection_guard_for(&id)
            .await
            .unwrap();
        let commit = async {
            let engine = context::context_run_engine();
            match kind {
                0 => {
                    engine
                        .commit_task_mutation(
                            &fixture.state,
                            &id,
                            task(),
                            ContextBlackboardPatchOp::AddTask,
                            serde_json::to_value(task()).unwrap(),
                            "context.task.created".into(),
                            ContextRunStatus::Queued,
                            None,
                            json!({}),
                        )
                        .await
                }
                1 => {
                    engine
                        .commit_run_event(&fixture.state, &id, event_input(), None)
                        .await
                }
                2 => {
                    engine
                        .commit_snapshot_with_event(
                            &fixture.state,
                            &id,
                            pending.clone(),
                            event_input(),
                            None,
                        )
                        .await
                }
                _ => {
                    engine
                        .commit_blackboard_patch(
                            &fixture.state,
                            &id,
                            ContextBlackboardPatchOp::SetRollingSummary,
                            json!("engine control"),
                        )
                        .await
                }
            }
        };
        tokio::pin!(commit);
        assert!(futures::poll!(commit.as_mut()).is_pending());
        drop(held);
        let committed = tokio::time::timeout(Duration::from_secs(5), commit)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(committed.run.objective, "pending projection repaired");
        assert_eq!(committed.event.seq, 2);
        assert_eq!(committed.run.last_event_seq, 2);
    }
}

#[tokio::test]
async fn buffered_context_projection_task_batch_lock_is_distinct_and_does_not_reenter_engine() {
    let fixture = group_fixture().await;
    let id = "projection-task-batch";
    fixture.context_run(id, "alice", "interactive").await;
    let task_lock = context::context_run_lock_for(id).await;
    let held = task_lock.lock().await;
    let engine = context::context_run_projection_guard_for(id).await.unwrap();
    drop(engine);
    let create = context::context_run_tasks_create(
        State(fixture.state.clone()),
        Extension(stream_tenant("alice")),
        axum::extract::Path(id.into()),
        Json(
            serde_json::from_value(json!({
                "tasks": [{"id": "batch-task", "task_type": "test", "payload": {}}]
            }))
            .unwrap(),
        ),
    );
    tokio::pin!(create);
    assert!(futures::poll!(create.as_mut()).is_pending());
    drop(held);
    let created = tokio::time::timeout(Duration::from_secs(5), create)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(created.0["tasks"][0]["id"], "batch-task");
    assert_eq!(
        context::load_context_run_state(&fixture.state, id)
            .await
            .unwrap()
            .tasks
            .len(),
        1
    );
}

#[tokio::test]
async fn buffered_context_projection_legacy_routine_owner_repair_remains_live() {
    let fixture = group_fixture().await;
    let native_id = "legacy-projection-control";
    let id = format!("routine-{native_id}");
    let native = serde_json::from_value(json!({
        "run_id": native_id, "routine_id": native_id,
        "tenant_context": stream_tenant("alice"), "trigger_type": "manual",
        "run_count": 1, "status": "queued", "created_at_ms": 1,
        "updated_at_ms": 1, "requires_approval": false, "entrypoint": "test"
    }))
    .unwrap();
    fixture
        .state
        .routine_runs
        .write()
        .await
        .insert(native_id.into(), native);
    fixture.context_run(&id, "alice", "routine").await;
    let mut run = context::load_context_run_state_sync(&fixture.state, &id).unwrap();
    run.tenant_context = TenantContext::local_implicit();
    run.source_client = Some("routine_runtime".into());
    context::save_context_run_state(&fixture.state, &run)
        .await
        .unwrap();
    let repaired = tokio::time::timeout(
        Duration::from_secs(5),
        context::load_context_run_state(&fixture.state, &id),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(repaired.tenant_context, stream_tenant("alice"));
    let ids = crate::http::context_run_authority::with_current_context_run_reads(
        &fixture.state,
        &stream_tenant("alice"),
        Some(&fixture.verified),
        &[id.clone()],
        |ids| ids,
        None,
    )
    .await;
    assert_eq!(ids, vec![id]);
}

#[tokio::test]
async fn buffered_context_projection_cancelled_blocking_save_keeps_actual_engine_owned() {
    let fixture = group_fixture().await;
    let id = "cancelled-projection-control";
    fixture.context_run(id, "alice", "interactive").await;
    let mut run = context::load_context_run_state_sync(&fixture.state, id).unwrap();
    run.tenant_context = stream_tenant("bob");
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
    let state = fixture.state.clone();
    let writer = tokio::spawn(async move {
        context::save_context_run_state_gated(
            &state,
            &run,
            context::ContextRunProjectionWriteTestGate {
                started: started_tx,
                resume: resume_rx,
            },
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), started_rx)
        .await
        .unwrap()
        .unwrap();
    writer.abort();
    assert!(writer.await.unwrap_err().is_cancelled());
    let read = context::context_run_projection_guard_for(id);
    tokio::pin!(read);
    assert!(
        futures::poll!(read.as_mut()).is_pending(),
        "the detached blocking writer still owns ENGINE"
    );
    resume_tx.send(()).unwrap();
    let guard = tokio::time::timeout(Duration::from_secs(5), read)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        context::load_context_run_state_with_projection_guard(&fixture.state, &guard)
            .await
            .unwrap()
            .tenant_context,
        stream_tenant("bob")
    );
}
