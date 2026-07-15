// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde_json::Value;

pub(crate) fn build_product_operator_block(
    session_id: &str,
    run_id: &str,
    artifacts: &Value,
) -> String {
    let artifact_json = serde_json::to_string(artifacts).unwrap_or_else(|_| "{}".to_string());
    [
        "<tandem_product_operator>".to_string(),
        "- You are operating Tandem on behalf of the authenticated user, not merely explaining how Tandem could be used.".to_string(),
        format!("- chat_session_id: {session_id}"),
        format!("- chat_run_id: {run_id}"),
        "- Use first-party workflow_plan_*, automation_*, orchestration_*, goal_*, and wait_* tools for product facts and mutations.".to_string(),
        "- For a new workflow or automation described in natural language, call workflow_plan_start first. Do not call automation_manage_draft with action=create or synthesize a raw Automation V2 definition.".to_string(),
        "- While authoring a disabled draft, represent external integrations as requirements or blockers. Do not discover or execute external MCP tools unless the user explicitly asks to inspect a live integration.".to_string(),
        "- The user's verified tenant identity is attached at tool dispatch. Never ask for an API key to access Tandem's own product APIs or Docs MCP.".to_string(),
        "- Distinguish requests for action from requests for explanation. For action requests, use tools and only claim results returned by tools.".to_string(),
        "- Make reversible assumptions while drafting. Ask only when an ambiguity would materially change behavior, recipients, timing, or external effects.".to_string(),
        "- Planner materialization creates a disabled Automation V2 draft. Enabling, publishing, archiving, sending, or other consequential effects require explicit approval.".to_string(),
        "- Treat tool failures as recoverable: inspect the returned blocker or active artifact, revise the draft, and retry with the same idempotency key only for the identical request.".to_string(),
        "- Follow-up references such as 'it', 'that workflow', or 'revise this' may use the active artifact only when artifact selection is single_active. When selection is ambiguous, ask the user to choose.".to_string(),
        format!("- durable_artifact_context: {artifact_json}"),
        "</tandem_product_operator>".to_string(),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_operator_routes_natural_language_creation_through_the_planner() {
        let block = build_product_operator_block("chat-1", "run-1", &serde_json::json!({}));

        assert!(block.contains("call workflow_plan_start first"));
        assert!(block.contains("Do not call automation_manage_draft with action=create"));
        assert!(block.contains("Do not discover or execute external MCP tools"));
    }
}

pub(crate) fn resolve_identity_block(config: &Value, agent_name: Option<&str>) -> Option<String> {
    let allow_agent_override = agent_name
        .map(|name| !matches!(name, "compaction" | "title" | "summary"))
        .unwrap_or(false);
    let legacy_bot_name = config
        .get("bot_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let bot_name = config
        .get("identity")
        .and_then(|identity| identity.get("bot"))
        .and_then(|bot| bot.get("canonical_name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .or(legacy_bot_name)
        .unwrap_or("Tandem");

    let default_profile = config
        .get("identity")
        .and_then(|identity| identity.get("personality"))
        .and_then(|personality| personality.get("default"));
    let default_preset = default_profile
        .and_then(|profile| profile.get("preset"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("balanced");
    let default_custom = default_profile
        .and_then(|profile| profile.get("custom_instructions"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string);
    let legacy_persona = config
        .get("persona")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string);

    let per_agent_profile = if allow_agent_override {
        agent_name.and_then(|name| {
            config
                .get("identity")
                .and_then(|identity| identity.get("personality"))
                .and_then(|personality| personality.get("per_agent"))
                .and_then(|per_agent| per_agent.get(name))
        })
    } else {
        None
    };
    let preset = per_agent_profile
        .and_then(|profile| profile.get("preset"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(default_preset);
    let custom = per_agent_profile
        .and_then(|profile| profile.get("custom_instructions"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .or(default_custom)
        .or(legacy_persona);

    let mut lines = vec![
        format!("You are {bot_name}, an AI assistant."),
        personality_preset_text(preset).to_string(),
    ];
    if let Some(custom) = custom {
        lines.push(format!("Additional personality instructions: {custom}"));
    }
    Some(lines.join("\n"))
}

pub(crate) fn build_memory_scope_block(
    session_id: &str,
    project_id: Option<&str>,
    workspace_root: Option<&str>,
) -> String {
    let mut lines = vec![
        "<memory_scope>".to_string(),
        format!("- current_session_id: {}", session_id),
    ];
    if let Some(project_id) = project_id.map(str::trim).filter(|value| !value.is_empty()) {
        lines.push(format!("- current_project_id: {}", project_id));
    }
    if let Some(workspace_root) = workspace_root
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("- workspace_root: {}", workspace_root));
    }
    lines.push(
        "- default_memory_search_behavior: search current session, then current project/workspace, then global memory"
            .to_string(),
    );
    lines.push(
        "- use memory_search without IDs for normal recall; only pass tier/session_id/project_id when narrowing scope"
            .to_string(),
    );
    lines.push(
        "- when memory is sparse or stale, inspect the workspace with glob, grep, and read"
            .to_string(),
    );
    lines.push("</memory_scope>".to_string());
    lines.join("\n")
}

pub(crate) fn build_kb_grounding_block(
    policy: &tandem_core::KnowledgebaseGroundingPolicy,
) -> String {
    let servers = if policy.server_names.is_empty() {
        "enabled knowledgebase MCP".to_string()
    } else {
        policy.server_names.join(", ")
    };
    let patterns = if policy.tool_patterns.is_empty() {
        "configured KB MCP tools".to_string()
    } else {
        policy.tool_patterns.join(", ")
    };
    let preferred_tools = kb_grounding_preferred_tools(policy);
    [
        "<knowledgebase_grounding_policy>".to_string(),
        format!("- required: {}", policy.required),
        format!("- strict: {}", policy.strict),
        format!("- servers: {}", servers),
        format!("- tool_patterns: {}", patterns),
        format!(
            "- preferred_question_tools: {}",
            preferred_tools.join(", ")
        ),
        "- For factual/project/product/channel questions, answer from the enabled KB MCP for this channel before using model knowledge, memory, or general chat.".to_string(),
        "- First choice: call the KB MCP `answer_question` tool with the user's question when that tool is available.".to_string(),
        "- Fallback: call the KB MCP search tool, then fetch the full matching document with `get_document` before answering.".to_string(),
        "- Do not answer from search result snippets alone when a full document tool is available.".to_string(),
        "- Use only the KB MCP tools listed by this policy for KB evidence; do not switch to unrelated MCPs or built-in docs search for this channel's KB questions.".to_string(),
        "- If the KB has no matching evidence, say `I do not see that in the connected knowledgebase.` instead of relying on model memory.".to_string(),
        "- When strict grounding is enabled, answer only from retrieved KB evidence and do not add external product instructions, inferred policy, or best-practice guidance.".to_string(),
        "</knowledgebase_grounding_policy>".to_string(),
    ]
    .join("\n")
}

fn personality_preset_text(preset: &str) -> &'static str {
    match preset {
        "concise" => {
            "Default style: concise and high-signal. Prefer short direct responses unless detail is requested."
        }
        "friendly" => {
            "Default style: friendly and supportive while staying technically rigorous and concrete."
        }
        "mentor" => {
            "Default style: mentor-like. Explain decisions and tradeoffs clearly when complexity is non-trivial."
        }
        "critical" => {
            "Default style: critical and risk-first. Surface failure modes and assumptions early."
        }
        _ => {
            "Default style: balanced, pragmatic, and factual. Focus on concrete outcomes and actionable guidance."
        }
    }
}

fn kb_grounding_preferred_tools(policy: &tandem_core::KnowledgebaseGroundingPolicy) -> Vec<String> {
    let mut tools = Vec::new();
    if !policy.server_names.is_empty() {
        for server in &policy.server_names {
            let namespace = mcp_namespace_segment_for_prompt(server);
            tools.push(format!("mcp.{namespace}.answer_question"));
            tools.push(format!("mcp.{namespace}.search_docs"));
            tools.push(format!("mcp.{namespace}.get_document"));
        }
    }
    if tools.is_empty() {
        tools.push("mcp.<knowledgebase>.answer_question".to_string());
        tools.push("mcp.<knowledgebase>.search_docs".to_string());
        tools.push("mcp.<knowledgebase>.get_document".to_string());
    }
    tools
}

fn mcp_namespace_segment_for_prompt(name: &str) -> String {
    let mut out = String::new();
    let mut previous_underscore = false;
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            previous_underscore = false;
        } else if !previous_underscore {
            out.push('_');
            previous_underscore = true;
        }
    }
    let cleaned = out.trim_matches('_');
    if cleaned.is_empty() {
        "server".to_string()
    } else {
        cleaned.to_string()
    }
}
