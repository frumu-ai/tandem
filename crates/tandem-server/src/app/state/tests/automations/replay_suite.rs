use super::*;
use serde_json::{json, Value};
use tandem_types::{MessagePart, MessageRole, Session};

fn structured_json_artifact_node(
    node_id: &str,
    output_path: &str,
    required_source_read_paths: &[&str],
    must_write_files: &[&str],
) -> AutomationFlowNode {
    AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: node_id.to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Read the resume materials and write the run artifact.".to_string(),
        depends_on: vec!["collect_inputs".to_string()],
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": output_path,
                "must_write_files": must_write_files,
                "required_source_read_paths": required_source_read_paths,
                "source_coverage_required": true
            }
        })),
    }
}

fn report_markdown_node(node_id: &str, output_path: &str) -> AutomationFlowNode {
    AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: node_id.to_string(),
        agent_id: "writer".to_string(),
        objective: "Generate the final report from upstream evidence.".to_string(),
        depends_on: vec!["collect_inputs".to_string(), "execute_goal".to_string()],
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": output_path
            }
        })),
    }
}

#[test]
fn resume_job_search_execute_goal_replay_accepts_upstream_resume_sources() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-resume-replay-upstream-sources-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    std::fs::write(
        workspace_root.join("RESUME.md"),
        "# Evan Green\n\nSenior Software Engineer focused on Rust and workflow automation.\n",
    )
    .expect("write resume");
    std::fs::write(
        workspace_root.join("resume_overview.md"),
        "# Resume Overview\n\n- Senior Software Engineer\n- Rust\n- Automation\n",
    )
    .expect("write overview");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = structured_json_artifact_node(
        "execute_goal",
        ".tandem/runs/run-123/artifacts/execute-goal.json",
        &["RESUME.md", "resume_overview.md"],
        &["job_search_results_2026-04-15.md"],
    );
    let artifact_text = json!({
        "status": "completed",
        "summary": "Used the resume inputs to append the daily job-search results file."
    })
    .to_string();
    let results_text = "# Job Search Results for 2026-04-15\n\n## Run at 22:11\n\n### Summary\n- Reused the existing resume overview.\n- Appended one new EU remote search pass.\n".to_string();
    std::fs::create_dir_all(workspace_root.join(".tandem/runs/run-123/artifacts"))
        .expect("create artifact directory");
    std::fs::write(
        workspace_root.join(".tandem/runs/run-123/artifacts/execute-goal.json"),
        &artifact_text,
    )
    .expect("write artifact");
    std::fs::write(
        workspace_root.join("job_search_results_2026-04-15.md"),
        &results_text,
    )
    .expect("write results");

    let mut session = Session::new(
        Some("resume-job-search-execute-goal-replay".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "read".to_string(),
                args: json!({"path":"resume_overview.md"}),
                result: Some(json!("ok")),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path": ".tandem/runs/run-123/artifacts/execute-goal.json",
                    "content": artifact_text
                }),
                result: Some(json!("ok")),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path": "job_search_results_2026-04-15.md",
                    "content": results_text
                }),
                result: Some(json!("ok")),
                error: None,
            },
        ],
    ));

    let tool_telemetry = json!({
        "requested_tools": ["read", "write"],
        "executed_tools": ["read", "write"],
        "tool_call_counts": {
            "read": 1,
            "write": 2
        }
    });
    let upstream_evidence = AutomationUpstreamEvidence {
        notion_identity_unconfirmed: false,
        external_citations_missing: false,
        read_paths: vec!["RESUME.md".to_string()],
        discovered_relevant_paths: vec![
            "RESUME.md".to_string(),
            "resume_overview.md".to_string(),
            "job_search_results_2026-04-15.md".to_string(),
        ],
        web_research_attempted: false,
        web_research_succeeded: false,
        citation_count: 0,
        citations: Vec::new(),
    };

    let (accepted_output, validation, rejected) = validate_automation_artifact_output_with_upstream(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        None,
        "{\"status\":\"completed\"}",
        &tool_telemetry,
        None,
        Some((
            ".tandem/runs/run-123/artifacts/execute-goal.json".to_string(),
            artifact_text,
        )),
        &snapshot,
        Some(&upstream_evidence),
    );

    assert!(accepted_output.is_some());
    assert_eq!(
        validation.get("validation_outcome").and_then(Value::as_str),
        Some("passed")
    );
    assert_eq!(rejected, None);
    assert!(!validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .any(|value| { value.as_str() == Some("required_source_paths_not_read") })));
    assert!(validation
        .get("validation_basis")
        .and_then(|value| value.get("must_write_file_statuses"))
        .and_then(Value::as_array)
        .is_some_and(|statuses| statuses.iter().any(|status| {
            status.get("path").and_then(Value::as_str) == Some("job_search_results_2026-04-15.md")
                && status
                    .get("materialized_by_current_attempt")
                    .and_then(Value::as_bool)
                    == Some(true)
        })));

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn resume_job_search_generate_report_replay_requires_multiple_upstream_anchors() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-resume-replay-synthesis-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = report_markdown_node(
        "generate_report",
        ".tandem/runs/run-123/artifacts/generate-report.md",
    );
    std::fs::create_dir_all(workspace_root.join(".tandem/runs/run-123/artifacts"))
        .expect("create artifact directory");

    let blocked_report = r#"# Final Job Search Report

## Executive Summary
The current run produced a stronger daily search log for European software roles and preserved the local source-of-truth workflow. The report is intentionally compact, but it keeps the main signal clear for the operator.

## Resume Signals
The `resume_overview.md` handoff shows a senior Rust and automation profile aimed at product-minded engineering roles. That profile should keep prioritizing remote-friendly openings and roles with workflow, AI, or platform ownership.

## Search Direction
The strongest opportunities are still Europe-based remote roles, direct company postings, and focused job boards that match seniority and systems work. Broader boards can still contribute, but they should not drive the shortlist.

## Recommendation
Continue the same search direction tomorrow with slightly tighter keyword filters around Rust, workflow automation, and senior backend leadership.
"#;
    std::fs::write(
        workspace_root.join(".tandem/runs/run-123/artifacts/generate-report.md"),
        blocked_report,
    )
    .expect("write blocked report");

    let mut blocked_session = Session::new(
        Some("resume-job-search-generate-report-blocked".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    blocked_session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": ".tandem/runs/run-123/artifacts/generate-report.md",
                "content": blocked_report
            }),
            result: Some(json!("ok")),
            error: None,
        }],
    ));

    let tool_telemetry = json!({
        "requested_tools": ["read", "write"],
        "executed_tools": ["read", "write"],
        "tool_call_counts": {
            "read": 2,
            "write": 1
        }
    });
    let upstream_evidence = AutomationUpstreamEvidence {
        notion_identity_unconfirmed: false,
        external_citations_missing: false,
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
    };

    let (_accepted_blocked, blocked_validation, blocked_rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &blocked_session,
            workspace_root.to_str().expect("workspace root"),
            None,
            "{\"status\":\"completed\"}",
            &tool_telemetry,
            None,
            Some((
                ".tandem/runs/run-123/artifacts/generate-report.md".to_string(),
                blocked_report.to_string(),
            )),
            &snapshot,
            Some(&upstream_evidence),
        );

    assert_eq!(
        blocked_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(
        blocked_rejected.as_deref(),
        Some("final artifact does not adequately synthesize the available upstream evidence")
    );
    assert!(blocked_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))));

    let repaired_report = r#"# Final Job Search Report

## Executive Summary
This report combines the durable role profile from `resume_overview.md` with the concrete search outcomes in `job_search_results_2026-04-15.md`. Together they show that the workflow stayed aligned with the resume source of truth while producing a reusable daily search log.

## Resume Signals
The `resume_overview.md` file keeps the search centered on senior Rust, workflow automation, and product-oriented engineering roles. It also preserves the remote and Europe-oriented targeting that the search run needs to respect.

## Search Outcomes
The `job_search_results_2026-04-15.md` results log shows the actual job-board sweep, the strongest direct listings, and the places where matches were weak or repeated. That evidence matters because it turns the report from a generic career summary into a real run summary grounded in what was found.

## Synthesis
Reading both `resume_overview.md` and `job_search_results_2026-04-15.md` makes the recommendation more trustworthy: keep prioritizing Europe-based remote roles, direct company postings, and senior backend or platform roles that overlap with Rust and workflow automation. The resume evidence explains why those roles fit, and the daily results evidence shows which sources are producing the best leads.

## Recommendation
Keep the next run focused on the same role family, but tighten keywords around Rust, workflow automation, platform ownership, and AI infrastructure so the future results log keeps improving rather than broadening into weak matches.
"#;
    std::fs::write(
        workspace_root.join(".tandem/runs/run-123/artifacts/generate-report.md"),
        repaired_report,
    )
    .expect("write repaired report");

    let mut repaired_session = Session::new(
        Some("resume-job-search-generate-report-repaired".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    repaired_session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": ".tandem/runs/run-123/artifacts/generate-report.md",
                "content": repaired_report
            }),
            result: Some(json!("ok")),
            error: None,
        }],
    ));

    let (_accepted_repaired, repaired_validation, repaired_rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &repaired_session,
            workspace_root.to_str().expect("workspace root"),
            None,
            "{\"status\":\"completed\"}",
            &tool_telemetry,
            None,
            Some((
                ".tandem/runs/run-123/artifacts/generate-report.md".to_string(),
                repaired_report.to_string(),
            )),
            &snapshot,
            Some(&upstream_evidence),
        );

    assert_eq!(
        repaired_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("accepted_with_warnings")
    );
    assert_eq!(repaired_rejected, None);
    assert!(!repaired_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))));

    let _ = std::fs::remove_dir_all(workspace_root);
}

/// Build a substantive (>= 600 chars, >= 4 headings) report-markdown artifact
/// that anchors exactly the provided upstream source filenames. Generic prose
/// never accidentally anchors a source, because anchoring requires the full
/// filename (or its collapsed/compact form), not generic words like "resume".
fn substantive_report_anchoring(mentioned_sources: &[&str]) -> String {
    let mut report = String::from(
        "# Final Job Search Report\n\n\
## Executive Summary\n\
This run produced a substantive synthesis of the available evidence and kept the \
workflow aligned with the durable source of truth while recording a reusable daily \
search log for the operator to act on tomorrow.\n\n\
## Role Profile\n\
The candidate profile stays centered on senior Rust, workflow automation, and \
product-minded platform engineering. Remote-friendly and Europe-oriented targeting \
remains the right shortlist strategy for the next several runs.\n\n\
## Search Outcomes\n\
The sweep surfaced direct company postings and focused job boards as the strongest \
sources, while broad aggregators contributed weaker, repeated matches that should \
not drive the shortlist.\n\n\
## Recommendation\n\
Continue the same role family next run with tighter keyword filters around Rust, \
workflow automation, and platform ownership so the results log keeps improving \
rather than broadening into weak matches.\n\n",
    );
    if !mentioned_sources.is_empty() {
        report.push_str("## Evidence Sources\n");
        for source in mentioned_sources {
            report.push_str(&format!(
                "Reviewed `{source}` and folded its concrete signal into the synthesis above.\n"
            ));
        }
        report.push('\n');
    }
    report
}

/// WRC-02 (TAN-46): calibrate rich-upstream synthesis thresholds with an
/// anchor-aware matrix — generic (zero-anchor) and single-anchor-when-multiple
/// sources block; a single source met by one anchor and multiple sources met by
/// matching anchors are accepted.
#[test]
fn generate_report_synthesis_anchor_threshold_matrix() {
    let resume = "resume_overview.md";
    let results = "job_search_results_2026-04-15.md";

    // (read_paths, anchored sources in the report, expected to block on synthesis)
    let cases: &[(&[&str], &[&str], bool)] = &[
        // Single source, anchored once -> target 1 met -> accepted.
        (&[resume], &[resume], false),
        // Single source, zero anchors (generic report) -> target 1 unmet -> blocked.
        (&[resume], &[], true),
        // Two sources, only one anchored -> target 2 unmet -> blocked.
        (&[resume, results], &[resume], true),
        // Two sources, both anchored -> target 2 met -> accepted.
        (&[resume, results], &[resume, results], false),
    ];

    for (idx, (read_paths, anchored, expect_blocked)) in cases.iter().enumerate() {
        let workspace_root = std::env::temp_dir().join(format!(
            "tandem-resume-replay-anchor-matrix-{idx}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&workspace_root).expect("create workspace");
        let snapshot = automation_workspace_root_file_snapshot(
            workspace_root.to_str().expect("workspace root"),
        );
        let node = report_markdown_node(
            "generate_report",
            ".tandem/runs/run-123/artifacts/generate-report.md",
        );
        std::fs::create_dir_all(workspace_root.join(".tandem/runs/run-123/artifacts"))
            .expect("create artifact directory");
        let report = substantive_report_anchoring(anchored);
        std::fs::write(
            workspace_root.join(".tandem/runs/run-123/artifacts/generate-report.md"),
            &report,
        )
        .expect("write report");

        let mut session = Session::new(
            Some(format!("resume-anchor-matrix-{idx}")),
            Some(workspace_root.to_str().expect("workspace root").to_string()),
        );
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path": ".tandem/runs/run-123/artifacts/generate-report.md",
                    "content": report
                }),
                result: Some(json!("ok")),
                error: None,
            }],
        ));

        let tool_telemetry = json!({
            "requested_tools": ["read", "write"],
            "executed_tools": ["read", "write"],
            "tool_call_counts": { "read": read_paths.len(), "write": 1 }
        });
        let upstream_evidence = AutomationUpstreamEvidence {
            notion_identity_unconfirmed: false,
            external_citations_missing: false,
            read_paths: read_paths.iter().map(|s| s.to_string()).collect(),
            discovered_relevant_paths: read_paths.iter().map(|s| s.to_string()).collect(),
            web_research_attempted: false,
            web_research_succeeded: false,
            citation_count: 0,
            citations: Vec::new(),
        };

        let (_accepted, validation, rejected) = validate_automation_artifact_output_with_upstream(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            None,
            "{\"status\":\"completed\"}",
            &tool_telemetry,
            None,
            Some((
                ".tandem/runs/run-123/artifacts/generate-report.md".to_string(),
                report.clone(),
            )),
            &snapshot,
            Some(&upstream_evidence),
        );

        let synthesis_unmet = validation
            .get("unmet_requirements")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))
            });

        if *expect_blocked {
            assert!(
                synthesis_unmet,
                "case {idx}: expected upstream_evidence_not_synthesized (read_paths={read_paths:?}, anchored={anchored:?}); validation={validation}"
            );
            assert_eq!(
                rejected.as_deref(),
                Some(
                    "final artifact does not adequately synthesize the available upstream evidence"
                ),
                "case {idx}: expected synthesis block reason"
            );
        } else {
            assert!(
                !synthesis_unmet,
                "case {idx}: did not expect a synthesis block (read_paths={read_paths:?}, anchored={anchored:?}); validation={validation}"
            );
        }

        let _ = std::fs::remove_dir_all(workspace_root);
    }
}

/// WRC-01 (TAN-45): exact-source-read failure — a structured_json node that
/// requires reading specific source files must block when one required source
/// is never read (neither in the current attempt nor upstream).
#[test]
fn execute_goal_replay_blocks_when_required_source_unread() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-resume-replay-missing-source-read-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    std::fs::write(
        workspace_root.join("RESUME.md"),
        "# Resume\n\nRust engineer.\n",
    )
    .expect("write resume");
    std::fs::write(
        workspace_root.join("cover_letter.md"),
        "# Cover Letter\n\nDear hiring manager.\n",
    )
    .expect("write cover letter");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    // Requires reading both source-of-truth inputs, but the run only reads
    // RESUME.md. `builder.input_files` drives the named-source-read requirement.
    let mut node = structured_json_artifact_node(
        "execute_goal",
        ".tandem/runs/run-123/artifacts/execute-goal.json",
        &["RESUME.md", "cover_letter.md"],
        &[],
    );
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/runs/run-123/artifacts/execute-goal.json",
            "input_files": ["RESUME.md", "cover_letter.md"],
            "source_coverage_required": true
        }
    }));
    let artifact_text = json!({
        "status": "completed",
        "summary": "Wrote the run artifact from the resume only."
    })
    .to_string();
    std::fs::create_dir_all(workspace_root.join(".tandem/runs/run-123/artifacts"))
        .expect("create artifact directory");
    std::fs::write(
        workspace_root.join(".tandem/runs/run-123/artifacts/execute-goal.json"),
        &artifact_text,
    )
    .expect("write artifact");

    let mut session = Session::new(
        Some("resume-missing-source-read".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "read".to_string(),
                args: json!({"path":"RESUME.md"}),
                result: Some(json!("ok")),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path": ".tandem/runs/run-123/artifacts/execute-goal.json",
                    "content": artifact_text
                }),
                result: Some(json!("ok")),
                error: None,
            },
        ],
    ));

    let tool_telemetry = json!({
        "requested_tools": ["read", "write"],
        "executed_tools": ["read", "write"],
        "tool_call_counts": { "read": 1, "write": 1 }
    });
    // Upstream did not cover cover_letter.md either.
    let upstream_evidence = AutomationUpstreamEvidence {
        notion_identity_unconfirmed: false,
        external_citations_missing: false,
        read_paths: vec!["RESUME.md".to_string()],
        discovered_relevant_paths: vec!["RESUME.md".to_string(), "cover_letter.md".to_string()],
        web_research_attempted: false,
        web_research_succeeded: false,
        citation_count: 0,
        citations: Vec::new(),
    };

    let (_accepted, validation, _rejected) = validate_automation_artifact_output_with_upstream(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        None,
        "{\"status\":\"completed\"}",
        &tool_telemetry,
        None,
        Some((
            ".tandem/runs/run-123/artifacts/execute-goal.json".to_string(),
            artifact_text,
        )),
        &snapshot,
        Some(&upstream_evidence),
    );

    assert!(
        validation
            .get("unmet_requirements")
            .and_then(Value::as_array)
            .is_some_and(|items| items
                .iter()
                .any(|value| value.as_str() == Some("required_source_paths_not_read"))),
        "expected required_source_paths_not_read; validation={validation}"
    );

    let _ = std::fs::remove_dir_all(workspace_root);
}

/// WRC-01 (TAN-45): file-only delivery suppression — a node that must write a
/// specific delivery file must block when that file is never materialized, even
/// if the structured artifact itself is written.
#[test]
fn execute_goal_replay_blocks_when_must_write_file_missing() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-resume-replay-missing-delivery-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    std::fs::write(
        workspace_root.join("RESUME.md"),
        "# Resume\n\nRust engineer.\n",
    )
    .expect("write resume");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    // Must deliver the daily results file, but the run never writes it.
    let node = structured_json_artifact_node(
        "execute_goal",
        ".tandem/runs/run-123/artifacts/execute-goal.json",
        &["RESUME.md"],
        &["job_search_results_2026-04-15.md"],
    );
    let artifact_text = json!({
        "status": "completed",
        "summary": "Wrote the artifact but not the required delivery file."
    })
    .to_string();
    std::fs::create_dir_all(workspace_root.join(".tandem/runs/run-123/artifacts"))
        .expect("create artifact directory");
    std::fs::write(
        workspace_root.join(".tandem/runs/run-123/artifacts/execute-goal.json"),
        &artifact_text,
    )
    .expect("write artifact");

    let mut session = Session::new(
        Some("resume-missing-delivery".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "read".to_string(),
                args: json!({"path":"RESUME.md"}),
                result: Some(json!("ok")),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path": ".tandem/runs/run-123/artifacts/execute-goal.json",
                    "content": artifact_text
                }),
                result: Some(json!("ok")),
                error: None,
            },
        ],
    ));

    let tool_telemetry = json!({
        "requested_tools": ["read", "write"],
        "executed_tools": ["read", "write"],
        "tool_call_counts": { "read": 1, "write": 1 }
    });
    let upstream_evidence = AutomationUpstreamEvidence {
        notion_identity_unconfirmed: false,
        external_citations_missing: false,
        read_paths: vec!["RESUME.md".to_string()],
        discovered_relevant_paths: vec!["RESUME.md".to_string()],
        web_research_attempted: false,
        web_research_succeeded: false,
        citation_count: 0,
        citations: Vec::new(),
    };

    let (_accepted, validation, _rejected) = validate_automation_artifact_output_with_upstream(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        None,
        "{\"status\":\"completed\"}",
        &tool_telemetry,
        None,
        Some((
            ".tandem/runs/run-123/artifacts/execute-goal.json".to_string(),
            artifact_text,
        )),
        &snapshot,
        Some(&upstream_evidence),
    );

    assert!(
        validation
            .get("unmet_requirements")
            .and_then(Value::as_array)
            .is_some_and(|items| items
                .iter()
                .any(|value| value.as_str() == Some("required_workspace_files_missing"))),
        "expected required_workspace_files_missing; validation={validation}"
    );

    let _ = std::fs::remove_dir_all(workspace_root);
}
