
#[test]
fn research_evidence_validation_matrix_covers_local_web_mixed_and_mcp_grounding() {
    let local_brief = "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources.\n\n## Campaign goal\nClarify positioning.\n\n## Target audience\n- Operators.\n\n## Core pain points\n- Coordination overhead.\n\n## Positioning angle\nTandem centralizes orchestration.\n\n## Competitor context\nLocal-only comparison for this run.\n\n## Proof points with citations\n1. Supported from docs/source.md. Source note: https://example.com/reference\n\n## Likely objections\n- Proof depth.\n\n## Channel considerations\n- Landing page.\n\n## Recommended message hierarchy\n1. Problem\n2. Promise\n\n## Files reviewed\n- docs/source.md\n\n## Files not reviewed\n- docs/extra.md: not needed for this first pass.\n";
    let mixed_brief = "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources and current external research.\n\n## Campaign goal\nClarify positioning.\n\n## Target audience\n- Operators.\n\n## Core pain points\n- Coordination overhead.\n\n## Positioning angle\nTandem centralizes orchestration.\n\n## Competitor context\nExternal validation confirmed the same positioning pressure points.\n\n## Proof points with citations\n1. Supported from docs/source.md. Source note: https://example.com/reference\n2. Supported by current market coverage. Source note: https://example.com/current-market\n\n## Likely objections\n- Proof depth.\n\n## Channel considerations\n- Landing page.\n\n## Recommended message hierarchy\n1. Problem\n2. Promise\n\n## Files reviewed\n- docs/source.md\n\n## Files not reviewed\n- docs/extra.md: not needed for this pass.\n\n## Web sources reviewed\n- https://example.com/current-market\n";
    let web_citations = "# Research Sources\n\n## Summary\nCurrent external research was gathered successfully.\n\n## Citations\n1. AI Agents in 2025: Expectations vs. Reality | IBM. Source note: https://www.ibm.com/think/insights/ai-agents-2025-expectations-vs-reality\n2. Agentic AI, explained | MIT Sloan. Source note: https://mitsloan.mit.edu/ideas-made-to-matter/agentic-ai-explained\n\n## Web sources reviewed\n- https://www.ibm.com/think/insights/ai-agents-2025-expectations-vs-reality\n- https://mitsloan.mit.edu/ideas-made-to-matter/agentic-ai-explained\n";
    let mcp_citations = "# Research Sources\n\n## Summary\nCollected current Tandem MCP documentation references.\n\n## Citations\n1. Tandem MCP Guide. Source note: tandem-mcp://docs/guide\n2. Tandem MCP API Reference. Source note: tandem-mcp://docs/api-reference\n";
    let cases = vec![
        ResearchEvidenceMatrixCase {
            name: "local-only",
            node: research_brief_matrix_node("marketing-brief.md", true),
            workspace_files: vec![("docs/source.md", "source")],
            tool_invocations: vec![
                ToolInvocationSpec {
                    tool: "read",
                    args: json!({"path":"docs/source.md"}),
                    result: json!({"output":"source"}),
                },
                ToolInvocationSpec {
                    tool: "write",
                    args: json!({"path":"marketing-brief.md","content":local_brief}),
                    result: json!({"ok": true}),
                },
            ],
            requested_tools: vec!["glob", "read", "write"],
            accepted_output_path: "marketing-brief.md",
            accepted_output_content: local_brief,
            session_text: "Done\n\n{\"status\":\"completed\"}",
            expected_validation_outcome: "passed",
            expected_external_research_mode: Some("waived_unavailable"),
            absent_unmet: vec!["no_concrete_reads", "missing_successful_web_research"],
            expected_read_paths: vec!["docs/source.md"],
        },
        ResearchEvidenceMatrixCase {
            name: "web-grounded",
            node: research_citations_matrix_node(
                "research_sources",
                ".tandem/artifacts/research-sources.json",
                true,
                &[],
            ),
            workspace_files: vec![("inputs/questions.md", "Question")],
            tool_invocations: vec![
                ToolInvocationSpec {
                    tool: "read",
                    args: json!({"path":"inputs/questions.md"}),
                    result: json!({"output":"Question"}),
                },
                ToolInvocationSpec {
                    tool: "websearch",
                    args: json!({"query":"autonomous AI agentic workflows 2024 2025"}),
                    result: json!({"output":"Search results found"}),
                },
                ToolInvocationSpec {
                    tool: "write",
                    args: json!({"path":".tandem/artifacts/research-sources.json","content":web_citations}),
                    result: json!({"output":"written"}),
                },
            ],
            requested_tools: vec!["read", "write", "websearch"],
            accepted_output_path: ".tandem/artifacts/research-sources.json",
            accepted_output_content: web_citations,
            session_text: "",
            expected_validation_outcome: "passed",
            expected_external_research_mode: None,
            absent_unmet: vec![
                "no_concrete_reads",
                "missing_successful_web_research",
                "files_reviewed_missing",
                "files_reviewed_not_backed_by_read",
            ],
            expected_read_paths: vec!["inputs/questions.md"],
        },
        ResearchEvidenceMatrixCase {
            name: "mixed-local-web",
            node: research_brief_matrix_node("marketing-brief.md", true),
            workspace_files: vec![("docs/source.md", "source")],
            tool_invocations: vec![
                ToolInvocationSpec {
                    tool: "read",
                    args: json!({"path":"docs/source.md"}),
                    result: json!({"output":"source"}),
                },
                ToolInvocationSpec {
                    tool: "websearch",
                    args: json!({"query":"workflow contract testing release safety"}),
                    result: json!({"output":"Search results found"}),
                },
                ToolInvocationSpec {
                    tool: "write",
                    args: json!({"path":"marketing-brief.md","content":mixed_brief}),
                    result: json!({"ok": true}),
                },
            ],
            requested_tools: vec!["glob", "read", "write", "websearch"],
            accepted_output_path: "marketing-brief.md",
            accepted_output_content: mixed_brief,
            session_text: "Done\n\n{\"status\":\"completed\"}",
            expected_validation_outcome: "passed",
            expected_external_research_mode: None,
            absent_unmet: vec!["no_concrete_reads", "missing_successful_web_research"],
            expected_read_paths: vec!["docs/source.md"],
        },
        ResearchEvidenceMatrixCase {
            name: "mcp-grounded",
            node: research_citations_matrix_node(
                "research_sources",
                ".tandem/runs/run-mcp-citations/artifacts/research-sources.json",
                false,
                &["tandem-mcp"],
            ),
            workspace_files: vec![],
            tool_invocations: vec![
                ToolInvocationSpec {
                    tool: "mcp.tandem_mcp.search_docs",
                    args: json!({"query":"research sources artifact contract"}),
                    result: json!({"output":"Matched Tandem MCP docs"}),
                },
                ToolInvocationSpec {
                    tool: "write",
                    args: json!({"path":".tandem/runs/run-mcp-citations/artifacts/research-sources.json","content":mcp_citations}),
                    result: json!({"output":"written"}),
                },
            ],
            requested_tools: vec!["mcp.tandem_mcp.search_docs", "write"],
            accepted_output_path: ".tandem/runs/run-mcp-citations/artifacts/research-sources.json",
            accepted_output_content: mcp_citations,
            session_text: "Done\n\n{\"status\":\"completed\"}",
            expected_validation_outcome: "passed",
            expected_external_research_mode: None,
            absent_unmet: vec![
                "current_attempt_output_missing",
                "no_concrete_reads",
                "missing_successful_web_research",
            ],
            expected_read_paths: vec![],
        },
    ];

    for case in cases {
        run_research_evidence_matrix_case(case);
    }
}

#[test]
fn research_retry_state_matrix_covers_repairable_and_exhausted_statuses() {
    let cases = vec![
        RepairStateMatrixCase {
            name: "repairable-completed-wrapper",
            session_text: "Done — `marketing-brief.md` was written.",
            repair_exhausted: false,
            expected_status: "needs_repair",
            expected_reason:
                "research completed without concrete file reads or required source coverage",
            expected_failure_kind: "research_missing_reads",
            expected_summary_outcome: "needs_repair",
        },
        RepairStateMatrixCase {
            name: "repair-exhausted-completed-wrapper",
            session_text: "Done — `marketing-brief.md` was written.",
            repair_exhausted: true,
            expected_status: "blocked",
            expected_reason:
                "research completed without concrete file reads or required source coverage",
            expected_failure_kind: "research_retry_exhausted",
            expected_summary_outcome: "blocked",
        },
        RepairStateMatrixCase {
            name: "repairable-overrides-llm-blocked",
            session_text:
                "The brief is blocked.\n\n{\"status\":\"blocked\",\"reason\":\"tools unavailable\"}",
            repair_exhausted: false,
            expected_status: "needs_repair",
            expected_reason:
                "research completed without concrete file reads or required source coverage",
            expected_failure_kind: "research_missing_reads",
            expected_summary_outcome: "needs_repair",
        },
        RepairStateMatrixCase {
            name: "repair-exhausted-keeps-llm-blocked",
            session_text:
                "The brief is blocked.\n\n{\"status\":\"blocked\",\"reason\":\"tools unavailable\"}",
            repair_exhausted: true,
            expected_status: "blocked",
            expected_reason: "tools unavailable",
            expected_failure_kind: "research_retry_exhausted",
            expected_summary_outcome: "blocked",
        },
    ];

    for case in cases {
        run_repair_state_matrix_case(case);
    }
}

#[test]
fn upstream_synthesis_validation_matrix_covers_markdown_and_html_evidence_preservation() {
    let thin_report = "# Strategic Summary\n\nTandem is an engineering agent for local execution.\n\n## Positioning\n\nIt connects human intent to repo changes.\n";
    let structured_report = "# Strategy Analysis Report\n\n## 1. Executive Summary\nThis analysis synthesizes Tandem's internal product definitions and external research to refine positioning. Tandem is positioned as a high-autonomy engineering engine rather than a generic code assistant.\n\n## 2. Product Positioning\n* **Core Identity:** Tandem by Frumu AI\n* **Key Positioning:** Workspace-aware AI collaboration embedded in the engineering workflow.\n\n## 3. Risks & Proof Gaps\n* Need stronger empirical time-saved metrics.\n\n---\nSource verification: based on `.tandem/artifacts/collect-inputs.json` and `.tandem/artifacts/research-sources.json`.\n";
    let generic_html_report = r#"
<html>
  <body>
    <h1>Strategic Summary</h1>
    <p>This report synthesizes the available upstream evidence into a concise outlook.</p>
    <p>Strategic positioning remains promising.</p>
  </body>
</html>
"#
    .trim();
    let anchored_html_report = r#"
<html>
  <body>
    <h1>Frumu AI Tandem: Strategic Summary</h1>
    <p>We synthesized the local Tandem docs and the external research into one report.</p>
    <h3>Core Value Proposition</h3>
    <p>Tandem is an engine-backed workflow system for local execution and agentic operations.</p>
    <ul>
      <li>Local workspace reads and patch-based code execution.</li>
      <li>Current web research for externally grounded synthesis.</li>
      <li>Explicit delivery gating for side effects.</li>
    </ul>
    <h3>Strategic Outlook</h3>
    <p>The positioning emphasizes deterministic execution, provenance, and operator control.</p>
    <p>Sources reviewed: <a href=".tandem/runs/run-123/artifacts/analyze-findings.md">analysis</a> and <a href=".tandem/runs/run-123/artifacts/research-sources.json">research</a>.</p>
  </body>
</html>
"#
    .trim();
    let single_anchor_markdown_report = "# Final Synthesis Report\n\n## Executive Summary\nThis report is grounded in the local workflow evidence and summarizes the strongest matches from the run. It is meant to be substantive enough for release review and rerun planning without collapsing into vague workflow commentary.\n\n## Resume Direction\nThe `resume_overview.md` handoff keeps the search aligned with senior Rust, automation, and Europe-friendly roles. That source should continue to shape the search keywords and the shortlist criteria for future runs.\n\n## Observed Patterns\nThe current run still favors direct company postings and focused boards over broad aggregators. Keeping the report concise is useful, but the evidence should stay specific enough to preserve operator trust.\n\n## Recommendation\nContinue the same search pattern tomorrow and tighten the filters further around Rust, workflow automation, and product-facing systems work so the daily review stays high-signal.\n";
    let rich_upstream = AutomationUpstreamEvidence {
        read_paths: vec![
            ".tandem/artifacts/collect-inputs.json".to_string(),
            ".tandem/artifacts/research-sources.json".to_string(),
            ".tandem/artifacts/analyze-findings.md".to_string(),
        ],
        discovered_relevant_paths: vec![
            ".tandem/artifacts/collect-inputs.json".to_string(),
            ".tandem/artifacts/research-sources.json".to_string(),
            ".tandem/artifacts/analyze-findings.md".to_string(),
        ],
        web_research_attempted: true,
        web_research_succeeded: true,
        citation_count: 3,
        citations: vec![
            "https://example.com/1".to_string(),
            "https://example.com/2".to_string(),
            "https://example.com/3".to_string(),
        ],
    };
    let cases = vec![
        UpstreamSynthesisMatrixCase {
            name: "markdown-generic-summary-blocked",
            node_id: "generate_report",
            output_path: "report.md",
            artifact_text: thin_report,
            session_text: "Completed the report.",
            write_path: "report.md",
            tool_telemetry: json!({
                "requested_tools": ["write"],
                "executed_tools": ["write"],
                "tool_call_counts": {
                    "write": 1
                }
            }),
            upstream_evidence: rich_upstream.clone(),
            expected_validation_outcome: "blocked",
            expected_rejected: Some(
                "final artifact does not adequately synthesize the available upstream evidence",
            ),
            expect_upstream_unsynthesized: true,
        },
        UpstreamSynthesisMatrixCase {
            name: "markdown-structured-synthesis-passes",
            node_id: "analyze_findings",
            output_path: "analyze-findings.md",
            artifact_text: structured_report,
            session_text: "Completed the report.",
            write_path: "analyze-findings.md",
            tool_telemetry: json!({
                "requested_tools": ["read", "write"],
                "executed_tools": ["read", "write"],
                "tool_call_counts": {
                    "read": 2,
                    "write": 1
                }
            }),
            upstream_evidence: AutomationUpstreamEvidence {
                read_paths: vec![
                    ".tandem/artifacts/collect-inputs.json".to_string(),
                    ".tandem/artifacts/research-sources.json".to_string(),
                ],
                discovered_relevant_paths: vec![
                    ".tandem/artifacts/collect-inputs.json".to_string(),
                    "README.md".to_string(),
                ],
                web_research_attempted: true,
                web_research_succeeded: true,
                citation_count: 3,
                citations: vec![
                    "https://example.com/1".to_string(),
                    "https://example.com/2".to_string(),
                    "https://example.com/3".to_string(),
                ],
            },
            expected_validation_outcome: "accepted_with_warnings",
            expected_rejected: None,
            expect_upstream_unsynthesized: false,
        },
        UpstreamSynthesisMatrixCase {
            name: "markdown-single-anchor-blocked-when-two-are-required",
            node_id: "generate_report",
            output_path: "generate-report.md",
            artifact_text: single_anchor_markdown_report,
            session_text: "Completed the report.",
            write_path: "generate-report.md",
            tool_telemetry: json!({
                "requested_tools": ["read", "write"],
                "executed_tools": ["read", "write"],
                "tool_call_counts": {
                    "read": 2,
                    "write": 1
                }
            }),
            upstream_evidence: AutomationUpstreamEvidence {
                read_paths: vec![
                    "resume_overview.md".to_string(),
                    "job_search_results_2026-04-15.md".to_string(),
                ],
                discovered_relevant_paths: vec![
                    "resume_overview.md".to_string(),
                    "job_search_results_2026-04-15.md".to_string(),
                ],
                web_research_attempted: false,
                web_research_succeeded: false,
                citation_count: 0,
                citations: Vec::new(),
            },
            expected_validation_outcome: "blocked",
            expected_rejected: Some(
                "final artifact does not adequately synthesize the available upstream evidence",
            ),
            expect_upstream_unsynthesized: true,
        },
        UpstreamSynthesisMatrixCase {
            name: "html-generic-summary-blocked",
            node_id: "generate_report",
            output_path: "generate-report.md",
            artifact_text: generic_html_report,
            session_text: "Completed the report.",
            write_path: "generate-report.md",
            tool_telemetry: json!({
                "requested_tools": ["write"],
                "executed_tools": ["write"],
                "tool_call_counts": {
                    "write": 1
                }
            }),
            upstream_evidence: rich_upstream.clone(),
            expected_validation_outcome: "blocked",
            expected_rejected: Some(
                "final artifact does not adequately synthesize the available upstream evidence",
            ),
            expect_upstream_unsynthesized: true,
        },
        UpstreamSynthesisMatrixCase {
            name: "html-anchored-synthesis-passes",
            node_id: "generate_report",
            output_path: "generate-report.md",
            artifact_text: anchored_html_report,
            session_text: "Completed the report.",
            write_path: "generate-report.md",
            tool_telemetry: json!({
                "requested_tools": ["write"],
                "executed_tools": ["write"],
                "tool_call_counts": {
                    "write": 1
                }
            }),
            upstream_evidence: rich_upstream,
            expected_validation_outcome: "passed",
            expected_rejected: None,
            expect_upstream_unsynthesized: false,
        },
    ];

    for case in cases {
        run_upstream_synthesis_matrix_case(case);
    }
}

#[test]
fn structured_json_node_passes_when_declared_workspace_files_are_written() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-must-write-present-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "extract_pain_points".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Write synthesis".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "extract.json",
                "must_write_files": ["02_reddit_pain_points.md"]
            }
        })),
    };
    let artifact_text =
        "{\"status\":\"completed\",\"summary\":\"Pain point synthesis completed.\"}".to_string();
    let markdown_text = "# Reddit pain points\n\n- Brittle automations.\n".to_string();
    std::fs::write(workspace_root.join("extract.json"), &artifact_text).expect("write artifact");
    std::fs::write(
        workspace_root.join("02_reddit_pain_points.md"),
        &markdown_text,
    )
    .expect("write markdown");
    let mut session = Session::new(
        Some("must write files".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"extract.json","content":artifact_text}),
                result: Some(json!({"ok": true})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"02_reddit_pain_points.md","content":markdown_text}),
                result: Some(json!({"ok": true})),
                error: None,
            },
        ],
    ));
    let tool_telemetry =
        summarize_automation_tool_activity(&node, &session, &["write".to_string()]);
    let (_accepted_output, metadata, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "{\"status\":\"completed\"}",
        &tool_telemetry,
        None,
        Some(("extract.json".to_string(), artifact_text)),
        &snapshot,
    );

    assert_eq!(rejected, None);
    assert_eq!(
        metadata.get("validation_outcome").and_then(Value::as_str),
        Some("passed")
    );
    assert!(metadata
        .get("validation_basis")
        .and_then(|value| value.get("must_write_file_statuses"))
        .and_then(Value::as_array)
        .is_some_and(|values| values.iter().any(|value| {
            value.get("path").and_then(Value::as_str) == Some("02_reddit_pain_points.md")
                && value
                    .get("materialized_by_current_attempt")
                    .and_then(Value::as_bool)
                    == Some(true)
        })));

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn explicit_output_files_override_legacy_must_write_files() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-explicit-output-files-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "draft_report".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Write report".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "extract.json",
                "must_write_files": ["legacy.md"],
                "output_files": ["reports/final.md"]
            }
        })),
    };
    let artifact_text =
        "{\"status\":\"completed\",\"summary\":\"Final report ready.\"}".to_string();
    let final_report = "# Final report\n\nDone.\n".to_string();
    std::fs::write(workspace_root.join("extract.json"), &artifact_text).expect("write artifact");
    std::fs::create_dir_all(workspace_root.join("reports")).expect("create reports directory");
    std::fs::write(workspace_root.join("reports/final.md"), &final_report)
        .expect("write final report");
    let mut session = Session::new(
        Some("explicit output files".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"extract.json","content":artifact_text}),
                result: Some(json!({"ok": true})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"reports/final.md","content":final_report}),
                result: Some(json!({"ok": true})),
                error: None,
            },
        ],
    ));
    let tool_telemetry =
        summarize_automation_tool_activity(&node, &session, &["write".to_string()]);
    let (_accepted_output, metadata, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "{\"status\":\"completed\"}",
        &tool_telemetry,
        None,
        Some(("extract.json".to_string(), artifact_text)),
        &snapshot,
    );

    assert_eq!(rejected, None);
    assert_eq!(
        metadata
            .get("validation_basis")
            .and_then(|value| value.get("explicit_output_files"))
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["reports/final.md"])
    );
    assert!(metadata
        .get("validation_basis")
        .and_then(|value| value.get("must_write_file_statuses"))
        .and_then(Value::as_array)
        .is_some_and(|values| values.iter().any(|value| {
            value.get("path").and_then(Value::as_str) == Some("reports/final.md")
                && value
                    .get("materialized_by_current_attempt")
                    .and_then(Value::as_bool)
                    == Some(true)
        })));
    assert!(metadata
        .get("validation_basis")
        .and_then(|value| value.get("must_write_file_statuses"))
        .and_then(Value::as_array)
        .is_some_and(|values| values
            .iter()
            .all(|value| { value.get("path").and_then(Value::as_str) != Some("legacy.md") })));

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn code_verification_status_matrix_covers_missing_failed_and_satisfied_checks() {
    let cases = vec![
        CodeVerificationMatrixCase {
            name: "verification-failed",
            verification_command: Some("cargo test"),
            session_text: "Done\n\n{\"status\":\"completed\"}",
            tool_telemetry: json!({
                "requested_tools": ["glob", "read", "edit", "apply_patch", "write", "bash"],
                "executed_tools": ["read", "apply_patch", "bash"],
                "verification_expected": true,
                "verification_ran": true,
                "verification_failed": true,
                "latest_verification_failure": "verification command failed with exit code 101: cargo test"
            }),
            expected_status: "verify_failed",
            expected_reason: Some("verification command failed with exit code 101: cargo test"),
            expected_failure_kind: Some("verification_failed"),
        },
        CodeVerificationMatrixCase {
            name: "verification-missing",
            verification_command: Some("cargo test"),
            session_text: "Done\n\n{\"status\":\"completed\"}",
            tool_telemetry: json!({
                "requested_tools": ["glob", "read", "edit", "apply_patch", "write", "bash"],
                "executed_tools": ["read", "apply_patch"],
                "verification_expected": true,
                "verification_ran": false,
                "verification_failed": false
            }),
            expected_status: "needs_repair",
            expected_reason: Some(
                "coding task completed without running the declared verification command",
            ),
            expected_failure_kind: None,
        },
        CodeVerificationMatrixCase {
            name: "verification-satisfied",
            verification_command: Some("cargo test"),
            session_text: "Done\n\n{\"status\":\"completed\"}",
            tool_telemetry: json!({
                "requested_tools": ["glob", "read", "edit", "apply_patch", "write", "bash"],
                "executed_tools": ["read", "apply_patch", "bash"],
                "verification_expected": true,
                "verification_ran": true,
                "verification_failed": false
            }),
            expected_status: "done",
            expected_reason: None,
            expected_failure_kind: Some("verification_passed"),
        },
        CodeVerificationMatrixCase {
            name: "verification-not-required",
            verification_command: None,
            session_text: "Done\n\n{\"status\":\"completed\"}",
            tool_telemetry: json!({
                "requested_tools": ["glob", "read", "edit", "apply_patch", "write"],
                "executed_tools": ["read", "apply_patch", "write"],
                "verification_expected": false,
                "verification_ran": false,
                "verification_failed": false
            }),
            expected_status: "done",
            expected_reason: None,
            expected_failure_kind: Some("verification_passed"),
        },
    ];

    for case in cases {
        run_code_verification_matrix_case(case);
    }
}

#[test]
fn email_delivery_status_matrix_covers_repairable_unavailable_failed_and_succeeded_paths() {
    let cases = vec![
        DeliveryMatrixCase {
            name: "offered-tools-not-executed",
            session_text: "A Gmail draft has been created.\n\n{\"status\":\"completed\",\"approved\":true}",
            tool_telemetry: json!({
                "requested_tools": ["glob", "read", "mcp_list"],
                "executed_tools": ["read", "glob", "mcp_list"],
                "email_delivery_attempted": false,
                "email_delivery_succeeded": false,
                "latest_email_delivery_failure": null,
                "attempt_evidence": {
                    "delivery": {"status": "not_attempted"}
                },
                "capability_resolution": {
                    "email_tool_diagnostics": {
                        "available_tools": ["mcp.composio_1.gmail_send_email", "mcp.composio_1.gmail_create_email_draft"],
                        "offered_tools": ["mcp.composio_1.gmail_send_email", "mcp.composio_1.gmail_create_email_draft"],
                        "available_send_tools": ["mcp.composio_1.gmail_send_email"],
                        "offered_send_tools": ["mcp.composio_1.gmail_send_email"],
                        "available_draft_tools": ["mcp.composio_1.gmail_create_email_draft"],
                        "offered_draft_tools": ["mcp.composio_1.gmail_create_email_draft"]
                    }
                }
            }),
            expected_status: "needs_repair",
            expected_reason:
                "email delivery to `test@example.com` was requested but no email draft/send tool executed",
            expected_blocker_category: "delivery_not_executed",
        },
        DeliveryMatrixCase {
            name: "no-email-tools-available",
            session_text: "{\"status\":\"completed\",\"approved\":true}",
            tool_telemetry: json!({
                "requested_tools": ["read", "mcp_list"],
                "executed_tools": ["read", "mcp_list"],
                "email_delivery_attempted": false,
                "email_delivery_succeeded": false,
                "latest_email_delivery_failure": null,
                "attempt_evidence": {
                    "delivery": {"status": "not_attempted"}
                },
                "capability_resolution": {
                    "mcp_tool_diagnostics": {
                        "selected_servers": ["gmail-main"],
                        "remote_tools": [],
                        "registered_tools": []
                    },
                    "email_tool_diagnostics": {
                        "available_tools": [],
                        "offered_tools": [],
                        "available_send_tools": [],
                        "offered_send_tools": [],
                        "available_draft_tools": [],
                        "offered_draft_tools": []
                    }
                }
            }),
            expected_status: "blocked",
            expected_reason:
                "email delivery to `test@example.com` was requested but no email-capable tools were available. Selected MCP servers: gmail-main. Remote MCP tools on selected servers: none. Registered tool-registry tools on selected servers: none. Discovered email-like tools: none. Offered email-like tools: none. This usually means the email connector is unavailable, MCP tools were not synced into the registry, or the tool names did not match email capability detection.",
            expected_blocker_category: "tool_unavailable",
        },
        DeliveryMatrixCase {
            name: "attempted-delivery-failed",
            session_text: "{\"status\":\"completed\",\"approved\":true}",
            tool_telemetry: json!({
                "requested_tools": ["mcp.composio_1.gmail_send_email"],
                "executed_tools": ["mcp.composio_1.gmail_send_email"],
                "email_delivery_attempted": true,
                "email_delivery_succeeded": false,
                "latest_email_delivery_failure": "smtp unauthorized",
                "attempt_evidence": {
                    "delivery": {"status": "attempted_failed"}
                },
                "capability_resolution": {
                    "email_tool_diagnostics": {
                        "available_tools": ["mcp.composio_1.gmail_send_email"],
                        "offered_tools": ["mcp.composio_1.gmail_send_email"],
                        "available_send_tools": ["mcp.composio_1.gmail_send_email"],
                        "offered_send_tools": ["mcp.composio_1.gmail_send_email"],
                        "available_draft_tools": [],
                        "offered_draft_tools": []
                    }
                }
            }),
            expected_status: "blocked",
            expected_reason: "smtp unauthorized",
            expected_blocker_category: "delivery_not_executed",
        },
        DeliveryMatrixCase {
            name: "delivery-succeeded",
            session_text: "{\"status\":\"completed\",\"approved\":true}",
            tool_telemetry: json!({
                "requested_tools": ["mcp.composio_1.gmail_send_email"],
                "executed_tools": ["mcp.composio_1.gmail_send_email"],
                "email_delivery_attempted": true,
                "email_delivery_succeeded": true,
                "latest_email_delivery_failure": null,
                "attempt_evidence": {
                    "delivery": {"status": "succeeded"}
                },
                "capability_resolution": {
                    "email_tool_diagnostics": {
                        "available_tools": ["mcp.composio_1.gmail_send_email"],
                        "offered_tools": ["mcp.composio_1.gmail_send_email"],
                        "available_send_tools": ["mcp.composio_1.gmail_send_email"],
                        "offered_send_tools": ["mcp.composio_1.gmail_send_email"],
                        "available_draft_tools": [],
                        "offered_draft_tools": []
                    }
                }
            }),
            expected_status: "completed",
            expected_reason: "",
            expected_blocker_category: "",
        },
    ];

    for case in cases {
        if case.expected_blocker_category.is_empty() {
            let node = email_delivery_matrix_node();
            let (status, reason, approved): (String, Option<String>, Option<bool>) =
                detect_automation_node_status(
                    &node,
                    case.session_text,
                    None,
                    &case.tool_telemetry,
                    None,
                );
            assert_eq!(status, case.expected_status, "case={}", case.name);
            assert_eq!(reason.as_deref(), None, "case={}", case.name);
            assert_eq!(approved, Some(true), "case={}", case.name);
            assert_eq!(
                detect_automation_blocker_category(
                    &node,
                    &status,
                    reason.as_deref(),
                    &case.tool_telemetry,
                    None,
                ),
                None,
                "case={}",
                case.name
            );
        } else {
            run_delivery_matrix_case(case);
        }
    }
}

#[test]
fn upstream_shape_matrix_covers_none_strict_rich_and_legacy_rich_modes() {
    let generic_report = "# Summary\n\nPlaceholder update.\n";
    let no_upstream_report = "# Summary\n\nA concise report without upstream dependencies.\n";
    let rich_upstream = AutomationUpstreamEvidence {
        read_paths: vec![
            ".tandem/artifacts/collect-inputs.json".to_string(),
            ".tandem/artifacts/research-sources.json".to_string(),
        ],
        discovered_relevant_paths: vec![
            ".tandem/artifacts/collect-inputs.json".to_string(),
            ".tandem/artifacts/research-sources.json".to_string(),
        ],
        web_research_attempted: true,
        web_research_succeeded: true,
        citation_count: 3,
        citations: vec![
            "https://example.com/legacy-1".to_string(),
            "https://example.com/legacy-2".to_string(),
            "https://example.com/legacy-3".to_string(),
        ],
    };
    let cases = vec![
        UpstreamShapeMatrixCase {
            name: "no-upstream-generic-summary-passes",
            quality_mode: None,
            legacy_rollback_enabled: None,
            artifact_text: no_upstream_report,
            upstream_evidence: None,
            expected_validation_outcome: "accepted_with_warnings",
            expected_rejected: None,
            expected_warning_count: None,
            expect_upstream_unsynthesized: false,
        },
        UpstreamShapeMatrixCase {
            name: "strict-rich-upstream-blocks-generic-summary",
            quality_mode: None,
            legacy_rollback_enabled: None,
            artifact_text: generic_report,
            upstream_evidence: Some(rich_upstream.clone()),
            expected_validation_outcome: "blocked",
            expected_rejected: Some(
                "final artifact does not adequately synthesize the available upstream evidence",
            ),
            expected_warning_count: Some(2),
            expect_upstream_unsynthesized: true,
        },
        UpstreamShapeMatrixCase {
            name: "legacy-rich-upstream-allows-generic-summary",
            quality_mode: Some("legacy"),
            legacy_rollback_enabled: Some(true),
            artifact_text: generic_report,
            upstream_evidence: Some(rich_upstream),
            expected_validation_outcome: "passed",
            expected_rejected: None,
            expected_warning_count: Some(0),
            expect_upstream_unsynthesized: false,
        },
    ];

    for case in cases {
        run_upstream_shape_matrix_case(case);
    }
}

#[test]
fn report_with_blocked_content_and_completed_status_is_not_blocked() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "generate_report".to_string(),
        agent_id: "writer".to_string(),
        objective: "Generate the final report".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "outputs/generate-report.md"
            }
        })),
    };
    let tool_telemetry = json!({
        "requested_tools": ["write"],
        "executed_tools": ["write"],
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "{\"status\":\"completed\"}",
            Some(&(
                "outputs/generate-report.md".to_string(),
                "# Report\n\nPipeline status: blocked by missing resume grounding artifacts.\n\nThe report is complete for the available evidence.".to_string(),
            )),
            &tool_telemetry,
            None,
        );

    assert_eq!(status, "completed");
    assert_eq!(reason, None);
    assert_eq!(approved, None);
}

#[test]
fn report_describing_test_failures_with_completed_status_passes() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "generate_report".to_string(),
        agent_id: "writer".to_string(),
        objective: "Generate the final report".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "outputs/generate-report.md"
            }
        })),
    };
    let tool_telemetry = json!({
        "requested_tools": ["write"],
        "executed_tools": ["write"],
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "{\"status\":\"completed\"}",
            Some(&(
                "outputs/generate-report.md".to_string(),
                "# CI Summary\n\nSeveral integration tests failed in the prior run, but this report artifact was generated successfully.".to_string(),
            )),
            &tool_telemetry,
            None,
        );

    assert_eq!(status, "completed");
    assert_eq!(reason, None);
    assert_eq!(approved, None);
}

#[test]
fn explicit_blocked_status_still_detected() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "generate_report".to_string(),
        agent_id: "writer".to_string(),
        objective: "Generate the final report".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "outputs/generate-report.md"
            }
        })),
    };
    let tool_telemetry = json!({
        "requested_tools": ["write"],
        "executed_tools": ["write"],
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "{\"status\":\"blocked\",\"reason\":\"waiting for more evidence\"}",
            Some(&(
                "outputs/generate-report.md".to_string(),
                "# Report\n\nPipeline status: blocked by missing resume grounding artifacts."
                    .to_string(),
            )),
            &tool_telemetry,
            None,
        );

    assert_eq!(status, "blocked");
    assert_eq!(reason.as_deref(), Some("waiting for more evidence"));
    assert_eq!(approved, None);
}

#[test]
fn render_automation_repair_brief_summarizes_previous_research_miss() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-brief".to_string(),
        agent_id: "research".to_string(),
        objective: "Write marketing-brief.md".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: None,
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "web_research_expected": true,
                "source_coverage_required": true
            }
        })),
    };
    let prior_output = json!({
        "status": "needs_repair",
        "validator_summary": {
            "reason": "research completed without required current web research",
            "unmet_requirements": [
                "missing_successful_web_research",
                "web_sources_reviewed_missing"
            ]
        },
        "tool_telemetry": {
            "requested_tools": ["glob", "read", "websearch", "write"],
            "executed_tools": ["glob", "write"]
        },
        "artifact_validation": {
            "blocking_classification": "tool_available_but_not_used",
            "unreviewed_relevant_paths": ["docs/pricing.md", "docs/customers.md"],
            "repair_attempt": 1,
            "repair_attempts_remaining": 4,
            "validation_basis": {
                "authority": "filesystem_and_receipts",
                "current_attempt_output_materialized": true,
                "current_attempt_has_recorded_activity": true,
                "current_attempt_has_read": false,
                "current_attempt_has_web_research": false,
                "workspace_inspection_satisfied": false
            },
            "required_next_tool_actions": [
                "Use `read` on the remaining relevant workspace files: docs/pricing.md, docs/customers.md.",
                "Use `websearch` successfully and include the resulting sources in `Web sources reviewed`."
            ]
        }
    });

    let brief = render_automation_repair_brief(&node, Some(&prior_output), 2, 5, Some("run-123"))
        .expect("repair brief");

    assert!(brief.contains("needs_repair"));
    assert!(brief.contains("missing_successful_web_research"));
    assert!(brief.contains("tool_available_but_not_used"));
    assert!(brief.contains("authority=filesystem_and_receipts"));
    assert!(brief.contains("output_materialized=true"));
    assert!(brief.contains("Required next tool actions"));
    assert!(brief.contains("Use `read` on the remaining relevant workspace files"));
    assert!(brief.contains("glob, read, websearch, write"));
    assert!(brief.contains("glob, write"));
    assert!(brief.contains("docs/pricing.md, docs/customers.md"));
    assert!(brief.contains("Remaining repair attempts after this run: 3"));
}

#[test]
fn render_automation_repair_brief_includes_exact_missing_required_source_reads() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-source-brief".to_string(),
        agent_id: "research".to_string(),
        objective: "Write marketing-brief.md".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: None,
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "source_coverage_required": true
            }
        })),
    };
    let prior_output = json!({
        "status": "needs_repair",
        "validator_summary": {
            "reason": "research completed without reading the exact required source files",
            "unmet_requirements": [
                "required_source_paths_not_read"
            ]
        },
        "tool_telemetry": {
            "requested_tools": ["glob", "read", "write"],
            "executed_tools": ["glob", "write"]
        },
        "artifact_validation": {
            "blocking_classification": "tool_available_but_not_used",
            "unreviewed_relevant_paths": ["docs/pricing.md"],
            "repair_attempt": 1,
            "repair_attempts_remaining": 4,
            "validation_basis": {
                "authority": "filesystem_and_receipts",
                "current_attempt_output_materialized": true,
                "current_attempt_has_recorded_activity": true,
                "current_attempt_has_read": false,
                "current_attempt_has_web_research": false,
                "workspace_inspection_satisfied": false,
                "required_source_read_paths": ["RESUME.md", "docs/resume.md"],
                "missing_required_source_read_paths": ["RESUME.md", "docs/resume.md"]
            },
            "required_next_tool_actions": [
                "Use `read` on the exact required source files before finalizing: RESUME.md, docs/resume.md. Similar backup or copy filenames do not satisfy the requirement."
            ]
        }
    });

    let brief = render_automation_repair_brief(&node, Some(&prior_output), 2, 5, Some("run-123"))
        .expect("repair brief");

    assert!(brief.contains("Required source read paths: RESUME.md, docs/resume.md"));
    assert!(brief.contains("Missing required source read paths: RESUME.md, docs/resume.md"));
    assert!(
        brief.contains("exact required source files before finalizing: RESUME.md, docs/resume.md")
    );
    assert!(brief.contains("required_source_paths_not_read"));
}

#[test]
fn render_automation_repair_brief_includes_upstream_paths_for_synthesis_repairs() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "generate_report".to_string(),
        agent_id: "writer".to_string(),
        objective: "Write the final report.".to_string(),
        depends_on: vec!["analyze_findings".to_string()],
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: None,
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "generate-report.md"
            }
        })),
    };
    let prior_output = json!({
        "status": "needs_repair",
        "validator_summary": {
            "reason": "final artifact does not adequately synthesize the available upstream evidence",
            "unmet_requirements": [
                "upstream_evidence_not_synthesized"
            ]
        },
        "tool_telemetry": {
            "requested_tools": ["read", "write"],
            "executed_tools": ["read", "write"]
        },
        "artifact_validation": {
            "blocking_classification": "artifact_contract_unmet",
            "required_next_tool_actions": [
                "Read and synthesize the strongest upstream artifacts before finalizing: .tandem/runs/run-1/artifacts/collect-inputs.json, .tandem/runs/run-1/artifacts/analyze-findings.md. Rewrite the final report as a substantive multi-section synthesis that reuses the concrete terminology, named entities, objections, risks, and proof points already present upstream, and mention at least 2 distinct upstream evidence anchors in the body."
            ],
            "validation_basis": {
                "authority": "filesystem_and_receipts",
                "current_attempt_output_materialized": true,
                "current_attempt_has_recorded_activity": true,
                "current_attempt_has_read": true,
                "current_attempt_has_web_research": false,
                "workspace_inspection_satisfied": true,
                "upstream_read_paths": [
                    ".tandem/runs/run-1/artifacts/collect-inputs.json",
                    ".tandem/runs/run-1/artifacts/analyze-findings.md"
                ]
            }
        }
    });

    let brief = render_automation_repair_brief(&node, Some(&prior_output), 2, 5, Some("run-123"))
        .expect("repair brief");

    assert!(brief.contains(
        "Upstream read paths available for synthesis: .tandem/runs/run-1/artifacts/collect-inputs.json, .tandem/runs/run-1/artifacts/analyze-findings.md"
    ));
    assert!(
        brief.contains("Read and synthesize the strongest upstream artifacts before finalizing")
    );
}

#[test]
fn code_patch_repair_brief_mentions_patch_apply_test_loop() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "code_patch".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Patch the code and verify the change.".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "code_patch".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::CodePatch),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "src/lib.rs",
                "verification_command": "cargo test",
                "write_scope": "repo-scoped edits"
            }
        })),
    };
    let prior_output = json!({
        "status": "needs_repair",
        "validator_summary": {
            "reason": "verification did not run",
            "unmet_requirements": ["verification_missing"]
        },
        "tool_telemetry": {
            "requested_tools": ["glob", "read", "edit", "apply_patch", "write"],
            "executed_tools": ["glob", "read", "write"]
        },
        "artifact_validation": {
            "blocking_classification": "verification_required",
            "repair_attempt": 1,
            "repair_attempts_remaining": 4,
            "required_next_tool_actions": [
                "Patch the code with `edit` or `apply_patch` before any new `write`.",
                "Run `cargo test` after the patch and fix the smallest failing root cause."
            ]
        }
    });

    let brief = render_automation_repair_brief(&node, Some(&prior_output), 2, 5, Some("run-123"))
        .expect("repair brief");

    assert!(brief.contains("Code workflow repair path"));
    assert!(brief.contains("inspect the touched files"));
    assert!(brief.contains("edit` or `apply_patch"));
    assert!(brief.contains("cargo test"));
    assert!(brief.contains("repo-scoped edits"));
}

#[test]
fn render_automation_repair_brief_adds_final_attempt_escalation() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-brief".to_string(),
        agent_id: "research".to_string(),
        objective: "Write marketing-brief.md".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: None,
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": ".tandem/artifacts/marketing-brief.md"
            }
        })),
    };
    let prior_output = json!({
        "status": "needs_repair",
        "validator_summary": {
            "reason": "research completed without required current web research",
            "unmet_requirements": ["missing_successful_web_research"]
        },
        "artifact_validation": {
            "blocking_classification": "tool_available_but_not_used",
            "repair_attempt": 2,
            "repair_attempts_remaining": 1
        }
    });

    let brief = render_automation_repair_brief(&node, Some(&prior_output), 3, 3, Some("run-123"))
        .expect("repair brief");

    assert!(brief.contains("FINAL ATTEMPT"));
    assert!(brief.contains(".tandem/runs/run-123/artifacts/marketing-brief.md"));
    assert!(!brief.contains("The engine will accept the output file at `.tandem/artifacts/"));
    assert!(brief.contains("{\"status\":\"completed\"}"));
    assert!(brief.contains("Do not ask follow-up questions."));
}

#[test]
fn repair_brief_detects_activity_despite_empty_telemetry() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "analyze-findings".to_string(),
        agent_id: "analyst".to_string(),
        objective: "Write analyze-findings.json".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": ".tandem/artifacts/analyze-findings.json"
            }
        })),
    };
    let prior_output = json!({
        "status": "needs_repair",
        "validator_summary": {
            "reason": "required output was not created",
            "unmet_requirements": []
        },
        "tool_telemetry": {
            "requested_tools": [],
            "executed_tools": []
        },
        "artifact_validation": {
            "blocking_classification": "execution_error",
            "repair_attempt": 2,
            "repair_attempts_remaining": 1,
            "required_next_tool_actions": [
                "Retry after provider connectivity recovers."
            ],
            "validation_basis": {
                "authority": "filesystem_and_receipts",
                "current_attempt_has_recorded_activity": true,
                "current_attempt_output_materialized": false,
                "current_attempt_has_read": true,
                "current_attempt_has_web_research": false,
                "workspace_inspection_satisfied": true
            }
        }
    });

    let brief = render_automation_repair_brief(&node, Some(&prior_output), 3, 3, Some("run-123"))
        .expect("repair brief");

    assert!(brief
        .contains("Tools offered last attempt: not recorded (but session activity was detected)."));
    assert!(brief.contains("Blocking classification: artifact_write_missing."));
    assert!(brief.contains(
        "Required next tool actions: write the required run artifact to the declared output path."
    ));
    assert!(brief.contains(".tandem/runs/run-123/artifacts/analyze-findings.json"));
}

#[test]
fn analyze_findings_final_attempt_repair_brief_stays_run_scoped() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "analyze_findings".to_string(),
        agent_id: "analyst".to_string(),
        objective: "Write analyze-findings.json".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": ".tandem/artifacts/analyze-findings.json"
            }
        })),
    };
    let prior_output = json!({
        "status": "needs_repair",
        "validator_summary": {
            "reason": "required output was not created",
            "unmet_requirements": ["current_attempt_output_missing"]
        },
        "tool_telemetry": {
            "requested_tools": ["glob", "read", "write"],
            "executed_tools": []
        },
        "artifact_validation": {
            "blocking_classification": "execution_error",
            "repair_attempt": 2,
            "repair_attempts_remaining": 1,
            "required_next_tool_actions": [
                "Retry after provider connectivity recovers."
            ],
            "validation_basis": {
                "authority": "filesystem_and_receipts",
                "current_attempt_has_recorded_activity": true,
                "current_attempt_output_materialized": false,
                "current_attempt_has_read": true,
                "workspace_inspection_satisfied": true
            }
        }
    });

    let brief = render_automation_repair_brief(&node, Some(&prior_output), 3, 3, Some("run-123"))
        .expect("repair brief");

    assert!(brief.contains("FINAL ATTEMPT"));
    assert!(brief.contains(".tandem/runs/run-123/artifacts/analyze-findings.json"));
    assert!(!brief.contains(".tandem/artifacts/analyze-findings.json"));
    assert!(brief.contains("Blocking classification: artifact_write_missing."));
    assert!(brief.contains(
        "Required next tool actions: write the required run artifact to the declared output path."
    ));
}

#[test]
fn repair_attempt_with_concrete_read_and_changed_output_is_accepted() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-repair-read-changed-output-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("docs")).expect("create workspace");
    std::fs::write(
        workspace_root.join("docs/pricing.md"),
        "# Pricing\n\n- Teams plan starts at $49 per seat.\n",
    )
    .expect("write source file");
    let preexisting_output = "# Marketing Brief\n\nOld draft.\n".to_string();
    std::fs::write(
        workspace_root.join("marketing-brief.md"),
        &preexisting_output,
    )
    .expect("write previous output");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-brief".to_string(),
        agent_id: "research".to_string(),
        objective: "Write marketing-brief.md".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: None,
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "web_research_expected": true,
                "source_coverage_required": true
            }
        })),
    };
    let final_output = "# Marketing Brief\n\n## Findings\nThe team plan starts at $49 per seat and the revised workflow now captures concrete pricing evidence from docs/pricing.md.\n\n## Files reviewed\n- docs/pricing.md\n".to_string();
    std::fs::write(workspace_root.join("marketing-brief.md"), &final_output)
        .expect("write repaired output");
    let mut session = Session::new(
        Some("repair attempt".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "read".to_string(),
                args: json!({"file_path":"docs/pricing.md"}),
                result: Some(json!({"ok": true})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"marketing-brief.md","content":"# Marketing Brief\n\nWorking draft.\n"}),
                result: Some(json!({"ok": true})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"marketing-brief.md","content":final_output}),
                result: Some(json!({"ok": true})),
                error: None,
            },
        ],
    ));
    let tool_telemetry = summarize_automation_tool_activity(
        &node,
        &session,
        &[
            "glob".to_string(),
            "read".to_string(),
            "websearch".to_string(),
            "write".to_string(),
        ],
    );
    let (_accepted_output, metadata, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "I repaired the artifact and rewrote the output file.",
        &tool_telemetry,
        Some(&preexisting_output),
        Some(("marketing-brief.md".to_string(), final_output)),
        &snapshot,
    );

    assert_eq!(rejected, None);
    assert_eq!(
        metadata.get("repair_succeeded").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        metadata
            .get("validation_basis")
            .and_then(|value| value.get("repair_promoted_after_read_and_output_change"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        metadata
            .get("unmet_requirements")
            .and_then(Value::as_array)
            .map(|values| values.len()),
        Some(0)
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn automation_output_enforcement_prefers_contract_over_legacy_builder_metadata() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Research".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: None,
            enforcement: Some(crate::AutomationOutputEnforcement {
                validation_profile: None,
                required_tools: vec!["read".to_string()],
                required_evidence: vec!["local_source_reads".to_string()],
                required_sections: vec!["files_reviewed".to_string()],
                prewrite_gates: vec!["workspace_inspection".to_string()],
                retry_on_missing: vec!["local_source_reads".to_string()],
                terminal_on: vec!["repair_budget_exhausted".to_string()],
                repair_budget: Some(2),
                session_text_recovery: Some("disabled".to_string()),
            }),
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "required_tools": ["read", "websearch"],
                "web_research_expected": true
            }
        })),
    };

    let enforcement = automation_node_output_enforcement(&node);
    assert_eq!(enforcement.required_tools, vec!["read"]);
    assert_eq!(enforcement.required_evidence, vec!["local_source_reads"]);
    assert_eq!(
        enforcement.session_text_recovery.as_deref(),
        Some("disabled")
    );
}

#[test]
fn automation_output_enforcement_backfills_research_contract_from_legacy_builder_metadata() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Research".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: None,
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "required_tools": ["read", "websearch"],
                "web_research_expected": true
            }
        })),
    };

    let enforcement = automation_node_output_enforcement(&node);
    assert!(enforcement.required_tools.iter().any(|tool| tool == "read"));
    assert!(enforcement
        .required_tools
        .iter()
        .any(|tool| tool == "websearch"));
    assert!(enforcement
        .required_sections
        .iter()
        .any(|item| item == "web_sources_reviewed"));
    assert_eq!(
        enforcement.session_text_recovery.as_deref(),
        Some("require_prewrite_satisfied")
    );
}

#[test]
fn upstream_evidence_can_satisfy_exact_required_source_read_paths() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-upstream-exact-source-reads-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    std::fs::write(
        workspace_root.join("RESUME.md"),
        "# Resume\n\nSource of truth.\n",
    )
    .expect("write resume");
    std::fs::write(
        workspace_root.join("resume_overview.md"),
        "# Resume overview\n\nDerived summary.\n",
    )
    .expect("write overview");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "execute_goal".to_string(),
        agent_id: "workspace-operator".to_string(),
        objective: "Analyze the local `RESUME.md` file and use it as the source of truth for skills, role targets, seniority, technologies, and geography preferences.".to_string(),
        depends_on: vec!["collect_inputs".to_string()],
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: None,
            enforcement: Some(crate::AutomationOutputEnforcement {
                validation_profile: Some("artifact_only".to_string()),
                required_tools: vec!["read".to_string(), "write".to_string()],
                required_evidence: vec!["local_source_reads".to_string()],
                required_sections: Vec::new(),
                prewrite_gates: Vec::new(),
                retry_on_missing: vec!["local_source_reads".to_string()],
                terminal_on: Vec::new(),
                repair_budget: Some(3),
                session_text_recovery: Some("disabled".to_string()),
            }),
            schema: None,
            summary_guidance: Some("Return a concise summary.".to_string()),
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "execute-goal.md"
            }
        })),
    };
    let artifact_text =
        "# Resume Summary\n\nCompleted summary using upstream evidence.".to_string();
    let session_text = artifact_text.clone();
    let artifact_path = ".tandem/runs/run-execute-goal/artifacts/execute-goal.md".to_string();

    let mut session = Session::new(
        Some("upstream-exact-source-reads".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "read".to_string(),
                args: json!({"path":"resume_overview.md"}),
                result: Some(json!({"output":"# Resume overview"})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"execute-goal.md","content":artifact_text.clone()}),
                result: Some(json!({"ok": true})),
                error: None,
            },
        ],
    ));
    let tool_telemetry = summarize_automation_tool_activity(
        &node,
        &session,
        &["read".to_string(), "write".to_string()],
    );
    let upstream_evidence = AutomationUpstreamEvidence {
        read_paths: vec!["RESUME.md".to_string()],
        discovered_relevant_paths: vec!["RESUME.md".to_string()],
        web_research_attempted: false,
        web_research_succeeded: false,
        citation_count: 0,
        citations: Vec::new(),
    };

    std::fs::write(workspace_root.join("execute-goal.md"), &artifact_text).expect("write artifact");
    let (accepted_output, artifact_validation, rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            Some("run-execute-goal"),
            session_text.as_str(),
            &tool_telemetry,
            None,
            Some((artifact_path, artifact_text.clone())),
            &snapshot,
            Some(&upstream_evidence),
        );

    assert!(accepted_output.is_some());
    assert_eq!(rejected, None);
    assert!(!artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|values| values
            .iter()
            .any(|value| value.as_str() == Some("required_source_paths_not_read"))));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn structured_handoff_workspace_bootstrap_nodes_treat_reads_as_optional() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "execute_goal".to_string(),
        agent_id: "workspace-operator".to_string(),
        objective: "Initialize any missing job-search workspace directories and files, read README.md if present, and update resume-overview.md, tracker/search-ledger/2026-04-07.json, tracker/seen-jobs.jsonl, and daily-recaps/2026-04-07-job-search-recap.md.".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
            schema: None,
            summary_guidance: Some("Return a structured handoff.".to_string()),
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: None,
    };

    let enforcement = automation_node_output_enforcement(&node);
    assert!(enforcement.required_tools.iter().any(|tool| tool == "glob"));
    assert!(enforcement
        .required_tools
        .iter()
        .any(|tool| tool == "write"));
    assert!(!enforcement.required_tools.iter().any(|tool| tool == "read"));
    assert_eq!(
        enforcement.validation_profile.as_deref(),
        Some("artifact_only")
    );
    assert!(!enforcement
        .required_evidence
        .iter()
        .any(|evidence| evidence == "local_source_reads"));

    let capabilities = automation_tool_capability_ids(&node, "artifact_write");
    assert!(capabilities
        .iter()
        .any(|capability| capability == "workspace_discover"));
    assert!(capabilities
        .iter()
        .any(|capability| capability == "artifact_write"));
    assert!(!capabilities
        .iter()
        .any(|capability| capability == "workspace_read"));
}

#[test]
fn bootstrap_workspace_output_nodes_require_inspection_but_not_concrete_reads() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "execute_goal".to_string(),
        agent_id: "workspace-operator".to_string(),
        objective: "Initialize any missing job-search workspace directories and files, read README.md if present, and update resume-overview.md, tracker/search-ledger/2026-04-07.json, tracker/seen-jobs.jsonl, and daily-recaps/2026-04-07-job-search-recap.md.".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
            schema: None,
            summary_guidance: Some("Return a structured handoff.".to_string()),
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "daily-recaps/2026-04-07-job-search-recap.md"
            }
        })),
    };

    let requirements = automation_node_prewrite_requirements(
        &node,
        &["glob".to_string(), "read".to_string(), "write".to_string()],
    )
    .expect("prewrite requirements");
    assert!(requirements.workspace_inspection_required);
    assert!(!requirements.concrete_read_required);
}
