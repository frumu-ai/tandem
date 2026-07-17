// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[test]
fn completion_deliverable_assertion_ignores_skipped_publication_artifact() {
    let workspace = completion_test_workspace();
    let mut automation = automation_with_required_output(
        ".tandem/artifacts/publish-approved-update.md",
        "report_markdown",
    );
    automation.flow.nodes[0].metadata = Some(json!({
        "artifact": { "path": "reports/final.md" },
        "builder": { "output_path": ".tandem/artifacts/publish-approved-update.md" }
    }));
    automation.output_targets = vec!["reports/final.md".to_string()];
    let mut run = test_run_with_output(json!({
        "status": "skipped",
        "summary": "Skipped: upstream triage found no work."
    }));
    run.checkpoint.completed_nodes = vec!["research-brief".to_string()];

    let state = assert_completion_deliverables(
        &automation,
        &run,
        workspace.to_str().expect("workspace path"),
    );

    assert_eq!(state, CompletionDeliverableState::Satisfied);
    let _ = std::fs::remove_dir_all(workspace);
}

#[test]
fn completion_deliverable_assertion_checks_extensionless_file_uri_output_target() {
    let workspace = completion_test_workspace();
    let mut automation = test_automation();
    automation.flow.nodes.clear();
    automation.output_targets = vec!["file://Dockerfile".to_string()];
    let mut run = test_run_with_output(json!({"status": "completed"}));
    run.checkpoint.node_outputs.clear();

    let state = assert_completion_deliverables(
        &automation,
        &run,
        workspace.to_str().expect("workspace path"),
    );

    assert!(matches!(
        state,
        CompletionDeliverableState::Failed { ref detail }
            if detail.contains("Dockerfile")
    ));
    let _ = std::fs::remove_dir_all(workspace);
}

#[test]
fn completion_deliverable_assertion_accepts_existing_file_uri_output_target() {
    let workspace = completion_test_workspace();
    write_completion_artifact(&workspace, "reports/final.md", &substantive_markdown());
    let mut automation = test_automation();
    automation.flow.nodes.clear();
    automation.output_targets = vec!["file://reports/final.md".to_string()];
    let mut run = test_run_with_output(json!({
        "status": "completed",
        "published_path": "reports/final.md"
    }));
    run.checkpoint.completed_nodes.clear();

    let state = assert_completion_deliverables(
        &automation,
        &run,
        workspace.to_str().expect("workspace path"),
    );

    assert_eq!(state, CompletionDeliverableState::Satisfied);
    let _ = std::fs::remove_dir_all(workspace);
}
