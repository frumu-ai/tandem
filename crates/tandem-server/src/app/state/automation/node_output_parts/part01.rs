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

fn automation_structured_handoff_source_material(session: &Session) -> Option<Value> {
    let workspace_root = session
        .workspace_root
        .as_deref()
        .unwrap_or(session.directory.as_str());
    let mut source_material = Vec::<Value>::new();
    let mut seen = std::collections::HashSet::<String>::new();
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool,
                args,
                result,
                error,
            } = part
            else {
                continue;
            };
            if !tool.eq_ignore_ascii_case("read")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }
            let Some(args) = super::tool_args_object(args) else {
                continue;
            };
            let Some(raw_path) = super::automation_write_arg_path(&args) else {
                continue;
            };
            let Some(content) = automation_tool_result_output_text(result.as_ref()) else {
                continue;
            };
            if content.trim().is_empty() {
                continue;
            }
            let normalized_path = super::normalize_workspace_display_path(workspace_root, raw_path)
                .unwrap_or_else(|| raw_path.trim().to_string());
            let fingerprint = format!(
                "{}:{}",
                normalized_path.to_ascii_lowercase(),
                crate::sha256_hex(&[content.as_str()])
            );
            if !seen.insert(fingerprint) {
                continue;
            }
            source_material.push(json!({
                "path": normalized_path,
                "content": content,
                "tool": "read",
            }));
        }
    }
    if source_material.is_empty() {
        None
    } else {
        Some(Value::Array(source_material))
    }
}

fn automation_attach_structured_handoff_source_material(
    structured_handoff: &mut Value,
    source_material: &Value,
) {
    let Some(source_rows) = source_material.as_array() else {
        return;
    };
    if source_rows.is_empty() {
        return;
    }
    let Some(object) = structured_handoff.as_object_mut() else {
        return;
    };
    let entry = object
        .entry("source_material".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(existing_rows) = entry.as_array_mut() else {
        *entry = source_material.clone();
        return;
    };
    for row in source_rows {
        if !existing_rows.iter().any(|existing| existing == row) {
            existing_rows.push(row.clone());
        }
    }
}

fn automation_path_references_read_only_source_of_truth(
    raw_path: &str,
    read_only_names: &std::collections::HashSet<String>,
    workspace_root: &str,
) -> bool {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return false;
    }
    let mut candidates = vec![trimmed.to_ascii_lowercase()];
    if let Some(filename) = std::path::Path::new(trimmed)
        .file_name()
        .and_then(|value| value.to_str())
    {
        candidates.push(filename.to_ascii_lowercase());
    }
    if let Some(normalized) = super::normalize_workspace_display_path(workspace_root, trimmed) {
        candidates.push(normalized.to_ascii_lowercase());
        if let Some(filename) = std::path::Path::new(&normalized)
            .file_name()
            .and_then(|value| value.to_str())
        {
            candidates.push(filename.to_ascii_lowercase());
        }
    }
    candidates
        .into_iter()
        .any(|candidate| read_only_names.contains(&candidate))
}

fn automation_value_references_read_only_source_of_truth(
    value: &Value,
    read_only_names: &std::collections::HashSet<String>,
    workspace_root: &str,
) -> bool {
    match value {
        Value::String(text) => automation_path_references_read_only_source_of_truth(
            text,
            read_only_names,
            workspace_root,
        ),
        Value::Object(object) => object
            .get("path")
            .and_then(Value::as_str)
            .is_some_and(|path| {
                automation_path_references_read_only_source_of_truth(
                    path,
                    read_only_names,
                    workspace_root,
                )
            }),
        _ => false,
    }
}

fn automation_sanitize_read_only_source_of_truth_writes(
    value: &mut Value,
    read_only_names: &std::collections::HashSet<String>,
    workspace_root: &str,
) {
    const WRITE_TARGET_KEYS: &[&str] = &[
        "must_write_files",
        "workspace_writes_needed",
        "required_workspace_writes",
        "write_targets",
        "approved_write_targets",
        "required_write_targets",
    ];
    match value {
        Value::Object(object) => {
            for key in WRITE_TARGET_KEYS {
                if let Some(child) = object.get_mut(*key) {
                    match child {
                        Value::Array(rows) => {
                            rows.retain(|row| {
                                !automation_value_references_read_only_source_of_truth(
                                    row,
                                    read_only_names,
                                    workspace_root,
                                )
                            });
                        }
                        Value::String(text) => {
                            if automation_path_references_read_only_source_of_truth(
                                text,
                                read_only_names,
                                workspace_root,
                            ) {
                                *child = Value::Null;
                            }
                        }
                        Value::Object(_) => {
                            if automation_value_references_read_only_source_of_truth(
                                child,
                                read_only_names,
                                workspace_root,
                            ) {
                                *child = Value::Null;
                            }
                        }
                        _ => {}
                    }
                }
            }
            for child in object.values_mut() {
                automation_sanitize_read_only_source_of_truth_writes(
                    child,
                    read_only_names,
                    workspace_root,
                );
            }
        }
        Value::Array(rows) => {
            for child in rows.iter_mut() {
                automation_sanitize_read_only_source_of_truth_writes(
                    child,
                    read_only_names,
                    workspace_root,
                );
            }
        }
        _ => {}
    }
}

pub(crate) fn augment_automation_attempt_evidence_with_validation(
    attempt_evidence: &Value,
    artifact_validation: Option<&Value>,
    accepted_output: Option<&(String, String)>,
    accepted_candidate_source: Option<&str>,
    blocker_category: Option<&str>,
    fallback_used: bool,
    final_backend_actionability_state: &str,
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
        "validation_basis".to_string(),
        artifact_validation
            .and_then(|value| value.get("validation_basis"))
            .cloned()
            .unwrap_or(Value::Null),
    );
    object.insert(
        "accepted_candidate_source".to_string(),
        accepted_candidate_source
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null),
    );
    object.insert(
        "blocker_category".to_string(),
        blocker_category
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null),
    );
    object.insert(
        "final_backend_actionability_state".to_string(),
        json!(final_backend_actionability_state),
    );
    object.insert("fallback_used".to_string(), json!(fallback_used));
    Value::Object(object)
}

pub(crate) fn automation_backend_actionability_state(status: &str) -> &'static str {
    match status.trim().to_ascii_lowercase().as_str() {
        "completed" | "done" | "passed" | "accepted_with_warnings" => "completed",
        "needs_repair" => "needs_repair",
        _ => "blocked",
    }
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

fn automation_provider_transport_failure(raw: &str) -> bool {
    let lowered = raw.trim().to_ascii_lowercase();
    lowered.contains("connect timeout")
        || lowered.contains("connection timeout")
        || lowered.contains("timed out")
        || lowered.contains("timeout")
        || lowered.contains("unauthorized")
        || lowered.contains("authentication")
        || lowered.contains("auth failed")
        || lowered.contains("dns error")
        || lowered.contains("connection refused")
        || lowered.contains("network error")
        || lowered.contains("provider stream")
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
    if unmet_requirements
        .iter()
        .any(|value| value == "current_attempt_output_missing")
    {
        return Some("artifact_write_missing");
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
                "no_concrete_reads" | "concrete_read_required" | "required_source_paths_not_read"
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
    missing_required_source_read_paths: &[String],
    upstream_read_paths: &[String],
    upstream_citations: &[String],
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
    if has_unmet("upstream_evidence_not_synthesized") {
        let anchor_target = source_evidence_anchor_target(upstream_read_paths, upstream_citations);
        let upstream_artifact_summary = upstream_read_paths
            .iter()
            .take(4)
            .cloned()
            .collect::<Vec<_>>();
        if !upstream_artifact_summary.is_empty() {
            actions.push(format!(
                "Read and synthesize the strongest upstream artifacts before finalizing: {}. Rewrite the final report as a substantive multi-section synthesis that reuses the concrete terminology, named entities, objections, risks, and proof points already present upstream, and mention at least {} distinct upstream evidence anchors in the body.",
                upstream_artifact_summary.join(", "),
                anchor_target.max(1)
            ));
        } else {
            actions.push(
                "Synthesize the upstream evidence into the final artifact using the concrete terminology, proof points, objections, risks, and citations already available upstream."
                    .to_string(),
            );
        }
    }
    if has_unmet("current_attempt_output_missing") {
        actions.push(
            "Write the required run artifact to the declared output path before ending this attempt."
                .to_string(),
        );
    }
    if has_unmet("required_workspace_files_missing") {
        actions.push(
            "Write the required workspace files approved for this node before ending this attempt."
                .to_string(),
        );
    }
    if requested_has_read
        && (!executed_has_read
            || has_unmet("no_concrete_reads")
            || has_unmet("required_source_paths_not_read")
            || has_unmet("files_reviewed_not_backed_by_read"))
    {
        if unreviewed_relevant_paths.is_empty() {
            if has_unmet("citations_missing") || has_unmet("research_citations_missing") {
                actions.push(
                    "No additional unreviewed files detected. If citations are missing, either: (a) re-read upstream handoff sources with `read` to extract specific proof points, or (b) add explicit `Files not reviewed` section listing sources that could not be verified with reasons.".to_string(),
                );
            } else if has_unmet("required_source_paths_not_read") {
                if !missing_required_source_read_paths.is_empty() {
                    actions.push(format!(
                        "Use `read` on the exact required source files before finalizing: {}. Similar backup or copy filenames do not satisfy the requirement.",
                        missing_required_source_read_paths.join(", ")
                    ));
                } else {
                    actions.push(
                        "Use `read` on the exact source file paths named in the workflow prompt before finalizing. Similar backup or copy filenames do not satisfy the requirement."
                            .to_string(),
                    );
                }
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
    required_output_path: Option<&str>,
    verified_output_resolution: Option<&super::AutomationVerifiedOutputResolution>,
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
    let offered_tools = preflight
        .get("offered_tools")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let resolved_output_path_absolute =
        verified_output_resolution.map(|resolution| resolution.path.to_string_lossy().to_string());
    let transcript_recovery = if required_output_path.is_none() {
        "not_attempted"
    } else if verified_output_resolution.map(|resolution| resolution.resolution_kind)
        == Some(super::AutomationVerifiedOutputResolutionKind::SessionTextRecovery)
    {
        "recovered"
    } else if verified_output_resolution.is_none() {
        "not_recoverable"
    } else {
        "not_attempted"
    };
    json!({
        "attempt": attempt,
        "created_at_ms": now_ms(),
        "session_id": session_id,
        "offered_tools": offered_tools,
        "requested_output_path": required_output_path,
        "resolved_output_path_absolute": resolved_output_path_absolute,
        "transcript_recovery_result": transcript_recovery,
        "validation_basis": Value::Null,
        "accepted_candidate_source": Value::Null,
        "blocker_category": Value::Null,
        "final_backend_actionability_state": Value::Null,
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

fn parse_status_json_with_tail_window(raw: &str) -> Option<Value> {
    parse_status_json(raw).or_else(|| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let total_chars = trimmed.chars().count();
        if total_chars <= 4000 {
            return None;
        }
        let tail = trimmed.chars().skip(total_chars - 4000).collect::<String>();
        parse_status_json(&tail)
    })
}

