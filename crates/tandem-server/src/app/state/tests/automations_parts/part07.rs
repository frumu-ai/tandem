#[test]
fn automation_v2_run_store_backfills_definition_metadata_from_snapshot() {
    let automation = AutomationSpecBuilder::new("automation-definition-backfill")
        .metadata(json!({ "definition_version": "release-backfill" }))
        .build();
    let mut run =
        AutomationRunBuilder::new("run-definition-backfill", "automation-definition-backfill")
            .build();
    run.automation_snapshot = Some(automation.clone());
    let mut run_value = serde_json::to_value(&run).expect("serialize run");
    let run_object = run_value.as_object_mut().expect("run object");
    run_object.remove("workflow_definition_version");
    run_object.remove("workflow_definition_snapshot_hash");

    let raw = json!({
        "schema_version": AUTOMATION_V2_RUNS_SCHEMA_VERSION,
        "runs": {
            "run-definition-backfill": run_value
        }
    })
    .to_string();
    let (runs, upgraded) = parse_automation_v2_runs_file(&raw).expect("parse backfill run");
    assert!(upgraded);
    let parsed = runs
        .get("run-definition-backfill")
        .expect("backfilled run");
    let expected_hash = crate::stateful_runtime::automation_definition_snapshot_hash(&automation);

    assert_eq!(
        parsed.workflow_definition_version.as_deref(),
        Some("release-backfill")
    );
    assert_eq!(
        parsed.workflow_definition_snapshot_hash.as_deref(),
        Some(expected_hash.as_str())
    );
}

#[tokio::test]
async fn create_automation_v2_run_records_definition_metadata() {
    use crate::automation_v2::execution_profile::ExecutionProfile;

    let root = std::env::temp_dir().join(format!(
        "tandem-definition-metadata-run-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("state root");
    let mut state = test_state_with_path(root.join("shared.json"));
    state.automations_v2_path = root.join("automations_v2.json");
    state.automation_v2_runs_path = root.join("automation_v2_runs.json");
    state.automation_governance_path = root.join("automation_governance.json");
    let automation = AutomationSpecBuilder::new("automation-definition-metadata")
        .name("Definition metadata")
        .execution_profile(ExecutionProfile::Strict)
        .build();
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("persist automation");
    let run = state
        .create_automation_v2_run_with_profile(&automation, "manual", Some(ExecutionProfile::Yolo))
        .await
        .expect("create run");
    let snapshot = run.automation_snapshot.as_ref().expect("snapshot set on run");
    let expected_hash = crate::stateful_runtime::automation_definition_snapshot_hash(snapshot);
    let expected_version =
        crate::stateful_runtime::automation_definition_version(snapshot, &expected_hash);

    assert_eq!(
        snapshot.execution.profile,
        Some(ExecutionProfile::Yolo),
        "snapshot must carry the run-level effective profile"
    );
    assert_eq!(
        run.workflow_definition_version.as_deref(),
        Some(expected_version.as_str())
    );
    assert_eq!(
        run.workflow_definition_snapshot_hash.as_deref(),
        Some(expected_hash.as_str())
    );
}

#[tokio::test]
async fn automation_v2_run_snapshot_replacement_restamps_definition_metadata() {
    let root = std::env::temp_dir().join(format!(
        "tandem-definition-metadata-restamp-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("state root");
    let mut state = ready_test_state().await;
    state.shared_resources_path = root.join("shared.json");
    state.automations_v2_path = root.join("automations_v2.json");
    state.automation_v2_runs_path = root.join("automation_v2_runs.json");
    state.automation_governance_path = root.join("automation_governance.json");
    let automation = AutomationSpecBuilder::new("automation-definition-restamp")
        .metadata(json!({ "definition_version": "initial-definition" }))
        .build();
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("persist automation");
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let original_hash = run.workflow_definition_snapshot_hash.clone();
    let mut replacement_snapshot = run
        .automation_snapshot
        .clone()
        .expect("snapshot set on run");
    replacement_snapshot.metadata = Some(json!({
        "definition_version": "manual-trigger-definition",
        "plan_package": {
            "manual_trigger_record": {
                "run_id": run.run_id,
                "trigger_source": "manual"
            }
        }
    }));
    let expected_hash =
        crate::stateful_runtime::automation_definition_snapshot_hash(&replacement_snapshot);
    let expected_version =
        crate::stateful_runtime::automation_definition_version(&replacement_snapshot, &expected_hash);

    let updated = state
        .update_automation_v2_run(&run.run_id, |row| {
            row.automation_snapshot = Some(replacement_snapshot.clone());
            crate::stateful_runtime::stamp_automation_run_definition_metadata(row);
        })
        .await
        .expect("updated run");

    assert_ne!(original_hash.as_deref(), Some(expected_hash.as_str()));
    assert_eq!(
        updated.workflow_definition_version.as_deref(),
        Some(expected_version.as_str())
    );
    assert_eq!(
        updated.workflow_definition_snapshot_hash.as_deref(),
        Some(expected_hash.as_str())
    );
}
