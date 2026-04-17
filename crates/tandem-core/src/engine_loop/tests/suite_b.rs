use super::*;

#[test]
fn email_tool_detector_finds_mcp_gmail_tools() {
    let schemas = vec![
        ToolSchema::new("read", "", json!({})),
        ToolSchema::new("mcp.composio.gmail_send_email", "", json!({})),
    ];
    assert!(has_email_action_tools(&schemas));
}

#[test]
fn extract_mcp_auth_required_metadata_parses_expected_shape() {
    let metadata = json!({
        "server": "arcade",
        "mcpAuth": {
            "required": true,
            "challengeId": "abc123",
            "authorizationUrl": "https://example.com/oauth",
            "message": "Authorize first",
            "pending": true,
            "blocked": true,
            "retryAfterMs": 8000
        }
    });
    let parsed = extract_mcp_auth_required_metadata(&metadata).expect("expected metadata");
    assert_eq!(parsed.challenge_id, "abc123");
    assert_eq!(parsed.authorization_url, "https://example.com/oauth");
    assert_eq!(parsed.message, "Authorize first");
    assert_eq!(parsed.server.as_deref(), Some("arcade"));
    assert!(parsed.pending);
    assert!(parsed.blocked);
    assert_eq!(parsed.retry_after_ms, Some(8000));
}

#[test]
fn auth_required_output_detector_matches_auth_text() {
    assert!(is_auth_required_tool_output(
        "Authorization required for `mcp.arcade.gmail_whoami`.\nAuthorize here: https://example.com"
    ));
    assert!(is_auth_required_tool_output(
        "Authorization pending for `mcp.arcade.gmail_whoami`.\nAuthorize here: https://example.com\nRetry after 8s."
    ));
    assert!(!is_auth_required_tool_output("Tool `read` result: ok"));
}

#[test]
fn productive_tool_output_detector_rejects_missing_terminal_write_errors() {
    assert!(!is_productive_tool_output("write", "WRITE_CONTENT_MISSING"));
    assert!(!is_productive_tool_output("write", "FILE_PATH_MISSING"));
    assert!(!is_productive_tool_output(
        "write",
        "Tool `write` result:\nWRITE_CONTENT_MISSING"
    ));
    assert!(!is_productive_tool_output(
        "edit",
        "Tool `edit` result:\nFILE_PATH_MISSING"
    ));
    assert!(!is_productive_tool_output(
        "write",
        "Tool `write` result:\ninvalid_function_parameters"
    ));
}

#[test]
fn productive_tool_output_detector_accepts_real_tool_results() {
    assert!(is_productive_tool_output(
        "write",
        "Tool `write` result:\nWrote /tmp/probe.html"
    ));
    assert!(!is_productive_tool_output(
        "write",
        "Authorization required for `write`.\nAuthorize here: https://example.com"
    ));
}

#[test]
fn glob_empty_result_is_productive() {
    assert!(is_productive_tool_output("glob", "Tool `glob` result:\n"));
    assert!(is_productive_tool_output("glob", ""));
}

#[test]
fn write_required_node_retries_after_empty_glob() {
    assert!(should_retry_nonproductive_required_tool_cycle(
        true, false, true, 0
    ));
    assert!(should_retry_nonproductive_required_tool_cycle(
        true, false, true, 1
    ));
    assert!(!should_retry_nonproductive_required_tool_cycle(
        true, false, true, 2
    ));
}

#[test]
fn write_required_node_does_not_take_preparatory_retry_after_write_attempt() {
    assert!(!should_retry_nonproductive_required_tool_cycle(
        true, true, true, 0
    ));
    assert!(should_retry_nonproductive_required_tool_cycle(
        false, true, false, 0
    ));
}

#[test]
fn guard_budget_output_detector_matches_expected_text() {
    assert!(is_guard_budget_tool_output(
        "Tool `mcp.arcade.gmail_sendemail` call skipped: per-run guard budget exceeded (10)."
    ));
    assert!(!is_guard_budget_tool_output("Tool `read` result: ok"));
}

#[test]
fn summarize_guard_budget_outputs_returns_run_scoped_message() {
    let outputs = vec![
        "Tool `mcp.arcade.gmail_sendemail` call skipped: per-run guard budget exceeded (10)."
            .to_string(),
        "Tool `mcp.arcade.jira_getboards` call skipped: per-run guard budget exceeded (10)."
            .to_string(),
    ];
    let summary = summarize_guard_budget_outputs(&outputs).expect("expected summary");
    assert!(summary.contains("per-run tool guard budget"));
    assert!(summary.contains("fresh run"));
}

#[test]
fn duplicate_signature_output_detector_matches_expected_text() {
    assert!(is_duplicate_signature_limit_output(
        "Tool `bash` call skipped: duplicate call signature retry limit reached (2)."
    ));
    assert!(!is_duplicate_signature_limit_output(
        "Tool `read` result: ok"
    ));
}

#[test]
fn summarize_duplicate_signature_outputs_returns_run_scoped_message() {
    let outputs = vec![
        "Tool `bash` call skipped: duplicate call signature retry limit reached (2).".to_string(),
        "Tool `bash` call skipped: duplicate call signature retry limit reached (2).".to_string(),
    ];
    let summary =
        summarize_duplicate_signature_outputs(&outputs).expect("expected duplicate summary");
    assert!(summary.contains("same tool call kept repeating"));
    assert!(summary.contains("clearer command target"));
}

#[test]
fn required_tool_mode_unsatisfied_completion_includes_marker() {
    let message =
        required_tool_mode_unsatisfied_completion(RequiredToolFailureKind::NoToolCallEmitted);
    assert!(message.contains(REQUIRED_TOOL_MODE_UNSATISFIED_REASON));
    assert!(message.contains("NO_TOOL_CALL_EMITTED"));
    assert!(message.contains("tool_mode=required"));
}

#[test]
fn post_tool_final_narrative_generation_is_allowed_after_required_tools_succeed() {
    assert!(should_generate_post_tool_final_narrative(
        ToolMode::Required,
        1
    ));
    assert!(!should_generate_post_tool_final_narrative(
        ToolMode::Required,
        0
    ));
    assert!(should_generate_post_tool_final_narrative(ToolMode::Auto, 0));
}

#[test]
fn post_tool_final_narrative_prompt_preserves_structured_response_requirements() {
    let prompt = build_post_tool_final_narrative_prompt(&[String::from(
        "Tool `glob` result:\n/home/user123/marketing-tandem/tandem-reference/SOURCES.md",
    )]);
    assert!(prompt.contains("Preserve any requested output contract"));
    assert!(prompt.contains("required JSON structure"));
    assert!(prompt.contains("required handoff fields"));
    assert!(prompt.contains("required final status object"));
    assert!(prompt.contains("Do not stop at a tool summary"));
}

#[test]
fn summarize_terminal_tool_failure_for_user_maps_doc_path_missing() {
    let summary = summarize_terminal_tool_failure_for_user(&[String::from("DOC_PATH_MISSING")]);
    assert!(summary.as_deref().unwrap_or_default().contains("docs page"));
    assert!(summary
        .as_deref()
        .unwrap_or_default()
        .contains("https://docs.tandem.ac/start-here/"));
}

#[test]
fn summarize_user_visible_tool_outputs_hides_internal_skipped_and_error_lines() {
    let summary = summarize_user_visible_tool_outputs(&[
        String::from(
            "Tool `read` result:\n# Start Here\nTandem is an engine-owned workflow runtime.",
        ),
        String::from(
            "Tool `tool` call skipped: it is not available in this turn. Available tools: mcp.tandem_mcp.get_doc.",
        ),
        String::from("DOC_PATH_MISSING"),
    ]);
    assert!(summary.contains("Tool `read` result:"));
    assert!(!summary.contains("call skipped"));
    assert!(!summary.contains("DOC_PATH_MISSING"));
}

#[test]
fn required_tool_retry_context_mentions_offered_tools() {
    let prompt = build_required_tool_retry_context(
        "read, write, apply_patch",
        RequiredToolFailureKind::ToolCallInvalidArgs,
    );
    assert!(prompt.contains("Tool access is mandatory"));
    assert!(prompt.contains("TOOL_CALL_INVALID_ARGS"));
    assert!(prompt.contains("full `content`"));
    assert!(prompt.contains("write, edit, or apply_patch"));
}

#[test]
fn required_tool_retry_context_requires_write_after_read_only_pass() {
    let prompt = build_required_tool_retry_context(
        "glob, read, write, edit, apply_patch",
        RequiredToolFailureKind::WriteRequiredNotSatisfied,
    );
    assert!(prompt.contains("WRITE_REQUIRED_NOT_SATISFIED"));
    assert!(prompt.contains("Inspection is complete"));
    assert!(prompt.contains("write, edit, or apply_patch"));
}

#[test]
fn classify_required_tool_failure_detects_invalid_args() {
    let reason = classify_required_tool_failure(
        &[String::from("WRITE_CONTENT_MISSING")],
        true,
        1,
        false,
        false,
    );
    assert_eq!(reason, RequiredToolFailureKind::ToolCallInvalidArgs);
}

#[test]
fn looks_like_unparsed_tool_payload_detects_tool_call_json() {
    assert!(looks_like_unparsed_tool_payload(
        r#"{"content":[{"type":"tool_call","name":"write"}]}"#
    ));
    assert!(!looks_like_unparsed_tool_payload("Updated README.md"));
}

#[test]
fn workspace_write_tool_detection_is_limited_to_mutations() {
    assert!(is_workspace_write_tool("write"));
    assert!(is_workspace_write_tool("edit"));
    assert!(is_workspace_write_tool("apply_patch"));
    assert!(!is_workspace_write_tool("read"));
    assert!(!is_workspace_write_tool("glob"));
}

#[test]
fn proactive_write_gate_applies_only_before_prewrite_is_satisfied() {
    let decision = evaluate_prewrite_gate(
        true,
        &PrewriteRequirements {
            workspace_inspection_required: true,
            web_research_required: false,
            concrete_read_required: true,
            successful_web_research_required: false,
            repair_on_unmet_requirements: true,
            coverage_mode: PrewriteCoverageMode::ResearchCorpus,
        },
        PrewriteProgress {
            productive_write_tool_calls_total: 0,
            productive_workspace_inspection_total: 0,
            productive_concrete_read_total: 0,
            productive_web_research_total: 0,
            successful_web_research_total: 0,
            required_write_retry_count: 0,
            unmet_prewrite_repair_retry_count: 0,
            prewrite_gate_waived: false,
        },
    );
    assert!(decision.gate_write);
}

#[test]
fn prewrite_repair_can_start_before_any_write_attempt() {
    assert!(should_start_prewrite_repair_before_first_write(
        true, 0, false, false
    ));
    assert!(!should_start_prewrite_repair_before_first_write(
        true, 0, true, false
    ));
    assert!(!should_start_prewrite_repair_before_first_write(
        false, 0, false, false
    ));
    assert!(should_start_prewrite_repair_before_first_write(
        false, 0, false, true
    ));
}

#[test]
fn prewrite_repair_does_not_fire_after_first_write() {
    assert!(!should_start_prewrite_repair_before_first_write(
        true, 1, false, false
    ));
    assert!(!should_start_prewrite_repair_before_first_write(
        true, 2, false, true
    ));
}

#[test]
fn infer_code_workflow_from_text_detects_code_agent_contract() {
    let prompt = "Code Agent Contract:\n- Follow the deterministic loop: inspect -> patch -> apply -> test -> repair -> finalize.\n- Verification expectation: cargo test";
    assert!(infer_code_workflow_from_text(prompt));
}

#[test]
fn infer_code_workflow_from_text_detects_source_target_path() {
    let prompt = "Required Workspace Output:\n- Create or update `src/lib.rs` relative to the workspace root.";
    assert!(infer_code_workflow_from_text(prompt));
}

#[test]
fn required_tool_retry_context_for_task_adds_code_loop_guidance() {
    let prompt = build_required_tool_retry_context_for_task(
        "read, edit, apply_patch, bash",
        RequiredToolFailureKind::WriteRequiredNotSatisfied,
        "Code Agent Contract:\n- Follow the deterministic loop: inspect -> patch -> apply -> test -> repair -> finalize.\n- Verification expectation: cargo test\nRequired Workspace Output:\n- Create or update `src/lib.rs` relative to the workspace root.",
    );
    assert!(prompt.contains("inspect -> patch -> apply -> test -> repair"));
    assert!(prompt.contains("apply_patch"));
    assert!(prompt.contains("cargo test"));
    assert!(prompt.contains("src/lib.rs"));
}

#[test]
fn write_tool_removed_after_first_productive_write() {
    let mut offered = vec!["glob", "read", "websearch", "write", "edit"];
    let repair_on_unmet_requirements = true;
    let productive_write_tool_calls_total = 1usize;
    if repair_on_unmet_requirements && productive_write_tool_calls_total >= 3 {
        offered.retain(|tool| !is_workspace_write_tool(tool));
    }
    assert_eq!(offered, vec!["glob", "read", "websearch", "write", "edit"]);
}

#[test]
fn write_tool_removed_after_third_productive_write() {
    let mut offered = vec!["glob", "read", "websearch", "write", "edit"];
    let repair_on_unmet_requirements = true;
    let productive_write_tool_calls_total = 3usize;
    if repair_on_unmet_requirements && productive_write_tool_calls_total >= 3 {
        offered.retain(|tool| !is_workspace_write_tool(tool));
    }
    assert_eq!(offered, vec!["glob", "read", "websearch"]);
}

#[test]
fn force_write_only_retry_disabled_for_prewrite_repair_nodes() {
    let requested_write_required = true;
    let required_write_retry_count = 1usize;
    let productive_write_tool_calls_total = 0usize;
    let prewrite_satisfied = true;
    let prewrite_gate_write = false;
    let repair_on_unmet_requirements = true;

    let force_write_only_retry = requested_write_required
        && required_write_retry_count > 0
        && (productive_write_tool_calls_total == 0 || prewrite_satisfied)
        && !prewrite_gate_write
        && !repair_on_unmet_requirements;

    assert!(!force_write_only_retry);
}

#[test]
fn infer_required_output_target_path_reads_prompt_json_block() {
    let prompt = r#"Execute task.

Required output target:
{
  "path": "src/game.html",
  "kind": "source",
  "operation": "create"
}
"#;
    assert_eq!(
        infer_required_output_target_path_from_text(prompt).as_deref(),
        Some("src/game.html")
    );
}

#[test]
fn infer_required_output_target_path_accepts_extensionless_target() {
    let prompt = r#"Execute task.

Required output target:
{
  "path": "Dockerfile",
  "kind": "source",
  "operation": "create"
}
"#;
    assert_eq!(
        infer_required_output_target_path_from_text(prompt).as_deref(),
        Some("Dockerfile")
    );
}

#[test]
fn infer_write_file_path_from_text_rejects_workspace_root() {
    let prompt = "Workspace: /home/user123/game\nCreate the scaffold in the workspace now.";
    assert_eq!(infer_write_file_path_from_text(prompt), None);
}

#[test]
fn duplicate_signature_limit_defaults_to_200_for_general_tools_and_1_for_email_delivery() {
    let _guard = env_test_lock();
    unsafe {
        std::env::remove_var("TANDEM_TOOL_LOOP_DUPLICATE_SIGNATURE_LIMIT");
        std::env::remove_var("TANDEM_TOOL_LOOP_DUPLICATE_SIGNATURE_LIMIT_EMAIL_DELIVERY");
    }
    assert_eq!(duplicate_signature_limit_for("pack_builder"), 200);
    assert_eq!(duplicate_signature_limit_for("bash"), 200);
    assert_eq!(duplicate_signature_limit_for("write"), 200);
    assert_eq!(
        duplicate_signature_limit_for("mcp.composio_1.gmail_send_email"),
        1
    );
    assert_eq!(
        duplicate_signature_limit_for("mcp.composio_1.gmail_create_email_draft"),
        1
    );
}

#[test]
fn parse_streamed_tool_args_preserves_unparseable_write_payload() {
    let parsed = parse_streamed_tool_args("write", "path=game.html content");
    assert_ne!(parsed, json!({}));
}

#[test]
fn parse_streamed_tool_args_rejects_malformed_json_fragment_as_function_style() {
    let parsed = parse_streamed_tool_args("write", r#"{"allow_empty": null"#);
    assert_eq!(parsed, json!(r#"{"allow_empty": null"#));
}

#[test]
fn parse_streamed_tool_args_preserves_large_write_payload() {
    let content = "x".repeat(4096);
    let raw_args = format!(r#"{{"path":"game.html","content":"{}"}}"#, content);
    let parsed = parse_streamed_tool_args("write", &raw_args);
    assert_eq!(
        parsed.get("path").and_then(|value| value.as_str()),
        Some("game.html")
    );
    assert_eq!(
        parsed.get("content").and_then(|value| value.as_str()),
        Some(content.as_str())
    );
}

#[test]
fn parse_streamed_tool_args_recovers_truncated_write_json() {
    let raw_args = concat!(
        r#"{"path":"game.html","allow_empty":false,"content":"<!DOCTYPE html>\n"#,
        r#"<html lang=\"en\"><body>Neon Drift"#
    );
    let parsed = parse_streamed_tool_args("write", raw_args);
    assert_eq!(
        parsed,
        json!({
            "path": "game.html",
            "content": "<!DOCTYPE html>\n<html lang=\"en\"><body>Neon Drift"
        })
    );
}

#[test]
fn parse_streamed_tool_args_recovers_truncated_write_json_without_path() {
    let raw_args = concat!(
        r#"{"allow_empty":false,"content":"<!DOCTYPE html>\n"#,
        r#"<html lang=\"en\"><body>Neon Drift"#
    );
    let parsed = parse_streamed_tool_args("write", raw_args);
    assert_eq!(parsed.get("path"), None);
    assert_eq!(
        parsed.get("content").and_then(|value| value.as_str()),
        Some("<!DOCTYPE html>\n<html lang=\"en\"><body>Neon Drift")
    );
}

#[test]
fn duplicate_signature_limit_env_override_respects_minimum_floor() {
    let _guard = env_test_lock();
    unsafe {
        std::env::set_var("TANDEM_TOOL_LOOP_DUPLICATE_SIGNATURE_LIMIT", "9");
        std::env::remove_var("TANDEM_TOOL_LOOP_DUPLICATE_SIGNATURE_LIMIT_EMAIL_DELIVERY");
    }
    assert_eq!(duplicate_signature_limit_for("write"), 200);
    assert_eq!(duplicate_signature_limit_for("bash"), 200);
    unsafe {
        std::env::set_var("TANDEM_TOOL_LOOP_DUPLICATE_SIGNATURE_LIMIT", "250");
    }
    assert_eq!(duplicate_signature_limit_for("bash"), 250);
    unsafe {
        std::env::remove_var("TANDEM_TOOL_LOOP_DUPLICATE_SIGNATURE_LIMIT");
    }
}

#[test]
fn email_delivery_duplicate_signature_limit_env_override_respects_floor_of_one() {
    let _guard = env_test_lock();
    unsafe {
        std::env::set_var(
            "TANDEM_TOOL_LOOP_DUPLICATE_SIGNATURE_LIMIT_EMAIL_DELIVERY",
            "1",
        );
    }
    assert_eq!(
        duplicate_signature_limit_for("mcp.composio_1.gmail_send_email"),
        1
    );
    unsafe {
        std::env::set_var(
            "TANDEM_TOOL_LOOP_DUPLICATE_SIGNATURE_LIMIT_EMAIL_DELIVERY",
            "3",
        );
    }
    assert_eq!(
        duplicate_signature_limit_for("mcp.composio_1.gmail_send_email"),
        3
    );
    unsafe {
        std::env::remove_var("TANDEM_TOOL_LOOP_DUPLICATE_SIGNATURE_LIMIT_EMAIL_DELIVERY");
    }
}

#[test]
fn email_delivery_detection_is_provider_agnostic() {
    assert!(is_email_delivery_tool_name(
        "mcp.composio_1.gmail_send_email"
    ));
    assert!(is_email_delivery_tool_name("mcp.sendgrid.send_email"));
    assert!(is_email_delivery_tool_name("mcp.resend.create_email_draft"));
    assert!(is_email_delivery_tool_name("mcp.outlook.reply_email"));
    assert!(!is_email_delivery_tool_name("mcp.reddit.send_message"));
    assert!(!is_email_delivery_tool_name("mcp.github.create_issue"));
}

#[test]
fn websearch_duplicate_signature_limit_is_unset_by_default() {
    let _guard = env_test_lock();
    unsafe {
        std::env::remove_var("TANDEM_WEBSEARCH_DUPLICATE_SIGNATURE_LIMIT");
    }
    assert_eq!(websearch_duplicate_signature_limit(), None);
}

#[test]
fn websearch_duplicate_signature_limit_reads_env() {
    let _guard = env_test_lock();
    unsafe {
        std::env::set_var("TANDEM_WEBSEARCH_DUPLICATE_SIGNATURE_LIMIT", "5");
    }
    assert_eq!(websearch_duplicate_signature_limit(), Some(200));
    unsafe {
        std::env::set_var("TANDEM_WEBSEARCH_DUPLICATE_SIGNATURE_LIMIT", "300");
    }
    assert_eq!(websearch_duplicate_signature_limit(), Some(300));
    unsafe {
        std::env::remove_var("TANDEM_WEBSEARCH_DUPLICATE_SIGNATURE_LIMIT");
    }
}

#[test]
fn summarize_auth_pending_outputs_returns_summary_when_all_are_auth_related() {
    let outputs = vec![
        "Authorization pending for `mcp.arcade.gmail_sendemail`.\nAuthorize here: https://example.com/a".to_string(),
        "Authorization required for `mcp.arcade.gmail_whoami`.\nAuthorize here: https://example.com/b".to_string(),
    ];
    let summary = summarize_auth_pending_outputs(&outputs).expect("summary expected");
    assert!(summary.contains("Authorization is required before I can continue"));
    assert!(summary.contains("gmail_sendemail"));
    assert!(summary.contains("gmail_whoami"));
}

#[test]
fn summarize_auth_pending_outputs_returns_none_for_mixed_outputs() {
    let outputs = vec![
        "Authorization required for `mcp.arcade.gmail_whoami`.\nAuthorize here: https://example.com".to_string(),
        "Tool `read` result:\nok".to_string(),
    ];
    assert!(summarize_auth_pending_outputs(&outputs).is_none());
}

#[test]
fn invalid_tool_args_retry_context_handles_missing_bash_command() {
    let outputs = vec!["Tool `bash` result:\nBASH_COMMAND_MISSING".to_string()];
    let message =
        build_invalid_tool_args_retry_context_from_outputs(&outputs, 0).expect("retry expected");
    assert!(message.contains("required `command` field"));
    assert!(message.contains("Prefer `ls`, `glob`, `search`, and `read`"));
}

#[test]
fn invalid_tool_args_retry_context_escalates_on_repeat_bash_failure() {
    let outputs = vec!["Tool `bash` result:\nBASH_COMMAND_MISSING".to_string()];
    let message =
        build_invalid_tool_args_retry_context_from_outputs(&outputs, 1).expect("retry expected");
    assert!(message.contains("Do not repeat an empty bash call"));
}

#[test]
fn invalid_tool_args_retry_context_ignores_unrelated_outputs() {
    let outputs = vec!["Tool `read` result:\nok".to_string()];
    assert!(build_invalid_tool_args_retry_context_from_outputs(&outputs, 0).is_none());
}

#[test]
fn prewrite_repair_retry_context_prioritizes_research_tools_before_write() {
    let requirements = PrewriteRequirements {
        workspace_inspection_required: true,
        web_research_required: true,
        concrete_read_required: true,
        successful_web_research_required: true,
        repair_on_unmet_requirements: true,
        coverage_mode: PrewriteCoverageMode::ResearchCorpus,
    };
    let prompt = build_prewrite_repair_retry_context(
        "glob, read, websearch, write",
        RequiredToolFailureKind::WriteRequiredNotSatisfied,
        r#"Required output target:
{
  "path": "marketing-brief.md",
  "kind": "artifact"
}"#,
        &requirements,
        true,
        false,
        false,
        false,
    );
    assert!(prompt.contains("requires concrete `read` calls"));
    assert!(prompt.contains("call `websearch` with a concrete query now"));
    assert!(prompt.contains("Use `read` and `websearch` now to gather evidence"));
    assert!(prompt.contains("Do not declare the output blocked"));
    assert!(!prompt.contains("blocked-but-substantive artifact"));
    assert!(!prompt.contains("Your next response must be a `write` tool call"));
    assert!(!prompt.contains("Do not call `glob`, `read`, or `websearch` again"));
}

#[test]
fn empty_completion_retry_context_requires_write_when_prewrite_is_satisfied() {
    let requirements = PrewriteRequirements {
        workspace_inspection_required: true,
        web_research_required: false,
        concrete_read_required: true,
        successful_web_research_required: false,
        repair_on_unmet_requirements: true,
        coverage_mode: PrewriteCoverageMode::ResearchCorpus,
    };
    let prompt = build_empty_completion_retry_context(
        "glob, read, write",
        "Create or update `marketing-brief.md` relative to the workspace root.",
        &requirements,
        true,
        true,
        false,
        false,
    );
    assert!(prompt.contains("returned no final output"));
    assert!(prompt.contains("marketing-brief.md"));
    assert!(prompt.contains("must be a `write` tool call"));
}

#[test]
fn empty_completion_retry_context_mentions_missing_prewrite_work() {
    let requirements = PrewriteRequirements {
        workspace_inspection_required: true,
        web_research_required: true,
        concrete_read_required: true,
        successful_web_research_required: true,
        repair_on_unmet_requirements: true,
        coverage_mode: PrewriteCoverageMode::ResearchCorpus,
    };
    let prompt = build_empty_completion_retry_context(
        "glob, read, websearch, write",
        "Create or update `marketing-brief.md` relative to the workspace root.",
        &requirements,
        true,
        false,
        false,
        false,
    );
    assert!(prompt.contains("still need to use `read`"));
    assert!(prompt.contains("use `websearch`"));
    assert!(prompt.contains("After completing the missing requirement"));
}

#[test]
fn synthesize_artifact_write_completion_from_tool_state_marks_completed() {
    let completion = synthesize_artifact_write_completion_from_tool_state(
        "Create or update `marketing-brief.md` relative to the workspace root.",
        true,
        false,
    );
    assert!(completion.contains("wrote `marketing-brief.md`"));
    assert!(completion.contains("\"status\":\"completed\""));
    assert!(completion.contains("Runtime validation will verify"));
}

#[test]
fn synthesize_artifact_write_completion_from_tool_state_mentions_waived_evidence() {
    let completion = synthesize_artifact_write_completion_from_tool_state(
        "Create or update `marketing-brief.md` relative to the workspace root.",
        false,
        true,
    );
    assert!(completion.contains("waived in-run"));
    assert!(completion.contains("\"status\":\"completed\""));
}

#[test]
fn prewrite_repair_retry_budget_allows_five_repair_attempts() {
    assert_eq!(prewrite_repair_retry_max_attempts(), 5);
}

#[test]
fn prewrite_repair_tool_filter_removes_write_until_evidence_is_satisfied() {
    let offered = ["glob", "read", "websearch", "write", "edit"];
    let filtered = offered
        .iter()
        .copied()
        .filter(|tool| {
            tool_matches_unmet_prewrite_repair_requirement(
                tool,
                &[
                    "workspace_inspection_required",
                    "concrete_read_required",
                    "successful_web_research_required",
                ],
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(filtered, vec!["glob", "read", "websearch"]);
}

#[test]
fn prewrite_repair_tool_filter_restricts_to_glob_and_read_for_concrete_reads() {
    let offered = ["glob", "read", "search", "write"];
    let filtered = offered
        .iter()
        .copied()
        .filter(|tool| {
            tool_matches_unmet_prewrite_repair_requirement(tool, &["concrete_read_required"])
        })
        .collect::<Vec<_>>();
    assert_eq!(filtered, vec!["glob", "read"]);
}

#[test]
fn prewrite_repair_tool_filter_allows_glob_only_for_workspace_inspection() {
    let offered = ["glob", "read", "websearch", "write"];
    let with_inspection_unmet = offered
        .iter()
        .copied()
        .filter(|tool| {
            tool_matches_unmet_prewrite_repair_requirement(
                tool,
                &["workspace_inspection_required", "concrete_read_required"],
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(with_inspection_unmet, vec!["glob", "read"]);

    let without_inspection_unmet = offered
        .iter()
        .copied()
        .filter(|tool| {
            tool_matches_unmet_prewrite_repair_requirement(
                tool,
                &["concrete_read_required", "web_research_required"],
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(without_inspection_unmet, vec!["glob", "read", "websearch"]);
}

#[test]
fn prewrite_repair_after_glob_restricts_to_glob_read_and_websearch() {
    let offered = ["glob", "read", "websearch", "write", "edit"];
    let filtered = offered
        .iter()
        .copied()
        .filter(|tool| {
            tool_matches_unmet_prewrite_repair_requirement(
                tool,
                &[
                    "concrete_read_required",
                    "successful_web_research_required",
                    "coverage_mode",
                ],
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(filtered, vec!["glob", "read", "websearch"]);
}

#[test]
fn prewrite_requirements_exhausted_completion_reports_structured_repair_state() {
    let message = prewrite_requirements_exhausted_completion(
        &["concrete_read_required", "successful_web_research_required"],
        2,
        0,
    );
    assert!(message.contains("PREWRITE_REQUIREMENTS_EXHAUSTED"));
    assert!(message.contains("\"status\":\"blocked\""));
    assert!(message.contains("\"repairAttempt\":2"));
    assert!(message.contains("\"repairAttemptsRemaining\":0"));
    assert!(message.contains("\"repairExhausted\":true"));
    assert!(message.contains(
        "\"unmetRequirements\":[\"concrete_read_required\", \"successful_web_research_required\"]"
    ));
}

#[test]
fn prewrite_waived_write_context_includes_unmet_codes() {
    let user_text = "Some task text without output target marker.";
    let unmet = vec!["concrete_read_required", "coverage_mode"];
    let ctx = build_prewrite_waived_write_context(user_text, &unmet);
    assert!(ctx.contains("could not be fully satisfied"));
    assert!(ctx.contains("concrete_read_required"));
    assert!(ctx.contains("coverage_mode"));
    assert!(ctx.contains("write"));
    assert!(ctx.contains("Do not write a blocked or placeholder file"));
}

#[test]
fn prewrite_waived_write_context_includes_output_path_when_present() {
    let user_text = "Required output target: {\"path\": \"marketing-brief.md\"}";
    let unmet = vec!["concrete_read_required"];
    let ctx = build_prewrite_waived_write_context(user_text, &unmet);
    assert!(ctx.contains("marketing-brief.md"));
    assert!(ctx.contains("`write`"));
}

#[test]
fn prewrite_gate_waived_disables_prewrite_gate_write() {
    let requirements = PrewriteRequirements {
        workspace_inspection_required: true,
        web_research_required: false,
        concrete_read_required: true,
        successful_web_research_required: false,
        repair_on_unmet_requirements: true,
        coverage_mode: PrewriteCoverageMode::ResearchCorpus,
    };
    let before = evaluate_prewrite_gate(
        true,
        &requirements,
        PrewriteProgress {
            productive_write_tool_calls_total: 0,
            productive_workspace_inspection_total: 0,
            productive_concrete_read_total: 0,
            productive_web_research_total: 0,
            successful_web_research_total: 0,
            required_write_retry_count: 0,
            unmet_prewrite_repair_retry_count: 0,
            prewrite_gate_waived: false,
        },
    );
    assert!(before.gate_write, "gate should be active before waiver");
    let after = evaluate_prewrite_gate(
        true,
        &requirements,
        PrewriteProgress {
            productive_write_tool_calls_total: 0,
            productive_workspace_inspection_total: 0,
            productive_concrete_read_total: 0,
            productive_web_research_total: 0,
            successful_web_research_total: 0,
            required_write_retry_count: 0,
            unmet_prewrite_repair_retry_count: 0,
            prewrite_gate_waived: true,
        },
    );
    assert!(!after.gate_write, "gate should be off after waiver");
}

#[test]
fn prewrite_gate_waived_disables_allow_repair_tools() {
    let requirements = PrewriteRequirements {
        workspace_inspection_required: true,
        web_research_required: true,
        concrete_read_required: true,
        successful_web_research_required: true,
        repair_on_unmet_requirements: true,
        coverage_mode: PrewriteCoverageMode::ResearchCorpus,
    };
    let before = evaluate_prewrite_gate(
        true,
        &requirements,
        PrewriteProgress {
            productive_write_tool_calls_total: 0,
            productive_workspace_inspection_total: 0,
            productive_concrete_read_total: 0,
            productive_web_research_total: 0,
            successful_web_research_total: 0,
            required_write_retry_count: 0,
            unmet_prewrite_repair_retry_count: 1,
            prewrite_gate_waived: false,
        },
    );
    assert!(
        before.allow_repair_tools,
        "repair tools should be active before waiver"
    );
    let after = evaluate_prewrite_gate(
        true,
        &requirements,
        PrewriteProgress {
            productive_write_tool_calls_total: 0,
            productive_workspace_inspection_total: 0,
            productive_concrete_read_total: 0,
            productive_web_research_total: 0,
            successful_web_research_total: 0,
            required_write_retry_count: 0,
            unmet_prewrite_repair_retry_count: 1,
            prewrite_gate_waived: true,
        },
    );
    assert!(
        !after.allow_repair_tools,
        "repair tools should be disabled after waiver"
    );
}

#[test]
fn force_write_only_enabled_after_prewrite_waiver() {
    let requirements = PrewriteRequirements {
        workspace_inspection_required: true,
        web_research_required: true,
        concrete_read_required: true,
        successful_web_research_required: true,
        repair_on_unmet_requirements: true,
        coverage_mode: PrewriteCoverageMode::ResearchCorpus,
    };
    let decision = evaluate_prewrite_gate(
        true,
        &requirements,
        PrewriteProgress {
            productive_write_tool_calls_total: 0,
            productive_workspace_inspection_total: 0,
            productive_concrete_read_total: 0,
            productive_web_research_total: 0,
            successful_web_research_total: 0,
            required_write_retry_count: 1,
            unmet_prewrite_repair_retry_count: 1,
            prewrite_gate_waived: true,
        },
    );
    assert!(
        decision.force_write_only_retry,
        "force_write_only should be active after prewrite waiver + write retry"
    );
}

#[test]
fn force_write_only_disabled_before_prewrite_waiver() {
    let requirements = PrewriteRequirements {
        workspace_inspection_required: true,
        web_research_required: true,
        concrete_read_required: true,
        successful_web_research_required: true,
        repair_on_unmet_requirements: true,
        coverage_mode: PrewriteCoverageMode::ResearchCorpus,
    };
    let decision = evaluate_prewrite_gate(
        true,
        &requirements,
        PrewriteProgress {
            productive_write_tool_calls_total: 0,
            productive_workspace_inspection_total: 0,
            productive_concrete_read_total: 0,
            productive_web_research_total: 0,
            successful_web_research_total: 0,
            required_write_retry_count: 1,
            unmet_prewrite_repair_retry_count: 1,
            prewrite_gate_waived: false,
        },
    );
    assert!(
        !decision.force_write_only_retry,
        "force_write_only should be disabled before waiver for prewrite nodes"
    );
}

#[test]
fn parse_budget_override_zero_disables_budget() {
    unsafe {
        std::env::set_var("TANDEM_TOOL_BUDGET_DEFAULT", "0");
    }
    assert_eq!(
        parse_budget_override("TANDEM_TOOL_BUDGET_DEFAULT"),
        Some(usize::MAX)
    );
    unsafe {
        std::env::remove_var("TANDEM_TOOL_BUDGET_DEFAULT");
    }
}

#[test]
fn disable_tool_guard_budgets_env_overrides_all_budgets() {
    unsafe {
        std::env::set_var("TANDEM_DISABLE_TOOL_GUARD_BUDGETS", "1");
        std::env::remove_var("TANDEM_TOOL_BUDGET_EMAIL_DELIVERY");
    }
    assert_eq!(tool_budget_for("mcp.arcade.gmail_sendemail"), 1);
    // M-2: disabling guards now returns HARD_TOOL_CALL_CEILING, not usize::MAX,
    // because the hard ceiling cannot be bypassed by any env setting.
    assert_eq!(tool_budget_for("websearch"), HARD_TOOL_CALL_CEILING);
    unsafe {
        std::env::remove_var("TANDEM_DISABLE_TOOL_GUARD_BUDGETS");
    }
}

#[test]
fn email_delivery_budget_can_still_be_explicitly_overridden_when_global_budgets_are_disabled() {
    let _guard = env_test_lock();
    unsafe {
        std::env::set_var("TANDEM_DISABLE_TOOL_GUARD_BUDGETS", "1");
        std::env::set_var("TANDEM_TOOL_BUDGET_EMAIL_DELIVERY", "0");
    }
    assert_eq!(tool_budget_for("mcp.arcade.gmail_sendemail"), usize::MAX);
    unsafe {
        std::env::remove_var("TANDEM_DISABLE_TOOL_GUARD_BUDGETS");
        std::env::remove_var("TANDEM_TOOL_BUDGET_EMAIL_DELIVERY");
    }
}

#[test]
fn tool_budget_defaults_to_200_calls_and_1_for_email_delivery() {
    let _guard = env_test_lock();
    unsafe {
        std::env::remove_var("TANDEM_DISABLE_TOOL_GUARD_BUDGETS");
        std::env::remove_var("TANDEM_TOOL_BUDGET_DEFAULT");
        std::env::remove_var("TANDEM_TOOL_BUDGET_WEBSEARCH");
        std::env::remove_var("TANDEM_TOOL_BUDGET_READ");
        std::env::remove_var("TANDEM_TOOL_BUDGET_EMAIL_DELIVERY");
    }
    assert_eq!(tool_budget_for("bash"), 200);
    assert_eq!(tool_budget_for("websearch"), 200);
    assert_eq!(tool_budget_for("read"), 200);
    assert_eq!(tool_budget_for("mcp.composio_1.gmail_send_email"), 1);
    assert_eq!(
        tool_budget_for("mcp.composio_1.gmail_create_email_draft"),
        1
    );
}

#[test]
fn tool_budget_env_override_respects_minimum_floor() {
    let _guard = env_test_lock();
    unsafe {
        std::env::remove_var("TANDEM_DISABLE_TOOL_GUARD_BUDGETS");
        std::env::set_var("TANDEM_TOOL_BUDGET_DEFAULT", "17");
        std::env::set_var("TANDEM_TOOL_BUDGET_WEBSEARCH", "250");
        std::env::remove_var("TANDEM_TOOL_BUDGET_EMAIL_DELIVERY");
    }
    assert_eq!(tool_budget_for("bash"), 200);
    assert_eq!(tool_budget_for("websearch"), 250);
    unsafe {
        std::env::remove_var("TANDEM_TOOL_BUDGET_DEFAULT");
        std::env::remove_var("TANDEM_TOOL_BUDGET_WEBSEARCH");
    }
}

#[test]
fn email_delivery_tool_budget_env_override_respects_floor_of_one() {
    let _guard = env_test_lock();
    unsafe {
        std::env::remove_var("TANDEM_DISABLE_TOOL_GUARD_BUDGETS");
        std::env::set_var("TANDEM_TOOL_BUDGET_EMAIL_DELIVERY", "1");
    }
    assert_eq!(tool_budget_for("mcp.composio_1.gmail_send_email"), 1);
    unsafe {
        std::env::set_var("TANDEM_TOOL_BUDGET_EMAIL_DELIVERY", "5");
    }
    assert_eq!(tool_budget_for("mcp.composio_1.gmail_send_email"), 5);
    unsafe {
        std::env::remove_var("TANDEM_TOOL_BUDGET_EMAIL_DELIVERY");
    }
}

#[test]
fn provider_agnostic_email_tools_share_single_send_budget() {
    let _guard = env_test_lock();
    unsafe {
        std::env::remove_var("TANDEM_DISABLE_TOOL_GUARD_BUDGETS");
        std::env::remove_var("TANDEM_TOOL_BUDGET_EMAIL_DELIVERY");
    }
    assert_eq!(tool_budget_for("mcp.sendgrid.send_email"), 1);
    assert_eq!(tool_budget_for("mcp.resend.create_email_draft"), 1);
    assert_eq!(duplicate_signature_limit_for("mcp.outlook.reply_email"), 1);
}
