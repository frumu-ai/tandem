
#[cfg(test)]
mod tests {
    use super::*;
    use crate::decomposition::workflow_plan_decomposition_observation;
    use tandem_workflows::plan_package::WorkflowPlanStep;

    fn test_plan_with_steps(
        steps: Vec<WorkflowPlanStep<Value, Value>>,
    ) -> WorkflowPlan<AutomationV2Schedule<Value>, WorkflowPlanStep<Value, Value>> {
        WorkflowPlan {
            plan_id: "wfplan-test".to_string(),
            planner_version: "v1".to_string(),
            plan_source: "unit_test".to_string(),
            original_prompt: "Test prompt".to_string(),
            normalized_prompt: "test prompt".to_string(),
            confidence: "medium".to_string(),
            title: "Test Plan".to_string(),
            description: None,
            schedule: manual_schedule("UTC".to_string(), json!({"type":"run_once"})),
            execution_target: "automation_v2".to_string(),
            workspace_root: "/tmp/workspace".to_string(),
            steps,
            requires_integrations: Vec::new(),
            allowed_mcp_servers: Vec::new(),
            operator_preferences: None,
            save_options: plan_save_options(),
        }
    }

    #[test]
    fn resolve_workspace_root_candidate_prefers_requested_root() {
        let resolved = resolve_workspace_root_candidate(
            Some("/tmp/requested"),
            "/tmp/default",
            Some("/tmp/cwd"),
        )
        .expect("requested root");

        assert_eq!(resolved, "/tmp/requested");
    }

    #[test]
    fn resolve_workspace_root_candidate_falls_back_to_cwd_when_default_is_invalid() {
        let resolved = resolve_workspace_root_candidate(None, "not-absolute", Some("/tmp/cwd"))
            .expect("cwd fallback");

        assert_eq!(resolved, "/tmp/cwd");
    }

    #[test]
    fn output_contract_is_research_brief_uses_explicit_or_inferred_validator() {
        assert!(output_contract_is_research_brief("brief", None));
        assert!(!output_contract_is_research_brief("structured_json", None));
        assert!(output_contract_is_research_brief(
            "structured_json",
            Some("research_brief")
        ));
        assert!(!output_contract_is_research_brief(
            "brief",
            Some("structured_json")
        ));
    }

    #[test]
    fn output_contract_is_code_patch_uses_explicit_or_inferred_validator() {
        assert!(output_contract_is_code_patch("code_patch", None));
        assert!(!output_contract_is_code_patch("structured_json", None));
        assert!(output_contract_is_code_patch(
            "structured_json",
            Some("code_patch")
        ));
        assert!(!output_contract_is_code_patch(
            "brief",
            Some("structured_json")
        ));
    }

    #[test]
    fn extract_json_value_from_text_handles_wrapped_json() {
        let text = r#"
Here is the planner response:

```json
{"action":"build","assistant_text":"ok","plan":{"title":"Demo","steps":[]}}
```
        "#;
        let value = extract_json_value_from_text(text).expect("json value");
        assert_eq!(value.get("action").and_then(Value::as_str), Some("build"));
        assert_eq!(
            value
                .get("plan")
                .and_then(|plan| plan.get("title"))
                .and_then(Value::as_str),
            Some("Demo")
        );
    }

    #[test]
    fn extract_json_value_from_text_handles_prefixed_json() {
        let text = r#"Planner output:
{"action":"clarify","assistant_text":"Need one detail","clarifier":{"field":"general","question":"Which repo?"}}
"#;
        let value = extract_json_value_from_text(text).expect("json value");
        assert_eq!(value.get("action").and_then(Value::as_str), Some("clarify"));
        assert_eq!(
            value
                .get("clarifier")
                .and_then(|clarifier| clarifier.get("question"))
                .and_then(Value::as_str),
            Some("Which repo?")
        );
    }

    #[test]
    fn truncate_text_respects_utf8_char_boundaries() {
        let input = format!("{}·tail", "a".repeat(599));
        let truncated = truncate_text(&input, 601);
        assert_eq!(truncated, format!("{}·", "a".repeat(599)));
    }

    #[test]
    fn planner_model_spec_falls_back_to_default_model() {
        let spec = planner_model_spec(Some(&json!({
            "model_provider": "openai",
            "model_id": "gpt-5.1"
        })))
        .expect("default planner spec");
        assert_eq!(spec.provider_id, "openai");
        assert_eq!(spec.model_id, "gpt-5.1");
    }

    #[test]
    fn normalize_operator_preferences_infers_parallel_defaults_for_execution_modes() {
        let single = normalize_operator_preferences(Some(json!({
            "execution_mode": "single",
            "max_parallel_agents": 8
        })))
        .expect("single preferences");
        assert_eq!(
            single.get("max_parallel_agents").and_then(Value::as_u64),
            Some(1)
        );

        let team = normalize_operator_preferences(Some(json!({
            "execution_mode": "team",
            "max_parallel_agents": 1
        })))
        .expect("team preferences");
        assert_eq!(
            team.get("max_parallel_agents").and_then(Value::as_u64),
            Some(2)
        );

        let swarm = normalize_operator_preferences(Some(json!({
            "execution_mode": "swarm",
            "max_parallel_agents": 2
        })))
        .expect("swarm preferences");
        assert_eq!(
            swarm.get("max_parallel_agents").and_then(Value::as_u64),
            Some(4)
        );
    }

    #[test]
    fn workflow_step_metadata_defaults_include_project_knowledge() {
        let defaults =
            workflow_step_metadata_defaults("research_sources", "research", "Map the topic", false)
                .expect("metadata defaults");
        let builder = defaults
            .get("builder")
            .and_then(Value::as_object)
            .expect("builder");
        let knowledge = builder
            .get("knowledge")
            .and_then(Value::as_object)
            .expect("knowledge defaults");

        assert_eq!(
            knowledge.get("enabled").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            knowledge.get("reuse_mode").and_then(Value::as_str),
            Some("preflight")
        );
        assert_eq!(
            knowledge.get("trust_floor").and_then(Value::as_str),
            Some("promoted")
        );
        assert_eq!(
            knowledge.get("subject").and_then(Value::as_str),
            Some("Map the topic")
        );
        assert_eq!(
            knowledge
                .get("read_spaces")
                .and_then(Value::as_array)
                .and_then(|spaces| spaces.first())
                .and_then(|space| space.get("scope"))
                .and_then(Value::as_str),
            Some("project")
        );
        assert_eq!(
            knowledge
                .get("promote_spaces")
                .and_then(Value::as_array)
                .and_then(|spaces| spaces.first())
                .and_then(|space| space.get("scope"))
                .and_then(Value::as_str),
            Some("project")
        );
    }

    #[test]
    fn workflow_step_decomposition_metadata_defaults_add_phase_and_retry_hints() {
        let profile = crate::decomposition::WorkflowDecompositionProfile {
            complexity_score: 80,
            tier: crate::decomposition::WorkflowDecompositionTier::VeryComplex,
            recommended_min_leaf_tasks: 30,
            recommended_max_leaf_tasks: 50,
            recommended_phase_count: 4,
            requires_phased_dag: true,
            signals: vec!["scheduled_workflow".to_string()],
            guidance: vec!["Use phased microtasks.".to_string()],
        };
        let mut step: WorkflowPlanStep<Value, Value> = WorkflowPlanStep {
            step_id: "send_report".to_string(),
            kind: "deliver".to_string(),
            objective: "Send the report by email.".to_string(),
            depends_on: vec!["analyze_findings".to_string()],
            agent_role: "sender".to_string(),
            input_refs: vec![],
            output_contract: Some(json!({"kind":"report_markdown"})),
            metadata: None,
        };

        workflow_step_decomposition_metadata_defaults(&mut step, &profile, 3, 4);

        let builder = step
            .metadata
            .as_ref()
            .and_then(|value| value.get("builder"))
            .and_then(Value::as_object)
            .expect("builder");
        assert_eq!(
            builder.get("phase_id").and_then(Value::as_str),
            Some("phase_4_deliver")
        );
        assert_eq!(
            builder.get("task_class").and_then(Value::as_str),
            Some("delivery")
        );
        assert_eq!(
            builder.get("task_kind").and_then(Value::as_str),
            Some("delivery")
        );
        assert_eq!(
            builder.get("retry_class").and_then(Value::as_str),
            Some("delivery_only")
        );
        assert_eq!(
            builder.get("parent_step_id").and_then(Value::as_str),
            Some("analyze_findings")
        );
    }

    #[test]
    fn planner_diagnostics_merges_decomposition_profile_into_payload() {
        let profile = crate::decomposition::WorkflowDecompositionProfile {
            complexity_score: 46,
            tier: crate::decomposition::WorkflowDecompositionTier::Complex,
            recommended_min_leaf_tasks: 20,
            recommended_max_leaf_tasks: 30,
            recommended_phase_count: 3,
            requires_phased_dag: true,
            signals: vec!["connector_backed_sources".to_string()],
            guidance: vec!["Use explicit phases.".to_string()],
        };
        let observation = workflow_plan_decomposition_observation(&profile, 12);
        let diagnostics = planner_diagnostics(None, None, Some(observation)).expect("diagnostics");

        assert_eq!(
            diagnostics
                .get("generated_step_count")
                .and_then(Value::as_u64),
            Some(12)
        );
        assert_eq!(
            diagnostics
                .get("decomposition_profile")
                .and_then(|value| value.get("recommended_phase_count"))
                .and_then(Value::as_u64),
            Some(3)
        );
    }

    #[test]
    fn derive_workflow_step_file_contracts_adds_upstream_input_and_output_files() {
        let mut plan = test_plan_with_steps(vec![
            WorkflowPlanStep {
                step_id: "collect_inputs".to_string(),
                kind: "collect".to_string(),
                objective: "Collect inputs.".to_string(),
                depends_on: vec![],
                agent_role: "planner".to_string(),
                input_refs: vec![],
                output_contract: Some(json!({"kind":"structured_json"})),
                metadata: Some(json!({
                    "builder": {
                        "output_path": ".tandem/artifacts/collect-inputs.json"
                    }
                })),
            },
            WorkflowPlanStep {
                step_id: "draft_report".to_string(),
                kind: "write".to_string(),
                objective: "Draft the report.".to_string(),
                depends_on: vec!["collect_inputs".to_string()],
                agent_role: "writer".to_string(),
                input_refs: vec![json!({
                    "from_step_id": "collect_inputs",
                    "alias": "inputs"
                })],
                output_contract: Some(json!({"kind":"report_markdown"})),
                metadata: Some(json!({
                    "builder": {
                        "output_path": "reports/final.md"
                    }
                })),
            },
        ]);

        derive_workflow_step_file_contracts(&mut plan);

        let collect_builder = plan.steps[0]
            .metadata
            .as_ref()
            .and_then(|value| value.get("builder"))
            .and_then(Value::as_object)
            .expect("collect builder");
        assert_eq!(
            collect_builder
                .get("output_files")
                .and_then(Value::as_array)
                .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
            Some(vec![".tandem/artifacts/collect-inputs.json"])
        );
        let draft_builder = plan.steps[1]
            .metadata
            .as_ref()
            .and_then(|value| value.get("builder"))
            .and_then(Value::as_object)
            .expect("draft builder");
        assert_eq!(
            draft_builder
                .get("input_files")
                .and_then(Value::as_array)
                .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
            Some(vec![".tandem/artifacts/collect-inputs.json"])
        );
        assert_eq!(
            draft_builder
                .get("output_files")
                .and_then(Value::as_array)
                .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
            Some(vec!["reports/final.md"])
        );
    }

    #[test]
    fn derive_workflow_step_file_contracts_preserves_explicit_contract_overrides() {
        let mut plan = test_plan_with_steps(vec![
            WorkflowPlanStep {
                step_id: "collect_inputs".to_string(),
                kind: "collect".to_string(),
                objective: "Collect inputs.".to_string(),
                depends_on: vec![],
                agent_role: "planner".to_string(),
                input_refs: vec![],
                output_contract: Some(json!({"kind":"structured_json"})),
                metadata: Some(json!({
                    "builder": {
                        "output_path": ".tandem/artifacts/collect-inputs.json",
                        "output_files": ["custom/inputs.json"]
                    }
                })),
            },
            WorkflowPlanStep {
                step_id: "draft_report".to_string(),
                kind: "write".to_string(),
                objective: "Draft the report.".to_string(),
                depends_on: vec!["collect_inputs".to_string()],
                agent_role: "writer".to_string(),
                input_refs: vec![json!({
                    "from_step_id": "collect_inputs",
                    "alias": "inputs"
                })],
                output_contract: Some(json!({"kind":"report_markdown"})),
                metadata: Some(json!({
                    "builder": {
                        "input_files": ["docs/brief.md"]
                    }
                })),
            },
        ]);

        derive_workflow_step_file_contracts(&mut plan);

        let collect_builder = plan.steps[0]
            .metadata
            .as_ref()
            .and_then(|value| value.get("builder"))
            .and_then(Value::as_object)
            .expect("collect builder");
        assert_eq!(
            collect_builder
                .get("output_files")
                .and_then(Value::as_array)
                .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
            Some(vec!["custom/inputs.json"])
        );
        let draft_builder = plan.steps[1]
            .metadata
            .as_ref()
            .and_then(|value| value.get("builder"))
            .and_then(Value::as_object)
            .expect("draft builder");
        assert_eq!(
            draft_builder
                .get("input_files")
                .and_then(Value::as_array)
                .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
            Some(vec!["docs/brief.md"])
        );
    }

    #[test]
    fn validate_workflow_plan_accepts_supported_step_id_suffix_variants() {
        let plan = test_plan_with_steps(vec![
            WorkflowPlanStep {
                step_id: "research_sources_web".to_string(),
                kind: "research".to_string(),
                objective: "Research sources from the web.".to_string(),
                depends_on: vec![],
                agent_role: "researcher".to_string(),
                input_refs: vec![],
                output_contract: Some(json!({"kind":"structured_json"})),
                metadata: None,
            },
            WorkflowPlanStep {
                step_id: "analyze_findings".to_string(),
                kind: "analysis".to_string(),
                objective: "Analyze findings.".to_string(),
                depends_on: vec!["research_sources_web".to_string()],
                agent_role: "analyst".to_string(),
                input_refs: vec![json!({
                    "from_step_id": "research_sources_web",
                    "alias": "source_data"
                })],
                output_contract: Some(json!({"kind":"structured_json"})),
                metadata: None,
            },
        ]);

        validate_workflow_plan(&plan).expect("step-id suffix variants should be accepted");
    }

    #[test]
    fn validate_workflow_plan_rejects_malformed_step_ids() {
        let plan = test_plan_with_steps(vec![WorkflowPlanStep {
            step_id: "123 totally custom step".to_string(),
            kind: "custom".to_string(),
            objective: "Do custom work.".to_string(),
            depends_on: vec![],
            agent_role: "worker".to_string(),
            input_refs: vec![],
            output_contract: Some(json!({"kind":"structured_json"})),
            metadata: None,
        }]);

        let error = validate_workflow_plan(&plan).expect_err("malformed step id should fail");
        assert!(error.contains("invalid workflow step id"));
    }

    #[test]
    fn infer_explicit_output_targets_extracts_path_like_workspace_targets() {
        let prompt = "Generate and save /home/user/marketing-tandem/YOUTUBE_TANDEM_MARKETING_RESEARCH_AND_SCRIPTS.md and also summarize the findings.";

        let targets = infer_explicit_output_targets(prompt);

        assert_eq!(
            targets,
            vec![
                "/home/user/marketing-tandem/YOUTUBE_TANDEM_MARKETING_RESEARCH_AND_SCRIPTS.md"
                    .to_string()
            ]
        );
    }

    #[test]
    fn infer_explicit_output_targets_extracts_bare_filenames_from_write_clauses() {
        let prompt = "Read RESUME.md as the source of truth for skills. If resume_overview.md does not exist, create it. Create or append to daily_results_2026-04-15.md in the workspace root and keep the source-of-truth file untouched.";

        let targets = infer_explicit_output_targets(prompt);

        assert_eq!(
            targets,
            vec![
                "daily_results_2026-04-15.md".to_string(),
                "resume_overview.md".to_string()
            ]
        );
    }

    #[test]
    fn infer_explicit_output_targets_extracts_filenames_from_adjacent_write_lines() {
        let prompt = "Create or append to this daily file in the workspace root:\n\n`job_search_results_YYYY-MM-DD.md`\n\nReplace `YYYY-MM-DD` with the actual resolved date for the run.";

        let targets = infer_explicit_output_targets(prompt);

        assert_eq!(
            targets,
            vec!["job_search_results_YYYY-MM-DD.md".to_string()]
        );
    }

    #[test]
    fn infer_explicit_output_targets_skips_read_only_source_of_truth_files() {
        let prompt = "Analyze RESUME.md as the source of truth, then create resume_overview.md and save daily_results_2026-04-15.md.";

        let targets = infer_explicit_output_targets(prompt);

        assert!(!targets.iter().any(|path| path == "RESUME.md"));
        assert!(targets.iter().any(|path| path == "resume_overview.md"));
        assert!(targets
            .iter()
            .any(|path| path == "daily_results_2026-04-15.md"));
    }

    #[test]
    fn infer_read_only_source_paths_extracts_source_of_truth_files() {
        let prompt = "Analyze RESUME.md as the source of truth for skills, role targets, seniority, technologies, and geography preferences. Never edit, rewrite, rename, move, or delete RESUME.md.";

        let sources = infer_read_only_source_paths(prompt);

        assert_eq!(sources, vec!["RESUME.md".to_string()]);
    }

    #[test]
    fn infer_explicit_output_targets_ignores_urls_and_deduplicates_targets() {
        let prompt =
            "Write to https://example.com/report.md, ./notes/final.md, and ./notes/final.md again.";

        let targets = infer_explicit_output_targets(prompt);

        assert_eq!(targets, vec!["./notes/final.md".to_string()]);
    }

    #[test]
    fn workflow_plan_should_surface_mcp_discovery_for_connector_backed_sources() {
        assert!(workflow_plan_should_surface_mcp_discovery(
            "Research Reddit threads about AI assistants.",
            &[]
        ));
        assert!(workflow_plan_should_surface_mcp_discovery(
            "Write the workflow plan.",
            &["github".to_string()]
        ));
        assert!(!workflow_plan_should_surface_mcp_discovery(
            "Summarize the local workspace docs.",
            &[]
        ));
    }

    #[test]
    fn workflow_plan_mentions_web_research_tools_for_explicit_web_search_prompts() {
        assert!(workflow_plan_mentions_web_research_tools(
            "Use websearch to find relevant job boards and use webfetch when needed."
        ));
        assert!(!workflow_plan_mentions_web_research_tools(
            "Summarize the local workspace docs."
        ));
    }

    #[test]
    fn workflow_plan_mentions_email_delivery_only_for_explicit_email_workflows() {
        assert!(workflow_plan_mentions_email_delivery(
            "Use email to send the final report."
        ));
        assert!(workflow_plan_mentions_email_delivery(
            "Draft an email update and send it to the team."
        ));
        assert!(!workflow_plan_mentions_email_delivery(
            "Create or append to a daily results file."
        ));
        assert!(!workflow_plan_mentions_email_delivery(
            "Publish the report to a markdown file."
        ));
    }
}
