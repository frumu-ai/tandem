// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

pub(crate) fn workflow_plan_common_sections() -> String {
    // Keep this formatted text stable: downstream behavior depends on the planner prompt shape.
    let step_id_examples = crate::workflow_plan::WORKFLOW_STEP_ID_EXAMPLES.join(", ");
    let teaching_library = workflow_plan_teaching_library_sections();
    format!(
        concat!(
            "Step id guidance: use descriptive lowercase snake_case ids such as {}.\n",
            "Plan invariants:\n",
            "- execution_target must be automation_v2\n",
            "- workspace_root must be a non-empty absolute path\n",
            "- step ids must stay descriptive and stable instead of reusing legacy placeholder names from unrelated workflow domains\n",
            "- decompose complex work into phase-aware microtask DAGs instead of forcing every plan into a flat 7-10 step graph\n",
            "- keep each leaf task narrow enough to own one objective, one output contract, and one validation path\n",
            "- steps must form a valid DAG\n",
            "- input_refs and depends_on must reference existing steps\n",
            "WorkflowPlan.step schema:\n",
            "- each step must use fields: step_id, kind, objective, agent_role, depends_on, input_refs, output_contract; metadata is optional\n",
            "- do not use alternate keys like id, type, label, or config as the primary step schema\n",
            "- input_refs must be objects shaped like {{\"from_step_id\":\"...\",\"alias\":\"...\"}}\n",
            "- output_contract must be either null or {{\"kind\":\"structured_json|report_markdown|text_summary|urls|citations|brief|review|review_summary|approval_gate|code_patch\",\"validator\":\"structured_json|generic_artifact|research_brief|review_decision|code_patch\"}}\n",
            "- use `code_patch` for code-editing steps that should be judged by patch/apply/test behavior rather than a prose summary\n",
            "- when a step mainly produces a markdown, json, or text artifact such as a recap, report, ledger, or merged daily summary, use `report_markdown`, `structured_json`, or `text_summary` instead of `code_patch` even if the step compiles earlier findings into one file\n",
            "- when a plan is complex, prefer metadata.builder.phase_id, task_class, retry_class, and parent_step_id so the runtime can keep retries narrow and phase-aware\n",
            "- when a research brief step needs current web coverage, set metadata.builder.web_research_expected to true; set it to false when local/file research is enough\n",
            "- when the request names connector-backed sources or `allowed_mcp_servers` is non-empty, plan MCP-backed steps instead of inventing hidden capabilities or defaulting to generic web search\n",
            "- when a prompt names a file as read-only or source of truth, never infer it as a write target; treat it as input-only unless the contract explicitly declares it as output\n",
            "- when a prompt names concrete source files or output files, carry those exact names into the relevant step objectives instead of replacing them with generic workspace language\n",
            "- when a prompt names `websearch`, `webfetch`, or another explicit tool, preserve that tool name in the step objective that will use it\n",
            "- when a step summarizes, merges, or finalizes upstream work, include explicit input_refs for the upstream steps it synthesizes instead of relying on depends_on alone\n",
            "- final synthesis steps must say which upstream artifacts or findings they are consolidating and must require a concrete synthesis rather than a generic recap\n",
            "{}",
        ),
        step_id_examples,
        teaching_library
    )
}

fn workflow_plan_teaching_library_sections() -> String {
    concat!(
        "Teaching library:\n",
        "- explain: summarize the plan, why the steps exist, and what output each produces\n",
        "- objections: call out missing inputs, unsafe assumptions, or missing connectors\n",
        "- proof points: cite evidence sources, validation checks, or artifacts that will prove success\n",
        "- connector-backed work: prefer the selected MCP inventory for source-specific systems such as Reddit, GitHub issues, Slack, or Jira, and clarify when no relevant connector is available\n",
        "- code changes: prefer `code_patch` plus an inspect -> patch -> apply -> test -> repair loop, and reserve `write` for brand-new files\n",
        "- recap and synthesis files: prefer `report_markdown`, `structured_json`, or `text_summary` for markdown/json/text deliverables even when they merge prior findings into a final artifact\n",
            "- monitor-pattern plans: when the user describes a recurring awareness task (checking email, watching for changes, monitoring a data source, scanning for new items), generate a triage-first plan\n",
            "  - first step must be an `assess` step that uses MCP tools to check the data source and outputs structured JSON: {\"has_work\": true/false, \"summary\": \"...\", \"items\": [...]}\n",
            "  - set metadata.triage_gate: true on the assess step so the engine skips downstream nodes when has_work is false\n",
            "  - subsequent steps depend on the assess step and can be as complex as needed; do not limit the plan to two steps\n",
            "  - the assess step should use a cheap model: set metadata.builder.triage_model: true\n",
            "  - if a node names concrete workspace files, require at least one `read` on a concrete file path before the node is complete; discovery tools like `glob`, `grep`, or `codesearch` do not count as source coverage\n",
            "  - when a node reads a source-of-truth file, carry the exact file text forward in structured handoff data such as `source_material` instead of turning that file into a write target\n",
            "  - when a node produces a final recap, merged summary, or daily rollup from upstream work, make it explicitly reread and synthesize the strongest upstream artifacts rather than summarizing from memory\n",
            "  - do not add a triage step for non-recurring or non-awareness tasks\n",
    )
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_plan_common_sections_include_teaching_library() {
        let sections = workflow_plan_common_sections();
        assert!(sections.contains("Teaching library:"));
        assert!(sections.contains("explain: summarize the plan"));
        assert!(sections.contains("proof points"));
        assert!(sections.contains("connector-backed work"));
        assert!(sections.contains("code_patch"));
        assert!(sections.contains("phase-aware microtask DAGs"));
        assert!(sections.contains("one objective, one output contract"));
        assert!(sections.contains("inspect -> patch -> apply -> test -> repair"));
        assert!(sections.contains("reserve `write` for brand-new files"));
        assert!(sections.contains("recap and synthesis files"));
        assert!(sections.contains("compiles earlier findings into one file"));
        assert!(sections.contains("monitor-pattern plans"));
        assert!(sections.contains("assess"));
        assert!(sections.contains("triage_gate"));
        assert!(sections.contains("concrete workspace files"));
        assert!(sections.contains("read-only or source of truth"));
        assert!(sections.contains("source_material"));
        assert!(sections.contains("concrete source files or output files"));
        assert!(sections.contains("websearch"));
        assert!(sections.contains("explicit input_refs for the upstream steps"));
        assert!(sections.contains("final recap, merged summary, or daily rollup"));
    }
}
