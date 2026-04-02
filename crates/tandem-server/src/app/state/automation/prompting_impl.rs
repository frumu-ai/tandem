use super::*;

fn automation_prompt_html_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn automation_prompt_render_canonical_html_body(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "<p></p>".to_string();
    }
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.contains("<html") || lowered.contains("<body") || lowered.contains("<div") {
        return trimmed.to_string();
    }

    let mut html = String::new();
    let mut in_list = false;
    let mut flush_list = |html: &mut String, in_list: &mut bool| {
        if *in_list {
            html.push_str("</ul>");
            *in_list = false;
        }
    };

    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() {
            flush_list(&mut html, &mut in_list);
            continue;
        }
        if let Some(rest) = line.strip_prefix("### ") {
            flush_list(&mut html, &mut in_list);
            html.push_str(&format!(
                "<h3>{}</h3>",
                automation_prompt_html_escape(rest.trim())
            ));
            continue;
        }
        if let Some(rest) = line.strip_prefix("## ") {
            flush_list(&mut html, &mut in_list);
            html.push_str(&format!(
                "<h2>{}</h2>",
                automation_prompt_html_escape(rest.trim())
            ));
            continue;
        }
        if let Some(rest) = line.strip_prefix("# ") {
            flush_list(&mut html, &mut in_list);
            html.push_str(&format!(
                "<h1>{}</h1>",
                automation_prompt_html_escape(rest.trim())
            ));
            continue;
        }
        if line.starts_with("- ") || line.starts_with("* ") {
            if !in_list {
                html.push_str("<ul>");
                in_list = true;
            }
            let item = line
                .strip_prefix("- ")
                .or_else(|| line.strip_prefix("* "))
                .unwrap_or(line);
            html.push_str(&format!(
                "<li>{}</li>",
                automation_prompt_html_escape(item.trim())
            ));
            continue;
        }

        flush_list(&mut html, &mut in_list);
        html.push_str(&format!("<p>{}</p>", automation_prompt_html_escape(line)));
    }
    flush_list(&mut html, &mut in_list);
    html
}

fn automation_prompt_extract_upstream_text(input: &Value) -> Option<String> {
    let mut candidates = Vec::new();
    for pointer in [
        "/output/content/text",
        "/output/content/raw_assistant_text",
        "/output/text",
        "/output/raw_text",
    ] {
        if let Some(text) = input.pointer(pointer).and_then(Value::as_str) {
            candidates.push(text.trim().to_string());
        }
    }
    if candidates.is_empty() {
        if let Some(output) = input.get("output") {
            if let Ok(rendered) = serde_json::to_string(output) {
                candidates.push(rendered);
            }
        }
    }
    candidates
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .max_by_key(|value| value.len())
}

fn automation_prompt_render_delivery_source_body(upstream_inputs: &[Value]) -> Option<String> {
    let mut best = upstream_inputs
        .iter()
        .filter_map(|input| {
            let text = automation_prompt_extract_upstream_text(input)?;
            let path = input
                .get("output")
                .and_then(|output| output.get("content"))
                .and_then(|content| content.get("path"))
                .or_else(|| input.get("path"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| "upstream artifact".to_string());
            Some((text, path))
        })
        .collect::<Vec<_>>();
    best.sort_by(|left, right| right.0.len().cmp(&left.0.len()).then(left.1.cmp(&right.1)));
    let (text, source_path) = best.into_iter().next()?;
    let rendered_html = automation_prompt_render_canonical_html_body(&text);
    Some(format!(
        "Deterministic Delivery Body:\n- Source artifact: `{}`\n- Canonical HTML body:\n{}\n- Use this exact body as the delivery source of truth.\n- Do not rewrite the body into a shorter teaser or substitute a fresh summary.",
        source_path,
        rendered_html
            .lines()
            .map(|line| format!("  {}", line))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

pub(crate) fn render_automation_v2_prompt(
    automation: &AutomationV2Spec,
    workspace_root: &str,
    run_id: &str,
    node: &AutomationFlowNode,
    attempt: u32,
    agent: &AutomationAgentProfile,
    upstream_inputs: &[Value],
    requested_tools: &[String],
    template_system_prompt: Option<&str>,
    standup_report_path: Option<&str>,
    memory_project_id: Option<&str>,
) -> String {
    render_automation_v2_prompt_with_options(
        automation,
        workspace_root,
        run_id,
        node,
        attempt,
        agent,
        upstream_inputs,
        requested_tools,
        template_system_prompt,
        standup_report_path,
        memory_project_id,
        AutomationPromptRenderOptions::default(),
    )
}

pub(crate) fn render_automation_v2_prompt_with_options(
    automation: &AutomationV2Spec,
    workspace_root: &str,
    run_id: &str,
    node: &AutomationFlowNode,
    attempt: u32,
    agent: &AutomationAgentProfile,
    upstream_inputs: &[Value],
    requested_tools: &[String],
    template_system_prompt: Option<&str>,
    standup_report_path: Option<&str>,
    memory_project_id: Option<&str>,
    options: AutomationPromptRenderOptions,
) -> String {
    let contract_kind = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.as_str())
        .unwrap_or("structured_json");
    let normalized_upstream_inputs = upstream_inputs
        .iter()
        .map(|input| {
            let mut normalized_input = input.clone();
            if let Some(output) = input.get("output") {
                if let Some(object) = normalized_input.as_object_mut() {
                    object.insert(
                        "output".to_string(),
                        normalize_upstream_research_output_paths(
                            workspace_root,
                            Some(run_id),
                            output,
                        ),
                    );
                }
            }
            normalized_input
        })
        .collect::<Vec<_>>();
    let preserve_full_upstream_inputs = automation_node_preserves_full_upstream_inputs(node);
    let summary_only_upstream = options.summary_only_upstream && !preserve_full_upstream_inputs;
    let mut sections = Vec::new();
    if let Some(system_prompt) = template_system_prompt
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("Template system prompt:\n{}", system_prompt));
    }
    if let Some(mission) = automation
        .metadata
        .as_ref()
        .and_then(|value| value.get("mission"))
    {
        let mission_title = mission
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(automation.name.as_str());
        let mission_goal = mission
            .get("goal")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let success_criteria = mission
            .get("success_criteria")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(|row| format!("- {}", row.trim()))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let shared_context = mission
            .get("shared_context")
            .and_then(Value::as_str)
            .unwrap_or_default();
        sections.push(format!(
            "Mission Brief:\nTitle: {mission_title}\nGoal: {mission_goal}\nShared context: {shared_context}\nSuccess criteria:\n{}",
            if success_criteria.is_empty() {
                "- none provided".to_string()
            } else {
                success_criteria
            }
        ));
    }
    sections.push(format!(
        "Automation ID: {}\nRun ID: {}\nNode ID: {}\nAgent: {}\nObjective: {}\nOutput contract kind: {}",
        automation.automation_id, run_id, node.node_id, agent.display_name, node.objective, contract_kind
    ));
    if let Some(contract) = node.output_contract.as_ref() {
        let schema = contract
            .schema
            .as_ref()
            .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()))
            .unwrap_or_else(|| "none".to_string());
        let guidance = contract.summary_guidance.as_deref().unwrap_or("none");
        sections.push(format!(
            "Output Contract:\nKind: {}\nSummary guidance: {}\nSchema:\n{}",
            contract.kind, guidance, schema
        ));
    }
    if let Some(builder) = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
    {
        let local_title = builder
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(node.node_id.as_str());
        let local_prompt = builder
            .get("prompt")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let local_role = builder
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        sections.push(format!(
            "Local Assignment:\nTitle: {local_title}\nRole: {local_role}\nInstructions: {local_prompt}"
        ));
    }
    if let Some(inputs) = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("inputs"))
        .filter(|value| !value.is_null())
    {
        let rendered = serde_json::to_string_pretty(inputs).unwrap_or_else(|_| inputs.to_string());
        sections.push(format!(
            "Node Inputs:\n- Use these values directly when they satisfy the objective.\n- Do not search `/tmp`, shell history, or undeclared temp files for duplicate copies of these inputs.\n{}",
            rendered
                .lines()
                .map(|line| format!("  {}", line))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    let execution_mode = automation_node_execution_mode(node, workspace_root);
    sections.push(format!(
        "Execution Policy:\n- Mode: `{}`.\n- Use only declared workflow artifact paths.\n- Keep status and blocker notes in the response JSON, not as placeholder file contents.",
        execution_mode
    ));
    if automation_node_is_code_workflow(node) {
        let task_kind =
            automation_node_task_kind(node).unwrap_or_else(|| "code_change".to_string());
        let project_backlog_tasks = automation_node_projects_backlog_tasks(node);
        let task_id = automation_node_task_id(node).unwrap_or_else(|| "unassigned".to_string());
        let repo_root = automation_node_repo_root(node).unwrap_or_else(|| ".".to_string());
        let write_scope =
            automation_node_write_scope(node).unwrap_or_else(|| "repo-scoped edits".to_string());
        let acceptance_criteria = automation_node_acceptance_criteria(node)
            .unwrap_or_else(|| "satisfy the declared coding task acceptance criteria".to_string());
        let task_dependencies =
            automation_node_task_dependencies(node).unwrap_or_else(|| "none declared".to_string());
        let verification_state =
            automation_node_verification_state(node).unwrap_or_else(|| "pending".to_string());
        let task_owner =
            automation_node_task_owner(node).unwrap_or_else(|| "unclaimed".to_string());
        let verification_command =
            automation_node_verification_command(node).unwrap_or_else(|| {
                "run the most relevant repo-local build, test, or lint commands".to_string()
            });
        sections.push(format!(
            "Coding Task Context:\n- Task id: `{}`.\n- Task kind: `{}`.\n- Repo root: `{}`.\n- Declared write scope: {}.\n- Acceptance criteria: {}.\n- Backlog dependencies: {}.\n- Verification state: {}.\n- Preferred owner: {}.\n- Verification expectation: {}.\n- Projects backlog tasks: {}.\n- Prefer repository edits plus a concise handoff artifact, not placeholder file rewrites.\n- Use `bash` for verification commands when tool access allows it.",
            task_id, task_kind, repo_root, write_scope, acceptance_criteria, task_dependencies, verification_state, task_owner, verification_command, if project_backlog_tasks { "yes" } else { "no" }
        ));
    }
    if let Some(output_path) = automation_node_required_output_path_for_run(node, Some(run_id)) {
        let output_rules = match execution_mode {
            "git_patch" => format!(
                "Required Run Artifact:\n- Create or update `{}` for this run.\n- Use `glob` to discover candidate paths and `read` only for concrete file paths.\n- Prefer `apply_patch` for multi-line source edits and `edit` for localized replacements.\n- Use `write` only for brand-new files or when patch/edit cannot express the change.\n- Do not replace an existing source file with a status note, preservation note, or placeholder summary.\n- Only write declared workflow artifact files.\n- Do not report success unless this run artifact exists when the stage ends.",
                output_path
            ),
            "filesystem_patch" => format!(
                "Required Run Artifact:\n- Create or update `{}` for this run.\n- Use `glob` to discover candidate paths and `read` only for concrete file paths.\n- Prefer `edit` for existing-file changes.\n- Use `write` for brand-new files or as a last resort when an edit cannot express the change.\n- Do not replace an existing file with a status note, preservation note, or placeholder summary.\n- Only write declared workflow artifact files.\n- Do not report success unless this run artifact exists when the stage ends.",
                output_path
            ),
            _ => format!(
                "Required Run Artifact:\n- Create or update `{}` for this run.\n- If this output path is already known, use `write` immediately to create the full file contents.\n- Use `glob` and `read` only when you must inspect existing companion files or verify a preexisting artifact before updating it.\n- Do not let an empty `glob` end the run; still create the required artifact.\n- Only write declared workflow artifact files; do not create auxiliary touch files, status files, marker files, or placeholder preservation notes.\n- Overwrite the declared output with the actual artifact contents for this run instead of preserving a prior placeholder.\n- Do not report success unless this run artifact exists when the stage ends.",
                output_path
            ),
        };
        sections.push(output_rules);
    }
    if automation_node_web_research_expected(node) {
        let requested_has_websearch = requested_tools.iter().any(|tool| tool == "websearch");
        let requested_has_webfetch = requested_tools
            .iter()
            .any(|tool| matches!(tool.as_str(), "webfetch" | "webfetch_html"));
        if requested_has_websearch {
            sections.push(
                "External Research Expectation:\n- Use `websearch` for current external evidence before finalizing the output file.\n- Use `webfetch` on concrete result URLs when search snippets are not enough.\n- Include only evidence you can support from local files or current web findings.\n- If `websearch` returns an authorization-required or unavailable result, treat external research as unavailable for this run, continue with local file reads, and note the web-research limitation instead of stopping."
                    .to_string(),
            );
        } else if requested_has_webfetch {
            sections.push(
                "External Research Expectation:\n- `websearch` is not available in this run.\n- Use `webfetch` only for concrete URLs already present in local sources or upstream handoffs.\n- If you cannot validate externally without search, record that limitation in the structured handoff and finish the node.\n- Do not ask the user for clarification or permission to continue; return the required JSON handoff for this run."
                    .to_string(),
            );
        } else {
            sections.push(
                "External Research Expectation:\n- No web research tool is available in this run.\n- Record the web-research limitation clearly in the structured handoff, continue with any allowed local reads, and finish without asking follow-up questions."
                    .to_string(),
            );
        }
    }
    let validator_kind = automation_output_validator_kind(node);
    let handoff_only_structured_json = validator_kind
        == crate::AutomationOutputValidatorKind::StructuredJson
        && automation_node_required_output_path(node).is_none();
    if handoff_only_structured_json {
        sections.push(
            "Structured Handoff Expectation:\n- Return the requested structured JSON handoff in the final response body.\n- The final response body should contain JSON only: the handoff JSON, then the final compact JSON status object.\n- Do not include headings, bullets, markdown fences, prose explanations, or follow-up questions.\n- Do not stop after tool calls alone; include a machine-readable JSON object or array with the requested fields."
                .to_string(),
        );
    }
    let mut prompt = sections.join("\n\n");
    if !normalized_upstream_inputs.is_empty() {
        prompt.push_str("\n\nUpstream Inputs:");
        for input in &normalized_upstream_inputs {
            let alias = input
                .get("alias")
                .and_then(Value::as_str)
                .unwrap_or("input");
            let from_step_id = input
                .get("from_step_id")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let output = input
                .get("output")
                .map(|value| {
                    compact_automation_prompt_output_with_mode(value, summary_only_upstream)
                })
                .unwrap_or(Value::Null);
            let rendered =
                serde_json::to_string_pretty(&output).unwrap_or_else(|_| output.to_string());
            prompt.push_str(&format!(
                "\n- {}\n  from_step_id: {}\n  output:\n{}",
                alias,
                from_step_id,
                rendered
                    .lines()
                    .map(|line| format!("    {}", line))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
    }
    if automation_node_is_research_finalize(node) {
        if let Some(summary) =
            render_research_finalize_upstream_summary(&normalized_upstream_inputs)
        {
            prompt.push_str("\n\n");
            prompt.push_str(&summary);
        }
    }
    if let Some(summary) =
        render_upstream_synthesis_guidance(node, &normalized_upstream_inputs, run_id)
    {
        prompt.push_str("\n\n");
        prompt.push_str(&summary);
    }
    if automation_node_requires_email_delivery(node) {
        prompt.push_str(
            "\n\nDelivery rules:\n- Prefer inline email body delivery by default.\n- Only include an email attachment when upstream inputs contain a concrete attachment artifact with a non-empty s3key or upload result.\n- Never send an attachment parameter with an empty or null s3key.\n- If no attachment artifact exists, omit the attachment parameter entirely.",
        );
        let delivery_target =
            automation_node_delivery_target(node).unwrap_or_else(|| "missing".to_string());
        let content_type =
            automation_node_email_content_type(node).unwrap_or_else(|| "text/html".to_string());
        let inline_body_only = automation_node_inline_body_only(node).unwrap_or(true);
        let attachments_allowed = automation_node_allows_attachments(node).unwrap_or(false);
        prompt.push_str(&format!(
            "\n\nDelivery target:\n- Method: `email`\n- Recipient: `{}`\n- Content-Type: `{}`\n- Inline body only: `{}`\n- Attachments allowed: `{}`\n- Treat this delivery target as authoritative for this run.\n- Do not say the recipient is missing when it is listed above.\n- Do not mark the node completed unless you actually execute an email draft or send tool.",
            delivery_target,
            content_type,
            inline_body_only,
            attachments_allowed
        ));
        if let Some(delivery_source) =
            automation_prompt_render_delivery_source_body(&normalized_upstream_inputs)
        {
            prompt.push_str("\n\n");
            prompt.push_str(&delivery_source);
        }
    }
    if let Some(report_path) = standup_report_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt.push_str(&format!(
            "\n\nStandup report path:\n- Write the final markdown report to `{}` relative to the workspace root.\n- Use the `write` tool for the report.\n- The report must remain inside the workspace.",
            report_path
        ));
    }
    if let Some(project_id) = memory_project_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt.push_str(&format!(
            "\n\nMemory search scope:\n- `memory_search` defaults to the current session, current project, and global memory.\n- Current project_id: `{}`.\n- Use `tier: \"project\"` when you need recall limited to this workspace.\n- Use workspace files via `glob`, `grep`, and `read` when memory is sparse or stale.",
            project_id
        ));
    }
    if let Some(knowledge_context) = options
        .knowledge_context
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt.push_str("\n\n");
        prompt.push_str(knowledge_context);
    }
    let enforce_completed_first_attempt = (validator_kind
        == crate::AutomationOutputValidatorKind::ResearchBrief
        || !automation_node_required_tools(node).is_empty()
        || handoff_only_structured_json)
        && attempt <= 1;
    if enforce_completed_first_attempt {
        if automation_node_required_output_path(node).is_some() {
            prompt.push_str(
                "\n\nFinal response requirements:\n- Return a concise completion.\n- Include a final compact JSON object in the response body with `status` set to `completed`.\n- Do not declare the output blocked while the required workflow tools remain available; use them first and finish the work.\n- Do not claim success unless the write tool actually created the output file.",
            );
        } else {
            prompt.push_str(
                "\n\nFinal response requirements:\n- Return a concise completion.\n- Include a final compact JSON object in the response body with `status` set to `completed`.\n- Do not declare the output blocked while the required workflow tools remain available; use them first and finish the work.\n- Do not claim success unless the required structured handoff was actually returned in the final response.",
            );
        }
    } else if handoff_only_structured_json {
        prompt.push_str(
            "\n\nFinal response requirements:\n- Return a concise completion.\n- Include the required structured handoff JSON in the response body before the final compact status object.\n- Include a final compact JSON object in the response body with at least `status` (`completed` or `blocked`).\n- For review-style nodes, also include `approved` (`true` or `false`).\n- If blocked, include a short `reason`.\n- Do not claim success unless the required structured handoff was actually returned in the final response.\n- Do not claim semantic success if the output is blocked or not approved.",
        );
    } else {
        prompt.push_str(
            "\n\nFinal response requirements:\n- Return a concise completion.\n- Include a final compact JSON object in the response body with at least `status` (`completed` or `blocked`).\n- For review-style nodes, also include `approved` (`true` or `false`).\n- If blocked, include a short `reason`.\n- Do not claim semantic success if the output is blocked or not approved.",
        );
    }
    prompt
}

fn truncate_automation_prompt_text(raw: &str, max_chars: usize) -> String {
    let trimmed = raw.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let truncated = trimmed.chars().take(max_chars).collect::<String>();
    format!("{truncated}...")
}

fn compact_automation_prompt_content(content: &Value, summary_only: bool) -> Value {
    let Some(object) = content.as_object() else {
        return content.clone();
    };
    let mut compact = serde_json::Map::new();
    if let Some(path) = object.get("path").cloned().filter(|value| !value.is_null()) {
        compact.insert("path".to_string(), path);
    }
    if let Some(handoff) = object
        .get("structured_handoff")
        .cloned()
        .filter(|value| !value.is_null())
    {
        compact.insert("structured_handoff".to_string(), handoff);
        return Value::Object(compact);
    }
    let candidate_text = object
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| object.get("raw_text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(text) = candidate_text {
        if let Ok(parsed) = serde_json::from_str::<Value>(text) {
            if summary_only {
                compact.insert("data_summary".to_string(), summarize_json_keys(&parsed));
            } else {
                compact.insert("data".to_string(), parsed);
            }
        } else {
            compact.insert(
                "text".to_string(),
                json!(truncate_automation_prompt_text(
                    text,
                    if summary_only { 800 } else { 4000 }
                )),
            );
        }
    }
    Value::Object(compact)
}

fn compact_automation_prompt_output_with_mode(output: &Value, summary_only: bool) -> Value {
    let Some(object) = output.as_object() else {
        return output.clone();
    };
    let mut compact = serde_json::Map::new();
    for key in [
        "status",
        "phase",
        "contract_kind",
        "summary",
        "blocked_reason",
        "workflow_class",
    ] {
        if let Some(value) = object.get(key).cloned().filter(|value| !value.is_null()) {
            compact.insert(key.to_string(), value);
        }
    }
    if let Some(validator_summary) = object.get("validator_summary").and_then(Value::as_object) {
        let mut validator = serde_json::Map::new();
        for key in [
            "kind",
            "outcome",
            "warning_count",
            "warning_requirements",
            "unmet_requirements",
            "validation_basis",
        ] {
            if let Some(value) = validator_summary
                .get(key)
                .cloned()
                .filter(|value| !value.is_null())
            {
                validator.insert(key.to_string(), value);
            }
        }
        if !validator.is_empty() {
            compact.insert("validator_summary".to_string(), Value::Object(validator));
        }
    }
    if let Some(artifact_validation) = object.get("artifact_validation").and_then(Value::as_object)
    {
        let mut validation = serde_json::Map::new();
        for key in [
            "accepted_artifact_path",
            "accepted_candidate_source",
            "validation_outcome",
            "validation_profile",
            "warning_count",
            "warning_requirements",
            "unmet_requirements",
            "semantic_block_reason",
            "rejected_artifact_reason",
            "validation_basis",
        ] {
            if let Some(value) = artifact_validation
                .get(key)
                .cloned()
                .filter(|value| !value.is_null())
            {
                validation.insert(key.to_string(), value);
            }
        }
        if !validation.is_empty() {
            compact.insert("artifact_validation".to_string(), Value::Object(validation));
        }
    }
    if let Some(content) = object.get("content") {
        let compact_content = compact_automation_prompt_content(content, summary_only);
        if compact_content
            .as_object()
            .is_some_and(|value| !value.is_empty())
        {
            compact.insert("content".to_string(), compact_content);
        }
    }
    Value::Object(compact)
}

pub(crate) fn render_automation_repair_brief(
    node: &AutomationFlowNode,
    prior_output: Option<&Value>,
    attempt: u32,
    max_attempts: u32,
) -> Option<String> {
    if attempt <= 1 {
        return None;
    }
    let prior_output = prior_output?;
    if !automation_output_needs_repair(prior_output) {
        return None;
    }

    let validator_summary = prior_output.get("validator_summary");
    let artifact_validation = prior_output.get("artifact_validation");
    let tool_telemetry = prior_output.get("tool_telemetry");
    let validator_outcome = validator_summary
        .and_then(|value| value.get("outcome"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let unmet_requirements_from_summary = validator_summary
        .and_then(|value| value.get("unmet_requirements"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let is_upstream_passed = validator_outcome
        .is_some_and(|outcome| outcome.eq_ignore_ascii_case("passed"))
        && unmet_requirements_from_summary.is_empty();
    if is_upstream_passed {
        return None;
    }
    let reason = validator_summary
        .and_then(|value| value.get("reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            artifact_validation
                .and_then(|value| value.get("semantic_block_reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("the previous attempt did not satisfy the runtime validator");
    let unmet_requirements = unmet_requirements_from_summary;
    let blocking_classification = artifact_validation
        .and_then(|value| value.get("blocking_classification"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unspecified");
    let required_next_tool_actions = artifact_validation
        .and_then(|value| value.get("required_next_tool_actions"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let validation_basis = artifact_validation
        .and_then(|value| value.get("validation_basis"))
        .and_then(Value::as_object)
        .cloned();
    let validation_basis_line = validation_basis
        .as_ref()
        .map(|basis| {
            let authority = basis
                .get("authority")
                .and_then(Value::as_str)
                .unwrap_or("unspecified");
            let current_attempt_output_materialized = basis
                .get("current_attempt_output_materialized")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let current_attempt_has_recorded_activity = basis
                .get("current_attempt_has_recorded_activity")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let current_attempt_has_read = basis
                .get("current_attempt_has_read")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let current_attempt_has_web_research = basis
                .get("current_attempt_has_web_research")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let workspace_inspection_satisfied = basis
                .get("workspace_inspection_satisfied")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            format!(
                "authority={}, output_materialized={}, recorded_activity={}, read={}, web_research={}, workspace_inspection={}",
                authority,
                current_attempt_output_materialized,
                current_attempt_has_recorded_activity,
                current_attempt_has_read,
                current_attempt_has_web_research,
                workspace_inspection_satisfied
            )
        })
        .unwrap_or_else(|| "none recorded".to_string());
    let tools_offered = tool_telemetry
        .and_then(|value| value.get("requested_tools"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let tools_executed = tool_telemetry
        .and_then(|value| value.get("executed_tools"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let unreviewed_relevant_paths = artifact_validation
        .and_then(|value| value.get("unreviewed_relevant_paths"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let repair_attempt = artifact_validation
        .and_then(|value| value.get("repair_attempt"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(attempt.saturating_sub(1));
    let repair_attempts_remaining = artifact_validation
        .and_then(|value| value.get("repair_attempts_remaining"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or_else(|| max_attempts.saturating_sub(attempt.saturating_sub(1)));

    let unmet_line = if unmet_requirements.is_empty() {
        "none recorded".to_string()
    } else {
        unmet_requirements.join(", ")
    };
    let tools_offered_line = if tools_offered.is_empty() {
        "none recorded".to_string()
    } else {
        tools_offered.join(", ")
    };
    let tools_executed_line = if tools_executed.is_empty() {
        "none recorded".to_string()
    } else {
        tools_executed.join(", ")
    };
    let unreviewed_line = if unreviewed_relevant_paths.is_empty() {
        "none recorded".to_string()
    } else {
        unreviewed_relevant_paths.join(", ")
    };
    let next_actions_line = if required_next_tool_actions.is_empty() {
        "none recorded".to_string()
    } else {
        required_next_tool_actions.join(" | ")
    };
    let code_workflow_line = if automation_node_is_code_workflow(node) {
        let verification_command =
            automation_node_verification_command(node).unwrap_or_else(|| {
                "run the most relevant repo-local build, test, or lint commands".to_string()
            });
        let write_scope =
            automation_node_write_scope(node).unwrap_or_else(|| "repo-scoped edits".to_string());
        format!(
            "\n- Code workflow repair path: inspect the touched files in `{}` first, patch with `edit` or `apply_patch` before any new `write`, then rerun verification with `{}` and fix the smallest failing root cause.",
            write_scope,
            verification_command
        )
    } else {
        String::new()
    };

    Some(format!(
        "Repair Brief:\n- Node `{}` is being retried because the previous attempt ended in `needs_repair`.\n- Previous validation reason: {}.\n- Validation basis: {}.\n- Unmet requirements: {}.\n- Blocking classification: {}.\n- Required next tool actions: {}.\n- Tools offered last attempt: {}.\n- Tools executed last attempt: {}.\n- Relevant files still unread or explicitly unreviewed: {}.\n- Previous repair attempt count: {}.\n- Remaining repair attempts after this run: {}{}.\n- For this retry, satisfy the unmet requirements before finalizing the artifact.\n- Do not write a blocked handoff unless the required tools were actually attempted and remained unavailable or failed.",
        node.node_id,
        reason,
        validation_basis_line,
        unmet_line,
        blocking_classification,
        next_actions_line,
        tools_offered_line,
        tools_executed_line,
        unreviewed_line,
        repair_attempt,
        repair_attempts_remaining.saturating_sub(1),
        code_workflow_line,
    ))
}
