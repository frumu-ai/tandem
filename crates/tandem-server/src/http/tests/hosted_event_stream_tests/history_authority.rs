// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use crate::app::state::{
    automation_v2_run_history_read_guard, current_automation_v2_run_read_source,
    load_automation_v2_run_history_shard_with_guard, load_automation_v2_run_read_sources,
    write_automation_v2_run_history_shard, write_automation_v2_run_history_shard_gated,
    AtomicWriteTestGate,
};

async fn archived(
    fixture: &StreamFixture,
    native_id: &str,
) -> (String, crate::AutomationV2RunRecord) {
    let id = managed_automation(fixture, native_id, "group").await;
    let mut run = fixture
        .state
        .automation_v2_runs
        .write()
        .await
        .remove(native_id)
        .unwrap();
    run.status = crate::AutomationRunStatus::Completed;
    run.automation_snapshot = fixture.state.automations_v2.write().await.remove(native_id);
    write_automation_v2_run_history_shard(&fixture.state.automation_v2_runs_path, &run)
        .await
        .unwrap();
    (id, run)
}

async fn ready_with_control(
    fixture: &StreamFixture,
    run_id: &str,
) -> tokio::sync::mpsc::Receiver<context::ContextRunsQueuedFrame> {
    fixture
        .context_run("history-control", "alice", "interactive")
        .await;
    let previous = fixture.state.event_bus.receiver_count();
    let receiver = frame_receiver(fixture, vec![run_id.into(), "history-control".into()]).await;
    wait_live_subscriber(&fixture.state, previous).await;
    receiver
}

#[tokio::test]
async fn buffered_context_history_ownership_changed_after_queue_filters_archived_frames() {
    let fixture = group_fixture().await;
    let (id, mut run) = archived(&fixture, "archived-owner-revoke").await;
    append_replay(&fixture, &id, "context_run_event", 1, 1, "archived-denied");
    append_replay(
        &fixture,
        &id,
        "blackboard_patch",
        1,
        2,
        "archived-patch-denied",
    );
    let receiver = ready_with_control(&fixture, &id).await;
    assert_eq!(receiver.len(), 3);
    run.tenant_context = stream_tenant("charlie");
    write_automation_v2_run_history_shard(&fixture.state.automation_v2_runs_path, &run)
        .await
        .unwrap();
    assert_resource_revoked_identity_current(&fixture, &id).await;
    let mut body = dequeue_body(&fixture, receiver).into_data_stream();
    assert_eq!(
        next_frame(&mut body).await["subscribed_run_ids"],
        json!(["history-control"])
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), body.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn buffered_context_history_snapshot_sharing_is_fresh_after_later_native_lookup_wait() {
    let fixture = group_fixture().await;
    let (id, mut run) = archived(&fixture, "archived-later-wait").await;
    fixture.workflow("history-later-workflow", "alice").await;
    fixture
        .context_run("workflow-history-later-workflow", "alice", "workflow")
        .await;
    let previous = fixture.state.event_bus.receiver_count();
    let receiver = frame_receiver(
        &fixture,
        vec![id.clone(), "workflow-history-later-workflow".into()],
    )
    .await;
    wait_live_subscriber(&fixture.state, previous).await;
    let held = fixture.state.workflow_runs.write().await;
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut body = dequeue_body_observed(&fixture, receiver, progress_tx).into_data_stream();
    let reader = tokio::spawn(async move { next_frame(&mut body).await });
    assert_eq!(resolved_id(&mut progress_rx).await, id);
    assert!(!reader.is_finished());
    run.automation_snapshot
        .as_mut()
        .unwrap()
        .metadata
        .as_mut()
        .unwrap()["resource_access"]["audience_principals"] = json!([]);
    write_automation_v2_run_history_shard(&fixture.state.automation_v2_runs_path, &run)
        .await
        .unwrap();
    assert_resource_revoked_identity_current(&fixture, &id).await;
    drop(held);
    let ready = tokio::time::timeout(Duration::from_secs(5), reader)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        ready["subscribed_run_ids"],
        json!(["workflow-history-later-workflow"])
    );
}

#[tokio::test]
async fn buffered_context_history_current_hot_snapshot_is_fresh_after_later_lookup_wait() {
    let fixture = group_fixture().await;
    let id = managed_automation(&fixture, "hot-snapshot-later-wait", "group").await;
    let snapshot = fixture
        .state
        .automations_v2
        .write()
        .await
        .remove("hot-snapshot-later-wait")
        .unwrap();
    fixture
        .state
        .automation_v2_runs
        .write()
        .await
        .get_mut("hot-snapshot-later-wait")
        .unwrap()
        .automation_snapshot = Some(snapshot);
    fixture.workflow("hot-snapshot-workflow", "alice").await;
    fixture
        .context_run("workflow-hot-snapshot-workflow", "alice", "workflow")
        .await;
    let previous = fixture.state.event_bus.receiver_count();
    let receiver = frame_receiver(
        &fixture,
        vec![id.clone(), "workflow-hot-snapshot-workflow".into()],
    )
    .await;
    wait_live_subscriber(&fixture.state, previous).await;
    let held = fixture.state.workflow_runs.write().await;
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut body = dequeue_body_observed(&fixture, receiver, progress_tx).into_data_stream();
    let reader = tokio::spawn(async move { next_frame(&mut body).await });
    assert_eq!(resolved_id(&mut progress_rx).await, id);
    fixture
        .state
        .automation_v2_runs
        .write()
        .await
        .get_mut("hot-snapshot-later-wait")
        .unwrap()
        .automation_snapshot
        .as_mut()
        .unwrap()
        .metadata
        .as_mut()
        .unwrap()["resource_access"]["audience_principals"] = json!([]);
    drop(held);
    let ready = tokio::time::timeout(Duration::from_secs(5), reader)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        ready["subscribed_run_ids"],
        json!(["workflow-hot-snapshot-workflow"])
    );
}

#[tokio::test]
async fn buffered_context_history_guard_reads_with_writer_queued_and_rejects_other_store_path() {
    let fixture = group_fixture().await;
    let (_, mut run) = archived(&fixture, "history-queued-writer").await;
    let guard = automation_v2_run_history_read_guard(&fixture.state.automation_v2_runs_path).await;
    run.tenant_context = stream_tenant("charlie");
    let write = write_automation_v2_run_history_shard(&fixture.state.automation_v2_runs_path, &run);
    tokio::pin!(write);
    assert!(futures::poll!(write.as_mut()).is_pending());
    let sources = tokio::time::timeout(
        Duration::from_secs(5),
        load_automation_v2_run_read_sources(&fixture.state, &guard, "history-queued-writer"),
    )
    .await
    .unwrap()
    .unwrap();
    let current =
        current_automation_v2_run_read_source("history-queued-writer", None, &sources).unwrap();
    assert_eq!(current.tenant_context, stream_tenant("bob"));
    let mut different = fixture.state.clone();
    different.automation_v2_runs_path = fixture._temp.path().join("other-store.json");
    assert!(
        load_automation_v2_run_read_sources(&different, &guard, "history-queued-writer")
            .await
            .is_none()
    );
    drop(guard);
    tokio::time::timeout(Duration::from_secs(5), write)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn buffered_context_history_cancelled_detached_blocking_write_retains_authority() {
    let fixture = group_fixture().await;
    let (_, mut run) = archived(&fixture, "history-cancelled-write").await;
    run.tenant_context = stream_tenant("charlie");
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
    let active_path = fixture.state.automation_v2_runs_path.clone();
    let writer = tokio::spawn(async move {
        write_automation_v2_run_history_shard_gated(
            &active_path,
            &run,
            AtomicWriteTestGate {
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
    let read = automation_v2_run_history_read_guard(&fixture.state.automation_v2_runs_path);
    tokio::pin!(read);
    assert!(
        futures::poll!(read.as_mut()).is_pending(),
        "cancelled caller cannot release the detached blocking writer's history guard"
    );
    resume_tx.send(()).unwrap();
    let guard = tokio::time::timeout(Duration::from_secs(5), read)
        .await
        .unwrap();
    let run = load_automation_v2_run_history_shard_with_guard(&guard, "history-cancelled-write")
        .await
        .unwrap();
    assert_eq!(run.tenant_context, stream_tenant("charlie"));
}

#[tokio::test]
async fn buffered_context_history_legitimate_archived_recovered_and_hot_snapshot_fallbacks_remain_live(
) {
    let fixture = group_fixture().await;
    let (archived_id, _) = archived(&fixture, "history-current-archived").await;

    let recovered_id = "automation-v2-history-current-recovered";
    fixture
        .context_run(recovered_id, "alice", "automation_v2")
        .await;
    let mut projection =
        context::load_context_run_state_sync(&fixture.state, recovered_id).unwrap();
    projection.status = crate::http::context_types::ContextRunStatus::Completed;
    context::save_context_run_state(&fixture.state, &projection)
        .await
        .unwrap();

    let hot_id = managed_automation(&fixture, "history-current-hot", "group").await;
    let hot = fixture
        .state
        .automation_v2_runs
        .read()
        .await
        .get("history-current-hot")
        .unwrap()
        .clone();
    let mut old_history = hot.clone();
    old_history.tenant_context = stream_tenant("charlie");
    // Equal detail means the canonical getter deliberately prefers hot.
    write_automation_v2_run_history_shard(&fixture.state.automation_v2_runs_path, &old_history)
        .await
        .unwrap();
    assert_eq!(
        fixture
            .state
            .get_automation_v2_run("history-current-hot")
            .await
            .unwrap()
            .tenant_context,
        hot.tenant_context
    );

    let snapshot_id = managed_automation(&fixture, "history-current-snapshot", "group").await;
    let snapshot = fixture
        .state
        .automations_v2
        .write()
        .await
        .remove("history-current-snapshot")
        .unwrap();
    fixture
        .state
        .automation_v2_runs
        .write()
        .await
        .get_mut("history-current-snapshot")
        .unwrap()
        .automation_snapshot = Some(snapshot);

    let ids = vec![snapshot_id, hot_id, recovered_id.into(), archived_id];
    let receiver = frame_receiver(&fixture, ids.clone()).await;
    wait_buffered(&receiver, 1).await;
    let mut body = dequeue_body(&fixture, receiver).into_data_stream();
    assert_eq!(
        next_frame(&mut body).await["subscribed_run_ids"],
        json!(ids)
    );
}

#[tokio::test]
async fn buffered_context_history_misbound_native_record_never_grants_a_requested_id() {
    let fixture = group_fixture().await;
    let id = managed_automation(&fixture, "history-misbound-record", "group").await;
    let receiver = ready_with_control(&fixture, &id).await;
    fixture
        .state
        .automation_v2_runs
        .write()
        .await
        .get_mut("history-misbound-record")
        .unwrap()
        .run_id = "other-native-record".into();
    let mut body = dequeue_body(&fixture, receiver).into_data_stream();
    assert_eq!(
        next_frame(&mut body).await["subscribed_run_ids"],
        json!(["history-control"])
    );
}
