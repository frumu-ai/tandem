use super::*;

fn automation_node_delivery_method_value(node: &AutomationFlowNode) -> String {
    automation_node_delivery_method(node).unwrap_or_else(|| "none".to_string())
}

fn normalized_output_target_token(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '"' | '\''
                    | '`'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | ','
                    | ';'
                    | ':'
                    | '.'
                    | '!'
                    | '?'
            )
        })
        .strip_prefix("file://")
        .unwrap_or(trimmed)
        .trim()
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '"' | '\''
                    | '`'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | ','
                    | ';'
                    | ':'
                    | '.'
                    | '!'
                    | '?'
            )
        })
        .replace('\\', "/");
    let normalized = normalized.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn canonicalize_output_path_placeholder_tokens(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut canonical = trimmed.to_string();
    for (needle, replacement) in [
        (
            "{{current_timestamp_filename}}",
            "{current_timestamp_filename}",
        ),
        ("{{current_date}}", "{current_date}"),
        ("{{current_time}}", "{current_time}"),
        ("{{current_timestamp}}", "{current_timestamp}"),
        ("{{date}}", "{current_date}"),
        ("{date}", "{current_date}"),
        ("YYYY-MM-DD_HH-MM-SS", "{current_timestamp_filename}"),
        ("YYYY-MM-DD-HH-MM-SS", "{current_timestamp_filename}"),
        ("YYYY-MM-DD_HHMMSS", "{current_timestamp_filename}"),
        ("YYYY-MM-DD-HHMMSS", "{current_timestamp_filename}"),
        ("YYYY-MM-DD_HHMM", "{current_date}_{current_time}"),
        ("YYYY-MM-DD-HHMM", "{current_date}-{current_time}"),
        ("YYYY-MM-DD", "{current_date}"),
    ] {
        canonical = canonical.replace(needle, replacement);
    }
    if !canonical.contains("HHMMSS") {
        canonical = canonical.replace("HHMM", "{current_time}");
    }
    canonical
}

fn canonicalize_output_path_list(paths: &[String]) -> (Vec<String>, bool) {
    let mut changed = false;
    let canonical = paths
        .iter()
        .filter_map(|path| {
            let canonical = canonicalize_output_path_placeholder_tokens(path);
            if canonical.is_empty() {
                return None;
            }
            if canonical != path.trim() {
                changed = true;
            }
            Some(canonical)
        })
        .collect::<Vec<_>>();
    (normalize_non_empty_list(canonical), changed)
}

fn canonicalize_output_path_value(value: &mut Value) -> bool {
    let Some(text) = value.as_str() else {
        return false;
    };
    let canonical = canonicalize_output_path_placeholder_tokens(text);
    if canonical.is_empty() || canonical == text.trim() {
        return false;
    }
    *value = Value::String(canonical);
    true
}

fn canonicalize_output_path_array(value: &mut Value) -> bool {
    let Some(items) = value.as_array() else {
        return false;
    };
    let values = items
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    let (canonical, changed) = canonicalize_output_path_list(&values);
    if !changed {
        return false;
    }
    *value = Value::Array(canonical.into_iter().map(Value::String).collect());
    true
}

pub(crate) fn canonicalize_automation_output_paths(automation: &mut AutomationV2Spec) -> bool {
    let (canonical_targets, targets_changed) =
        canonicalize_output_path_list(&automation.output_targets.clone());
    if targets_changed {
        automation.output_targets = canonical_targets;
    }

    let mut changed = targets_changed;
    for node in &mut automation.flow.nodes {
        let Some(metadata) = node.metadata.as_mut() else {
            continue;
        };
        for scope in ["builder", "studio"] {
            let Some(section) = metadata.get_mut(scope).and_then(Value::as_object_mut) else {
                continue;
            };
            if let Some(output_path) = section.get_mut("output_path") {
                changed = canonicalize_output_path_value(output_path) || changed;
            }
            for key in ["output_files", "must_write_files"] {
                if let Some(items) = section.get_mut(key) {
                    changed = canonicalize_output_path_array(items) || changed;
                }
            }
        }
    }
    changed
}

fn output_target_matches_suffix(target: &str, suffixes: &[&str]) -> bool {
    normalized_output_target_token(target)
        .is_some_and(|normalized| suffixes.iter().any(|suffix| normalized.ends_with(suffix)))
}

fn classify_output_target_contract_kind(target: &str) -> Option<&'static str> {
    if output_target_matches_suffix(target, &[".md", ".markdown", ".mdx"]) {
        return Some("report_markdown");
    }
    if output_target_matches_suffix(target, &[".txt", ".text", ".log"]) {
        return Some("text_summary");
    }
    if output_target_matches_suffix(target, &[".json", ".jsonl", ".ndjson", ".geojson"]) {
        return Some("structured_json");
    }
    if output_target_matches_suffix(
        target,
        &[
            ".py", ".rs", ".js", ".jsx", ".ts", ".tsx", ".go", ".java", ".kt", ".kts", ".swift",
            ".rb", ".php", ".c", ".cc", ".cpp", ".cxx", ".h", ".hh", ".hpp", ".html", ".htm",
            ".css", ".scss", ".less", ".xml", ".svg", ".yaml", ".yml", ".toml", ".ini", ".env",
            ".sql", ".graphql", ".gql", ".sh", ".bash", ".zsh", ".fish", ".csv", ".tsv",
        ],
    ) {
        return Some("code_patch");
    }
    None
}

fn output_contract_validator_for_kind(kind: &str) -> Option<AutomationOutputValidatorKind> {
    match kind {
        "report_markdown" | "text_summary" => Some(AutomationOutputValidatorKind::GenericArtifact),
        "structured_json" => Some(AutomationOutputValidatorKind::StructuredJson),
        "code_patch" => Some(AutomationOutputValidatorKind::CodePatch),
        _ => None,
    }
}

fn output_contract_kind_is_specialized(kind: &str) -> bool {
    matches!(
        kind,
        "brief" | "review" | "review_summary" | "approval_gate" | "urls" | "citations"
    )
}

fn output_contract_kind_is_weak(kind: &str) -> bool {
    kind.is_empty() || matches!(kind, "structured_json" | "generic_artifact")
}

fn output_writer_text_blob(step_id: &str, step_kind: &str, objective: &str) -> String {
    format!(
        "{}\n{}\n{}",
        step_id.to_ascii_lowercase(),
        step_kind.to_ascii_lowercase(),
        objective.to_ascii_lowercase()
    )
}

fn output_writer_has_writer_intent(text: &str) -> bool {
    [
        "write", "save", "export", "output", "deliver", "generate", "draft", "final", "finalize",
        "prepare", "render", "produce", "append", "patch", "update", "create",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn output_writer_has_report_intent(text: &str) -> bool {
    [
        "markdown", "report", "writeup", "write-up", "analysis", "briefing", "brief ",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn output_writer_has_text_intent(text: &str) -> bool {
    [
        "plain text",
        "text summary",
        "summary",
        "notes",
        "transcript",
        "log file",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn output_writer_mentions_target(text: &str, targets: &[String]) -> bool {
    targets.iter().any(|target| {
        normalized_output_target_token(target).is_some_and(|normalized| {
            text.contains(&normalized)
                || normalized
                    .rsplit('/')
                    .next()
                    .is_some_and(|basename| text.contains(basename))
        })
    })
}

fn infer_output_contract_kind_from_targets(
    targets: &[String],
    text: &str,
    require_writer_intent: bool,
) -> Option<&'static str> {
    if targets.is_empty() {
        return None;
    }
    let mentions_target = output_writer_mentions_target(text, targets);
    let writer_intent = output_writer_has_writer_intent(text);
    if require_writer_intent && !mentions_target && !writer_intent {
        return None;
    }
    targets
        .iter()
        .find_map(|target| classify_output_target_contract_kind(target))
}

fn automation_input_ref_alias_from_node_id(node_id: &str) -> String {
    let alias = node_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    let alias = alias.trim_matches('_').replace("__", "_");
    if alias.is_empty() {
        "upstream_artifact".to_string()
    } else if alias.ends_with("_artifact") {
        alias
    } else {
        format!("{alias}_artifact")
    }
}

fn automation_node_should_restore_upstream_inputs(
    node: &AutomationFlowNode,
    effective_contract_kind: &str,
    builder_targets: &[String],
    explicit_output_targets: &[String],
) -> bool {
    if node.depends_on.is_empty() {
        return false;
    }
    let text = output_writer_text_blob(&node.node_id, "", &node.objective);
    let writer_target_match = !builder_targets.is_empty()
        || infer_output_contract_kind_from_targets(builder_targets, &text, false).is_some()
        || infer_output_contract_kind_from_targets(explicit_output_targets, &text, true).is_some();
    writer_target_match
        || matches!(
            effective_contract_kind,
            "report_markdown" | "text_summary" | "review_summary" | "approval_gate" | "code_patch"
        )
        || [
            "summar",
            "synthes",
            "report",
            "final",
            "finalize",
            "deliverable",
            "append",
            "merge",
            "consolidat",
            "recap",
        ]
        .iter()
        .any(|needle| text.contains(needle))
}

fn ensure_upstream_input_refs(node: &mut AutomationFlowNode) -> bool {
    if node.depends_on.is_empty() {
        return false;
    }
    let mut changed = false;
    let mut existing_aliases = node
        .input_refs
        .iter()
        .map(|input_ref| input_ref.alias.trim().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    let existing_inputs = node
        .input_refs
        .iter()
        .map(|input_ref| input_ref.from_step_id.trim().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    for upstream_node_id in &node.depends_on {
        let trimmed = upstream_node_id.trim();
        if trimmed.is_empty() || existing_inputs.contains(trimmed) {
            continue;
        }
        let alias_base = automation_input_ref_alias_from_node_id(trimmed);
        let mut alias = alias_base.clone();
        let mut index = 2u32;
        while existing_aliases.contains(&alias) {
            alias = format!("{alias_base}_{index}");
            index += 1;
        }
        existing_aliases.insert(alias.clone());
        node.input_refs.push(AutomationFlowInputRef {
            from_step_id: trimmed.to_string(),
            alias,
        });
        changed = true;
    }
    changed
}

fn ensure_report_or_text_summary_guidance(node: &mut AutomationFlowNode) -> bool {
    let Some(contract) = node.output_contract.as_mut() else {
        return false;
    };
    let kind = contract.kind.trim().to_ascii_lowercase();
    if !matches!(kind.as_str(), "report_markdown" | "text_summary") {
        return false;
    }
    let upstream_node_ids = node
        .input_refs
        .iter()
        .map(|input_ref| input_ref.from_step_id.trim())
        .filter(|value| !value.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    if upstream_node_ids.is_empty() {
        return false;
    }
    let upstream_summary = upstream_node_ids
        .iter()
        .take(4)
        .map(|node_id| format!("`{node_id}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let synthesis_guidance = format!(
        "Read and synthesize the strongest upstream artifacts from {}. Reuse the concrete filenames, named entities, URLs, counts, match reasons, risks, and proof points from those upstream steps instead of producing a generic recap.",
        upstream_summary
    );
    match contract.summary_guidance.take() {
        Some(existing)
            if existing
                .to_ascii_lowercase()
                .contains("read and synthesize the strongest upstream artifacts") =>
        {
            contract.summary_guidance = Some(existing);
            false
        }
        Some(existing) if existing.trim().is_empty() => {
            contract.summary_guidance = Some(synthesis_guidance);
            true
        }
        Some(existing) => {
            contract.summary_guidance = Some(format!("{existing}\n\n{synthesis_guidance}"));
            true
        }
        None => {
            contract.summary_guidance = Some(synthesis_guidance);
            true
        }
    }
}

pub(crate) fn automation_builder_declared_output_targets(metadata: Option<&Value>) -> Vec<String> {
    let Some(builder) = metadata
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };
    let mut targets = Vec::new();
    if let Some(path) = builder
        .get("output_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        targets.push(path.to_string());
    }
    for key in ["output_files", "must_write_files"] {
        if let Some(items) = builder.get(key).and_then(Value::as_array) {
            targets.extend(
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
            );
        }
    }
    normalize_non_empty_list(targets)
}

pub(crate) fn infer_automation_output_contract(
    step_id: &str,
    step_kind: &str,
    objective: &str,
    current_contract: Option<&AutomationFlowOutputContract>,
    explicit_output_targets: &[String],
    builder_output_targets: &[String],
) -> Option<AutomationFlowOutputContract> {
    let current_kind = current_contract
        .map(|contract| contract.kind.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if output_contract_kind_is_specialized(&current_kind) {
        return None;
    }

    let text = output_writer_text_blob(step_id, step_kind, objective);
    let inferred_kind =
        infer_output_contract_kind_from_targets(builder_output_targets, &text, false)
            .or_else(|| {
                infer_output_contract_kind_from_targets(explicit_output_targets, &text, true)
            })
            .or_else(|| {
                if !output_contract_kind_is_weak(&current_kind)
                    || !output_writer_has_writer_intent(&text)
                {
                    return None;
                }
                if output_writer_has_report_intent(&text) {
                    Some("report_markdown")
                } else if output_writer_has_text_intent(&text) {
                    Some("text_summary")
                } else {
                    None
                }
            })?;

    if !output_contract_kind_is_weak(&current_kind) && current_kind != inferred_kind {
        return None;
    }

    let mut contract = current_contract
        .cloned()
        .unwrap_or_else(|| AutomationFlowOutputContract {
            kind: inferred_kind.to_string(),
            validator: output_contract_validator_for_kind(inferred_kind),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        });
    contract.kind = inferred_kind.to_string();
    contract.validator = output_contract_validator_for_kind(inferred_kind);
    if inferred_kind != "structured_json" {
        contract.schema = None;
    }
    Some(contract)
}

pub(crate) fn repair_automation_output_contracts(automation: &mut AutomationV2Spec) -> bool {
    let explicit_output_targets = normalize_non_empty_list(automation.output_targets.clone());
    let mut changed = false;
    for node in &mut automation.flow.nodes {
        let before_input_refs = serde_json::to_value(&node.input_refs).ok();
        let before_contract = serde_json::to_value(&node.output_contract).ok();
        let builder_targets = automation_builder_declared_output_targets(node.metadata.as_ref());
        if let Some(contract) = infer_automation_output_contract(
            &node.node_id,
            "",
            &node.objective,
            node.output_contract.as_ref(),
            &explicit_output_targets,
            &builder_targets,
        ) {
            node.output_contract = Some(contract);
        }
        let effective_contract_kind = node
            .output_contract
            .as_ref()
            .map(|contract| contract.kind.trim().to_ascii_lowercase())
            .unwrap_or_default();
        if automation_node_should_restore_upstream_inputs(
            node,
            &effective_contract_kind,
            &builder_targets,
            &explicit_output_targets,
        ) {
            changed = ensure_upstream_input_refs(node) || changed;
        }
        changed = ensure_report_or_text_summary_guidance(node) || changed;
        let after_input_refs = serde_json::to_value(&node.input_refs).ok();
        let after_contract = serde_json::to_value(&node.output_contract).ok();
        if before_input_refs != after_input_refs || before_contract != after_contract {
            changed = true;
        }
    }
    changed
}

pub(crate) fn automation_output_session_id(output: &Value) -> Option<String> {
    output
        .get("content")
        .and_then(Value::as_object)
        .and_then(|content| {
            content
                .get("session_id")
                .or_else(|| content.get("sessionId"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn build_automation_pending_gate(
    node: &AutomationFlowNode,
) -> Option<AutomationPendingGate> {
    let gate = node.gate.as_ref()?;
    Some(AutomationPendingGate {
        node_id: node.node_id.clone(),
        title: node
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("builder"))
            .and_then(|builder| builder.get("title"))
            .and_then(Value::as_str)
            .unwrap_or(node.objective.as_str())
            .to_string(),
        instructions: gate.instructions.clone(),
        decisions: gate.decisions.clone(),
        rework_targets: gate.rework_targets.clone(),
        requested_at_ms: now_ms(),
        upstream_node_ids: node.depends_on.clone(),
    })
}

fn automation_node_builder_metadata(node: &AutomationFlowNode, key: &str) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(|builder| builder.get(key))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn automation_node_research_stage(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "research_stage")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

fn automation_node_is_research_finalize(node: &AutomationFlowNode) -> bool {
    automation_node_research_stage(node).as_deref() == Some("research_finalize")
}

fn automation_node_builder_priority(node: &AutomationFlowNode) -> i32 {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(|builder| builder.get("priority"))
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or(0)
}

fn truncate_path_list_for_prompt(paths: Vec<String>, limit: usize) -> Vec<String> {
    let mut deduped = normalize_non_empty_list(paths);
    if deduped.len() > limit {
        deduped.truncate(limit);
    }
    deduped
}

fn value_object_path_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
}

fn render_research_finalize_upstream_summary(upstream_inputs: &[Value]) -> Option<String> {
    let source_inventory =
        automation_upstream_output_for_alias(upstream_inputs, "source_inventory")
            .and_then(automation_upstream_structured_handoff);
    let local_source_notes =
        automation_upstream_output_for_alias(upstream_inputs, "local_source_notes")
            .and_then(automation_upstream_structured_handoff);
    let external_research =
        automation_upstream_output_for_alias(upstream_inputs, "external_research")
            .and_then(automation_upstream_structured_handoff);

    let discovered_files = source_inventory
        .and_then(|handoff| handoff.get("discovered_paths"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| match row {
                    Value::String(path) => Some(path.trim().to_string()),
                    Value::Object(_) => value_object_path_field(row, "path"),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let priority_files = source_inventory
        .and_then(|handoff| handoff.get("priority_paths"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| match row {
                    Value::String(path) => Some(path.trim().to_string()),
                    Value::Object(_) => value_object_path_field(row, "path"),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let files_reviewed = local_source_notes
        .and_then(|handoff| handoff.get("files_reviewed"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| match row {
                    Value::String(path) => Some(path.trim().to_string()),
                    Value::Object(_) => value_object_path_field(row, "path"),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let files_not_reviewed = local_source_notes
        .and_then(|handoff| handoff.get("files_not_reviewed"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| match row {
                    Value::String(path) => Some(path.trim().to_string()),
                    Value::Object(_) => value_object_path_field(row, "path"),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let web_sources_reviewed = external_research
        .and_then(|handoff| handoff.get("sources_reviewed"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| match row {
                    Value::String(path) => Some(path.trim().to_string()),
                    Value::Object(_) => value_object_path_field(row, "url")
                        .or_else(|| value_object_path_field(row, "path")),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let discovered_files = truncate_path_list_for_prompt(discovered_files, 12);
    let priority_files = truncate_path_list_for_prompt(priority_files, 12);
    let files_reviewed = truncate_path_list_for_prompt(files_reviewed, 12);
    let files_not_reviewed = truncate_path_list_for_prompt(files_not_reviewed, 12);
    let web_sources_reviewed = truncate_path_list_for_prompt(web_sources_reviewed, 8);

    if discovered_files.is_empty()
        && priority_files.is_empty()
        && files_reviewed.is_empty()
        && files_not_reviewed.is_empty()
        && web_sources_reviewed.is_empty()
    {
        return None;
    }

    let list_or_none = |items: &[String]| {
        if items.is_empty() {
            "none recorded".to_string()
        } else {
            items
                .iter()
                .map(|item| format!("- `{}`", item))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    Some(format!(
        "Research Coverage Summary:\nRelevant discovered files from upstream:\n{}\nPriority paths from upstream:\n{}\nUpstream files already reviewed:\n{}\nUpstream files already marked not reviewed:\n{}\nUpstream web sources reviewed:\n{}\nFinal brief rule: every relevant discovered file should appear in `Files reviewed` or `Files not reviewed`, and proof points must stay citation-backed.",
        list_or_none(&discovered_files),
        list_or_none(&priority_files),
        list_or_none(&files_reviewed),
        list_or_none(&files_not_reviewed),
        list_or_none(&web_sources_reviewed),
    ))
}

fn split_research_template_config(template_id: &str) -> Option<SplitResearchTemplateConfig> {
    match template_id {
        "marketing-content-pipeline" => Some(SplitResearchTemplateConfig {
            template_id: "marketing-content-pipeline",
            final_node_id: "research-brief",
            final_agent_id: "research",
            discover_node_id: "research-discover-sources",
            discover_agent_id: "research-discover",
            discover_title: "Discover Sources",
            discover_objective: "Enumerate the workspace, identify the relevant source corpus, and prioritize which local files must be read for the marketing brief.",
            discover_display_name: "Research Discover",
            local_node_id: "research-local-sources",
            local_agent_id: "research-local-sources",
            local_title: "Read Local Sources",
            local_objective: "Read the prioritized local product and marketing files and produce source-backed notes for the brief.",
            local_display_name: "Research Local Sources",
            external_node_id: "research-external-research",
            external_agent_id: "research-external",
            external_title: "External Research",
            external_objective: "Perform targeted external research that complements the local source notes and record what web evidence was gathered or unavailable.",
            external_display_name: "Research External",
            final_title: "Research Brief",
            final_objective: "Write `marketing-brief.md` from the structured discovery, local source notes, and external research gathered earlier in the workflow.",
        }),
        "competitor-research-pipeline" => Some(SplitResearchTemplateConfig {
            template_id: "competitor-research-pipeline",
            final_node_id: "scan-market",
            final_agent_id: "market-scan",
            discover_node_id: "scan-market-discover",
            discover_agent_id: "market-discover",
            discover_title: "Discover Market Sources",
            discover_objective: "Identify the local source corpus and file inventory that should guide the competitor scan.",
            discover_display_name: "Market Discover",
            local_node_id: "scan-market-local-sources",
            local_agent_id: "market-local-sources",
            local_title: "Read Market Sources",
            local_objective: "Read the prioritized local competitor and strategy sources before external scanning.",
            local_display_name: "Market Local Sources",
            external_node_id: "scan-market-external-research",
            external_agent_id: "market-external",
            external_title: "Research Market",
            external_objective: "Gather current external competitor evidence guided by the local market context.",
            external_display_name: "Market External",
            final_title: "Scan Market",
            final_objective: "Synthesize the discovered local and external evidence into the final competitor scan.",
        }),
        "weekly-newsletter-builder" => Some(SplitResearchTemplateConfig {
            template_id: "weekly-newsletter-builder",
            final_node_id: "curate-issue",
            final_agent_id: "curator",
            discover_node_id: "curate-issue-discover",
            discover_agent_id: "curator-discover",
            discover_title: "Discover Issue Sources",
            discover_objective: "Identify the local source corpus and candidate files that should feed this week's issue.",
            discover_display_name: "Curator Discover",
            local_node_id: "curate-issue-local-sources",
            local_agent_id: "curator-local-sources",
            local_title: "Read Issue Sources",
            local_objective: "Read the prioritized local source files and extract the strongest issue candidates.",
            local_display_name: "Curator Local Sources",
            external_node_id: "curate-issue-external-research",
            external_agent_id: "curator-external",
            external_title: "Research Issue",
            external_objective: "Gather timely external signals that should influence this week's issue.",
            external_display_name: "Curator External",
            final_title: "Curate Issue",
            final_objective: "Curate the best items for this week's issue from the staged research handoffs.",
        }),
        "sales-prospecting-team" => Some(SplitResearchTemplateConfig {
            template_id: "sales-prospecting-team",
            final_node_id: "research-account",
            final_agent_id: "account-research",
            discover_node_id: "research-account-discover",
            discover_agent_id: "account-discover",
            discover_title: "Discover Account Sources",
            discover_objective: "Identify the source corpus that should guide account research.",
            discover_display_name: "Account Discover",
            local_node_id: "research-account-local-sources",
            local_agent_id: "account-local-sources",
            local_title: "Read Account Sources",
            local_objective: "Read the prioritized local account and ICP files before drafting the account brief.",
            local_display_name: "Account Local Sources",
            external_node_id: "research-account-external-research",
            external_agent_id: "account-external",
            external_title: "Research Account Externally",
            external_objective: "Gather targeted external account context and buying signals to support the brief.",
            external_display_name: "Account External",
            final_title: "Research Account",
            final_objective: "Prepare the final account brief from the staged discovery, local evidence, and external research.",
        }),
        _ => None,
    }
}

fn studio_template_id(automation: &AutomationV2Spec) -> Option<String> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("studio"))
        .and_then(Value::as_object)
        .and_then(|studio| {
            studio
                .get("template_id")
                .or_else(|| studio.get("templateId"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn split_research_stage_metadata(
    title: &str,
    role: &str,
    prompt: String,
    research_stage: &str,
    output_path: Option<&str>,
    required_tools: &[&str],
    write_required: bool,
) -> Option<Value> {
    let mut builder = serde_json::Map::new();
    builder.insert("title".to_string(), json!(title));
    builder.insert("role".to_string(), json!(role));
    builder.insert("prompt".to_string(), json!(prompt));
    builder.insert("research_stage".to_string(), json!(research_stage));
    if let Some(path) = output_path {
        builder.insert("output_path".to_string(), json!(path));
    }
    if !required_tools.is_empty() {
        builder.insert("required_tools".to_string(), json!(required_tools));
    }
    if write_required {
        builder.insert("write_required".to_string(), json!(true));
    }
    let mut studio = serde_json::Map::new();
    studio.insert("research_stage".to_string(), json!(research_stage));
    if let Some(path) = output_path {
        studio.insert("output_path".to_string(), json!(path));
    }
    Some(json!({
        "builder": Value::Object(builder),
        "studio": Value::Object(studio),
    }))
}

fn migrated_stage_agent(
    base: &AutomationAgentProfile,
    agent_id: &str,
    display_name: &str,
    allowlist: &[&str],
) -> AutomationAgentProfile {
    let mut agent = base.clone();
    agent.agent_id = agent_id.to_string();
    agent.display_name = display_name.to_string();
    agent.template_id = None;
    agent.tool_policy.allowlist = config::channels::normalize_allowed_tools(
        allowlist.iter().map(|value| (*value).to_string()).collect(),
    );
    agent.tool_policy.denylist =
        config::channels::normalize_allowed_tools(agent.tool_policy.denylist.clone());
    agent
}

fn migrate_split_research_studio_metadata(metadata: &mut Value) {
    let Some(root) = metadata.as_object_mut() else {
        return;
    };
    let studio = root
        .entry("studio".to_string())
        .or_insert_with(|| json!({}));
    let Some(studio_obj) = studio.as_object_mut() else {
        return;
    };
    studio_obj.insert("version".to_string(), json!(2));
    studio_obj.insert("workflow_structure_version".to_string(), json!(2));
    studio_obj.remove("agent_drafts");
    studio_obj.remove("node_drafts");
    studio_obj.remove("node_layout");
}

pub(crate) fn migrate_bundled_studio_research_split_automation(
    automation: &mut AutomationV2Spec,
) -> bool {
    let Some(template_id) = studio_template_id(automation) else {
        return false;
    };
    let Some(config) = split_research_template_config(&template_id) else {
        return false;
    };
    if automation
        .flow
        .nodes
        .iter()
        .any(|node| node.node_id == config.discover_node_id)
        || automation
            .flow
            .nodes
            .iter()
            .find(|node| node.node_id == config.final_node_id)
            .is_some_and(automation_node_is_research_finalize)
    {
        if let Some(metadata) = automation.metadata.as_mut() {
            migrate_split_research_studio_metadata(metadata);
        }
        return false;
    }
    let Some(final_node_index) = automation
        .flow
        .nodes
        .iter()
        .position(|node| node.node_id == config.final_node_id)
    else {
        return false;
    };
    let Some(base_agent) = automation
        .agents
        .iter()
        .find(|agent| agent.agent_id == config.final_agent_id)
        .cloned()
    else {
        return false;
    };
    let existing_final_node = automation.flow.nodes[final_node_index].clone();
    let output_path = automation_node_required_output_path(&existing_final_node);
    let final_contract_kind = existing_final_node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.clone())
        .unwrap_or_else(|| "artifact".to_string());
    let final_is_brief_like = final_contract_kind.trim().eq_ignore_ascii_case("brief");
    let final_summary_guidance = existing_final_node
        .output_contract
        .as_ref()
        .and_then(|contract| contract.summary_guidance.clone());
    let discover_prompt = "Enumerate the workspace, identify the relevant source corpus, and return a structured handoff with `workspace_inventory_summary`, `discovered_paths`, `priority_paths`, and `skipped_paths_initial`. If a curated source index such as `SOURCES.md` exists, read it first. Perform at least one concrete `read` before finishing, but read only enough to identify the corpus for the next stage. Do not write final workspace artifacts in this stage.".to_string();
    let local_prompt = "Use the upstream `source_inventory` handoff to decide which concrete local files to read. Perform concrete `read` calls, extract the product or market facts supported by those reads, and return a structured handoff with `read_paths`, `reviewed_facts`, `files_reviewed`, `files_not_reviewed`, and `citations_local`. Do not invent facts from filenames alone.".to_string();
    let external_prompt = "Use the upstream `source_inventory` and `local_source_notes` handoffs to guide targeted external research. Perform `websearch` and fetch result pages when snippets are not enough, then return `external_research_mode`, `queries_attempted`, `sources_reviewed`, `citations_external`, and `research_limitations`. If search is unavailable, record that limitation clearly instead of inventing evidence.".to_string();
    let final_prompt = match config.template_id {
        "marketing-content-pipeline" => "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs as the source of truth. Read `marketing-brief.md` from disk only as a fallback or verification step. Synthesize the final marketing brief from those handoffs instead of repeating discovery or fresh web research in this stage. Include a workspace source audit, audience, positioning, proof points with citations, `Files reviewed`, `Files not reviewed`, and `Web sources reviewed`, and clearly note any research limitations. In source-audit sections, list only exact concrete workspace-relative file paths or exact reviewed URLs; do not use directory names, wildcard paths, or glob patterns.".to_string(),
        "competitor-research-pipeline" => "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs as the source of truth for the final competitor scan. Separate observed evidence from inference, keep the scan current and signal-focused, and do not rerun discovery or fresh web research in this stage.".to_string(),
        "weekly-newsletter-builder" => "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs to curate the final issue. Turn them into the final shortlist and section order without repeating discovery or fresh web research in this stage.".to_string(),
        "sales-prospecting-team" => "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs as the source of truth for the final account brief. Separate observed facts from hypotheses and do not rerun discovery or fresh web research in this stage.".to_string(),
        _ => "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs as the source of truth and synthesize the final artifact without repeating discovery or fresh web research in this stage.".to_string(),
    };

    let discover_node = AutomationFlowNode {
        node_id: config.discover_node_id.to_string(),
        agent_id: config.discover_agent_id.to_string(),
        objective: config.discover_objective.to_string(),
        knowledge: Default::default(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: Some(crate::AutomationOutputEnforcement {
                validation_profile: Some("local_research".to_string()),
                required_tools: vec!["read".to_string()],
                required_evidence: vec!["local_source_reads".to_string()],
                required_sections: Vec::new(),
                prewrite_gates: vec![
                    "workspace_inspection".to_string(),
                    "concrete_reads".to_string(),
                ],
                retry_on_missing: vec![
                    "local_source_reads".to_string(),
                    "workspace_inspection".to_string(),
                    "concrete_reads".to_string(),
                ],
                terminal_on: vec![
                    "tool_unavailable".to_string(),
                    "repair_budget_exhausted".to_string(),
                ],
                repair_budget: Some(5),
                session_text_recovery: Some("require_prewrite_satisfied".to_string()),
            }),
            schema: None,
            summary_guidance: Some(
                "Return a structured handoff in the final response instead of writing workspace files."
                    .to_string(),
            ),
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: split_research_stage_metadata(
            config.discover_title,
            "watcher",
            discover_prompt,
            "research_discover",
            None,
            &["glob", "read"],
            false,
        ),
    };
    let local_node = AutomationFlowNode {
        node_id: config.local_node_id.to_string(),
        agent_id: config.local_agent_id.to_string(),
        objective: config.local_objective.to_string(),
        knowledge: Default::default(),
        depends_on: vec![config.discover_node_id.to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: config.discover_node_id.to_string(),
            alias: "source_inventory".to_string(),
        }],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: Some(crate::AutomationOutputEnforcement {
                validation_profile: Some("local_research".to_string()),
                required_tools: vec!["read".to_string()],
                required_evidence: vec!["local_source_reads".to_string()],
                required_sections: Vec::new(),
                prewrite_gates: vec!["concrete_reads".to_string()],
                retry_on_missing: vec![
                    "local_source_reads".to_string(),
                    "concrete_reads".to_string(),
                ],
                terminal_on: vec![
                    "tool_unavailable".to_string(),
                    "repair_budget_exhausted".to_string(),
                ],
                repair_budget: Some(5),
                session_text_recovery: Some("require_prewrite_satisfied".to_string()),
            }),
            schema: None,
            summary_guidance: Some(
                "Return a structured handoff backed by concrete local file reads.".to_string(),
            ),
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: split_research_stage_metadata(
            config.local_title,
            "watcher",
            local_prompt,
            "research_local_sources",
            None,
            &["read"],
            false,
        ),
    };
    let external_node = AutomationFlowNode {
        node_id: config.external_node_id.to_string(),
        agent_id: config.external_agent_id.to_string(),
        objective: config.external_objective.to_string(),
        knowledge: Default::default(),
        depends_on: vec![
            config.discover_node_id.to_string(),
            config.local_node_id.to_string(),
        ],
        input_refs: vec![
            AutomationFlowInputRef {
                from_step_id: config.discover_node_id.to_string(),
                alias: "source_inventory".to_string(),
            },
            AutomationFlowInputRef {
                from_step_id: config.local_node_id.to_string(),
                alias: "local_source_notes".to_string(),
            },
        ],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: Some(crate::AutomationOutputEnforcement {
                validation_profile: Some("external_research".to_string()),
                required_tools: vec!["websearch".to_string()],
                required_evidence: vec!["external_sources".to_string()],
                required_sections: Vec::new(),
                prewrite_gates: vec!["successful_web_research".to_string()],
                retry_on_missing: vec![
                    "external_sources".to_string(),
                    "successful_web_research".to_string(),
                ],
                terminal_on: vec![
                    "tool_unavailable".to_string(),
                    "repair_budget_exhausted".to_string(),
                ],
                repair_budget: Some(5),
                session_text_recovery: Some("require_prewrite_satisfied".to_string()),
            }),
            schema: None,
            summary_guidance: Some(
                "Return a structured handoff describing external research findings or limitations."
                    .to_string(),
            ),
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: split_research_stage_metadata(
            config.external_title,
            "watcher",
            external_prompt,
            "research_external_sources",
            None,
            &["websearch", "webfetch", "read"],
            false,
        ),
    };
    let mut final_node = existing_final_node.clone();
    final_node.objective = config.final_objective.to_string();
    final_node.depends_on = vec![
        config.discover_node_id.to_string(),
        config.local_node_id.to_string(),
        config.external_node_id.to_string(),
    ];
    final_node.input_refs = vec![
        AutomationFlowInputRef {
            from_step_id: config.discover_node_id.to_string(),
            alias: "source_inventory".to_string(),
        },
        AutomationFlowInputRef {
            from_step_id: config.local_node_id.to_string(),
            alias: "local_source_notes".to_string(),
        },
        AutomationFlowInputRef {
            from_step_id: config.external_node_id.to_string(),
            alias: "external_research".to_string(),
        },
    ];
    final_node.stage_kind = Some(AutomationNodeStageKind::Workstream);
    final_node.output_contract = Some(AutomationFlowOutputContract {
        kind: final_contract_kind,
        validator: existing_final_node
            .output_contract
            .as_ref()
            .and_then(|contract| contract.validator)
            .or(if final_is_brief_like {
                Some(crate::AutomationOutputValidatorKind::ResearchBrief)
            } else {
                None
            }),
        enforcement: Some(crate::AutomationOutputEnforcement {
            validation_profile: Some("research_synthesis".to_string()),
            required_tools: Vec::new(),
            required_evidence: vec![
                "local_source_reads".to_string(),
                "external_sources".to_string(),
            ],
            required_sections: if final_is_brief_like {
                vec!["citations".to_string()]
            } else {
                Vec::new()
            },
            prewrite_gates: Vec::new(),
            retry_on_missing: if final_is_brief_like {
                vec![
                    "local_source_reads".to_string(),
                    "external_sources".to_string(),
                    "citations".to_string(),
                ]
            } else {
                vec![
                    "local_source_reads".to_string(),
                    "external_sources".to_string(),
                ]
            },
            terminal_on: vec![
                "tool_unavailable".to_string(),
                "repair_budget_exhausted".to_string(),
            ],
            repair_budget: Some(5),
            session_text_recovery: Some("require_prewrite_satisfied".to_string()),
        }),
        schema: existing_final_node
            .output_contract
            .as_ref()
            .and_then(|contract| contract.schema.clone()),
        summary_guidance: final_summary_guidance,
    });
    final_node.metadata = split_research_stage_metadata(
        config.final_title,
        "watcher",
        final_prompt,
        "research_finalize",
        output_path.as_deref(),
        &[],
        output_path.is_some(),
    );

    let mut new_nodes = Vec::with_capacity(automation.flow.nodes.len() + 3);
    let mut inserted = false;
    for node in automation.flow.nodes.clone() {
        if node.node_id == config.final_node_id {
            new_nodes.push(discover_node.clone());
            new_nodes.push(local_node.clone());
            new_nodes.push(external_node.clone());
            new_nodes.push(final_node.clone());
            inserted = true;
        } else if node.node_id != config.discover_node_id
            && node.node_id != config.local_node_id
            && node.node_id != config.external_node_id
        {
            new_nodes.push(node);
        }
    }
    if !inserted {
        return false;
    }
    automation.flow.nodes = new_nodes;

    for candidate in [
        migrated_stage_agent(
            &base_agent,
            config.discover_agent_id,
            config.discover_display_name,
            &["glob", "read"],
        ),
        migrated_stage_agent(
            &base_agent,
            config.local_agent_id,
            config.local_display_name,
            &["read"],
        ),
        migrated_stage_agent(
            &base_agent,
            config.external_agent_id,
            config.external_display_name,
            &["websearch", "webfetch", "read"],
        ),
    ] {
        if !automation
            .agents
            .iter()
            .any(|agent| agent.agent_id == candidate.agent_id)
        {
            automation.agents.push(candidate);
        }
    }
    if let Some(final_agent) = automation
        .agents
        .iter_mut()
        .find(|agent| agent.agent_id == config.final_agent_id)
    {
        final_agent.tool_policy.allowlist = config::channels::normalize_allowed_tools(vec![
            "read".to_string(),
            "write".to_string(),
        ]);
    }
    if let Some(metadata) = automation.metadata.as_mut() {
        migrate_split_research_studio_metadata(metadata);
    } else {
        automation.metadata = Some(json!({
            "studio": {
                "template_id": config.template_id,
                "version": 2,
                "workflow_structure_version": 2
            }
        }));
    }
    true
}

fn automation_phase_execution_mode_map(
    automation: &AutomationV2Spec,
) -> std::collections::HashMap<String, String> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("mission"))
        .and_then(|mission| mission.get("phases"))
        .and_then(Value::as_array)
        .map(|phases| {
            phases
                .iter()
                .filter_map(|phase| {
                    let phase_id = phase.get("phase_id").and_then(Value::as_str)?.trim();
                    if phase_id.is_empty() {
                        return None;
                    }
                    let mode = phase
                        .get("execution_mode")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .unwrap_or("soft");
                    Some((phase_id.to_string(), mode.to_string()))
                })
                .collect::<std::collections::HashMap<_, _>>()
        })
        .unwrap_or_default()
}

pub(crate) fn automation_current_open_phase(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> Option<(String, usize, String)> {
    let phase_rank = automation_phase_rank_map(automation);
    if phase_rank.is_empty() {
        return None;
    }
    let phase_modes = automation_phase_execution_mode_map(automation);
    let completed = run
        .checkpoint
        .completed_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    automation
        .flow
        .nodes
        .iter()
        .filter(|node| !completed.contains(&node.node_id))
        .filter_map(|node| {
            automation_node_builder_metadata(node, "phase_id").and_then(|phase_id| {
                phase_rank
                    .get(&phase_id)
                    .copied()
                    .map(|rank| (phase_id, rank))
            })
        })
        .min_by_key(|(_, rank)| *rank)
        .map(|(phase_id, rank)| {
            let mode = phase_modes
                .get(&phase_id)
                .cloned()
                .unwrap_or_else(|| "soft".to_string());
            (phase_id, rank, mode)
        })
}

pub(crate) fn automation_phase_rank_map(
    automation: &AutomationV2Spec,
) -> std::collections::HashMap<String, usize> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("mission"))
        .and_then(|mission| mission.get("phases"))
        .and_then(Value::as_array)
        .map(|phases| {
            phases
                .iter()
                .enumerate()
                .filter_map(|(index, phase)| {
                    phase
                        .get("phase_id")
                        .and_then(Value::as_str)
                        .map(|phase_id| (phase_id.to_string(), index))
                })
                .collect::<std::collections::HashMap<_, _>>()
        })
        .unwrap_or_default()
}

pub(crate) fn automation_node_sort_key(
    node: &AutomationFlowNode,
    phase_rank: &std::collections::HashMap<String, usize>,
    current_open_phase_rank: Option<usize>,
) -> (usize, usize, i32, String) {
    let phase_order = automation_node_builder_metadata(node, "phase_id")
        .as_ref()
        .and_then(|phase_id| phase_rank.get(phase_id))
        .copied()
        .unwrap_or(usize::MAX / 2);
    let open_phase_bias = current_open_phase_rank
        .map(|open_rank| usize::from(phase_order != open_rank))
        .unwrap_or(0);
    (
        open_phase_bias,
        phase_order,
        -automation_node_builder_priority(node),
        node.node_id.clone(),
    )
}

pub(crate) fn automation_filter_runnable_by_open_phase(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
    runnable: Vec<AutomationFlowNode>,
) -> Vec<AutomationFlowNode> {
    let Some((_, open_rank, _)) = automation_current_open_phase(automation, run) else {
        return runnable;
    };
    let phase_rank = automation_phase_rank_map(automation);
    let in_open_phase = runnable
        .iter()
        .filter(|node| {
            automation_node_builder_metadata(node, "phase_id")
                .as_ref()
                .and_then(|phase_id| phase_rank.get(phase_id))
                .copied()
                == Some(open_rank)
        })
        .cloned()
        .collect::<Vec<_>>();
    if in_open_phase.is_empty() {
        runnable
    } else {
        in_open_phase
    }
}

fn normalize_write_scope_entries(scope: Option<String>) -> Vec<String> {
    let Some(scope) = scope else {
        return vec!["__repo__".to_string()];
    };
    let entries = scope
        .split(|ch| matches!(ch, ',' | '\n' | ';'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_matches('/').to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if entries.is_empty() {
        vec!["__repo__".to_string()]
    } else {
        entries
    }
}

fn write_scope_entries_conflict(left: &[String], right: &[String]) -> bool {
    left.iter().any(|a| {
        right.iter().any(|b| {
            a == "__repo__"
                || b == "__repo__"
                || a == b
                || a == "."
                || b == "."
                || a == "*"
                || b == "*"
                || a.starts_with(&format!("{}/", b))
                || b.starts_with(&format!("{}/", a))
        })
    })
}

pub(crate) fn automation_filter_runnable_by_write_scope_conflicts(
    runnable: Vec<AutomationFlowNode>,
    max_parallel: usize,
) -> Vec<AutomationFlowNode> {
    if max_parallel <= 1 {
        return runnable.into_iter().take(1).collect();
    }
    let mut selected = Vec::new();
    let mut selected_scopes = Vec::<Vec<String>>::new();
    for node in runnable {
        let is_code = automation_node_is_code_workflow(&node);
        let scope_entries = if is_code {
            normalize_write_scope_entries(automation_node_write_scope(&node))
        } else {
            Vec::new()
        };
        let conflicts = is_code
            && selected.iter().enumerate().any(|(index, existing)| {
                automation_node_is_code_workflow(existing)
                    && write_scope_entries_conflict(&scope_entries, &selected_scopes[index])
            });
        if conflicts {
            continue;
        }
        if is_code {
            selected_scopes.push(scope_entries);
        } else {
            selected_scopes.push(Vec::new());
        }
        selected.push(node);
        if selected.len() >= max_parallel {
            break;
        }
    }
    selected
}

pub(crate) fn automation_blocked_nodes(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> Vec<String> {
    let completed = run
        .checkpoint
        .completed_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let pending = run
        .checkpoint
        .pending_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let phase_rank = automation_phase_rank_map(automation);
    let current_open_phase = automation_current_open_phase(automation, run);
    automation
        .flow
        .nodes
        .iter()
        .filter(|node| pending.contains(&node.node_id))
        .filter_map(|node| {
            let missing_deps = node.depends_on.iter().any(|dep| !completed.contains(dep));
            if missing_deps {
                return Some(node.node_id.clone());
            }
            let Some((_, open_rank, mode)) = current_open_phase.as_ref() else {
                return None;
            };
            if mode != "barrier" {
                return None;
            }
            let node_phase_rank = automation_node_builder_metadata(node, "phase_id")
                .as_ref()
                .and_then(|phase_id| phase_rank.get(phase_id))
                .copied();
            if node_phase_rank.is_some_and(|rank| rank > *open_rank) {
                return Some(node.node_id.clone());
            }
            None
        })
        .collect::<Vec<_>>()
}

pub(crate) fn record_automation_open_phase_event(
    automation: &AutomationV2Spec,
    run: &mut AutomationV2RunRecord,
) {
    let Some((phase_id, phase_rank, execution_mode)) =
        automation_current_open_phase(automation, run)
    else {
        return;
    };
    let last_recorded = run
        .checkpoint
        .lifecycle_history
        .iter()
        .rev()
        .find(|entry| entry.event == "phase_opened")
        .and_then(|entry| entry.metadata.as_ref())
        .and_then(|metadata| metadata.get("phase_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    if last_recorded.as_deref() == Some(phase_id.as_str()) {
        return;
    }
    record_automation_lifecycle_event_with_metadata(
        run,
        "phase_opened",
        Some(format!("phase `{}` is now open", phase_id)),
        None,
        Some(json!({
            "phase_id": phase_id,
            "phase_rank": phase_rank,
            "execution_mode": execution_mode,
        })),
    );
}

pub fn refresh_automation_runtime_state(
    automation: &AutomationV2Spec,
    run: &mut AutomationV2RunRecord,
) {
    run.checkpoint.blocked_nodes = automation_blocked_nodes(automation, run);
    record_automation_open_phase_event(automation, run);
}

fn automation_mission_milestones(automation: &AutomationV2Spec) -> Vec<Value> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("mission"))
        .and_then(|mission| mission.get("milestones"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn completed_mission_milestones(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> std::collections::HashSet<String> {
    let completed = run
        .checkpoint
        .completed_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    automation_mission_milestones(automation)
        .iter()
        .filter_map(|milestone| {
            let milestone_id = milestone
                .get("milestone_id")
                .and_then(Value::as_str)?
                .trim();
            if milestone_id.is_empty() {
                return None;
            }
            let required = milestone
                .get("required_stage_ids")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter_map(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            (!required.is_empty()
                && required
                    .iter()
                    .all(|stage_id| completed.contains(*stage_id)))
            .then_some(milestone_id.to_string())
        })
        .collect()
}

pub(crate) fn record_milestone_promotions(
    automation: &AutomationV2Spec,
    row: &mut AutomationV2RunRecord,
    promoted_by_node_id: &str,
) {
    let already_recorded = row
        .checkpoint
        .lifecycle_history
        .iter()
        .filter(|entry| entry.event == "milestone_promoted")
        .filter_map(|entry| {
            entry.metadata.as_ref().and_then(|metadata| {
                metadata
                    .get("milestone_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
        })
        .collect::<std::collections::HashSet<_>>();
    let completed = completed_mission_milestones(automation, row);
    for milestone in automation_mission_milestones(automation) {
        let milestone_id = milestone
            .get("milestone_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if milestone_id.is_empty()
            || !completed.contains(milestone_id)
            || already_recorded.contains(milestone_id)
        {
            continue;
        }
        let title = milestone
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or(milestone_id);
        let phase_id = milestone
            .get("phase_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let required_stage_ids = milestone
            .get("required_stage_ids")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        record_automation_lifecycle_event_with_metadata(
            row,
            "milestone_promoted",
            Some(format!("milestone `{title}` promoted")),
            None,
            Some(json!({
                "milestone_id": milestone_id,
                "title": title,
                "phase_id": phase_id,
                "required_stage_ids": required_stage_ids,
                "promoted_by_node_id": promoted_by_node_id,
            })),
        );
    }
}

pub fn collect_automation_descendants(
    automation: &AutomationV2Spec,
    root_ids: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    let mut descendants = root_ids.clone();
    let mut changed = true;
    while changed {
        changed = false;
        for node in &automation.flow.nodes {
            if descendants.contains(&node.node_id) {
                continue;
            }
            if node.depends_on.iter().any(|dep| descendants.contains(dep)) {
                descendants.insert(node.node_id.clone());
                changed = true;
            }
        }
    }
    descendants
}

pub fn collect_automation_ancestors(
    automation: &AutomationV2Spec,
    node_id: &str,
) -> std::collections::HashSet<String> {
    let mut ancestors = std::collections::HashSet::new();
    let mut queue = vec![node_id.to_string()];
    while let Some(current_id) = queue.pop() {
        if let Some(node) = automation
            .flow
            .nodes
            .iter()
            .find(|n| n.node_id == current_id)
        {
            for dep in &node.depends_on {
                if ancestors.insert(dep.clone()) {
                    queue.push(dep.clone());
                }
            }
        }
    }
    ancestors
}
