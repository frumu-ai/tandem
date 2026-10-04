// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[tokio::test]
async fn solution_observation_is_read_only_and_rejects_unknown_content() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().to_str().unwrap();
    let runtime = AgentTeamRuntime::new(dir.path().join("audit"));
    let (template, owner) = fixture();
    assert!(runtime
        .observe_solution_template(workspace, &template.template_id, &owner)
        .await
        .is_err());
    assert!(!dir.path().join(".tandem").exists());
    runtime
        .stage_solution_template(workspace, template.clone(), owner.clone())
        .await
        .unwrap();
    let path = dir
        .path()
        .join(".tandem/agent-team/templates")
        .join(AgentTeamRuntime::template_filename(&template.template_id));
    let mut document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    document["unrecognized_content"] = serde_json::json!("must not retain the old receipt");
    std::fs::write(&path, canonical_json(&document).unwrap()).unwrap();
    assert!(runtime
        .observe_solution_template(workspace, &template.template_id, &owner)
        .await
        .is_err());
    std::fs::remove_file(&path).unwrap();
    assert!(runtime
        .observe_solution_template(workspace, &template.template_id, &owner)
        .await
        .is_err());
    assert!(
        !path.exists(),
        "observation must never restage a missing file"
    );
}
