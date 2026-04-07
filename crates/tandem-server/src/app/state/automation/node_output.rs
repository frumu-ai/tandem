use super::*;
use crate::util::time::now_ms;
use serde_json::{json, Value};
use tandem_types::MessagePart;

/// Returns true when all user-visible standup fields consist entirely of known
/// meta-commentary phrases. Used by the `StandupUpdate` fast path to trigger
/// a repair rather than silently accepting empty-substance output.
///
/// Detection layers (applied per-field, both must be filler to reject):
/// 1. Standup-specific phrase list — catches standup-unique meta-commentary.
/// 2. Generic `placeholder_like_artifact_text()` — catches status-only markers
///    and strong placeholder patterns shared across all automation outputs.
fn standup_output_contains_only_filler(parsed: &Value) -> bool {
    // Phrases that indicate the agent described its search process instead of
    // reporting actual deliverables. Kept intentionally specific to avoid
    // false positives on legitimate updates that happen to share words.
    const FILLER_PATTERNS: &[&str] = &[
        "reviewed workspace",
        "reviewed prior project memory",
        "reviewed prior standup",
        "identified relevant",
        "approved findings",
        "evidence-limited",
        "evidence remains",
        "evidence is limited",
        "no prior work evidence",
        "cannot be expanded without",
        "prepared the standup",
        "prepare the daily standup",
        "workspace context",
        "source of truth",
        "no broader copy draft",
        "workspace artifacts and tandem",
        "based on workspace artifacts",
        "reviewing workspace",
        "standup preparation from available",
    ];
    // Both yesterday and today must be filler for the whole update to be rejected.
    // An agent that has a real "today" but filler "yesterday" is partially useful.
    let fields = ["yesterday", "today"];
    fields.iter().all(|field| {
        parsed
            .get(field)
            .and_then(Value::as_str)
            .map(|text| {
                let trimmed = text.trim();
                let lower = trimmed.to_ascii_lowercase();
                lower.is_empty()
                    || FILLER_PATTERNS.iter().any(|p| lower.contains(p))
                    || placeholder_like_artifact_text(trimmed)
            })
            .unwrap_or(true) // missing field counts as filler
    })
}

/// Builds a structured repair reason for standup filler rejection that includes
/// what the agent tried (tools used, directories searched, files read) so the
/// repair attempt has actionable context instead of just a generic message.
fn standup_filler_repair_reason(tool_telemetry: &Value) -> String {
    let tools_used = tool_telemetry
        .get("executed_tools")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "none recorded".to_string());
    let dirs_searched = tool_telemetry
        .get("glob_directories")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "none recorded".to_string());
    let files_read = tool_telemetry
        .get("read_paths")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .take(10)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "none recorded".to_string());
    format!(
        "Standup update contains only meta-commentary. \
         Your previous attempt used these tools: [{tools_used}], \
         searched directories: [{dirs_searched}], \
         and read files: [{files_read}]. \
         Report concrete file names, deliverables, or drafts found in the workspace. \
         If genuinely nothing exists, write exactly: \"No [role] deliverables found in workspace.\""
    )
}

pub(crate) fn augment_automation_attempt_evidence_with_validation(
    attempt_evidence: &Value,
    artifact_validation: Option<&Value>,
    accepted_output: Option<&(String, String)>,
    accepted_candidate_source: Option<&str>,
    blocker_category: Option<&str>,
    fallback_used: bool,
) -> Value {
    let Some(mut object) = attempt_evidence.as_object().cloned() else {
        return attempt_evidence.clone();
    };
    let mut evidence = object
        .get("evidence")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(validation) = artifact_validation {
        evidence.insert(
            "citation_count".to_string(),
            validation
                .get("citation_count")
                .cloned()
                .unwrap_or_else(|| json!(0)),
        );
        evidence.insert(
            "web_sources_reviewed_present".to_string(),
            validation
                .get("web_sources_reviewed_present")
                .cloned()
                .unwrap_or(json!(false)),
        );
        evidence.insert(
            "reviewed_paths".to_string(),
            validation
                .get("read_paths")
                .cloned()
                .unwrap_or_else(|| json!([])),
        );
    }
    object.insert("evidence".to_string(), Value::Object(evidence));
    let mut artifact = object
        .get("artifact")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some((path, text)) = accepted_output {
        artifact.insert("status".to_string(), json!("written"));
        artifact.insert("path".to_string(), json!(path));
        artifact.insert(
            "content_digest".to_string(),
            json!(crate::sha256_hex(&[text])),
        );
    }
    if let Some(source) = accepted_candidate_source {
        artifact.insert("accepted_candidate_source".to_string(), json!(source));
        if source == "session_write" || source == "preexisting_output" {
            artifact.insert("status".to_string(), json!("reused_valid"));
            artifact.insert("recovery_source".to_string(), json!(source));
        }
    }
    object.insert("artifact".to_string(), Value::Object(artifact));
    object.insert(
        "blocker_category".to_string(),
        blocker_category
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null),
    );
    object.insert("fallback_used".to_string(), json!(fallback_used));
    Value::Object(object)
}

fn automation_node_output_provenance(
    node: &AutomationFlowNode,
    session_id: &str,
    run_id: Option<&str>,
    verified_output: Option<&(String, String)>,
    artifact_validation: Option<&Value>,
) -> Option<crate::AutomationNodeOutputProvenance> {
    let current_attempt = artifact_validation
        .and_then(|value| value.get("accepted_candidate_source"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none_or(|source| source != "preexisting_output");
    Some(crate::AutomationNodeOutputProvenance {
        session_id: session_id.to_string(),
        node_id: node.node_id.clone(),
        run_id: run_id.map(str::to_string),
        output_path: verified_output.map(|(path, _)| path.clone()),
        content_digest: verified_output.map(|(_, text)| crate::sha256_hex(&[text])),
        accepted_candidate_source: artifact_validation.and_then(|validation| {
            validation
                .get("accepted_candidate_source")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        }),
        validation_outcome: artifact_validation.and_then(|validation| {
            validation
                .get("validation_outcome")
                .and_then(Value::as_str)
                .map(str::to_string)
        }),
        repair_attempt: artifact_validation
            .and_then(|validation| validation.get("repair_attempt").and_then(Value::as_u64)),
        repair_succeeded: artifact_validation
            .and_then(|validation| validation.get("repair_succeeded").and_then(Value::as_bool)),
        reuse_allowed: Some(super::automation_node_allows_preexisting_output_reuse(node)),
        freshness: crate::AutomationNodeOutputFreshness {
            current_run: run_id.is_some(),
            current_attempt,
        },
    })
}

pub(crate) fn normalize_web_research_failure_label(raw: &str) -> String {
    let lowered = raw.trim().to_ascii_lowercase();
    if lowered.contains("authorization required")
        || lowered.contains("authorization_required")
        || lowered.contains("authorize")
    {
        "web research authorization required".to_string()
    } else if lowered.contains("backend unavailable")
        || lowered.contains("backend_unavailable")
        || lowered.contains("web research unavailable")
        || lowered.contains("service unavailable")
        || lowered.contains("currently unavailable")
        || lowered.contains("connection refused")
        || lowered.contains("dns error")
        || lowered.contains("network error")
        || lowered.contains("temporarily unavailable")
    {
        "web research unavailable".to_string()
    } else if lowered.contains("timed out") || lowered.contains("timeout") {
        "web research timed out".to_string()
    } else {
        raw.trim().to_string()
    }
}

pub(crate) fn web_research_unavailable_failure(raw: &str) -> bool {
    let lowered = raw.trim().to_ascii_lowercase();
    lowered.contains("authorization required")
        || lowered.contains("authorization_required")
        || lowered.contains("authorize")
        || lowered.contains("backend unavailable")
        || lowered.contains("backend_unavailable")
        || lowered.contains("web research unavailable")
        || lowered.contains("service unavailable")
        || lowered.contains("currently unavailable")
        || lowered.contains("temporarily unavailable")
        || lowered.contains("timed out")
        || lowered.contains("timeout")
}

pub(crate) fn web_research_unavailable(latest_web_research_failure: Option<&str>) -> bool {
    latest_web_research_failure.is_some_and(web_research_unavailable_failure)
}

pub(crate) fn classify_research_validation_state(
    requested_tools: &[Value],
    executed_tools: &[Value],
    web_research_expected: bool,
    unmet_requirements: &[String],
    latest_web_research_failure: Option<&str>,
    repair_exhausted: bool,
) -> Option<&'static str> {
    if unmet_requirements.is_empty() {
        return None;
    }
    if unmet_requirements
        .iter()
        .any(|value| value == "structured_handoff_missing")
    {
        return Some("handoff_missing");
    }
    let requested_has_read = requested_tools
        .iter()
        .any(|value| value.as_str() == Some("read"));
    let requested_has_websearch = requested_tools
        .iter()
        .any(|value| value.as_str() == Some("websearch"));
    let executed_has_read = executed_tools
        .iter()
        .any(|value| value.as_str() == Some("read"));
    let executed_has_websearch = executed_tools
        .iter()
        .any(|value| value.as_str() == Some("websearch"));
    if repair_exhausted {
        return Some("coverage_incomplete_after_retry");
    }
    if web_research_expected && web_research_unavailable(latest_web_research_failure) {
        return Some("tool_unavailable");
    }
    if (!requested_has_read
        && unmet_requirements.iter().any(|value| {
            matches!(
                value.as_str(),
                "no_concrete_reads" | "concrete_read_required"
            )
        }))
        || (web_research_expected
            && !requested_has_websearch
            && unmet_requirements
                .iter()
                .any(|value| value == "missing_successful_web_research"))
    {
        return Some("tool_unavailable");
    }
    if (requested_has_read && !executed_has_read)
        || (web_research_expected && requested_has_websearch && !executed_has_websearch)
    {
        return Some("tool_available_but_not_used");
    }
    Some("tool_attempted_but_failed")
}

pub(crate) fn research_required_next_tool_actions(
    requested_tools: &[Value],
    executed_tools: &[Value],
    web_research_expected: bool,
    unmet_requirements: &[String],
    unreviewed_relevant_paths: &[String],
    latest_web_research_failure: Option<&str>,
) -> Vec<String> {
    let requested_has_read = requested_tools
        .iter()
        .any(|value| value.as_str() == Some("read"));
    let requested_has_websearch = requested_tools
        .iter()
        .any(|value| value.as_str() == Some("websearch"));
    let executed_has_read = executed_tools
        .iter()
        .any(|value| value.as_str() == Some("read"));
    let executed_has_websearch = executed_tools
        .iter()
        .any(|value| value.as_str() == Some("websearch"));
    let has_unmet = |needle: &str| unmet_requirements.iter().any(|value| value == needle);

    let mut actions = Vec::new();
    if has_unmet("structured_handoff_missing") {
        actions.push(
            "Return the required structured JSON handoff in the final response instead of ending after tool calls or tool summaries."
                .to_string(),
        );
    }
    if requested_has_read
        && (!executed_has_read
            || has_unmet("no_concrete_reads")
            || has_unmet("files_reviewed_not_backed_by_read"))
    {
        if unreviewed_relevant_paths.is_empty() {
            if has_unmet("citations_missing") || has_unmet("research_citations_missing") {
                actions.push(
                    "No additional unreviewed files detected. If citations are missing, either: (a) re-read upstream handoff sources with `read` to extract specific proof points, or (b) add explicit `Files not reviewed` section listing sources that could not be verified with reasons.".to_string(),
                );
            } else {
                actions.push(
                    "Use `read` on concrete workspace files before finalizing the brief."
                        .to_string(),
                );
            }
        } else {
            actions.push(format!(
                "Use `read` on the remaining relevant workspace files: {}.",
                unreviewed_relevant_paths.join(", ")
            ));
            actions.push(
                "If any discovered file is not relevant to the brief's claims, add it to the `Files not reviewed` section with a brief reason (e.g., 'not applicable to positioning'). Use exact paths.".to_string(),
            );
        }
    }
    if requested_has_websearch
        && web_research_expected
        && (!executed_has_websearch
            || has_unmet("missing_successful_web_research")
            || has_unmet("web_sources_reviewed_missing"))
    {
        if web_research_unavailable(latest_web_research_failure) {
            actions.push(
                "Skip `websearch` for this run because external research is unavailable. Continue with local file reads and note that web research could not be completed."
                    .to_string(),
            );
        } else {
            actions.push(
                "Use `websearch` successfully and include the resulting sources in `Web sources reviewed`."
                    .to_string(),
            );
        }
    }
    if has_unmet("citations_missing") {
        actions.push(
            "Add citation-backed proof points instead of unsupported claims before writing the final brief."
                .to_string(),
        );
    }
    if has_unmet("files_reviewed_missing") {
        actions.push(
            "Include a `Files reviewed` section that lists the exact local paths you actually read in this run."
                .to_string(),
        );
    }
    if has_unmet("relevant_files_not_reviewed_or_skipped") {
        actions.push(
            "Move every discovered relevant file into either `Files reviewed` after `read`, or `Files not reviewed` with a reason. Use only exact concrete workspace-relative file paths; do not use directories or glob patterns."
                .to_string(),
        );
    }
    actions
}

pub(crate) fn build_automation_attempt_evidence(
    node: &AutomationFlowNode,
    attempt: u32,
    session: &Session,
    session_id: &str,
    workspace_root: &str,
    tool_telemetry: &Value,
    preflight: &Value,
    capability_resolution: &Value,
    verified_output: Option<&(String, String)>,
) -> Value {
    let mut attempted_tools = Vec::new();
    let mut succeeded_tools = Vec::new();
    let mut failed_tools = Vec::new();
    let mut normalized_failures = serde_json::Map::new();
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool,
                error,
                result,
                ..
            } = part
            else {
                continue;
            };
            let normalized = tool.trim().to_ascii_lowercase().replace('-', "_");
            if !attempted_tools.iter().any(|value| value == &normalized) {
                attempted_tools.push(normalized.clone());
            }
            if error.as_ref().is_some_and(|value| !value.trim().is_empty()) {
                if !failed_tools.iter().any(|value| value == &normalized) {
                    failed_tools.push(normalized.clone());
                }
                normalized_failures.insert(
                    normalized.clone(),
                    json!(normalize_web_research_failure_label(
                        error.as_deref().unwrap_or_default()
                    )),
                );
                continue;
            }
            if automation_tool_result_output_value(result.as_ref()).is_some() {
                if !succeeded_tools.iter().any(|value| value == &normalized) {
                    succeeded_tools.push(normalized.clone());
                }
            }
        }
    }
    let read_paths = session_read_paths(session, workspace_root);
    let discovered_paths = session_discovered_relevant_paths(session, workspace_root);
    let web_research_status = automation_attempt_evidence_web_research_status(tool_telemetry)
        .unwrap_or_else(|| {
            if tool_telemetry
                .get("web_research_succeeded")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                "succeeded".to_string()
            } else if tool_telemetry
                .get("web_research_used")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                let failure = tool_telemetry
                    .get("latest_web_research_failure")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if web_research_unavailable_failure(&failure) {
                    "unavailable".to_string()
                } else if failure.contains("timed out") {
                    "timed_out".to_string()
                } else {
                    "unusable".to_string()
                }
            } else if automation_node_web_research_expected(node) {
                "not_attempted".to_string()
            } else {
                "not_required".to_string()
            }
        });
    let delivery_status = automation_attempt_evidence_delivery_status(tool_telemetry)
        .unwrap_or_else(|| {
            if automation_node_requires_email_delivery(node) {
                if tool_telemetry
                    .get("email_delivery_succeeded")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    "succeeded".to_string()
                } else if tool_telemetry
                    .get("email_delivery_attempted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    "attempted_failed".to_string()
                } else {
                    "not_attempted".to_string()
                }
            } else {
                "not_required".to_string()
            }
        });
    let artifact_status = if let Some((path, text)) = verified_output {
        json!({
            "status": "written",
            "path": path,
            "content_digest": crate::sha256_hex(&[text]),
        })
    } else if automation_node_required_output_path(node).is_some() {
        json!({
            "status": "missing",
            "path": automation_node_required_output_path(node),
        })
    } else {
        json!({
            "status": "not_required"
        })
    };
    json!({
        "attempt": attempt,
        "created_at_ms": now_ms(),
        "session_id": session_id,
        "preflight": preflight,
        "capability_resolution": capability_resolution,
        "tool_execution": {
            "attempted_tools": attempted_tools,
            "succeeded_tools": succeeded_tools,
            "failed_tools": failed_tools,
            "normalized_failures": normalized_failures,
            "tool_call_counts": tool_telemetry.get("tool_call_counts").cloned().unwrap_or_else(|| json!({})),
        },
        "evidence": {
            "read_paths": read_paths,
            "discovered_paths": discovered_paths,
            "web_research": {
                "status": web_research_status,
                "latest_failure": tool_telemetry.get("latest_web_research_failure").cloned().unwrap_or(Value::Null),
                "used": tool_telemetry.get("web_research_used").cloned().unwrap_or(json!(false)),
                "succeeded": tool_telemetry.get("web_research_succeeded").cloned().unwrap_or(json!(false)),
            },
        },
        "delivery": {
            "method": automation_node_delivery_method_value(node),
            "recipient": automation_node_delivery_target(node),
            "status": delivery_status,
            "attempted": tool_telemetry.get("email_delivery_attempted").cloned().unwrap_or(json!(false)),
            "succeeded": tool_telemetry.get("email_delivery_succeeded").cloned().unwrap_or(json!(false)),
            "latest_failure": tool_telemetry.get("latest_email_delivery_failure").cloned().unwrap_or(Value::Null),
        },
        "artifact": artifact_status,
    })
}

pub(crate) fn automation_output_validated_artifact(output: &Value) -> Option<(String, String)> {
    let evidence = output.get("attempt_evidence")?;
    let artifact = evidence.get("artifact")?;
    let status = artifact.get("status")?.as_str()?;
    if status == "written" || status == "reused_valid" {
        let path = artifact.get("path")?.as_str()?.to_string();
        let digest = artifact.get("content_digest")?.as_str()?.to_string();
        Some((path, digest))
    } else {
        None
    }
}

pub(crate) fn detect_automation_node_status(
    node: &AutomationFlowNode,
    session_text: &str,
    verified_output: Option<&(String, String)>,
    tool_telemetry: &Value,
    artifact_validation: Option<&Value>,
) -> (String, Option<String>, Option<bool>) {
    let research_repair_exhausted = artifact_validation
        .and_then(|value| value.get("repair_exhausted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let validator_kind = automation_output_validator_kind(node);

    // --- StandupUpdate fast path ---
    // Standup participants bypass all text-marker matching, approval-gate logic,
    // and research-brief validation. They produce structured JSON with three keys;
    // anything else triggers a targeted repair signal.
    if validator_kind == crate::AutomationOutputValidatorKind::StandupUpdate {
        let parsed = parse_status_json(session_text);
        let has_required_keys = parsed
            .as_ref()
            .is_some_and(|v| v.get("yesterday").is_some() && v.get("today").is_some());
        if has_required_keys {
            let explicit_reason = parsed
                .as_ref()
                .and_then(|v| v.get("reason"))
                .and_then(Value::as_str)
                .map(str::to_string);
            // Filler rejection: if every user-visible field is meta-commentary,
            // trigger a repair so the agent tries again with the repair hint.
            // The repair reason includes structured tool telemetry so the agent
            // knows what it already tried and can adjust its search strategy.
            if standup_output_contains_only_filler(parsed.as_ref().unwrap()) {
                return (
                    if research_repair_exhausted {
                        "blocked".to_string()
                    } else {
                        "needs_repair".to_string()
                    },
                    Some(standup_filler_repair_reason(tool_telemetry)),
                    None,
                );
            }
            return ("completed".to_string(), explicit_reason, None);
        }
        // Missing required keys — clear repair signal
        return (
            if research_repair_exhausted {
                "blocked".to_string()
            } else {
                "needs_repair".to_string()
            },
            Some(
                "standup update is missing required JSON keys: `yesterday` and `today` \
                 must be present in the returned JSON object"
                    .to_string(),
            ),
            None,
        );
    }

    let handoff_only_structured_json = validator_kind
        == crate::AutomationOutputValidatorKind::StructuredJson
        && automation_node_required_output_path(node).is_none();
    let has_required_tools = !automation_node_required_tools(node).is_empty();
    let validation_repairable = (validator_kind
        == crate::AutomationOutputValidatorKind::ResearchBrief
        || validator_kind == crate::AutomationOutputValidatorKind::GenericArtifact
        || has_required_tools
        || handoff_only_structured_json)
        && !research_repair_exhausted;
    let parsed = parse_status_json(session_text);
    let approved = parsed
        .as_ref()
        .and_then(|value| value.get("approved"))
        .and_then(Value::as_bool);
    let explicit_reason = parsed
        .as_ref()
        .and_then(|value| value.get("reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let tool_mode_required_unsatisfied = session_text.contains("TOOL_MODE_REQUIRED_NOT_SATISFIED");
    if tool_mode_required_unsatisfied && parsed.is_none() {
        let reason = if session_text.contains("WRITE_REQUIRED_NOT_SATISFIED") {
            automation_node_required_output_path(node)
                .map(|path| {
                    format!("required output `{path}` was not created in the current attempt")
                })
                .unwrap_or_else(|| {
                    "required output was not created in the current attempt".to_string()
                })
        } else if session_text.contains("TOOL_CALL_REJECTED_BY_POLICY") {
            "required tool call was rejected before the node completed".to_string()
        } else if session_text.contains("TOOL_CALL_INVALID_ARGS")
            || session_text.contains("WRITE_ARGS_EMPTY_FROM_PROVIDER")
            || session_text.contains("WRITE_ARGS_UNPARSEABLE_FROM_PROVIDER")
        {
            "required tool call used invalid arguments and should be retried with corrected inputs"
                .to_string()
        } else {
            "required tool call was not completed before finalizing the node".to_string()
        };
        return (
            if validation_repairable {
                "needs_repair".to_string()
            } else {
                "blocked".to_string()
            },
            Some(reason),
            approved,
        );
    }
    if parsed
        .as_ref()
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .is_some_and(|status| status.eq_ignore_ascii_case("verify_failed"))
    {
        return (
            "verify_failed".to_string(),
            explicit_reason.or_else(|| Some("verification command failed".to_string())),
            approved,
        );
    }
    if parsed
        .as_ref()
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .is_some_and(|status| status.eq_ignore_ascii_case("blocked"))
    {
        let has_actionable_validation = artifact_validation
            .and_then(|value| {
                value
                    .get("rejected_artifact_reason")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .or_else(|| {
                        value
                            .get("semantic_block_reason")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                    })
            })
            .is_some();
        if !validation_repairable || !has_actionable_validation {
            return ("blocked".to_string(), explicit_reason, approved);
        }
    }
    // Only ReviewDecision nodes act as approval gates. StructuredJson nodes (e.g. standup
    // participants) return structured data that may contain an `approved` field with unrelated
    // semantics — treat approval gating as a ReviewDecision-exclusive concern.
    if approved == Some(false)
        && validator_kind == crate::AutomationOutputValidatorKind::ReviewDecision
    {
        return (
            "blocked".to_string(),
            explicit_reason
                .or_else(|| Some("upstream review did not approve the output".to_string())),
            approved,
        );
    }
    if let Some(reason) = artifact_validation.and_then(|value| {
        value
            .get("rejected_artifact_reason")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }) {
        let repairable_rejected_artifact = reason
            .contains("was not created in the current attempt")
            || session_text.contains("TOOL_MODE_REQUIRED_NOT_SATISFIED");
        return (
            if repairable_rejected_artifact && !research_repair_exhausted {
                "needs_repair".to_string()
            } else {
                "blocked".to_string()
            },
            Some(reason),
            approved,
        );
    }
    if let Some(reason) = artifact_validation.and_then(|value| {
        value
            .get("semantic_block_reason")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }) {
        return (
            if validation_repairable {
                "needs_repair".to_string()
            } else {
                "blocked".to_string()
            },
            Some(reason),
            approved,
        );
    }
    let output_text = verified_output
        .map(|(_, text)| text.as_str())
        .unwrap_or_else(|| session_text.trim());
    let lowered = output_text
        .chars()
        .take(1600)
        .collect::<String>()
        .to_ascii_lowercase();
    // TODO(coding-hardening): Replace these content markers with structured node
    // status signals from the runtime/session wrapper. Prompt text should not be the
    // primary source of truth for blocked vs completed vs verify_failed decisions.
    let blocked_markers = [
        "status: blocked",
        "status blocked",
        "## status blocked",
        "blocked pending",
        "this brief is blocked",
        "brief is blocked",
        "partially blocked",
        "provisional",
        "path-level evidence",
        "based on filenames not content",
        "could not be confirmed from file contents",
        "could not safely cite exact file-derived claims",
        "not approved",
        "approval has not happened",
        "publication is blocked",
        "i’m blocked",
        "i'm blocked",
    ];
    // TODO(coding-hardening): Same here for verification failures. We should rely on
    // explicit verification result metadata and command outcomes, not phrase matching.
    let verify_failed_markers = [
        "status: verify_failed",
        "status verify_failed",
        "verification failed",
        "tests failed",
        "build failed",
        "lint failed",
        "verify failed",
    ];
    if verify_failed_markers
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        return (
            "verify_failed".to_string(),
            explicit_reason.or_else(|| Some("verification command failed".to_string())),
            approved,
        );
    }
    if blocked_markers
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        let reason = explicit_reason.or_else(|| {
            if automation_output_validator_kind(node)
                == crate::AutomationOutputValidatorKind::ReviewDecision
            {
                Some("review output was not approved".to_string())
            } else {
                Some("node produced a blocked handoff artifact".to_string())
            }
        });
        return ("blocked".to_string(), reason, approved);
    }
    let requested_tools = tool_telemetry
        .get("requested_tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let executed_tools = tool_telemetry
        .get("executed_tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let requested_has_read = requested_tools
        .iter()
        .any(|value| value.as_str() == Some("read"));
    let executed_has_read = executed_tools
        .iter()
        .any(|value| value.as_str() == Some("read"));
    let email_delivery_attempted = tool_telemetry
        .get("email_delivery_attempted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let email_delivery_succeeded = tool_telemetry
        .get("email_delivery_succeeded")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let latest_email_delivery_failure = tool_telemetry
        .get("latest_email_delivery_failure")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let available_email_like_tools =
        automation_capability_resolution_email_tools(tool_telemetry, "available_tools");
    let offered_email_like_tools =
        automation_capability_resolution_email_tools(tool_telemetry, "offered_tools");
    let offered_email_send_tools =
        automation_capability_resolution_email_tools(tool_telemetry, "offered_send_tools");
    let offered_email_draft_tools =
        automation_capability_resolution_email_tools(tool_telemetry, "offered_draft_tools");
    let selected_mcp_servers =
        automation_capability_resolution_mcp_tools(tool_telemetry, "selected_servers");
    let discovered_remote_mcp_tools =
        automation_capability_resolution_mcp_tools(tool_telemetry, "remote_tools");
    let discovered_registered_mcp_tools =
        automation_capability_resolution_mcp_tools(tool_telemetry, "registered_tools");
    let canonical_delivery_status = automation_attempt_evidence_delivery_status(tool_telemetry);
    let is_brief_contract = validator_kind == crate::AutomationOutputValidatorKind::ResearchBrief;
    let requires_read = automation_node_required_tools(node)
        .iter()
        .any(|value| value == "read");
    let verification_expected = tool_telemetry
        .get("verification_expected")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let verification_ran = tool_telemetry
        .get("verification_ran")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let verification_failed = tool_telemetry
        .get("verification_failed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let verification_outcome = tool_telemetry
        .get("verification_outcome")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    let verification_completed = tool_telemetry
        .get("verification_completed")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let verification_total = tool_telemetry
        .get("verification_total")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let verification_failure_reason = tool_telemetry
        .get("latest_verification_failure")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if verification_expected && verification_failed {
        return (
            "verify_failed".to_string(),
            explicit_reason.or(verification_failure_reason),
            approved,
        );
    }
    if automation_node_is_code_workflow(node)
        && verification_expected
        && verification_outcome.as_deref() == Some("partial")
    {
        return (
            "needs_repair".to_string(),
            Some(format!(
                "coding task completed with only {} of {} declared verification commands run",
                verification_completed, verification_total
            )),
            approved,
        );
    }
    if automation_node_is_code_workflow(node) && verification_expected && !verification_ran {
        return (
            "needs_repair".to_string(),
            Some(
                "coding task completed without running the declared verification command"
                    .to_string(),
            ),
            approved,
        );
    }
    let mentions_missing_file_evidence = lowered.contains("file contents were not")
        || lowered.contains("could not safely cite exact file-derived claims")
        || lowered.contains("could not be confirmed from file contents")
        || lowered.contains("path-level evidence")
        || lowered.contains("based on filenames not content")
        || lowered.contains("partially blocked")
        || lowered.contains("provisional")
        || lowered.contains("this brief is blocked")
        || lowered.contains("brief is blocked");
    let artifact_semantic_block = artifact_validation
        .and_then(|value| value.get("semantic_block_reason"))
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    if ((is_brief_contract && requested_has_read && !executed_has_read)
        || (requires_read && requested_has_read && !executed_has_read))
        && (artifact_semantic_block || verified_output.is_none())
    {
        return (
            if validation_repairable {
                "needs_repair".to_string()
            } else {
                "blocked".to_string()
            },
            Some(if mentions_missing_file_evidence {
                if is_brief_contract {
                    "research brief did not read concrete workspace files, so source-backed validation is incomplete".to_string()
                } else {
                    "node did not use required read tool calls before finalizing the artifact"
                        .to_string()
                }
            } else {
                if is_brief_contract {
                    "research brief cited workspace sources without using read, so source-backed validation is incomplete".to_string()
                } else {
                    "node finalized its artifact without required concrete file reads".to_string()
                }
            }),
            approved,
        );
    }
    if automation_node_requires_email_delivery(node)
        && canonical_delivery_status
            .as_deref()
            .unwrap_or(if email_delivery_succeeded {
                "succeeded"
            } else if email_delivery_attempted {
                "attempted_failed"
            } else {
                "not_attempted"
            })
            != "succeeded"
    {
        let discovered_summary = if available_email_like_tools.is_empty() {
            "none".to_string()
        } else {
            available_email_like_tools.join(", ")
        };
        let offered_summary = if offered_email_like_tools.is_empty() {
            "none".to_string()
        } else {
            offered_email_like_tools.join(", ")
        };
        let reason = if email_delivery_attempted {
            latest_email_delivery_failure.unwrap_or_else(|| {
                "email delivery was attempted but did not complete successfully".to_string()
            })
        } else if offered_email_send_tools.is_empty() && offered_email_draft_tools.is_empty() {
            let selected_servers_summary = if selected_mcp_servers.is_empty() {
                "none".to_string()
            } else {
                selected_mcp_servers.join(", ")
            };
            let remote_mcp_tools_summary = if discovered_remote_mcp_tools.is_empty() {
                "none".to_string()
            } else {
                discovered_remote_mcp_tools.join(", ")
            };
            let registered_mcp_tools_summary = if discovered_registered_mcp_tools.is_empty() {
                "none".to_string()
            } else {
                discovered_registered_mcp_tools.join(", ")
            };
            if let Some(target) = automation_node_delivery_target(node) {
                format!(
                    "email delivery to `{}` was requested but no email-capable tools were available. Selected MCP servers: {}. Remote MCP tools on selected servers: {}. Registered tool-registry tools on selected servers: {}. Discovered email-like tools: {}. Offered email-like tools: {}. This usually means the email connector is unavailable, MCP tools were not synced into the registry, or the tool names did not match email capability detection.",
                    target,
                    selected_servers_summary,
                    remote_mcp_tools_summary,
                    registered_mcp_tools_summary,
                    discovered_summary,
                    offered_summary
                )
            } else {
                format!(
                    "email delivery was requested but no email-capable tools were available. Selected MCP servers: {}. Remote MCP tools on selected servers: {}. Registered tool-registry tools on selected servers: {}. Discovered email-like tools: {}. Offered email-like tools: {}. This usually means the email connector is unavailable, MCP tools were not synced into the registry, or the tool names did not match email capability detection.",
                    selected_servers_summary,
                    remote_mcp_tools_summary,
                    registered_mcp_tools_summary,
                    discovered_summary,
                    offered_summary
                )
            }
        } else if let Some(target) = automation_node_delivery_target(node) {
            format!(
                "email delivery to `{}` was requested but no email draft/send tool executed",
                target
            )
        } else {
            "email delivery was requested but no email draft/send tool executed".to_string()
        };
        let delivery_repairable = !email_delivery_attempted
            && (!offered_email_send_tools.is_empty() || !offered_email_draft_tools.is_empty());
        return (
            if delivery_repairable {
                "needs_repair".to_string()
            } else {
                "blocked".to_string()
            },
            Some(reason),
            approved,
        );
    }
    if automation_node_is_code_workflow(node) {
        return ("done".to_string(), explicit_reason, approved);
    }
    ("completed".to_string(), explicit_reason, approved)
}

pub(crate) fn automation_node_workflow_class(node: &AutomationFlowNode) -> String {
    if automation_node_is_code_workflow(node) {
        "code".to_string()
    } else if automation_output_validator_kind(node)
        == crate::AutomationOutputValidatorKind::ResearchBrief
    {
        "research".to_string()
    } else {
        "artifact".to_string()
    }
}

pub(crate) fn detect_automation_node_failure_kind(
    node: &AutomationFlowNode,
    status: &str,
    approved: Option<bool>,
    blocked_reason: Option<&str>,
    artifact_validation: Option<&Value>,
) -> Option<String> {
    let normalized_status = status.trim().to_ascii_lowercase();
    let reason = blocked_reason
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let unmet_requirements = artifact_validation
        .and_then(|value| value.get("unmet_requirements"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let has_unmet = |needle: &str| {
        unmet_requirements
            .iter()
            .any(|value| value.as_str() == Some(needle))
    };
    let has_required_tools = !automation_node_required_tools(node).is_empty();
    let handoff_only_structured_json = automation_output_validator_kind(node)
        == crate::AutomationOutputValidatorKind::StructuredJson
        && automation_node_required_output_path(node).is_none();
    let research_requirements_blocked = automation_output_validator_kind(node)
        == crate::AutomationOutputValidatorKind::ResearchBrief
        && (has_unmet("no_concrete_reads")
            || has_unmet("concrete_read_required")
            || has_unmet("missing_successful_web_research")
            || has_unmet("citations_missing")
            || has_unmet("web_sources_reviewed_missing")
            || has_unmet("files_reviewed_missing")
            || has_unmet("files_reviewed_not_backed_by_read")
            || has_unmet("relevant_files_not_reviewed_or_skipped")
            || has_unmet("coverage_mode"));
    let required_tools_blocked = has_required_tools
        && (has_unmet("no_concrete_reads")
            || has_unmet("concrete_read_required")
            || has_unmet("missing_successful_web_research"));
    let editorial_requirements_blocked = has_unmet("editorial_substance_missing")
        || has_unmet("markdown_structure_missing")
        || has_unmet("upstream_evidence_not_synthesized")
        || has_unmet("editorial_clearance_required");
    let verification_expected = artifact_validation
        .and_then(|value| value.get("verification_expected"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let verification_ran = artifact_validation
        .and_then(|value| value.get("verification_ran"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let verification_failed = artifact_validation
        .and_then(|value| value.get("verification"))
        .and_then(|value| value.get("verification_failed"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if verification_failed || normalized_status == "verify_failed" {
        return Some("verification_failed".to_string());
    }
    if automation_node_is_code_workflow(node) && verification_expected && !verification_ran {
        return Some("verification_missing".to_string());
    }
    if let Some(rejected_reason) = artifact_validation
        .and_then(|value| value.get("rejected_artifact_reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if rejected_reason.contains("placeholder") {
            return Some("placeholder_overwrite_rejected".to_string());
        }
        if rejected_reason.contains("unsafe raw source rewrite")
            || rejected_reason.contains("raw write without patch/edit")
        {
            return Some("unsafe_raw_write_rejected".to_string());
        }
        return Some("artifact_rejected".to_string());
    }
    if artifact_validation
        .and_then(|value| value.get("semantic_block_reason"))
        .and_then(Value::as_str)
        .is_some()
        || (automation_output_validator_kind(node)
            == crate::AutomationOutputValidatorKind::ResearchBrief
            && matches!(normalized_status.as_str(), "blocked" | "needs_repair")
            && research_requirements_blocked)
        || (has_required_tools
            && matches!(normalized_status.as_str(), "blocked" | "needs_repair")
            && required_tools_blocked)
        || (automation_output_validator_kind(node)
            == crate::AutomationOutputValidatorKind::GenericArtifact
            && normalized_status == "blocked"
            && editorial_requirements_blocked)
    {
        let repair_exhausted = artifact_validation
            .and_then(|value| value.get("repair_exhausted"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if repair_exhausted && research_requirements_blocked {
            return Some("research_retry_exhausted".to_string());
        }
        if handoff_only_structured_json && has_unmet("structured_handoff_missing") {
            return Some("structured_handoff_missing".to_string());
        }
        if has_unmet("no_concrete_reads") || has_unmet("concrete_read_required") {
            if automation_output_validator_kind(node)
                == crate::AutomationOutputValidatorKind::ResearchBrief
            {
                return Some("research_missing_reads".to_string());
            }
            return Some("required_tool_unused_read".to_string());
        }
        if has_unmet("missing_successful_web_research") {
            if automation_output_validator_kind(node)
                == crate::AutomationOutputValidatorKind::ResearchBrief
            {
                return Some("research_missing_web_research".to_string());
            }
            return Some("required_tool_unused_websearch".to_string());
        }
        if has_unmet("citations_missing") || has_unmet("web_sources_reviewed_missing") {
            return Some("research_citations_missing".to_string());
        }
        if has_unmet("files_reviewed_missing")
            || has_unmet("files_reviewed_not_backed_by_read")
            || has_unmet("relevant_files_not_reviewed_or_skipped")
            || has_unmet("coverage_mode")
        {
            return Some("research_coverage_failed".to_string());
        }
        if editorial_requirements_blocked {
            return Some("editorial_quality_failed".to_string());
        }
        return Some("semantic_blocked".to_string());
    }
    if normalized_status == "blocked" && approved == Some(false) {
        return Some("review_not_approved".to_string());
    }
    if normalized_status == "blocked" && reason.contains("upstream review did not approve") {
        return Some("upstream_not_approved".to_string());
    }
    if normalized_status == "failed" {
        return Some("run_failed".to_string());
    }
    if automation_node_is_code_workflow(node) && normalized_status == "done" {
        return Some("verification_passed".to_string());
    }
    None
}

pub(crate) fn build_automation_validator_summary(
    validator_kind: crate::AutomationOutputValidatorKind,
    status: &str,
    blocked_reason: Option<&str>,
    artifact_validation: Option<&Value>,
) -> crate::AutomationValidatorSummary {
    let normalized_status = status.trim().to_ascii_lowercase();
    let verification_outcome = artifact_validation
        .and_then(|value| value.get("verification"))
        .and_then(|value| {
            value
                .get("verification_outcome")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    if value
                        .get("verification_failed")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        Some("failed".to_string())
                    } else if value
                        .get("verification_ran")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        Some("passed".to_string())
                    } else {
                        None
                    }
                })
        });
    let unmet_requirements = artifact_validation
        .and_then(|value| value.get("unmet_requirements"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let warning_requirements = artifact_validation
        .and_then(|value| value.get("warning_requirements"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let warning_count = artifact_validation
        .and_then(|value| value.get("warning_count"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or_else(|| warning_requirements.len() as u32);
    let accepted_candidate_source = artifact_validation
        .and_then(|value| value.get("accepted_candidate_source"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let repair_attempted = artifact_validation
        .and_then(|value| value.get("repair_attempted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let repair_attempt = artifact_validation
        .and_then(|value| value.get("repair_attempt"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let repair_attempts_remaining = artifact_validation
        .and_then(|value| value.get("repair_attempts_remaining"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or_else(|| tandem_core::prewrite_repair_retry_max_attempts() as u32);
    let repair_succeeded = artifact_validation
        .and_then(|value| value.get("repair_succeeded"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let repair_exhausted = artifact_validation
        .and_then(|value| value.get("repair_exhausted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let reason = blocked_reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            artifact_validation
                .and_then(|value| value.get("rejected_artifact_reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            artifact_validation
                .and_then(|value| value.get("semantic_block_reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        });
    let outcome = artifact_validation
        .and_then(|value| value.get("validation_outcome"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(match normalized_status.as_str() {
            "completed" | "done" => "passed",
            "verify_failed" => "verify_failed",
            "blocked" => "blocked",
            "failed" => "failed",
            other => other,
        })
        .to_string();
    let validation_basis = artifact_validation
        .and_then(|value| value.get("validation_basis"))
        .cloned()
        .filter(|value| !value.is_null());
    crate::AutomationValidatorSummary {
        kind: validator_kind,
        outcome,
        reason,
        unmet_requirements,
        warning_requirements,
        warning_count,
        accepted_candidate_source,
        verification_outcome,
        validation_basis,
        repair_attempted,
        repair_attempt,
        repair_attempts_remaining,
        repair_succeeded,
        repair_exhausted,
    }
}

fn automation_status_used_legacy_fallback(
    session_text: &str,
    artifact_validation: Option<&Value>,
) -> bool {
    if artifact_validation
        .and_then(|value| value.get("semantic_block_reason"))
        .and_then(Value::as_str)
        .is_some()
    {
        return false;
    }
    let lowered = session_text
        .chars()
        .take(1600)
        .collect::<String>()
        .to_ascii_lowercase();
    [
        "status: blocked",
        "status blocked",
        "## status blocked",
        "blocked pending",
        "this brief is blocked",
        "brief is blocked",
        "partially blocked",
        "provisional",
        "path-level evidence",
        "based on filenames not content",
        "could not be confirmed from file contents",
        "could not safely cite exact file-derived claims",
        "not approved",
        "approval has not happened",
        "publication is blocked",
        "i’m blocked",
        "i'm blocked",
        "status: verify_failed",
        "status verify_failed",
        "verification failed",
        "tests failed",
        "build failed",
        "lint failed",
        "verify failed",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
}

pub(crate) fn detect_automation_blocker_category(
    node: &AutomationFlowNode,
    status: &str,
    blocked_reason: Option<&str>,
    tool_telemetry: &Value,
    artifact_validation: Option<&Value>,
) -> Option<String> {
    if !matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "blocked" | "needs_repair" | "verify_failed"
    ) {
        return None;
    }
    let reason = blocked_reason.unwrap_or_default().to_ascii_lowercase();
    let missing_capabilities = automation_attempt_evidence_missing_capabilities(tool_telemetry);
    let offered_email_like_tools =
        automation_capability_resolution_email_tools(tool_telemetry, "offered_tools");
    if reason.contains("prompt tokens limit exceeded")
        || tool_telemetry
            .get("preflight")
            .and_then(|value| value.get("budget_status"))
            .and_then(Value::as_str)
            .is_some_and(|status| status == "high")
            && missing_capabilities.is_empty()
            && tool_telemetry
                .get("executed_tools")
                .and_then(Value::as_array)
                .is_none_or(|rows| rows.is_empty())
    {
        return Some("prompt_budget".to_string());
    }
    let verification_expected = tool_telemetry
        .get("verification_expected")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let verification_ran = tool_telemetry
        .get("verification_ran")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if automation_node_is_code_workflow(node) && verification_expected && !verification_ran {
        return Some("verification_required".to_string());
    }
    if automation_node_requires_email_delivery(node)
        && offered_email_like_tools.is_empty()
        && automation_attempt_evidence_delivery_status(tool_telemetry)
            .as_deref()
            .is_some_and(|status| status != "succeeded" && status != "not_required")
    {
        return Some("tool_unavailable".to_string());
    }
    if automation_node_requires_email_delivery(node)
        && automation_attempt_evidence_delivery_status(tool_telemetry)
            .as_deref()
            .is_some_and(|status| status != "succeeded" && status != "not_required")
    {
        return Some("delivery_not_executed".to_string());
    }
    if !missing_capabilities.is_empty() {
        return Some("tool_unavailable".to_string());
    }
    let web_status = automation_attempt_evidence_web_research_status(tool_telemetry);
    if web_status.as_deref() == Some("unavailable") {
        return Some("tool_unavailable".to_string());
    }
    if matches!(
        web_status.as_deref(),
        Some("timed_out" | "unusable" | "not_attempted")
    ) {
        return Some("tool_result_unusable".to_string());
    }
    if artifact_validation
        .and_then(|value| value.get("semantic_block_reason"))
        .and_then(Value::as_str)
        .is_some()
        || artifact_validation
            .and_then(|value| value.get("rejected_artifact_reason"))
            .and_then(Value::as_str)
            .is_some()
    {
        return Some("artifact_contract_unmet".to_string());
    }
    None
}

pub(crate) fn enrich_automation_node_output_for_contract(
    node: &AutomationFlowNode,
    output: &Value,
) -> Value {
    let Some(mut object) = output.as_object().cloned() else {
        return output.clone();
    };
    let status = object
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("completed")
        .to_string();
    let blocked_reason = object
        .get("blocked_reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let approved = object
        .get("approved")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let artifact_validation = object.get("artifact_validation").cloned();
    let validator_kind = automation_output_validator_kind(node);

    object.insert(
        "contract_kind".to_string(),
        json!(node
            .output_contract
            .as_ref()
            .map(|row| row.kind.clone())
            .unwrap_or_else(|| "structured_json".to_string())),
    );
    object.insert("validator_kind".to_string(), json!(validator_kind));
    object.insert(
        "workflow_class".to_string(),
        json!(automation_node_workflow_class(node)),
    );
    object.insert(
        "phase".to_string(),
        json!(detect_automation_node_phase(
            node,
            &status,
            artifact_validation.as_ref()
        )),
    );
    object.insert(
        "failure_kind".to_string(),
        detect_automation_node_failure_kind(
            node,
            &status,
            Some(approved),
            blocked_reason.as_deref(),
            artifact_validation.as_ref(),
        )
        .map(Value::String)
        .unwrap_or(Value::Null),
    );
    object.insert(
        "validator_summary".to_string(),
        json!(build_automation_validator_summary(
            validator_kind,
            &status,
            blocked_reason.as_deref(),
            artifact_validation.as_ref(),
        )),
    );
    Value::Object(object)
}

pub(crate) fn detect_automation_node_phase(
    node: &AutomationFlowNode,
    status: &str,
    artifact_validation: Option<&Value>,
) -> String {
    let workflow_class = automation_node_workflow_class(node);
    let normalized_status = status.trim().to_ascii_lowercase();
    match workflow_class.as_str() {
        "research" => {
            let unmet_requirements = artifact_validation
                .and_then(|value| value.get("unmet_requirements"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let has_unmet = |needle: &str| {
                unmet_requirements
                    .iter()
                    .any(|value| value.as_str() == Some(needle))
            };
            let research_validation_blocked = artifact_validation
                .and_then(|value| value.get("semantic_block_reason"))
                .and_then(Value::as_str)
                .is_some()
                || (automation_output_validator_kind(node)
                    == crate::AutomationOutputValidatorKind::ResearchBrief
                    && normalized_status == "blocked"
                    && (has_unmet("no_concrete_reads")
                        || has_unmet("concrete_read_required")
                        || has_unmet("missing_successful_web_research")
                        || has_unmet("citations_missing")
                        || has_unmet("web_sources_reviewed_missing")
                        || has_unmet("files_reviewed_missing")
                        || has_unmet("files_reviewed_not_backed_by_read")
                        || has_unmet("relevant_files_not_reviewed_or_skipped")
                        || has_unmet("coverage_mode")));
            if research_validation_blocked {
                "research_validation".to_string()
            } else if normalized_status == "completed" {
                "completed".to_string()
            } else {
                "research".to_string()
            }
        }
        "code" => {
            let verification_expected = artifact_validation
                .and_then(|value| value.get("verification"))
                .and_then(|value| value.get("verification_expected"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if verification_expected {
                if normalized_status == "done" {
                    "completed".to_string()
                } else {
                    "verification".to_string()
                }
            } else if normalized_status == "done" {
                "completed".to_string()
            } else {
                "implementation".to_string()
            }
        }
        _ => {
            let unmet_requirements = artifact_validation
                .and_then(|value| value.get("unmet_requirements"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let has_unmet = |needle: &str| {
                unmet_requirements
                    .iter()
                    .any(|value| value.as_str() == Some(needle))
            };
            let editorial_validation_blocked = (has_unmet("editorial_substance_missing")
                || has_unmet("markdown_structure_missing")
                || has_unmet("editorial_clearance_required"))
                && (artifact_validation
                    .and_then(|value| value.get("semantic_block_reason"))
                    .and_then(Value::as_str)
                    .is_some()
                    || normalized_status == "blocked");
            if editorial_validation_blocked {
                "editorial_validation".to_string()
            } else if normalized_status == "completed" {
                "completed".to_string()
            } else {
                "artifact_write".to_string()
            }
        }
    }
}

pub(crate) fn wrap_automation_node_output(
    node: &AutomationFlowNode,
    session: &Session,
    requested_tools: &[String],
    session_id: &str,
    run_id: Option<&str>,
    session_text: &str,
    verified_output: Option<(String, String)>,
    artifact_validation: Option<Value>,
) -> Value {
    let contract_kind = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.clone())
        .unwrap_or_else(|| "structured_json".to_string());
    let summary = if let Some((path, _)) = verified_output.as_ref() {
        format!(
            "Verified workspace output `{}` for node `{}`.",
            path, node.node_id
        )
    } else if let Some(reason) = artifact_validation
        .as_ref()
        .and_then(|value| value.get("rejected_artifact_reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        format!(
            "Artifact validation rejected node `{}` output: {}.",
            node.node_id, reason
        )
    } else if session_text.trim().is_empty() {
        format!("Node `{}` completed successfully.", node.node_id)
    } else {
        truncate_text(session_text.trim(), 240)
    };
    let primary_text = verified_output
        .as_ref()
        .map(|(_, text)| text.as_str())
        .unwrap_or_else(|| session_text.trim());
    let validator_kind = automation_output_validator_kind(node);
    let structured_handoff = if validator_kind
        == crate::AutomationOutputValidatorKind::StructuredJson
        && verified_output.is_none()
    {
        extract_structured_handoff_json(session_text)
    } else {
        None
    };
    let structured_primary_text = structured_handoff
        .as_ref()
        .and_then(|value| serde_json::to_string_pretty(value).ok());
    let tool_telemetry = summarize_automation_tool_activity(node, session, requested_tools);
    let (status, blocked_reason, approved) = detect_automation_node_status(
        node,
        session_text,
        verified_output.as_ref(),
        &tool_telemetry,
        artifact_validation.as_ref(),
    );
    let blocker_category = detect_automation_blocker_category(
        node,
        &status,
        blocked_reason.as_deref(),
        &tool_telemetry,
        artifact_validation.as_ref(),
    );
    let fallback_used =
        automation_status_used_legacy_fallback(session_text, artifact_validation.as_ref());
    let quality_mode_resolution = enforcement::automation_node_quality_mode_resolution(node);
    let provenance = automation_node_output_provenance(
        node,
        session_id,
        run_id,
        verified_output.as_ref(),
        artifact_validation.as_ref(),
    );
    let final_attempt_evidence = tool_telemetry
        .get("attempt_evidence")
        .cloned()
        .map(|value| {
            augment_automation_attempt_evidence_with_validation(
                &value,
                artifact_validation.as_ref(),
                verified_output.as_ref(),
                artifact_validation
                    .as_ref()
                    .and_then(|value| value.get("accepted_candidate_source"))
                    .and_then(Value::as_str),
                blocker_category.as_deref(),
                fallback_used,
            )
        });
    let workflow_class = automation_node_workflow_class(node);
    let phase = detect_automation_node_phase(node, &status, artifact_validation.as_ref());
    let failure_kind = detect_automation_node_failure_kind(
        node,
        &status,
        approved,
        blocked_reason.as_deref(),
        artifact_validation.as_ref(),
    );
    let validator_summary = build_automation_validator_summary(
        validator_kind,
        &status,
        blocked_reason.as_deref(),
        artifact_validation.as_ref(),
    );
    let preflight = tool_telemetry.get("preflight").cloned();
    let capability_resolution = tool_telemetry.get("capability_resolution").cloned();
    let content = match contract_kind.as_str() {
        "report_markdown" | "text_summary" => {
            json!({
                "text": primary_text,
                "path": verified_output.as_ref().map(|(path, _)| path.clone()),
                "raw_assistant_text": session_text.trim(),
                "session_id": session_id
            })
        }
        "urls" => json!({
            "items": [],
            "raw_text": primary_text,
            "path": verified_output.as_ref().map(|(path, _)| path.clone()),
            "raw_assistant_text": session_text.trim(),
            "session_id": session_id
        }),
        "citations" => {
            json!({
                "items": [],
                "raw_text": primary_text,
                "path": verified_output.as_ref().map(|(path, _)| path.clone()),
                "raw_assistant_text": session_text.trim(),
                "session_id": session_id
            })
        }
        _ => {
            let mut content = json!({
                "text": structured_primary_text
                    .as_deref()
                    .unwrap_or(primary_text),
                "path": verified_output.as_ref().map(|(path, _)| path.clone()),
                "raw_assistant_text": session_text.trim(),
                "session_id": session_id
            });
            if let Some(handoff) = structured_handoff {
                if let Some(object) = content.as_object_mut() {
                    object.insert("structured_handoff".to_string(), handoff);
                }
            }
            content
        }
    };
    json!(AutomationNodeOutput {
        contract_kind,
        validator_kind: Some(validator_kind),
        validator_summary: Some(validator_summary),
        summary,
        content,
        created_at_ms: now_ms(),
        node_id: node.node_id.clone(),
        status: Some(status),
        blocked_reason,
        approved,
        workflow_class: Some(workflow_class),
        phase: Some(phase),
        failure_kind,
        tool_telemetry: Some(tool_telemetry),
        preflight,
        knowledge_preflight: None,
        capability_resolution,
        attempt_evidence: final_attempt_evidence,
        blocker_category,
        receipt_timeline: None,
        quality_mode: Some(quality_mode_resolution.effective.stable_key().to_string()),
        requested_quality_mode: quality_mode_resolution
            .requested
            .map(|mode| mode.stable_key().to_string()),
        emergency_rollback_enabled: Some(quality_mode_resolution.legacy_rollback_enabled),
        fallback_used: Some(fallback_used),
        artifact_validation,
        provenance,
    })
}
