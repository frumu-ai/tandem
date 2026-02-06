use std::path::PathBuf;
use tandem_lib::orchestrator::policy::{PolicyConfig, PolicyEngine};
use tandem_lib::orchestrator::{
    Budget, OrchestratorConfig, OrchestratorEvent, OrchestratorStore, Run, Task, TaskScheduler,
    TaskState,
};
use tempfile::tempdir;

#[tokio::test]
async fn test_orchestrator_full_flow() {
    let dir = tempdir().unwrap();
    let workspace = dir.path();
    let store = OrchestratorStore::new(workspace).expect("Failed to create store");

    // 1. Persistence
    let config = OrchestratorConfig::default();
    let mut run = Run::new(
        "test-run-1".to_string(),
        "test-session-1".to_string(),
        "Test objective".to_string(),
        config,
    );

    store.save_run(&run).expect("Failed to save run");
    let loaded = store.load_run("test-run-1").expect("Failed to load run");
    assert_eq!(loaded.run_id, "test-run-1");

    // 2. Tasks
    let mut task1 = Task::new("t1".into(), "Task 1".into(), "Do it".into());
    task1.state = TaskState::Pending;
    run.tasks.push(task1);

    store.save_run(&run).expect("Failed to update run");
    let loaded = store.load_run("test-run-1").expect("Failed to reload run");
    assert_eq!(loaded.tasks.len(), 1);
    assert_eq!(loaded.tasks[0].id, "t1");

    // 3. Events
    let event = OrchestratorEvent::RunCreated {
        run_id: "test-run-1".into(),
        objective: "Test objective".into(),
        timestamp: chrono::Utc::now(),
    };
    store
        .append_event("test-run-1", &event)
        .expect("Failed to append event");
    let events = store
        .load_events("test-run-1")
        .expect("Failed to load events");
    assert!(events.len() >= 1);

    // 4. Budget
    run.budget.tokens_used = 500u64;
    store.save_run(&run).expect("Failed to save budget");
    let loaded = store
        .load_run("test-run-1")
        .expect("Failed to reload budget");
    assert_eq!(loaded.budget.tokens_used, 500u64);
}

#[tokio::test]
async fn test_policy_logic() {
    let dir = tempdir().unwrap();
    let workspace = dir.path();
    let policy_config = PolicyConfig::new(workspace.to_path_buf());
    let policy = PolicyEngine::new(policy_config);

    let inside_path = workspace.join("file.txt").to_string_lossy().to_string();
    assert!(policy.is_within_workspace(&inside_path));

    let outside_path = if cfg!(windows) {
        "C:\\tmp\\external.txt"
    } else {
        "/tmp/external.txt"
    };
    assert!(!policy.is_within_workspace(outside_path));
}

#[test]
fn test_budget_math() {
    let mut budget = Budget {
        max_iterations: 10,
        iterations_used: 5,
        max_tokens: 1000,
        tokens_used: 500,
        max_wall_time_secs: 3600,
        wall_time_secs: 1800,
        max_subagent_runs: 20,
        subagent_runs_used: 10,
        exceeded: false,
        exceeded_reason: None,
    };

    assert_eq!(budget.usage_percentage(), 0.5f64);

    budget.tokens_used = 1100;
    assert!(budget.is_exceeded());
}

#[test]
fn test_scheduler_logic() {
    let mut task1 = Task::new("t1".into(), "T1".into(), "Desc".into());
    task1.state = TaskState::Pending;

    let mut task2 = Task::new("t2".into(), "T2".into(), "Desc".into());
    task2.dependencies = vec!["t1".into()];
    task2.state = TaskState::Pending;

    let tasks = vec![task1, task2];

    let next = TaskScheduler::get_next_runnable(&tasks);
    assert_eq!(next.unwrap().id, "t1");
}

#[test]
fn test_config_defaults() {
    let config = OrchestratorConfig::default();
    assert!(config.max_iterations > 0);
}
