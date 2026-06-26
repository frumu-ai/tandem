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
    fn planner_capability_summary_exposes_notion_connector_writer_contract() {
        let summary = build_planner_capability_summary(&[PlannerMcpServerToolSet {
            server: "notion".to_string(),
            tool_names: vec![
                "mcp.notion.notion_fetch".to_string(),
                "mcp.notion.notion_search".to_string(),
                "mcp.notion.notion_create_pages".to_string(),
                "mcp.notion.notion_update_page".to_string(),
            ],
        }]);
        let contracts = summary
            .get("connector_writer_contracts")
            .and_then(Value::as_array)
            .expect("connector writer contracts");
        assert_eq!(contracts.len(), 1);
        assert_eq!(contracts[0].get("connector").and_then(Value::as_str), Some("notion"));
        assert_eq!(
            contracts[0].get("writer_kind").and_then(Value::as_str),
            Some("database_rows")
        );
        assert!(contracts[0]
            .get("required_metadata")
            .and_then(Value::as_array)
            .is_some_and(|items| items
                .iter()
                .any(|item| item.as_str() == Some("property_mappings"))));
    }

    #[test]
    fn planner_capability_summary_requires_search_for_notion_connector_writer_contract() {
        let summary = build_planner_capability_summary(&[PlannerMcpServerToolSet {
            server: "notion".to_string(),
            tool_names: vec![
                "mcp.notion.notion_fetch".to_string(),
                "mcp.notion.notion_create_pages".to_string(),
                "mcp.notion.notion_update_page".to_string(),
            ],
        }]);
        let contracts = summary
            .get("connector_writer_contracts")
            .and_then(Value::as_array)
            .expect("connector writer contracts");

        assert!(
            contracts.is_empty(),
            "do not advertise the writer contract unless duplicate-search tooling is available"
        );
    }

    #[test]
    fn optional_web_context_does_not_expect_required_web_research() {
        let objective = "Use web research and web_fetch only when useful to add supporting context for tools, market references, or claims that emerged from collect_reddit_signals. Do not replace Reddit as the primary evidence source. Return concise citations with URLs; if no web context is needed, return an empty citations list with rationale.";

        assert!(workflow_step_allows_optional_web_research(objective));
        assert!(
            !workflow_step_expects_web_research("gather_supporting_context", "research", objective,),
            "optional web context must not become a required web-research step"
        );

        let metadata = workflow_step_metadata_defaults(
            "gather_supporting_context",
            "research",
            objective,
            true,
        )
        .expect("metadata");
        assert_eq!(
            metadata
                .pointer("/builder/web_research_expected")
                .and_then(Value::as_bool),
            Some(false)
        );

        let enforcement = workflow_step_enforcement_defaults(
            "gather_supporting_context",
            "research",
            objective,
            true,
        )
        .expect("enforcement");
        assert!(
            !enforcement
                .get("required_tools")
                .and_then(Value::as_array)
                .is_some_and(|tools| tools.iter().any(|tool| tool.as_str() == Some("websearch"))),
            "optional web context must not install required websearch enforcement"
        );
    }

    #[test]
    fn connector_source_collection_defaults_enable_connector_capture() {
        let source_metadata = workflow_step_metadata_defaults(
            "search_reddit_threads",
            "research",
            "Use Reddit MCP to search across subreddits, collect candidate threads, and return source counts for downstream filtering.",
            false,
        )
        .expect("metadata");

        assert_eq!(
            source_metadata
                .pointer("/connector_capture/enabled")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            source_metadata
                .pointer("/builder/connector_capture/enabled")
                .and_then(Value::as_bool),
            Some(true)
        );

        let writer_metadata = workflow_step_metadata_defaults(
            "write_notion_rows",
            "action",
            "Save filtered Reddit leads to the Notion database and update duplicate rows.",
            false,
        )
        .expect("metadata");
        assert!(
            writer_metadata.get("connector_capture").is_none(),
            "destination writer steps should not auto-enable source-result capture"
        );
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
    fn generated_research_destination_plan_compacts_to_request_aware_macro_steps() {
        let prompt = r#"research this topic:

"What are the current approaches to making AI agents reliable for business workflows?"

Use the connected Tandem MCP docs as reference if needed, and use the connected Reddit MCP plus web research to gather current market signals, discussions, examples, and source links.

Then create a concise market brief and save the completed report into the Notion database:

Operational Workflow Results
collection://892d3e9b-2bf8-4b3e-a541-dc725f77295d

The Notion page should include:
- Summary
- Key Findings
- Market Notes
- Reddit Signals
- Sources
- Tandem Run details"#;
        let profile = crate::decomposition::derive_workflow_decomposition_profile(
            prompt,
            &[
                "tandem_mcp".to_string(),
                "reddit".to_string(),
                "notion".to_string(),
            ],
            &[],
            false,
        );
        let mut original_steps = Vec::new();
        let objectives = [
            "Define scope, success criteria, and report requirements.",
            "Use mcp.tandem_mcp.search_docs for reliable workflow design docs.",
            "Use mcp.tandem_mcp.get_doc for selected Tandem docs.",
            "Use web_research and web_fetch for current market approaches.",
            "Collect vendor and enterprise examples with web source links.",
            "Collect observability, guardrails, evals, retries, and fallback practices.",
            "Use mcp.composio.reddit_get_subreddits_search for Reddit signals.",
            "Use mcp.composio.reddit_search_across_subreddits for candidate posts.",
            "Use mcp.composio.reddit_retrieve_reddit_post for discussion excerpts.",
            "Extract practitioner Reddit concerns and reliability tactics.",
            "Normalize sources into one ledger.",
            "Synthesize a taxonomy of reliable AI agent workflow approaches.",
            "Draft Summary section.",
            "Draft Key Findings section.",
            "Draft Market Notes section.",
            "Draft Reddit Signals section.",
            "Draft Sources section.",
            "Draft Tandem Run details section.",
            "Assemble concise market brief.",
            "Validate the brief is current, concise, and section-complete.",
            "Transform brief into Notion page payload.",
            "Create Notion page in collection://892d3e9b-2bf8-4b3e-a541-dc725f77295d.",
            "Verify Notion page has Summary.",
            "Verify Notion page has Key Findings.",
            "Verify Notion page has Market Notes.",
            "Verify Notion page has Reddit Signals.",
            "Verify Notion page has Sources.",
            "Verify Notion page has Tandem Run details.",
            "Capture final Notion page URL and run details.",
        ];
        for (index, objective) in objectives.iter().enumerate() {
            original_steps.push(WorkflowPlanStep {
                step_id: format!("generated_step_{:02}", index + 1),
                kind: if objective.contains("Draft")
                    || objective.contains("Assemble")
                    || objective.contains("Synthesize")
                {
                    "synthesize".to_string()
                } else if objective.contains("Notion") || objective.contains("collection://") {
                    "deliver".to_string()
                } else {
                    "research".to_string()
                },
                objective: objective.to_string(),
                depends_on: if index == 0 {
                    Vec::new()
                } else {
                    vec![format!("generated_step_{index:02}")]
                },
                agent_role: "agent_planner".to_string(),
                input_refs: Vec::new(),
                output_contract: Some(json!({"kind":"structured_json"})),
                metadata: None,
            });
        }
        original_steps[11].input_refs = vec![json!({
            "from_step_id": "generated_step_04",
            "alias": "market_sources"
        })];
        let mut plan = test_plan_with_steps(original_steps);
        plan.original_prompt = prompt.to_string();
        plan.normalized_prompt = normalize_prompt(prompt);
        plan.allowed_mcp_servers = vec![
            "tandem_mcp".to_string(),
            "reddit".to_string(),
            "notion".to_string(),
        ];

        let (compacted, report) = compact_generated_workflow_plan_to_budget(plan, &profile);

        assert!(compacted.steps.len() <= GENERATED_WORKFLOW_MAX_STEPS);
        assert_eq!(
            report
                .as_ref()
                .and_then(|value| value.get("status"))
                .and_then(Value::as_str),
            Some("compacted")
        );
        assert_eq!(
            report
                .as_ref()
                .and_then(|value| value.get("original_step_count"))
                .and_then(Value::as_u64),
            Some(29)
        );
        let step_ids = compacted
            .steps
            .iter()
            .map(|step| step.step_id.as_str())
            .collect::<Vec<_>>();
        assert!(step_ids.contains(&"confirm_scope_and_destination"));
        assert!(step_ids.contains(&"gather_tandem_docs"));
        assert!(step_ids.contains(&"gather_market_sources"));
        assert!(step_ids.contains(&"gather_reddit_signals"));
        assert!(step_ids.contains(&"draft_market_brief"));
        assert!(step_ids.contains(&"create_and_verify_notion_page"));
        let all_objectives = compacted
            .steps
            .iter()
            .map(|step| step.objective.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all_objectives.contains("mcp.tandem_mcp.search_docs"));
        assert!(all_objectives.contains("web_research"));
        assert!(all_objectives.contains("mcp.composio.reddit"));
        let draft_step = compacted
            .steps
            .iter()
            .find(|step| step.step_id == "draft_market_brief")
            .expect("draft macro step");
        assert!(draft_step
            .depends_on
            .iter()
            .any(|dependency| dependency == "gather_market_sources"));
        assert!(draft_step.input_refs.iter().any(|input_ref| {
            input_ref
                .get("from_step_id")
                .and_then(Value::as_str)
                == Some("gather_market_sources")
                && input_ref.get("alias").and_then(Value::as_str) == Some("market_sources")
        }));
        assert!(all_objectives.contains("collection://892d3e9b-2bf8-4b3e-a541-dc725f77295d"));
        assert!(all_objectives.contains("Summary"));
        assert!(all_objectives.contains("Key Findings"));
        assert!(all_objectives.contains("Market Notes"));
        assert!(all_objectives.contains("Reddit Signals"));
        assert!(all_objectives.contains("Sources"));
        assert!(all_objectives.contains("Tandem Run details"));
    }

    #[test]
    fn manual_or_imported_plans_are_exempt_from_generated_task_budget() {
        let steps = (0..12)
            .map(|index| WorkflowPlanStep {
                step_id: format!("manual_step_{index}"),
                kind: "manual".to_string(),
                objective: "Human-authored workflow step.".to_string(),
                depends_on: Vec::new(),
                agent_role: "operator".to_string(),
                input_refs: Vec::new(),
                output_contract: None,
                metadata: None,
            })
            .collect::<Vec<_>>();
        let mut plan = test_plan_with_steps(steps);
        plan.plan_source = "workflow_studio_manual".to_string();

        assert!(!workflow_plan_generated_task_budget_exceeded(&plan));
        assert_eq!(
            workflow_task_budget_report_for_plan(&plan, None, None, None)
                .get("status")
                .and_then(Value::as_str),
            Some("exempt_manual")
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
    fn validate_workflow_plan_rejects_depends_on_cycles() {
        let plan = test_plan_with_steps(vec![
            WorkflowPlanStep {
                step_id: "collect".to_string(),
                kind: "research".to_string(),
                objective: "Collect inputs.".to_string(),
                depends_on: vec!["summarize".to_string()],
                agent_role: "researcher".to_string(),
                input_refs: vec![],
                output_contract: Some(json!({"kind":"structured_json"})),
                metadata: None,
            },
            WorkflowPlanStep {
                step_id: "summarize".to_string(),
                kind: "synthesis".to_string(),
                objective: "Summarize inputs.".to_string(),
                depends_on: vec!["collect".to_string()],
                agent_role: "writer".to_string(),
                input_refs: vec![],
                output_contract: Some(json!({"kind":"structured_json"})),
                metadata: None,
            },
        ]);

        let error = validate_workflow_plan(&plan).expect_err("cycle should fail validation");
        assert!(error.contains("dependency cycle"), "{error}");
        assert!(error.contains("collect"), "{error}");
        assert!(error.contains("summarize"), "{error}");
    }

    #[test]
    fn validate_workflow_plan_rejects_input_refs_missing_from_depends_on() {
        let plan = test_plan_with_steps(vec![
            WorkflowPlanStep {
                step_id: "collect".to_string(),
                kind: "research".to_string(),
                objective: "Collect inputs.".to_string(),
                depends_on: vec![],
                agent_role: "researcher".to_string(),
                input_refs: vec![],
                output_contract: Some(json!({"kind":"structured_json"})),
                metadata: None,
            },
            WorkflowPlanStep {
                step_id: "summarize".to_string(),
                kind: "synthesis".to_string(),
                objective: "Summarize inputs.".to_string(),
                depends_on: vec![],
                agent_role: "writer".to_string(),
                input_refs: vec![json!({
                    "from_step_id": "collect",
                    "alias": "source_data"
                })],
                output_contract: Some(json!({"kind":"structured_json"})),
                metadata: None,
            },
        ]);

        let error = validate_workflow_plan(&plan)
            .expect_err("input_ref without depends_on should fail validation");
        assert!(error.contains("must also be listed in depends_on"), "{error}");
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
    fn workflow_step_allows_optional_connector_references_for_tandem_docs() {
        let objective = "Use Tandem MCP docs as reference if needed via mcp.tandem_mcp.search_docs, mcp.tandem_mcp.get_doc, mcp.tandem_mcp.get_tandem_guide, or mcp.tandem_mcp.answer_how_to to collect relevant Tandem guidance. Return only relevant excerpts and citations.";

        assert!(workflow_step_allows_optional_connector_references(
            objective
        ));
        assert!(workflow_plan_should_surface_mcp_discovery(objective, &[]));
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
