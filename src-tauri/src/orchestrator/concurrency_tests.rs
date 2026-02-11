use crate::orchestrator::engine::OrchestratorEngine;
use crate::orchestrator::locks::PathLockManager;
use crate::orchestrator::policy::{PolicyConfig, PolicyEngine};
use crate::orchestrator::store::OrchestratorStore;
use crate::orchestrator::types::{OrchestratorConfig, Run, RunStatus, Task, TaskState};
use crate::sidecar::{SidecarConfig, SidecarManager};
use crate::stream_hub::StreamHub;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, Mutex};

#[tokio::test]
async fn executes_multiple_tasks_concurrently() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_nanos();

    let workspace_path = std::env::temp_dir().join(format!("tandem_orch_test_{}", now));
    std::fs::create_dir_all(&workspace_path).unwrap();

    let mut config = OrchestratorConfig::default();
    config.max_parallel_tasks = 3;
    config.llm_parallel = 3;

    let mut run = Run::new(
        "test-run".to_string(),
        "base-session".to_string(),
        "".to_string(),
        config,
    );
    run.status = RunStatus::AwaitingApproval;
    run.tasks = vec![
        Task::new("t1".to_string(), "t1".to_string(), "".to_string()),
        Task::new("t2".to_string(), "t2".to_string(), "".to_string()),
        Task::new("t3".to_string(), "t3".to_string(), "".to_string()),
    ];

    let policy = PolicyEngine::new(PolicyConfig::new(workspace_path.clone()));
    let store = OrchestratorStore::new(&workspace_path).unwrap();
    store.create_run_dir(&run.run_id).unwrap();
    store.save_run(&run).unwrap();

    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let sidecar = Arc::new(SidecarManager::new(SidecarConfig::default()));
    let stream_hub = Arc::new(StreamHub::new());

    let current_running = Arc::new(AtomicUsize::new(0));
    let max_running = Arc::new(AtomicUsize::new(0));
    let start_order: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let engine = OrchestratorEngine::new(
        run,
        policy,
        store,
        sidecar,
        stream_hub,
        workspace_path.clone(),
        event_tx,
    )
    .with_test_task_executor({
        let current_running = current_running.clone();
        let max_running = max_running.clone();
        let start_order = start_order.clone();
        move |engine: OrchestratorEngine, task: Task| {
            let current_running = current_running.clone();
            let max_running = max_running.clone();
            let start_order = start_order.clone();
            async move {
                {
                    let mut order = start_order.lock().await;
                    order.push(task.id.clone());
                }

                let running_now = current_running.fetch_add(1, Ordering::SeqCst) + 1;
                loop {
                    let prev = max_running.load(Ordering::SeqCst);
                    if running_now > prev {
                        if max_running
                            .compare_exchange(prev, running_now, Ordering::SeqCst, Ordering::SeqCst)
                            .is_ok()
                        {
                            break;
                        }
                    } else {
                        break;
                    }
                }

                tokio::time::sleep(Duration::from_millis(200)).await;

                current_running.fetch_sub(1, Ordering::SeqCst);

                engine.set_task_state(&task.id, TaskState::Done).await;

                Ok(())
            }
        }
    });

    engine.execute().await.unwrap();

    assert_eq!(max_running.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn path_write_lock_excludes_concurrent_writers() {
    let locks = Arc::new(PathLockManager::new());
    let path = PathBuf::from("C:\\tmp\\some-file.txt");

    let started = Arc::new(AtomicUsize::new(0));
    let acquired_late = Arc::new(AtomicUsize::new(0));

    let locks1 = locks.clone();
    let path1 = path.clone();
    let started1 = started.clone();

    let t1 = tokio::spawn(async move {
        let _guard = locks1.write_lock(&path1).await;
        started1.store(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(150)).await;
    });

    while started.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }

    let locks2 = locks.clone();
    let path2 = path.clone();
    let acquired_late2 = acquired_late.clone();

    let start = tokio::time::Instant::now();
    let t2 = tokio::spawn(async move {
        let _guard = locks2.write_lock(&path2).await;
        let elapsed = start.elapsed();
        if elapsed >= Duration::from_millis(120) {
            acquired_late2.store(1, Ordering::SeqCst);
        }
    });

    t1.await.unwrap();
    t2.await.unwrap();

    assert_eq!(acquired_late.load(Ordering::SeqCst), 1);
}

fn make_workspace_path(prefix: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_nanos();
    std::env::temp_dir().join(format!("{}_{}", prefix, now))
}

fn build_engine(run: Run, workspace_path: PathBuf) -> OrchestratorEngine {
    std::fs::create_dir_all(&workspace_path).unwrap();

    let policy = PolicyEngine::new(PolicyConfig::new(workspace_path.clone()));
    let store = OrchestratorStore::new(&workspace_path).unwrap();
    store.save_run(&run).unwrap();

    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let sidecar = Arc::new(SidecarManager::new(SidecarConfig::default()));
    let stream_hub = Arc::new(StreamHub::new());

    OrchestratorEngine::new(run, policy, store, sidecar, stream_hub, workspace_path, event_tx)
}

#[tokio::test]
async fn cancel_and_finalize_transitions_paused_run() {
    let workspace_path = make_workspace_path("tandem_orch_cancel_paused");

    let mut run = Run::new(
        "cancel-paused".to_string(),
        "session-cancel-paused".to_string(),
        "cancel paused".to_string(),
        OrchestratorConfig::default(),
    );
    run.status = RunStatus::Paused;

    let engine = build_engine(run, workspace_path);
    engine.cancel_and_finalize().await.unwrap();

    let snapshot = engine.get_snapshot().await;
    assert_eq!(snapshot.status, RunStatus::Cancelled);
}

#[tokio::test]
async fn resume_requires_paused_status() {
    let workspace_path = make_workspace_path("tandem_orch_resume_requires_paused");

    let mut run = Run::new(
        "resume-requires-paused".to_string(),
        "session-resume".to_string(),
        "resume gate".to_string(),
        OrchestratorConfig::default(),
    );
    run.status = RunStatus::Executing;

    let engine = build_engine(run, workspace_path);
    let err = engine.resume().await.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("Run is not paused"));
}

#[tokio::test]
async fn set_base_session_for_resume_requires_paused() {
    let workspace_path = make_workspace_path("tandem_orch_set_session_requires_paused");

    let mut run = Run::new(
        "set-session-requires-paused".to_string(),
        "session-old".to_string(),
        "set session".to_string(),
        OrchestratorConfig::default(),
    );
    run.status = RunStatus::Executing;

    let engine = build_engine(run, workspace_path);
    let err = engine
        .set_base_session_for_resume("session-new".to_string())
        .await
        .unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("Run must be paused"));
}

#[tokio::test]
async fn set_base_session_for_resume_clears_non_done_task_sessions() {
    let workspace_path = make_workspace_path("tandem_orch_set_session_clears_tasks");

    let mut run = Run::new(
        "set-session-clears-tasks".to_string(),
        "session-old".to_string(),
        "set session".to_string(),
        OrchestratorConfig::default(),
    );
    run.status = RunStatus::Paused;

    let mut done_task = Task::new("done".to_string(), "Done".to_string(), "".to_string());
    done_task.state = TaskState::Done;
    done_task.session_id = Some("session-done".to_string());

    let mut pending_task = Task::new("pending".to_string(), "Pending".to_string(), "".to_string());
    pending_task.state = TaskState::Pending;
    pending_task.session_id = Some("session-pending".to_string());

    run.tasks = vec![done_task, pending_task];

    let engine = build_engine(run, workspace_path);
    engine
        .set_base_session_for_resume("session-new".to_string())
        .await
        .unwrap();

    let base_session = engine.get_base_session_id().await;
    assert_eq!(base_session, "session-new");

    let tasks = engine.get_tasks().await;
    let done = tasks.iter().find(|t| t.id == "done").unwrap();
    let pending = tasks.iter().find(|t| t.id == "pending").unwrap();

    assert_eq!(done.session_id.as_deref(), Some("session-done"));
    assert_eq!(pending.session_id, None);
}

#[tokio::test]
async fn set_base_session_for_resume_allows_cancelled() {
    let workspace_path = make_workspace_path("tandem_orch_set_session_cancelled");

    let mut run = Run::new(
        "set-session-cancelled".to_string(),
        "session-old".to_string(),
        "set session".to_string(),
        OrchestratorConfig::default(),
    );
    run.status = RunStatus::Cancelled;
    run.tasks = vec![Task::new(
        "pending".to_string(),
        "Pending".to_string(),
        "".to_string(),
    )];

    let engine = build_engine(run, workspace_path);
    engine
        .set_base_session_for_resume("session-new".to_string())
        .await
        .unwrap();

    let base_session = engine.get_base_session_id().await;
    assert_eq!(base_session, "session-new");
}

#[tokio::test]
async fn restart_after_cancel_resets_cancel_and_pause() {
    let workspace_path = make_workspace_path("tandem_orch_restart_after_cancel");

    let mut run = Run::new(
        "restart-after-cancel".to_string(),
        "session-restart".to_string(),
        "restart run".to_string(),
        OrchestratorConfig::default(),
    );
    run.status = RunStatus::Cancelled;
    run.tasks = vec![Task::new(
        "task-1".to_string(),
        "Task 1".to_string(),
        "".to_string(),
    )];

    let engine = build_engine(run, workspace_path).with_test_task_executor(
        move |engine: OrchestratorEngine, task: Task| async move {
            engine.set_task_state(&task.id, TaskState::Done).await;
            Ok(())
        },
    );

    engine.restart().await.unwrap();
    let snapshot = engine.get_snapshot().await;
    assert_eq!(snapshot.status, RunStatus::Completed);
}

#[tokio::test]
async fn start_honors_preexisting_cancel_signal() {
    let workspace_path = make_workspace_path("tandem_orch_start_pre_cancel");

    let run = Run::new(
        "start-pre-cancel".to_string(),
        "session-pre-cancel".to_string(),
        "start cancel".to_string(),
        OrchestratorConfig::default(),
    );

    let engine = build_engine(run, workspace_path);
    engine.cancel();
    engine.start().await.unwrap();

    let snapshot = engine.get_snapshot().await;
    assert_eq!(snapshot.status, RunStatus::Cancelled);
}

#[tokio::test]
async fn cancellation_wins_over_completion_race() {
    let workspace_path = make_workspace_path("tandem_orch_cancel_race");

    let mut run = Run::new(
        "cancel-race".to_string(),
        "session-cancel-race".to_string(),
        "cancel race".to_string(),
        OrchestratorConfig::default(),
    );
    run.status = RunStatus::AwaitingApproval;
    run.tasks = vec![Task::new(
        "task-race".to_string(),
        "Task Race".to_string(),
        "".to_string(),
    )];

    let engine = build_engine(run, workspace_path).with_test_task_executor(
        move |engine: OrchestratorEngine, task: Task| async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            engine.set_task_state(&task.id, TaskState::Done).await;
            Ok(())
        },
    );

    let exec_engine = engine.clone();
    let exec_handle = tokio::spawn(async move { exec_engine.execute().await });

    tokio::time::sleep(Duration::from_millis(25)).await;
    engine.cancel_and_finalize().await.unwrap();

    let exec_result = exec_handle.await.unwrap();
    assert!(exec_result.is_ok());

    let snapshot = engine.get_snapshot().await;
    assert_eq!(snapshot.status, RunStatus::Cancelled);
}

#[tokio::test]
async fn pause_interrupts_long_running_task_promptly() {
    let workspace_path = make_workspace_path("tandem_orch_pause_interrupts");

    let mut run = Run::new(
        "pause-interrupt".to_string(),
        "session-pause-interrupt".to_string(),
        "pause interrupt".to_string(),
        OrchestratorConfig::default(),
    );
    run.status = RunStatus::AwaitingApproval;
    run.tasks = vec![Task::new(
        "task-pause".to_string(),
        "Task Pause".to_string(),
        "".to_string(),
    )];

    let engine = build_engine(run, workspace_path).with_test_task_executor(
        move |_engine: OrchestratorEngine, _task: Task| async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            Ok(())
        },
    );

    let exec_engine = engine.clone();
    let exec_handle = tokio::spawn(async move { exec_engine.execute().await });

    tokio::time::sleep(Duration::from_millis(50)).await;
    engine.pause().await;

    let joined = tokio::time::timeout(Duration::from_secs(2), exec_handle).await;
    assert!(
        joined.is_ok(),
        "execute did not return promptly after pause"
    );
    assert!(joined.unwrap().unwrap().is_ok());

    let snapshot = engine.get_snapshot().await;
    assert_eq!(snapshot.status, RunStatus::Paused);
    let tasks = engine.get_tasks().await;
    assert!(tasks.iter().all(|t| t.state != TaskState::InProgress));
}

#[tokio::test]
async fn cancel_interrupts_long_running_task_promptly() {
    let workspace_path = make_workspace_path("tandem_orch_cancel_interrupts");

    let mut run = Run::new(
        "cancel-interrupt".to_string(),
        "session-cancel-interrupt".to_string(),
        "cancel interrupt".to_string(),
        OrchestratorConfig::default(),
    );
    run.status = RunStatus::AwaitingApproval;
    run.tasks = vec![Task::new(
        "task-cancel".to_string(),
        "Task Cancel".to_string(),
        "".to_string(),
    )];

    let engine = build_engine(run, workspace_path).with_test_task_executor(
        move |_engine: OrchestratorEngine, _task: Task| async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            Ok(())
        },
    );

    let exec_engine = engine.clone();
    let exec_handle = tokio::spawn(async move { exec_engine.execute().await });

    tokio::time::sleep(Duration::from_millis(50)).await;
    engine.cancel_and_finalize().await.unwrap();

    let joined = tokio::time::timeout(Duration::from_secs(2), exec_handle).await;
    assert!(
        joined.is_ok(),
        "execute did not return promptly after cancel"
    );
    assert!(joined.unwrap().unwrap().is_ok());

    let snapshot = engine.get_snapshot().await;
    assert_eq!(snapshot.status, RunStatus::Cancelled);
    let tasks = engine.get_tasks().await;
    assert!(tasks.iter().all(|t| t.state != TaskState::InProgress));
}

#[tokio::test]
async fn failed_task_without_runnable_work_transitions_run_to_failed() {
    let workspace_path = make_workspace_path("tandem_orch_failed_terminal");

    let mut run = Run::new(
        "failed-terminal".to_string(),
        "session-failed-terminal".to_string(),
        "failed terminal".to_string(),
        OrchestratorConfig::default(),
    );
    run.status = RunStatus::AwaitingApproval;

    let mut failed_task = Task::new(
        "task-failed".to_string(),
        "Task Failed".to_string(),
        "".to_string(),
    );
    failed_task.state = TaskState::Failed;
    failed_task.retry_count = 3;
    failed_task.error_message = Some("Max retries exceeded".to_string());

    let mut done_task = Task::new(
        "task-done".to_string(),
        "Task Done".to_string(),
        "".to_string(),
    );
    done_task.state = TaskState::Done;

    run.tasks = vec![failed_task, done_task];

    let engine = build_engine(run, workspace_path);
    let result = tokio::time::timeout(Duration::from_secs(2), engine.execute()).await;
    assert!(
        result.is_ok(),
        "execute did not return for terminal failed state"
    );
    assert!(result.unwrap().is_ok());

    let snapshot = engine.get_snapshot().await;
    assert_eq!(snapshot.status, RunStatus::Failed);
}
