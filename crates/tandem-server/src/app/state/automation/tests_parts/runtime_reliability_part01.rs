// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[test]
fn required_source_read_paths_do_not_leak_from_downstream_nodes() {
    let mut producer = bare_node();
    producer.node_id = "validate_incident_event".to_string();
    producer.objective = "Validate only the supplied webhook payload and write demo-output/context/incident-context.md. This stage must not read workspace files."
        .to_string();
    producer.metadata = Some(json!({
        "builder": {
            "output_files": ["demo-output/context/incident-context.md"]
        }
    }));

    let mut consumer = bare_node();
    consumer.node_id = "draft_incident_update".to_string();
    consumer.objective = "Use the read tool to read the concrete source file demo-output/context/incident-context.md before writing the draft."
        .to_string();
    consumer.depends_on = vec![producer.node_id.clone()];
    consumer.metadata = Some(json!({
        "builder": {
            "input_files": ["demo-output/context/incident-context.md"]
        }
    }));

    let automation = automation_with_output_targets(
        vec![producer.clone(), consumer.clone()],
        Vec::new(),
    );
    let producer_write_files = automation_node_must_write_files_for_automation(
        &automation,
        &producer,
        None,
    );
    let producer_paths =
        super::enforcement::automation_node_required_source_read_paths_for_automation(
            &automation,
            &producer,
            "/tmp/workspace",
            None,
        );
    let consumer_paths =
        super::enforcement::automation_node_required_source_read_paths_for_automation(
            &automation,
            &consumer,
            "/tmp/workspace",
            None,
        );

    assert!(
        producer_paths.is_empty(),
        "producer must not read its own future output"
    );
    assert_eq!(
        producer_write_files,
        vec!["demo-output/context/incident-context.md".to_string()]
    );
    assert_eq!(
        consumer_paths,
        vec!["demo-output/context/incident-context.md".to_string()]
    );
}

#[test]
fn planner_artifact_paths_publish_without_duplicate_workspace_writes() {
    let mut node = bare_node();
    node.node_id = "validate_incident_event".to_string();
    node.objective =
        "Write the validated incident context to demo-output/context/incident-context.md."
            .to_string();
    node.metadata = Some(json!({
        "artifact": {
            "path": "demo-output/context/incident-context.md",
            "visible_in_run": true
        },
        "filesystem_policy": {
            "read_paths": [],
            "write_paths": ["demo-output/context/incident-context.md"]
        }
    }));
    let mut downstream = bare_node();
    downstream.node_id = "draft_incident_update".to_string();
    downstream.depends_on = vec![node.node_id.clone()];
    let automation = automation_with_output_targets(
        vec![node.clone(), downstream],
        vec!["demo-output/context/incident-context.md".to_string()],
    );

    assert!(!automation_node_is_terminal_for_automation(&automation, &node));
    let must_write_files =
        automation_node_must_write_files_for_automation(&automation, &node, None);

    assert!(must_write_files.is_empty());
    assert!(automation_node_can_access_declared_output_targets(&automation, &node));
}
