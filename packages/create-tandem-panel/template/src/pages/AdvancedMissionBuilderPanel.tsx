import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { TandemClient } from "@frumu/tandem-client";
import { renderIcons } from "../app/icons.js";
import agenticDesignPresetSource from "../presets/mission-builder/agentic-design.yaml?raw";
import aiOpportunityPresetSource from "../presets/mission-builder/ai-opportunity.yaml?raw";
import automationRolloutPresetSource from "../presets/mission-builder/automation-rollout.yaml?raw";
import workflowAuditPresetSource from "../presets/mission-builder/workflow-audit.yaml?raw";

type ApiFn = (path: string, init?: RequestInit) => Promise<any>;

type ProviderOption = {
  id: string;
  models: string[];
  configured?: boolean;
};

type McpServerOption = {
  name: string;
  connected?: boolean;
  enabled?: boolean;
};

type CreateModeTab = "mission" | "team" | "workstreams" | "review" | "compile";
type ScheduleKind = "manual" | "interval" | "cron";
type ModelDraft = { provider: string; model: string };
type StarterPresetId =
  | "ai-opportunity"
  | "workflow-audit"
  | "agentic-design"
  | "automation-rollout";

type MissionBlueprint = {
  mission_id: string;
  title: string;
  goal: string;
  success_criteria: string[];
  shared_context?: string;
  workspace_root: string;
  orchestrator_template_id?: string;
  phases: Array<{
    phase_id: string;
    title: string;
    description?: string;
    execution_mode?: "soft" | "barrier";
  }>;
  milestones: Array<{
    milestone_id: string;
    title: string;
    description?: string;
    phase_id?: string;
    required_stage_ids?: string[];
  }>;
  team: {
    allowed_template_ids?: string[];
    default_model_policy?: Record<string, unknown> | null;
    allowed_mcp_servers?: string[];
    max_parallel_agents?: number;
    mission_budget?: {
      max_total_tokens?: number;
      max_total_cost_usd?: number;
      max_total_runtime_ms?: number;
      max_total_tool_calls?: number;
    };
    orchestrator_only_tool_calls?: boolean;
  };
  workstreams: Array<{
    workstream_id: string;
    title: string;
    objective: string;
    role: string;
    template_id?: string;
    prompt: string;
    priority?: number;
    phase_id?: string;
    lane?: string;
    milestone?: string;
    model_override?: Record<string, unknown> | null;
    tool_allowlist_override?: string[];
    mcp_servers_override?: string[];
    depends_on: string[];
    input_refs: Array<{ from_step_id: string; alias: string }>;
    output_contract: {
      kind: string;
      schema?: unknown;
      summary_guidance?: string;
    };
  }>;
  review_stages: Array<{
    stage_id: string;
    stage_kind: "review" | "test" | "approval";
    title: string;
    target_ids: string[];
    role?: string;
    template_id?: string;
    prompt: string;
    checklist?: string[];
    priority?: number;
    phase_id?: string;
    lane?: string;
    milestone?: string;
    model_override?: Record<string, unknown> | null;
    tool_allowlist_override?: string[];
    mcp_servers_override?: string[];
    gate?: {
      required?: boolean;
      decisions?: string[];
      rework_targets?: string[];
      instructions?: string;
    } | null;
  }>;
  metadata?: unknown;
};

type MissionPreset = {
  id: StarterPresetId;
  label: string;
  description: string;
  blueprint: MissionBlueprint;
};

function normalizeMcpServers(raw: any): McpServerOption[] {
  if (Array.isArray(raw?.servers)) {
    return raw.servers
      .map((row: any) => {
        const name = String(row?.name || "").trim();
        if (!name) return null;
        return { name, connected: !!row?.connected, enabled: row?.enabled !== false };
      })
      .filter(Boolean) as McpServerOption[];
  }
  if (raw && typeof raw === "object") {
    return Object.entries(raw)
      .map(([name, row]) => {
        const clean = String(name || "").trim();
        if (!clean) return null;
        return {
          name: clean,
          connected: !!(row as any)?.connected,
          enabled: (row as any)?.enabled !== false,
        };
      })
      .filter(Boolean) as McpServerOption[];
  }
  return [];
}

function splitCsv(raw: string) {
  return String(raw || "")
    .split(",")
    .map((value) => value.trim())
    .filter(Boolean);
}

function toModelPolicy(draft: ModelDraft) {
  const provider = String(draft.provider || "").trim();
  const model = String(draft.model || "").trim();
  if (!provider || !model) return null;
  return { default_model: { provider_id: provider, model_id: model } };
}

function fromModelPolicy(policy: any): ModelDraft {
  const defaultModel = policy?.default_model || policy?.defaultModel || {};
  return {
    provider: String(defaultModel?.provider_id || defaultModel?.providerId || "").trim(),
    model: String(defaultModel?.model_id || defaultModel?.modelId || "").trim(),
  };
}

function scheduleToPayload(kind: ScheduleKind, intervalSeconds: string, cron: string) {
  const misfirePolicy = { type: "run_once" as const };
  if (kind === "cron") {
    return {
      type: "cron",
      cron_expression: String(cron || "").trim(),
      timezone: "UTC",
      misfire_policy: misfirePolicy,
    };
  }
  if (kind === "interval") {
    return {
      type: "interval",
      interval_seconds: Math.max(1, Number.parseInt(String(intervalSeconds || "3600"), 10) || 3600),
      timezone: "UTC",
      misfire_policy: misfirePolicy,
    };
  }
  return { type: "manual", timezone: "UTC", misfire_policy: misfirePolicy };
}

function defaultBlueprint(workspaceRoot: string): MissionBlueprint {
  return {
    mission_id: `mission_${crypto.randomUUID().slice(0, 8)}`,
    title: "",
    goal: "",
    success_criteria: [],
    shared_context: "",
    workspace_root: workspaceRoot,
    orchestrator_template_id: "",
    phases: [{ phase_id: "phase_1", title: "Phase 1", description: "", execution_mode: "soft" }],
    milestones: [],
    team: {
      allowed_template_ids: [],
      default_model_policy: null,
      allowed_mcp_servers: [],
      max_parallel_agents: 4,
      mission_budget: {},
      orchestrator_only_tool_calls: false,
    },
    workstreams: [
      {
        workstream_id: `workstream_${crypto.randomUUID().slice(0, 8)}`,
        title: "Workstream 1",
        objective: "",
        role: "worker",
        prompt: "",
        priority: 1,
        phase_id: "phase_1",
        lane: "lane_1",
        milestone: "",
        depends_on: [],
        input_refs: [],
        tool_allowlist_override: [],
        mcp_servers_override: [],
        output_contract: { kind: "report_markdown", summary_guidance: "" },
      },
    ],
    review_stages: [],
    metadata: null,
  };
}

function parseMissionPreset(source: string): MissionPreset {
  return Function(`"use strict"; return (${source});`)() as MissionPreset;
}

const STARTER_PRESETS = [
  parseMissionPreset(aiOpportunityPresetSource),
  parseMissionPreset(workflowAuditPresetSource),
  parseMissionPreset(agenticDesignPresetSource),
  parseMissionPreset(automationRolloutPresetSource),
];

function starterBlueprint(preset: StarterPresetId, workspaceRoot: string): MissionBlueprint {
  const root = defaultBlueprint(workspaceRoot);
  switch (preset) {
    case "ai-opportunity":
      return {
        ...root,
        title: "AI opportunity assessment",
        goal: "Identify the highest-value AI opportunities in the target business process and produce a prioritized implementation brief.",
        success_criteria: [
          "Workflow bottlenecks and repetitive work are identified",
          "AI opportunities are ranked by value, feasibility, and risk",
          "Final brief recommends concrete next pilots",
        ],
        shared_context:
          "Audience is operators and product leadership. Focus on realistic AI leverage, not hype. Separate automation, copilots, and fully agentic opportunities. Flag data, tooling, and approval constraints explicitly.",
        phases: [
          { phase_id: "discovery", title: "Discovery", execution_mode: "soft" },
          { phase_id: "recommendation", title: "Recommendation", execution_mode: "barrier" },
        ],
        milestones: [
          {
            milestone_id: "opportunity-map-ready",
            title: "Opportunity map ready",
            phase_id: "discovery",
            required_stage_ids: ["workflow-analysis", "capability-map"],
          },
        ],
        workstreams: [
          {
            workstream_id: "workflow-analysis",
            title: "Workflow analysis",
            objective:
              "Break down the current workflow, identify friction, handoffs, delays, and repeated manual work.",
            role: "analyst",
            prompt:
              "Act as an AI workflow analyst. Map the current workflow step by step, identify where humans are spending time, where handoffs fail, where decisions bottleneck, and where information must be gathered, transformed, or checked. Produce a workflow analysis memo that distinguishes repetitive work, judgment-heavy work, and coordination-heavy work.",
            priority: 1,
            phase_id: "discovery",
            lane: "workflow",
            milestone: "opportunity-map-ready",
            depends_on: [],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "report_markdown",
              summary_guidance:
                "Workflow steps, bottlenecks, repeated work, approval points, and observed failure modes.",
            },
          },
          {
            workstream_id: "capability-map",
            title: "AI capability map",
            objective:
              "Match workflow problems to realistic AI patterns such as extraction, triage, drafting, routing, review, and autonomous follow-through.",
            role: "strategist",
            prompt:
              "Act as an agentic systems strategist. Review the workflow analysis context and map each major problem to practical AI patterns. Distinguish simple automation, LLM copilots, tool-using agents, and multi-agent orchestration. Be explicit about prerequisites, risks, and where human approval is still required.",
            priority: 2,
            phase_id: "discovery",
            lane: "capability",
            milestone: "opportunity-map-ready",
            depends_on: [],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "report_markdown",
              summary_guidance:
                "Problem-to-capability map with candidate AI patterns, prerequisites, and operational risks.",
            },
          },
          {
            workstream_id: "opportunity-brief",
            title: "Opportunity brief",
            objective:
              "Synthesize the findings into a prioritized AI opportunity brief with clear next pilots.",
            role: "analyst",
            prompt:
              "Synthesize the workflow analysis and capability map into an executive-ready AI opportunity brief. Rank candidates by expected value, implementation complexity, operator trust requirements, and data/tool prerequisites. Recommend the best near-term pilots and explain why weaker options were not prioritized.",
            priority: 1,
            phase_id: "recommendation",
            lane: "synthesis",
            depends_on: ["workflow-analysis", "capability-map"],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "brief_markdown",
              summary_guidance:
                "Ranked AI opportunities, reasoning, operational implications, and recommended pilots.",
            },
          },
        ],
        review_stages: [
          {
            stage_id: "opportunity-review",
            stage_kind: "review",
            title: "Feasibility and trust review",
            target_ids: ["opportunity-brief"],
            role: "reviewer",
            prompt:
              "Review the opportunity brief for realism. Reject vague AI claims, hidden implementation assumptions, and missing trust or approval considerations. The brief should make clear what can be automated, what needs a copilot, and what still requires strong human judgment.",
            checklist: [
              "Opportunities are realistically scoped",
              "Human approval needs are explicit",
              "Recommended pilots are specific and actionable",
            ],
            priority: 1,
            phase_id: "recommendation",
            lane: "review",
            tool_allowlist_override: [],
            mcp_servers_override: [],
          },
        ],
      };
    case "workflow-audit":
      return {
        ...root,
        title: "Workflow automation audit",
        goal: "Audit an existing workflow and produce a concrete automation design with failure points, control points, and implementation recommendations.",
        success_criteria: [
          "Current-state workflow is documented",
          "Automation candidates include controls and failure handling",
          "A practical implementation plan is produced",
        ],
        shared_context:
          "Focus on operational reliability. Make human approvals, logging needs, repair loops, and recovery paths explicit. Favor concrete workflow shapes over abstract transformation language.",
        phases: [
          { phase_id: "audit", title: "Audit", execution_mode: "soft" },
          { phase_id: "design", title: "Design", execution_mode: "barrier" },
        ],
        milestones: [
          {
            milestone_id: "audit-complete",
            title: "Audit complete",
            phase_id: "audit",
            required_stage_ids: ["current-state", "failure-analysis"],
          },
        ],
        workstreams: [
          {
            workstream_id: "current-state",
            title: "Current-state workflow",
            objective: "Document the current workflow, actors, triggers, tools, and handoffs.",
            role: "operator",
            prompt:
              "Act as an operations architect. Document the workflow as it exists today: triggers, inputs, outputs, handoffs, approvals, tools, data dependencies, and places where work stalls or gets retried.",
            priority: 1,
            phase_id: "audit",
            lane: "mapping",
            milestone: "audit-complete",
            depends_on: [],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "report_markdown",
              summary_guidance:
                "Current-state workflow map, actors, tools, handoffs, and bottlenecks.",
            },
          },
          {
            workstream_id: "failure-analysis",
            title: "Failure and control analysis",
            objective:
              "Identify where workflow automation can fail, what needs approval, and what must be observable.",
            role: "reviewer",
            prompt:
              "Act as a workflow reliability reviewer. Inspect the current-state map and identify failure modes, ambiguous steps, risky autonomous actions, missing approvals, recovery gaps, and required logging or metrics.",
            priority: 1,
            phase_id: "audit",
            lane: "controls",
            milestone: "audit-complete",
            depends_on: [],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "brief_markdown",
              summary_guidance:
                "Failure modes, control points, approval needs, logging, and recovery requirements.",
            },
          },
          {
            workstream_id: "automation-design",
            title: "Automation design",
            objective: "Turn the audit into a practical automation architecture and rollout plan.",
            role: "planner",
            prompt:
              "Design the target workflow automation. Specify which steps remain human, which become deterministic automation, which should use agents, and where review gates, kill switches, repair loops, and observability are required. Produce a rollout plan that can be implemented incrementally.",
            priority: 2,
            phase_id: "design",
            lane: "design",
            depends_on: ["current-state", "failure-analysis"],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "plan_markdown",
              summary_guidance:
                "Target workflow design, control model, observability plan, and incremental rollout steps.",
            },
          },
        ],
        review_stages: [
          {
            stage_id: "automation-review",
            stage_kind: "approval",
            title: "Automation readiness review",
            target_ids: ["automation-design"],
            role: "approver",
            prompt:
              "Review the automation design for realism, controllability, operator trust, and failure recovery. Reject designs that automate too aggressively without approvals or observability.",
            checklist: [
              "Control points are explicit",
              "Failure recovery is defined",
              "Rollout is incremental and realistic",
            ],
            priority: 1,
            phase_id: "design",
            lane: "approval",
            tool_allowlist_override: [],
            mcp_servers_override: [],
            gate: {
              required: true,
              decisions: ["approve", "rework", "cancel"],
              rework_targets: ["automation-design"],
              instructions:
                "Approve only when the automation design is safe, observable, and realistically deployable.",
            },
          },
        ],
      };
    case "agentic-design":
      return {
        ...root,
        title: "Agentic system design mission",
        goal: "Design a multi-agent workflow for a target operation, including orchestration, handoffs, models, tools, and safeguards.",
        success_criteria: [
          "Agent roles and responsibilities are clearly defined",
          "Handoffs, gates, and dependencies are explicit",
          "The design includes observability, stop controls, and repair flow",
        ],
        shared_context:
          "This is a design mission, not code generation. Optimize for robust orchestration, role clarity, bounded autonomy, and human trust. Make model/tool choices explicit where they matter.",
        phases: [
          { phase_id: "architecture", title: "Architecture", execution_mode: "soft" },
          { phase_id: "governance", title: "Governance", execution_mode: "barrier" },
        ],
        milestones: [
          {
            milestone_id: "design-basis-ready",
            title: "Design basis ready",
            phase_id: "architecture",
            required_stage_ids: ["role-design", "flow-design"],
          },
        ],
        workstreams: [
          {
            workstream_id: "role-design",
            title: "Role and agent design",
            objective:
              "Define the orchestrator and worker roles with clear responsibilities and escalation boundaries.",
            role: "architect",
            prompt:
              "Design the agent roles for this system. Define the orchestrator, specialized workers, reviewers, testers, and approval actors. Be explicit about what each role owns, when it should escalate, and what outputs it produces.",
            priority: 1,
            phase_id: "architecture",
            lane: "roles",
            milestone: "design-basis-ready",
            depends_on: [],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "report_markdown",
              summary_guidance:
                "Agent roster, role boundaries, escalation points, and expected outputs.",
            },
          },
          {
            workstream_id: "flow-design",
            title: "Flow and handoff design",
            objective: "Design the mission graph, dependencies, handoffs, and artifact contracts.",
            role: "analyst",
            prompt:
              "Design the multi-agent workflow graph. Specify phases, dependencies, artifact contracts, fan-out and fan-in points, and where review or approval gates should exist. Make the handoffs explicit and operationally understandable.",
            priority: 1,
            phase_id: "architecture",
            lane: "flow",
            milestone: "design-basis-ready",
            depends_on: [],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "report_markdown",
              summary_guidance: "Mission graph, dependencies, handoffs, and output contracts.",
            },
          },
          {
            workstream_id: "governance-design",
            title: "Governance and safety design",
            objective:
              "Define runtime controls, observability, kill switches, and repair mechanisms.",
            role: "coordinator",
            prompt:
              "Design the governance layer for the system. Specify approval gates, model and tool boundaries, logging, token or budget guardrails, kill switch semantics, pause and resume behavior, and step-level repair expectations.",
            priority: 1,
            phase_id: "governance",
            lane: "governance",
            depends_on: ["role-design", "flow-design"],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "plan_markdown",
              summary_guidance:
                "Safety model, review points, observability, recovery, and operator controls.",
            },
          },
        ],
        review_stages: [
          {
            stage_id: "design-approval",
            stage_kind: "approval",
            title: "Agentic design review",
            target_ids: ["governance-design"],
            role: "approver",
            prompt:
              "Review the complete agentic design for coherence. Reject designs with unclear role boundaries, missing observability, or unbounded autonomy.",
            checklist: [
              "Role boundaries are clear",
              "Handoffs and gates are explicit",
              "Safety and repair paths are credible",
            ],
            priority: 1,
            phase_id: "governance",
            lane: "approval",
            tool_allowlist_override: [],
            mcp_servers_override: [],
            gate: {
              required: true,
              decisions: ["approve", "rework", "cancel"],
              rework_targets: ["governance-design"],
              instructions:
                "Use rework if the design lacks control boundaries, clear outputs, or repairability.",
            },
          },
        ],
      };
    case "automation-rollout":
      return {
        ...root,
        title: "Automation rollout mission",
        goal: "Plan the rollout of an AI or agentic automation initiative across process, tooling, operating model, and measurement.",
        success_criteria: [
          "Rollout plan includes sequencing, owners, risks, and readiness needs",
          "Metrics and operator controls are explicit",
          "Human change-management needs are addressed",
        ],
        shared_context:
          "Optimize for actual operational rollout. Include enablement, communications, governance, training, and success measurement. Avoid purely technical plans.",
        phases: [
          { phase_id: "readiness", title: "Readiness", execution_mode: "soft" },
          { phase_id: "launch", title: "Launch", execution_mode: "barrier" },
        ],
        milestones: [],
        workstreams: [
          {
            workstream_id: "process-readiness",
            title: "Process readiness",
            objective: "Define the target operating process, roles, and readiness gaps.",
            role: "operator",
            prompt:
              "Plan the operating-process changes required to adopt the automation. Identify role changes, review points, ownership, runbooks, and readiness blockers.",
            priority: 1,
            phase_id: "readiness",
            lane: "operations",
            depends_on: [],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "plan_markdown",
              summary_guidance:
                "Operating model changes, ownership, readiness blockers, and process notes.",
            },
          },
          {
            workstream_id: "platform-readiness",
            title: "Platform and tooling readiness",
            objective:
              "Define the tooling, integration, data, and observability requirements for launch.",
            role: "planner",
            prompt:
              "Plan the platform requirements for rollout: integrations, tools, permissions, logs, metrics, data dependencies, guardrails, and monitoring expectations.",
            priority: 1,
            phase_id: "readiness",
            lane: "platform",
            depends_on: [],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "plan_markdown",
              summary_guidance: "Tooling, integrations, observability, and launch prerequisites.",
            },
          },
          {
            workstream_id: "launch-plan",
            title: "Rollout and adoption plan",
            objective:
              "Create the rollout sequence, communications, operator enablement, and measurement plan.",
            role: "coordinator",
            prompt:
              "Using the readiness work, create a rollout plan with phases, launch criteria, internal communications, operator training, fallback plans, and success metrics. Make the rollout sequence explicit.",
            priority: 2,
            phase_id: "launch",
            lane: "rollout",
            depends_on: ["process-readiness", "platform-readiness"],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "plan_markdown",
              summary_guidance:
                "Rollout sequencing, enablement, communications, fallback, and success metrics.",
            },
          },
        ],
        review_stages: [
          {
            stage_id: "launch-gate",
            stage_kind: "approval",
            title: "Launch readiness gate",
            target_ids: ["launch-plan"],
            role: "approver",
            prompt:
              "Review the rollout plan for operational readiness. Confirm that controls, training, communications, fallback, and measurement are credible before launch.",
            checklist: [
              "Launch prerequisites are explicit",
              "Operator enablement is covered",
              "Fallback and measurement are defined",
            ],
            priority: 1,
            phase_id: "launch",
            lane: "approval",
            tool_allowlist_override: [],
            mcp_servers_override: [],
            gate: {
              required: true,
              decisions: ["approve", "rework", "cancel"],
              rework_targets: ["launch-plan"],
              instructions:
                "Use rework if the rollout plan lacks readiness criteria, operator enablement, or fallback handling.",
            },
          },
        ],
      };
  }
}

function validateWorkspaceRootInput(raw: string) {
  const value = String(raw || "").trim();
  if (!value) return "Workspace root is required.";
  if (!value.startsWith("/")) return "Workspace root must be an absolute path.";
  return "";
}

function validateMissionBlueprintForUi(blueprint: MissionBlueprint) {
  const messages: string[] = [];
  if (!String(blueprint.title || "").trim()) messages.push("Mission title is required.");
  if (!String(blueprint.goal || "").trim()) messages.push("Mission goal is required.");
  const workspaceError = validateWorkspaceRootInput(blueprint.workspace_root);
  if (workspaceError) messages.push(workspaceError);
  if (!Array.isArray(blueprint.workstreams) || blueprint.workstreams.length === 0) {
    messages.push("At least one workstream is required.");
  }
  return messages;
}

function extractMissionBlueprint(automation: any, workspaceRoot: string): MissionBlueprint | null {
  const metadata =
    automation?.metadata && typeof automation.metadata === "object" ? automation.metadata : {};
  const blueprint =
    metadata.mission_blueprint || metadata.missionBlueprint || metadata.mission_blueprint_v1;
  if (!blueprint || typeof blueprint !== "object") return null;
  const next = blueprint as MissionBlueprint;
  return {
    ...defaultBlueprint(workspaceRoot),
    ...next,
    workspace_root: String(next.workspace_root || workspaceRoot || "").trim(),
    phases:
      Array.isArray(next.phases) && next.phases.length
        ? next.phases
        : defaultBlueprint(workspaceRoot).phases,
    milestones: Array.isArray(next.milestones) ? next.milestones : [],
    workstreams: Array.isArray(next.workstreams) ? next.workstreams : [],
    review_stages: Array.isArray(next.review_stages) ? next.review_stages : [],
  };
}

function Section({
  title,
  subtitle,
  icon,
  children,
}: {
  title: string;
  subtitle?: string;
  icon?: string;
  children: any;
}) {
  return (
    <div className="rounded-xl border border-slate-700/50 bg-slate-950/50 p-4">
      <div className="mb-3">
        <div className="flex items-center gap-2 text-sm font-semibold text-slate-100">
          {icon ? <i data-lucide={icon}></i> : null}
          <span>{title}</span>
        </div>
        {subtitle ? <div className="tcp-subtle mt-1 text-xs">{subtitle}</div> : null}
      </div>
      <div className="grid gap-3">{children}</div>
    </div>
  );
}

function LabeledInput({
  label,
  value,
  onInput,
  placeholder,
  type = "text",
}: {
  label: string;
  value: string | number;
  onInput: (value: string) => void;
  placeholder?: string;
  type?: string;
}) {
  return (
    <label className="block text-sm">
      <div className="mb-1 font-medium text-slate-200">{label}</div>
      <input
        type={type}
        value={value as any}
        placeholder={placeholder}
        onInput={(event) => onInput((event.target as HTMLInputElement).value)}
        className="h-10 w-full rounded-lg border border-slate-700 bg-slate-950/80 px-3 text-sm text-slate-100 outline-none focus:border-amber-400"
      />
    </label>
  );
}

function LabeledTextArea({
  label,
  value,
  onInput,
  placeholder,
  rows = 5,
}: {
  label: string;
  value: string;
  onInput: (value: string) => void;
  placeholder?: string;
  rows?: number;
}) {
  return (
    <label className="block text-sm">
      <div className="mb-1 font-medium text-slate-200">{label}</div>
      <textarea
        rows={rows}
        value={value}
        placeholder={placeholder}
        onInput={(event) => onInput((event.target as HTMLTextAreaElement).value)}
        className="min-h-[108px] w-full rounded-lg border border-slate-700 bg-slate-950/80 px-3 py-2 text-sm text-slate-100 outline-none focus:border-amber-400"
      />
    </label>
  );
}

function ToggleChip({
  active,
  label,
  icon,
  onClick,
}: {
  active: boolean;
  label: string;
  icon?: string;
  onClick: () => void;
}) {
  return (
    <button
      className={`tcp-btn h-8 px-3 text-xs ${active ? "border-amber-400/60 bg-amber-400/10 text-amber-300" : ""}`}
      onClick={onClick}
      type="button"
    >
      <span className="inline-flex items-center gap-2">
        {icon ? <i data-lucide={icon}></i> : null}
        <span>{label}</span>
      </span>
    </button>
  );
}

function InlineHint({ children }: { children: any }) {
  return <div className="tcp-subtle -mt-1 text-xs">{children}</div>;
}

function EmptyState({ text }: { text: string }) {
  return (
    <div className="tcp-subtle rounded-lg border border-slate-800 bg-slate-950/50 p-3 text-xs">
      {text}
    </div>
  );
}

function WorkspaceDirectoryPicker({
  value,
  error,
  open,
  browseDir,
  search,
  parentDir,
  currentDir,
  directories,
  onOpen,
  onClose,
  onClear,
  onSearchChange,
  onBrowseParent,
  onBrowseDirectory,
  onSelectDirectory,
}: {
  value: string;
  error: string;
  open: boolean;
  browseDir: string;
  search: string;
  parentDir: string;
  currentDir: string;
  directories: any[];
  onOpen: () => void;
  onClose: () => void;
  onClear: () => void;
  onSearchChange: (value: string) => void;
  onBrowseParent: () => void;
  onBrowseDirectory: (path: string) => void;
  onSelectDirectory: () => void;
}) {
  const searchQuery = String(search || "")
    .trim()
    .toLowerCase();
  return (
    <>
      <label className="block text-sm">
        <div className="mb-1 font-medium text-slate-200">Workspace root</div>
        <div className="grid gap-2 md:grid-cols-[auto_1fr_auto]">
          <button className="tcp-btn h-10 px-3" type="button" onClick={onOpen}>
            <i data-lucide="folder-open"></i>
            Browse
          </button>
          <input
            className={`tcp-input text-sm ${error ? "border-red-500/70 text-red-100" : ""}`}
            value={value}
            readOnly
            placeholder="No local directory selected. Use Browse."
          />
          <button className="tcp-btn h-10 px-3" type="button" onClick={onClear} disabled={!value}>
            <i data-lucide="x"></i>
            Clear
          </button>
        </div>
      </label>
      {open ? (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
          <button
            type="button"
            className="tcp-confirm-backdrop"
            aria-label="Close workspace directory dialog"
            onClick={onClose}
          />
          <div className="tcp-confirm-dialog max-w-2xl">
            <h3 className="tcp-confirm-title">Select Workspace Folder</h3>
            <p className="tcp-confirm-message">Current: {currentDir || browseDir || "n/a"}</p>
            <div className="mb-2 flex flex-wrap gap-2">
              <button
                className="tcp-btn"
                type="button"
                onClick={onBrowseParent}
                disabled={!parentDir}
              >
                <i data-lucide="arrow-left-to-line"></i>
                Up
              </button>
              <button
                className="tcp-btn-primary"
                type="button"
                onClick={onSelectDirectory}
                disabled={!currentDir}
              >
                <i data-lucide="badge-check"></i>
                Select This Folder
              </button>
              <button className="tcp-btn" type="button" onClick={onClose}>
                <i data-lucide="x"></i>
                Close
              </button>
            </div>
            <div className="mb-2">
              <input
                className="tcp-input"
                placeholder="Type to filter folders..."
                value={search}
                onInput={(event) => onSearchChange((event.target as HTMLInputElement).value)}
              />
            </div>
            <div className="max-h-[360px] overflow-auto rounded-lg border border-slate-700/60 bg-slate-900/20 p-2">
              {directories.length ? (
                directories.map((entry: any) => (
                  <button
                    key={String(entry?.path || entry?.name)}
                    className="tcp-list-item mb-1 w-full text-left"
                    type="button"
                    onClick={() => onBrowseDirectory(String(entry?.path || ""))}
                  >
                    <span className="inline-flex items-center gap-2">
                      <i data-lucide="folder-open"></i>
                      <span>{String(entry?.name || entry?.path || "")}</span>
                    </span>
                  </button>
                ))
              ) : (
                <EmptyState
                  text={
                    searchQuery
                      ? "No folders match your search."
                      : "No subdirectories in this folder."
                  }
                />
              )}
            </div>
          </div>
        </div>
      ) : null}
    </>
  );
}

function ProviderModelSelector({
  providerLabel,
  modelLabel,
  draft,
  providers,
  onChange,
  inheritLabel = "Inherit team default",
}: {
  providerLabel: string;
  modelLabel: string;
  draft: ModelDraft;
  providers: ProviderOption[];
  onChange: (draft: ModelDraft) => void;
  inheritLabel?: string;
}) {
  const modelOptions = providers.find((provider) => provider.id === draft.provider)?.models || [];
  return (
    <div className="grid gap-3 md:grid-cols-2">
      <label className="block text-sm">
        <div className="mb-1 flex items-center gap-2 font-medium text-slate-200">
          <i data-lucide="cpu"></i>
          <span>{providerLabel}</span>
        </div>
        <select
          value={draft.provider}
          onInput={(event) => {
            const provider = (event.target as HTMLSelectElement).value;
            const nextModels = providers.find((row) => row.id === provider)?.models || [];
            onChange({ provider, model: nextModels[0] || "" });
          }}
          className="tcp-select h-10 w-full"
        >
          <option value="">{inheritLabel}</option>
          {providers.map((provider) => (
            <option key={provider.id} value={provider.id}>
              {provider.id}
              {provider.configured === false ? " (not configured)" : ""}
            </option>
          ))}
        </select>
      </label>
      <label className="block text-sm">
        <div className="mb-1 flex items-center gap-2 font-medium text-slate-200">
          <i data-lucide="sparkles"></i>
          <span>{modelLabel}</span>
        </div>
        <select
          value={draft.model}
          onInput={(event) =>
            onChange({ ...draft, model: (event.target as HTMLSelectElement).value })
          }
          className="tcp-select h-10 w-full"
          disabled={!draft.provider}
        >
          <option value="">{draft.provider ? "Select a model" : inheritLabel}</option>
          {modelOptions.map((model) => (
            <option key={model} value={model}>
              {model}
            </option>
          ))}
        </select>
      </label>
    </div>
  );
}

export function AdvancedMissionBuilderPanel({
  client,
  api,
  toast,
  defaultProvider,
  defaultModel,
  editingAutomation = null,
  onShowAutomations,
  onShowRuns,
  onClearEditing,
}: {
  client: TandemClient;
  api: ApiFn;
  toast: (kind: "ok" | "info" | "warn" | "err", text: string) => void;
  defaultProvider: string;
  defaultModel: string;
  editingAutomation?: any | null;
  onShowAutomations: () => void;
  onShowRuns: () => void;
  onClearEditing?: () => void;
}) {
  const queryClient = useQueryClient();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [activeTab, setActiveTab] = useState<CreateModeTab>("mission");
  const [scheduleKind, setScheduleKind] = useState<ScheduleKind>("manual");
  const [intervalSeconds, setIntervalSeconds] = useState("3600");
  const [cronExpression, setCronExpression] = useState("");
  const [runAfterCreate, setRunAfterCreate] = useState(true);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<"" | "preview" | "apply">("");
  const [preview, setPreview] = useState<any>(null);
  const [workspaceRoot, setWorkspaceRoot] = useState("");
  const [blueprint, setBlueprint] = useState<MissionBlueprint>(defaultBlueprint(""));
  const [teamModel, setTeamModel] = useState<ModelDraft>({
    provider: defaultProvider,
    model: defaultModel,
  });
  const [workstreamModels, setWorkstreamModels] = useState<Record<string, ModelDraft>>({});
  const [reviewModels, setReviewModels] = useState<Record<string, ModelDraft>>({});
  const [showGuide, setShowGuide] = useState(false);
  const [workspaceBrowserOpen, setWorkspaceBrowserOpen] = useState(false);
  const [workspaceBrowserDir, setWorkspaceBrowserDir] = useState("");
  const [workspaceBrowserSearch, setWorkspaceBrowserSearch] = useState("");

  const providersCatalogQuery = useQuery({
    queryKey: ["settings", "providers", "catalog"],
    queryFn: () => client.providers.catalog().catch(() => ({ all: [] })),
    refetchInterval: 30000,
  });
  const providersConfigQuery = useQuery({
    queryKey: ["settings", "providers", "config"],
    queryFn: () => client.providers.config().catch(() => ({})),
    refetchInterval: 30000,
  });
  const mcpServersQuery = useQuery({
    queryKey: ["mcp", "servers"],
    queryFn: () => client.mcp.list().catch(() => ({})),
    refetchInterval: 12000,
  });
  const toolIdsQuery = useQuery({
    queryKey: ["tool", "ids"],
    queryFn: () => client.listToolIds().catch(() => []),
    refetchInterval: 30000,
  });
  const templatesQuery = useQuery({
    queryKey: ["agent-team", "templates"],
    queryFn: async () => {
      const response = await client.agentTeams.listTemplates().catch(() => ({ templates: [] }));
      return Array.isArray((response as any)?.templates) ? (response as any).templates : [];
    },
    refetchInterval: 30000,
  });
  const healthQuery = useQuery({
    queryKey: ["global", "health"],
    queryFn: () => client.health().catch(() => ({})),
    refetchInterval: 30000,
  });
  const workspaceBrowserQuery = useQuery({
    queryKey: ["advanced-mission-builder", "workspace-browser", workspaceBrowserDir],
    enabled: workspaceBrowserOpen && !!workspaceBrowserDir,
    queryFn: () =>
      api(`/api/orchestrator/workspaces/list?dir=${encodeURIComponent(workspaceBrowserDir)}`, {
        method: "GET",
      }),
  });

  useEffect(() => {
    const nextWorkspace = String(
      (healthQuery.data as any)?.workspaceRoot || (healthQuery.data as any)?.workspace_root || ""
    ).trim();
    if (!nextWorkspace) return;
    setWorkspaceRoot(nextWorkspace);
    setBlueprint((current) =>
      current.workspace_root
        ? current
        : {
            ...defaultBlueprint(nextWorkspace),
            workspace_root: nextWorkspace,
          }
    );
  }, [healthQuery.data]);

  useEffect(() => {
    const root =
      workspaceRoot ||
      String(
        (healthQuery.data as any)?.workspaceRoot || (healthQuery.data as any)?.workspace_root || ""
      ).trim();
    if (!editingAutomation) {
      setBlueprint(defaultBlueprint(root));
      setPreview(null);
      setError("");
      setRunAfterCreate(true);
      setScheduleKind("manual");
      setIntervalSeconds("3600");
      setCronExpression("");
      setTeamModel({ provider: defaultProvider, model: defaultModel });
      setWorkstreamModels({});
      setReviewModels({});
      return;
    }
    const saved = extractMissionBlueprint(editingAutomation, root);
    if (!saved) return;
    setBlueprint(saved);
    setTeamModel(fromModelPolicy(saved.team.default_model_policy));
    const nextWorkstreamModels: Record<string, ModelDraft> = {};
    for (const workstream of saved.workstreams) {
      nextWorkstreamModels[workstream.workstream_id] = fromModelPolicy(workstream.model_override);
    }
    setWorkstreamModels(nextWorkstreamModels);
    const nextReviewModels: Record<string, ModelDraft> = {};
    for (const stage of saved.review_stages) {
      nextReviewModels[stage.stage_id] = fromModelPolicy(stage.model_override);
    }
    setReviewModels(nextReviewModels);
    const schedule = editingAutomation?.schedule || {};
    const type = String(schedule?.type || "")
      .trim()
      .toLowerCase();
    if (type === "cron") {
      setScheduleKind("cron");
      setCronExpression(String(schedule?.cron_expression || "").trim());
    } else if (type === "interval") {
      setScheduleKind("interval");
      setIntervalSeconds(String(schedule?.interval_seconds || 3600));
    } else {
      setScheduleKind("manual");
      setCronExpression("");
      setIntervalSeconds("3600");
    }
    setRunAfterCreate(false);
    setPreview(null);
    setError("");
  }, [
    editingAutomation?.automation_id,
    workspaceRoot,
    defaultProvider,
    defaultModel,
    healthQuery.data,
  ]);

  const providers = useMemo<ProviderOption[]>(() => {
    const rows = Array.isArray((providersCatalogQuery.data as any)?.all)
      ? (providersCatalogQuery.data as any).all
      : [];
    const configProviders =
      ((providersConfigQuery.data as any)?.providers as Record<string, any> | undefined) || {};
    const mapped = rows
      .map((provider: any) => ({
        id: String(provider?.id || "").trim(),
        models: Object.keys(provider?.models || {}),
        configured: !!configProviders[String(provider?.id || "").trim()],
      }))
      .filter((provider: ProviderOption) => provider.id)
      .sort((a, b) => a.id.localeCompare(b.id));
    if (defaultProvider && !mapped.some((row) => row.id === defaultProvider)) {
      mapped.unshift({
        id: defaultProvider,
        models: defaultModel ? [defaultModel] : [],
        configured: true,
      });
    }
    return mapped;
  }, [defaultModel, defaultProvider, providersCatalogQuery.data, providersConfigQuery.data]);

  const mcpServers = useMemo(
    () => normalizeMcpServers(mcpServersQuery.data),
    [mcpServersQuery.data]
  );
  const toolIds = useMemo(
    () =>
      (Array.isArray(toolIdsQuery.data) ? toolIdsQuery.data : [])
        .map((value) => String(value || "").trim())
        .filter(Boolean)
        .sort(),
    [toolIdsQuery.data]
  );
  const templates = useMemo(
    () =>
      (Array.isArray(templatesQuery.data) ? templatesQuery.data : [])
        .map((row: any) => ({
          template_id: String(row?.template_id || row?.templateId || "").trim(),
          role: String(row?.role || "").trim(),
        }))
        .filter((row) => row.template_id),
    [templatesQuery.data]
  );
  const workspaceDirectories = Array.isArray((workspaceBrowserQuery.data as any)?.directories)
    ? (workspaceBrowserQuery.data as any).directories
    : [];
  const workspaceParentDir = String((workspaceBrowserQuery.data as any)?.parent || "").trim();
  const workspaceCurrentBrowseDir = String(
    (workspaceBrowserQuery.data as any)?.dir || workspaceBrowserDir || ""
  ).trim();
  const filteredWorkspaceDirectories = useMemo(() => {
    const search = String(workspaceBrowserSearch || "")
      .trim()
      .toLowerCase();
    if (!search) return workspaceDirectories;
    return workspaceDirectories.filter((entry: any) =>
      String(entry?.name || entry?.path || "")
        .trim()
        .toLowerCase()
        .includes(search)
    );
  }, [workspaceBrowserSearch, workspaceDirectories]);
  const workspaceRootError = validateWorkspaceRootInput(blueprint.workspace_root || workspaceRoot);

  useEffect(() => {
    try {
      if (rootRef.current) renderIcons(rootRef.current);
      else renderIcons();
    } catch {}
  }, [activeTab, showGuide, preview, busy, blueprint, teamModel, workstreamModels, reviewModels]);

  const effectiveBlueprint = useMemo(() => {
    return {
      ...blueprint,
      workspace_root: blueprint.workspace_root || workspaceRoot,
      team: {
        ...blueprint.team,
        default_model_policy: toModelPolicy(teamModel),
      },
      workstreams: blueprint.workstreams.map((workstream) => ({
        ...workstream,
        model_override: toModelPolicy(
          workstreamModels[workstream.workstream_id] || { provider: "", model: "" }
        ),
      })),
      review_stages: blueprint.review_stages.map((stage) => ({
        ...stage,
        model_override: toModelPolicy(reviewModels[stage.stage_id] || { provider: "", model: "" }),
      })),
    };
  }, [blueprint, workspaceRoot, teamModel, workstreamModels, reviewModels]);
  const uiValidationMessages = useMemo(
    () => validateMissionBlueprintForUi(effectiveBlueprint),
    [effectiveBlueprint]
  );

  const stageIds = useMemo(
    () => [
      ...effectiveBlueprint.workstreams.map((workstream) => workstream.workstream_id),
      ...effectiveBlueprint.review_stages.map((stage) => stage.stage_id),
    ],
    [effectiveBlueprint]
  );

  function updateBlueprint(patch: Partial<MissionBlueprint>) {
    setBlueprint((current) => ({ ...current, ...patch }));
    setPreview(null);
  }

  function addWorkstream() {
    setBlueprint((current) => ({
      ...current,
      workstreams: [
        ...current.workstreams,
        {
          workstream_id: `workstream_${crypto.randomUUID().slice(0, 8)}`,
          title: `Workstream ${current.workstreams.length + 1}`,
          objective: "",
          role: "worker",
          prompt: "",
          priority: current.workstreams.length + 1,
          phase_id: current.phases[0]?.phase_id || "",
          lane: `lane_${current.workstreams.length + 1}`,
          milestone: "",
          depends_on: [],
          input_refs: [],
          tool_allowlist_override: [],
          mcp_servers_override: [],
          output_contract: { kind: "report_markdown", summary_guidance: "" },
        },
      ],
    }));
    setPreview(null);
  }

  function addReviewStage() {
    setBlueprint((current) => ({
      ...current,
      review_stages: [
        ...current.review_stages,
        {
          stage_id: `review_${crypto.randomUUID().slice(0, 8)}`,
          stage_kind: "approval",
          title: `Gate ${current.review_stages.length + 1}`,
          target_ids: [],
          role: "reviewer",
          prompt: "",
          checklist: [],
          priority: current.review_stages.length + 1,
          phase_id: current.phases[0]?.phase_id || "",
          lane: "review",
          milestone: "",
          tool_allowlist_override: [],
          mcp_servers_override: [],
          gate: {
            required: true,
            decisions: ["approve", "rework", "cancel"],
            rework_targets: [],
            instructions: "",
          },
        },
      ],
    }));
    setPreview(null);
  }

  function applyStarterPreset(preset: StarterPresetId) {
    const presetRecord = STARTER_PRESETS.find((entry) => entry.id === preset);
    if (!presetRecord) return;
    const next = {
      ...defaultBlueprint(blueprint.workspace_root || workspaceRoot),
      ...presetRecord.blueprint,
      mission_id: `mission_${crypto.randomUUID().slice(0, 8)}`,
      workspace_root: blueprint.workspace_root || workspaceRoot,
    };
    setBlueprint(next);
    setPreview(null);
    setError("");
    setActiveTab("mission");
    setTeamModel({ provider: defaultProvider, model: defaultModel });
    setWorkstreamModels({});
    setReviewModels({});
    toast(
      "info",
      `Loaded ${presetRecord.label}. Review the prompts and adapt them to your mission.`
    );
  }

  async function compilePreview() {
    if (uiValidationMessages.length) {
      const message = uiValidationMessages.join(" ");
      setError(message);
      toast("err", message);
      setActiveTab("mission");
      return;
    }
    setBusy("preview");
    setError("");
    try {
      const response = await api("/api/engine/mission-builder/compile-preview", {
        method: "POST",
        body: JSON.stringify({
          blueprint: effectiveBlueprint,
          schedule: scheduleToPayload(scheduleKind, intervalSeconds, cronExpression),
        }),
      });
      setPreview(response);
      setActiveTab("compile");
    } catch (compileError) {
      const fallback =
        compileError instanceof Error ? compileError.message : String(compileError || "");
      const message =
        fallback === "mission blueprint validation failed" && uiValidationMessages.length
          ? uiValidationMessages.join(" ")
          : fallback;
      setError(message);
      toast("err", message);
    } finally {
      setBusy("");
    }
  }

  async function saveMission() {
    if (uiValidationMessages.length) {
      const message = uiValidationMessages.join(" ");
      setError(message);
      toast("err", message);
      setActiveTab("mission");
      return;
    }
    setBusy("apply");
    setError("");
    try {
      const schedule = scheduleToPayload(scheduleKind, intervalSeconds, cronExpression);
      if (editingAutomation?.automation_id) {
        const compiled = await api("/api/engine/mission-builder/compile-preview", {
          method: "POST",
          body: JSON.stringify({ blueprint: effectiveBlueprint, schedule }),
        });
        await client.automationsV2.update(editingAutomation.automation_id, {
          name: compiled?.automation?.name,
          description: compiled?.automation?.description || null,
          schedule: compiled?.automation?.schedule,
          agents: compiled?.automation?.agents,
          flow: compiled?.automation?.flow,
          execution: compiled?.automation?.execution,
          workspace_root: compiled?.automation?.workspace_root,
          metadata: compiled?.automation?.metadata,
        });
        await Promise.all([
          queryClient.invalidateQueries({ queryKey: ["automations"] }),
          queryClient.invalidateQueries({ queryKey: ["automations", "v2", "list"] }),
        ]);
        toast("ok", "Advanced mission updated.");
        onClearEditing?.();
        onShowAutomations();
        return;
      }
      const response = await api("/api/engine/mission-builder/apply", {
        method: "POST",
        body: JSON.stringify({
          blueprint: effectiveBlueprint,
          creator_id: "control-panel",
          schedule,
        }),
      });
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["automations"] }),
        queryClient.invalidateQueries({ queryKey: ["automations", "v2", "list"] }),
      ]);
      const automationId = String(response?.automation?.automation_id || "").trim();
      if (runAfterCreate && automationId) {
        await client.automationsV2.runNow(automationId);
        toast("ok", "Advanced mission created and started.");
        onShowRuns();
      } else {
        toast("ok", "Advanced mission created.");
        onShowAutomations();
      }
      setBlueprint(defaultBlueprint(workspaceRoot));
      setPreview(null);
      setRunAfterCreate(true);
    } catch (applyError) {
      const fallback = applyError instanceof Error ? applyError.message : String(applyError || "");
      const message =
        fallback === "mission blueprint validation failed" && uiValidationMessages.length
          ? uiValidationMessages.join(" ")
          : fallback;
      setError(message);
      toast("err", message);
    } finally {
      setBusy("");
    }
  }

  return (
    <div ref={rootRef} className="grid gap-4">
      <div className="rounded-xl border border-slate-700/50 bg-slate-950/50 p-3">
        <div className="mb-2 text-xs font-medium uppercase tracking-[0.24em] text-slate-500">
          Mission Builder
        </div>
        <div className="tcp-subtle text-xs">
          Build one coordinated swarm mission with shared context, per-lane roles, explicit
          handoffs, and a compiled preview before launch.
        </div>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <button className="tcp-btn h-8 px-3 text-xs" onClick={() => setShowGuide(true)}>
            <i data-lucide="book-open"></i>
            How this works
          </button>
          <span className="tcp-subtle text-xs">Start from example:</span>
          <button
            className="tcp-btn h-8 px-3 text-xs"
            onClick={() => applyStarterPreset("ai-opportunity")}
          >
            <i data-lucide="sparkles"></i>
            AI Opportunities
          </button>
          <button
            className="tcp-btn h-8 px-3 text-xs"
            onClick={() => applyStarterPreset("workflow-audit")}
          >
            <i data-lucide="workflow"></i>
            Workflow Audit
          </button>
          <button
            className="tcp-btn h-8 px-3 text-xs"
            onClick={() => applyStarterPreset("agentic-design")}
          >
            <i data-lucide="bot"></i>
            Agentic Design
          </button>
          <button
            className="tcp-btn h-8 px-3 text-xs"
            onClick={() => applyStarterPreset("automation-rollout")}
          >
            <i data-lucide="arrow-up-circle"></i>
            Rollout
          </button>
        </div>
        <div className="mt-3 flex flex-wrap gap-2">
          {(
            [
              ["mission", "clipboard-list"],
              ["team", "users"],
              ["workstreams", "workflow"],
              ["review", "shield-check"],
              ["compile", "binary"],
            ] as Array<[CreateModeTab, string]>
          ).map(([tab, icon]) => (
            <ToggleChip
              key={tab}
              active={activeTab === tab}
              label={tab === "workstreams" ? "workstreams" : tab}
              icon={icon}
              onClick={() => setActiveTab(tab)}
            />
          ))}
        </div>
      </div>

      {error ? (
        <div className="rounded-xl border border-red-500/40 bg-red-500/10 p-3 text-sm text-red-200">
          {error}
        </div>
      ) : null}

      {editingAutomation ? (
        <div className="rounded-xl border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-200">
          Editing advanced mission:{" "}
          <strong>
            {String(editingAutomation?.name || editingAutomation?.automation_id || "")}
          </strong>
        </div>
      ) : null}

      {showGuide ? (
        <div className="fixed inset-0 z-50 flex items-start justify-center bg-slate-950/80 p-4 backdrop-blur-sm">
          <div className="max-h-[90vh] w-full max-w-4xl overflow-y-auto rounded-2xl border border-slate-700 bg-slate-950 p-5 shadow-2xl">
            <div className="mb-4 flex items-start justify-between gap-4">
              <div>
                <div className="text-lg font-semibold text-slate-100">
                  How the Advanced Swarm Builder Works
                </div>
                <div className="tcp-subtle mt-1 text-sm">
                  Think of this as a mission compiler: one shared goal, many scoped workstreams,
                  explicit handoffs, and optional review gates.
                </div>
              </div>
              <button className="tcp-btn h-9 px-3 text-sm" onClick={() => setShowGuide(false)}>
                Close
              </button>
            </div>

            <div className="grid gap-4 lg:grid-cols-2">
              <Section
                title="What goes where"
                subtitle="Use the right field for the right level of instruction."
              >
                <div className="grid gap-2 text-sm text-slate-300">
                  <div>
                    <strong className="text-slate-100">Mission goal:</strong> the one shared outcome
                    for the whole operation.
                  </div>
                  <div>
                    <strong className="text-slate-100">Success criteria:</strong> concrete checks
                    for whether the mission is done well.
                  </div>
                  <div>
                    <strong className="text-slate-100">Shared context:</strong> facts, constraints,
                    tone, audience, deadlines, approved sources.
                  </div>
                  <div>
                    <strong className="text-slate-100">Workstream objective:</strong> the local
                    assignment for that lane.
                  </div>
                  <div>
                    <strong className="text-slate-100">Workstream prompt:</strong> the operating
                    instructions for how that lane should work.
                  </div>
                  <div>
                    <strong className="text-slate-100">Output contract:</strong> the artifact that
                    downstream work expects to receive.
                  </div>
                  <div>
                    <strong className="text-slate-100">Review / gate prompt:</strong> what a
                    reviewer or approver must check before promotion.
                  </div>
                </div>
              </Section>

              <Section
                title="How to get good results"
                subtitle="The builder works best when each stage is explicit."
              >
                <div className="grid gap-2 text-sm text-slate-300">
                  <div>Keep the mission goal outcome-based, not a long checklist.</div>
                  <div>Make success criteria measurable.</div>
                  <div>Give each workstream one clear responsibility.</div>
                  <div>Use dependencies only for real handoffs.</div>
                  <div>Define outputs as concrete artifacts, not vague intentions.</div>
                  <div>Use review gates for quality and promotion, not for every step.</div>
                  <div>
                    Prefer prompts that say what evidence, format, and audience the step should
                    target.
                  </div>
                </div>
              </Section>

              <Section
                title="Prompt pattern"
                subtitle="A reliable starting scaffold for most workstreams."
              >
                <div className="rounded-lg border border-slate-800 bg-slate-900/70 p-3 text-xs text-slate-300">
                  <div>
                    <strong className="text-slate-100">Mission goal</strong>
                  </div>
                  <div className="mt-1">
                    Produce a coordinated launch plan for Product X for the next 30 days.
                  </div>
                  <div className="mt-3">
                    <strong className="text-slate-100">Shared context</strong>
                  </div>
                  <div className="mt-1">
                    Audience is SMB owners. Tone is clear and practical. Use approved workspace and
                    MCP sources only. Avoid speculative claims.
                  </div>
                  <div className="mt-3">
                    <strong className="text-slate-100">Workstream objective</strong>
                  </div>
                  <div className="mt-1">
                    Analyze the target workflow and identify the 3 highest-value AI opportunities
                    for a near-term pilot.
                  </div>
                  <div className="mt-3">
                    <strong className="text-slate-100">Workstream prompt</strong>
                  </div>
                  <div className="mt-1">
                    Act as an AI workflow strategist. Map the current process, identify repeated
                    manual work, decision bottlenecks, and coordination pain, then recommend
                    realistic AI patterns with evidence and operational caveats.
                  </div>
                  <div className="mt-3">
                    <strong className="text-slate-100">Output contract</strong>
                  </div>
                  <div className="mt-1">
                    A markdown memo with sections: workflow summary, AI opportunities, feasibility
                    constraints, risks, and recommended pilots.
                  </div>
                </div>
              </Section>

              <Section
                title="Starter examples"
                subtitle="Use these when you do not want to begin from a blank blueprint."
              >
                <div className="grid gap-2 text-sm text-slate-300">
                  <div>
                    <strong className="text-slate-100">AI Opportunities:</strong> workflow analysis
                    and capability mapping feeding a ranked pilot brief.
                  </div>
                  <div>
                    <strong className="text-slate-100">Workflow Audit:</strong> current-state
                    mapping and failure analysis feeding an automation design review.
                  </div>
                  <div>
                    <strong className="text-slate-100">Agentic Design:</strong> role design and flow
                    design feeding governance and safety review.
                  </div>
                  <div>
                    <strong className="text-slate-100">Rollout:</strong> process readiness and
                    platform readiness feeding a launch plan and approval gate.
                  </div>
                </div>
              </Section>
            </div>
          </div>
        </div>
      ) : null}

      {activeTab === "mission" ? (
        <Section
          title="Mission"
          subtitle="Global brief, success criteria, and schedule."
          icon="clipboard-list"
        >
          <div className="grid gap-3 md:grid-cols-2">
            <LabeledInput
              label="Mission title"
              value={blueprint.title}
              onInput={(value) => updateBlueprint({ title: value })}
            />
            <LabeledInput
              label="Mission ID"
              value={blueprint.mission_id}
              onInput={(value) => updateBlueprint({ mission_id: value })}
            />
          </div>
          <InlineHint>
            Use a short title a human operator would recognize later in the automation list.
          </InlineHint>
          <WorkspaceDirectoryPicker
            value={blueprint.workspace_root}
            error={workspaceRootError}
            open={workspaceBrowserOpen}
            browseDir={workspaceBrowserDir}
            search={workspaceBrowserSearch}
            parentDir={workspaceParentDir}
            currentDir={workspaceCurrentBrowseDir}
            directories={filteredWorkspaceDirectories}
            onOpen={() => {
              const seed = String(blueprint.workspace_root || workspaceRoot || "/").trim();
              setWorkspaceBrowserDir(seed || "/");
              setWorkspaceBrowserSearch("");
              setWorkspaceBrowserOpen(true);
            }}
            onClose={() => {
              setWorkspaceBrowserOpen(false);
              setWorkspaceBrowserSearch("");
            }}
            onClear={() => updateBlueprint({ workspace_root: "" })}
            onSearchChange={setWorkspaceBrowserSearch}
            onBrowseParent={() => {
              if (!workspaceParentDir) return;
              setWorkspaceBrowserDir(workspaceParentDir);
            }}
            onBrowseDirectory={(path) => setWorkspaceBrowserDir(path)}
            onSelectDirectory={() => {
              if (!workspaceCurrentBrowseDir) return;
              updateBlueprint({ workspace_root: workspaceCurrentBrowseDir });
              setWorkspaceBrowserOpen(false);
              setWorkspaceBrowserSearch("");
              toast("ok", `Workspace selected: ${workspaceCurrentBrowseDir}`);
            }}
          />
          <InlineHint>
            This is the shared working directory the mission can use for files and artifacts.
          </InlineHint>
          {workspaceRootError ? (
            <div className="text-xs text-red-300">{workspaceRootError}</div>
          ) : null}
          <LabeledTextArea
            label="Mission goal"
            value={blueprint.goal}
            onInput={(value) => updateBlueprint({ goal: value })}
            placeholder="Describe the shared objective all participants are working toward."
          />
          <InlineHint>
            Write the one shared outcome for the whole swarm, not a list of steps.
          </InlineHint>
          <LabeledTextArea
            label="Shared context"
            value={blueprint.shared_context || ""}
            onInput={(value) => updateBlueprint({ shared_context: value })}
            placeholder="Shared constraints, references, context, and operator guidance."
          />
          <InlineHint>
            Put the facts every lane should inherit here: audience, constraints, tone, deadlines,
            approved sources, and things to avoid.
          </InlineHint>
          <LabeledInput
            label="Success criteria"
            value={blueprint.success_criteria.join(", ")}
            onInput={(value) => updateBlueprint({ success_criteria: splitCsv(value) })}
            placeholder="comma-separated"
          />
          <InlineHint>
            These should be measurable checks like “brief includes 5 competitors” or “plan contains
            owner, timeline, and risks”.
          </InlineHint>
          <div className="grid gap-3 md:grid-cols-3">
            <label className="block text-sm">
              <div className="mb-1 font-medium text-slate-200">Schedule</div>
              <select
                value={scheduleKind}
                onInput={(event) =>
                  setScheduleKind((event.target as HTMLSelectElement).value as ScheduleKind)
                }
                className="h-10 w-full rounded-lg border border-slate-700 bg-slate-950/80 px-3 text-sm text-slate-100 outline-none focus:border-amber-400"
              >
                <option value="manual">Manual</option>
                <option value="interval">Interval</option>
                <option value="cron">Cron</option>
              </select>
            </label>
            {scheduleKind === "interval" ? (
              <LabeledInput
                label="Interval seconds"
                value={intervalSeconds}
                onInput={setIntervalSeconds}
              />
            ) : null}
            {scheduleKind === "cron" ? (
              <LabeledInput
                label="Cron expression"
                value={cronExpression}
                onInput={setCronExpression}
              />
            ) : null}
          </div>
        </Section>
      ) : null}

      {activeTab === "team" ? (
        <Section
          title="Team"
          subtitle="Templates, default model, concurrency, and mission limits."
          icon="users"
        >
          <div className="grid gap-3 md:grid-cols-2">
            <label className="block text-sm">
              <div className="mb-1 font-medium text-slate-200">Orchestrator template</div>
              <select
                value={blueprint.orchestrator_template_id || ""}
                onInput={(event) =>
                  updateBlueprint({
                    orchestrator_template_id: (event.target as HTMLSelectElement).value,
                  })
                }
                className="h-10 w-full rounded-lg border border-slate-700 bg-slate-950/80 px-3 text-sm text-slate-100 outline-none focus:border-amber-400"
              >
                <option value="">None</option>
                {templates.map((template) => (
                  <option key={template.template_id} value={template.template_id}>
                    {template.template_id} ({template.role || "role"})
                  </option>
                ))}
              </select>
            </label>
            <LabeledInput
              label="Allowed templates"
              value={(blueprint.team.allowed_template_ids || []).join(", ")}
              onInput={(value) =>
                updateBlueprint({
                  team: { ...blueprint.team, allowed_template_ids: splitCsv(value) },
                })
              }
              placeholder="comma-separated"
            />
          </div>
          <InlineHint>
            The orchestrator keeps the mission coherent. Allowed templates restrict which reusable
            agent profiles lanes are permitted to use.
          </InlineHint>
          <ProviderModelSelector
            providerLabel="Default model provider"
            modelLabel="Default model"
            draft={teamModel}
            providers={providers}
            inheritLabel="No team default"
            onChange={setTeamModel}
          />
          <div className="grid gap-3 md:grid-cols-2 lg:grid-cols-4">
            <LabeledInput
              label="Max parallel agents"
              value={String(blueprint.team.max_parallel_agents || 4)}
              onInput={(value) =>
                updateBlueprint({
                  team: {
                    ...blueprint.team,
                    max_parallel_agents: Math.max(
                      1,
                      Number.parseInt(String(value || "4"), 10) || 4
                    ),
                  },
                })
              }
              type="number"
            />
            <LabeledInput
              label="Token ceiling"
              value={String(blueprint.team.mission_budget?.max_total_tokens || "")}
              onInput={(value) =>
                updateBlueprint({
                  team: {
                    ...blueprint.team,
                    mission_budget: {
                      ...(blueprint.team.mission_budget || {}),
                      max_total_tokens: value ? Number(value) : undefined,
                    },
                  },
                })
              }
              type="number"
            />
            <LabeledInput
              label="Cost ceiling USD"
              value={String(blueprint.team.mission_budget?.max_total_cost_usd || "")}
              onInput={(value) =>
                updateBlueprint({
                  team: {
                    ...blueprint.team,
                    mission_budget: {
                      ...(blueprint.team.mission_budget || {}),
                      max_total_cost_usd: value ? Number(value) : undefined,
                    },
                  },
                })
              }
              type="number"
            />
            <LabeledInput
              label="Tool-call ceiling"
              value={String(blueprint.team.mission_budget?.max_total_tool_calls || "")}
              onInput={(value) =>
                updateBlueprint({
                  team: {
                    ...blueprint.team,
                    mission_budget: {
                      ...(blueprint.team.mission_budget || {}),
                      max_total_tool_calls: value ? Number(value) : undefined,
                    },
                  },
                })
              }
              type="number"
            />
          </div>
          <LabeledInput
            label="Allowed MCP servers"
            value={(blueprint.team.allowed_mcp_servers || []).join(", ")}
            onInput={(value) =>
              updateBlueprint({
                team: {
                  ...blueprint.team,
                  allowed_mcp_servers: splitCsv(value),
                },
              })
            }
            placeholder={mcpServers.map((server) => server.name).join(", ")}
          />
          <InlineHint>
            Team defaults apply everywhere unless a workstream or review stage overrides its own
            tool or MCP scope.
          </InlineHint>
        </Section>
      ) : null}

      {activeTab === "workstreams" ? (
        <Section
          title="Workstreams"
          subtitle="Scoped sub-objectives, dependencies, tools, MCP, and output contracts."
          icon="workflow"
        >
          <InlineHint>
            A workstream is one scoped lane of work. Give it one responsibility, one artifact to
            produce, and only the dependencies it truly needs.
          </InlineHint>
          <div className="flex justify-end">
            <button className="tcp-btn h-8 px-3 text-xs" onClick={addWorkstream}>
              <i data-lucide="plus"></i>
              Add workstream
            </button>
          </div>
          {effectiveBlueprint.workstreams.map((workstream, index) => {
            const modelDraft = workstreamModels[workstream.workstream_id] || {
              provider: "",
              model: "",
            };
            return (
              <div
                key={workstream.workstream_id}
                className="rounded-xl border border-slate-800 bg-slate-900/70 p-3"
              >
                <div className="mb-3 flex items-center justify-between gap-2">
                  <div className="text-sm font-semibold text-slate-100">
                    {workstream.title || `Workstream ${index + 1}`}
                  </div>
                  <button
                    className="tcp-btn-danger h-7 px-2 text-xs"
                    onClick={() =>
                      updateBlueprint({
                        workstreams: effectiveBlueprint.workstreams.filter(
                          (row) => row.workstream_id !== workstream.workstream_id
                        ),
                      })
                    }
                  >
                    Remove
                  </button>
                </div>
                <div className="grid gap-3 md:grid-cols-2">
                  <LabeledInput
                    label="Title"
                    value={workstream.title}
                    onInput={(value) =>
                      updateBlueprint({
                        workstreams: effectiveBlueprint.workstreams.map((row) =>
                          row.workstream_id === workstream.workstream_id
                            ? { ...row, title: value }
                            : row
                        ),
                      })
                    }
                  />
                  <LabeledInput
                    label="Role"
                    value={workstream.role}
                    onInput={(value) =>
                      updateBlueprint({
                        workstreams: effectiveBlueprint.workstreams.map((row) =>
                          row.workstream_id === workstream.workstream_id
                            ? { ...row, role: value }
                            : row
                        ),
                      })
                    }
                  />
                </div>
                <div className="grid gap-3 md:grid-cols-3">
                  <LabeledInput
                    label="Phase"
                    value={workstream.phase_id || ""}
                    onInput={(value) =>
                      updateBlueprint({
                        workstreams: effectiveBlueprint.workstreams.map((row) =>
                          row.workstream_id === workstream.workstream_id
                            ? { ...row, phase_id: value }
                            : row
                        ),
                      })
                    }
                  />
                  <LabeledInput
                    label="Lane"
                    value={workstream.lane || ""}
                    onInput={(value) =>
                      updateBlueprint({
                        workstreams: effectiveBlueprint.workstreams.map((row) =>
                          row.workstream_id === workstream.workstream_id
                            ? { ...row, lane: value }
                            : row
                        ),
                      })
                    }
                  />
                  <LabeledInput
                    label="Priority"
                    value={String(workstream.priority || 0)}
                    onInput={(value) =>
                      updateBlueprint({
                        workstreams: effectiveBlueprint.workstreams.map((row) =>
                          row.workstream_id === workstream.workstream_id
                            ? { ...row, priority: Number(value) || 0 }
                            : row
                        ),
                      })
                    }
                    type="number"
                  />
                </div>
                <InlineHint>
                  `Phase` says when this lane belongs in the mission. `Lane` groups related work.
                  `Priority` decides ordering among runnable work in the same open phase.
                </InlineHint>
                <LabeledTextArea
                  label="Objective"
                  value={workstream.objective}
                  onInput={(value) =>
                    updateBlueprint({
                      workstreams: effectiveBlueprint.workstreams.map((row) =>
                        row.workstream_id === workstream.workstream_id
                          ? { ...row, objective: value }
                          : row
                      ),
                    })
                  }
                  rows={3}
                />
                <InlineHint>
                  Objective is the local assignment. Keep it crisp: what this lane must accomplish.
                </InlineHint>
                <LabeledTextArea
                  label="Prompt"
                  value={workstream.prompt}
                  onInput={(value) =>
                    updateBlueprint({
                      workstreams: effectiveBlueprint.workstreams.map((row) =>
                        row.workstream_id === workstream.workstream_id
                          ? { ...row, prompt: value }
                          : row
                      ),
                    })
                  }
                  rows={5}
                />
                <InlineHint>
                  Prompt is how the lane should operate: role, evidence standard, audience, format,
                  and what good work looks like.
                </InlineHint>
                <div className="grid gap-3 md:grid-cols-2">
                  <LabeledInput
                    label="Depends on"
                    value={workstream.depends_on.join(", ")}
                    onInput={(value) =>
                      updateBlueprint({
                        workstreams: effectiveBlueprint.workstreams.map((row) =>
                          row.workstream_id === workstream.workstream_id
                            ? { ...row, depends_on: splitCsv(value) }
                            : row
                        ),
                      })
                    }
                    placeholder="comma-separated stage ids"
                  />
                  <LabeledInput
                    label="Output contract kind"
                    value={workstream.output_contract.kind}
                    onInput={(value) =>
                      updateBlueprint({
                        workstreams: effectiveBlueprint.workstreams.map((row) =>
                          row.workstream_id === workstream.workstream_id
                            ? {
                                ...row,
                                output_contract: { ...row.output_contract, kind: value },
                              }
                            : row
                        ),
                      })
                    }
                  />
                </div>
                <InlineHint>
                  `Depends on` should list upstream stage IDs that must finish first. `Output
                  contract kind` names the artifact downstream lanes expect.
                </InlineHint>
                <div className="grid gap-3 md:grid-cols-2">
                  <LabeledInput
                    label="Tool allowlist override"
                    value={(workstream.tool_allowlist_override || []).join(", ")}
                    onInput={(value) =>
                      updateBlueprint({
                        workstreams: effectiveBlueprint.workstreams.map((row) =>
                          row.workstream_id === workstream.workstream_id
                            ? { ...row, tool_allowlist_override: splitCsv(value) }
                            : row
                        ),
                      })
                    }
                    placeholder={toolIds.join(", ")}
                  />
                  <LabeledInput
                    label="MCP servers override"
                    value={(workstream.mcp_servers_override || []).join(", ")}
                    onInput={(value) =>
                      updateBlueprint({
                        workstreams: effectiveBlueprint.workstreams.map((row) =>
                          row.workstream_id === workstream.workstream_id
                            ? { ...row, mcp_servers_override: splitCsv(value) }
                            : row
                        ),
                      })
                    }
                    placeholder={mcpServers.map((server) => server.name).join(", ")}
                  />
                </div>
                <InlineHint>
                  Leave tool and MCP overrides empty to inherit team defaults. Override only when
                  this lane needs a narrower or different scope.
                </InlineHint>
                <ProviderModelSelector
                  providerLabel="Model provider"
                  modelLabel="Model"
                  draft={modelDraft}
                  providers={providers}
                  onChange={(draft) =>
                    setWorkstreamModels((current) => ({
                      ...current,
                      [workstream.workstream_id]: draft,
                    }))
                  }
                />
              </div>
            );
          })}
        </Section>
      ) : null}

      {activeTab === "review" ? (
        <Section
          title="Review & Gates"
          subtitle="Reviewer, tester, and approval stages."
          icon="shield-check"
        >
          <InlineHint>
            Use review stages to check quality or readiness before later work is promoted. Approval
            stages are the right place for human checkpoints.
          </InlineHint>
          <div className="flex justify-between gap-2">
            <button className="tcp-btn h-8 px-3 text-xs" onClick={addReviewStage}>
              <i data-lucide="plus"></i>
              Add review stage
            </button>
            <button
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() =>
                updateBlueprint({
                  phases: [
                    ...effectiveBlueprint.phases,
                    {
                      phase_id: `phase_${effectiveBlueprint.phases.length + 1}`,
                      title: `Phase ${effectiveBlueprint.phases.length + 1}`,
                      description: "",
                      execution_mode: "soft",
                    },
                  ],
                })
              }
            >
              <i data-lucide="copy-plus"></i>
              Add phase
            </button>
          </div>
          <div className="grid gap-2">
            {effectiveBlueprint.phases.map((phase, index) => (
              <div
                key={phase.phase_id}
                className="rounded-lg border border-slate-800 bg-slate-900/70 p-3"
              >
                <div className="grid gap-3 md:grid-cols-4">
                  <LabeledInput
                    label="Phase ID"
                    value={phase.phase_id}
                    onInput={(value) =>
                      updateBlueprint({
                        phases: effectiveBlueprint.phases.map((row, rowIndex) =>
                          rowIndex === index ? { ...row, phase_id: value } : row
                        ),
                      })
                    }
                  />
                  <LabeledInput
                    label="Title"
                    value={phase.title}
                    onInput={(value) =>
                      updateBlueprint({
                        phases: effectiveBlueprint.phases.map((row, rowIndex) =>
                          rowIndex === index ? { ...row, title: value } : row
                        ),
                      })
                    }
                  />
                  <LabeledInput
                    label="Description"
                    value={phase.description || ""}
                    onInput={(value) =>
                      updateBlueprint({
                        phases: effectiveBlueprint.phases.map((row, rowIndex) =>
                          rowIndex === index ? { ...row, description: value } : row
                        ),
                      })
                    }
                  />
                  <label className="block text-sm">
                    <div className="mb-1 font-medium text-slate-200">Execution mode</div>
                    <select
                      value={phase.execution_mode || "soft"}
                      onInput={(event) =>
                        updateBlueprint({
                          phases: effectiveBlueprint.phases.map((row, rowIndex) =>
                            rowIndex === index
                              ? {
                                  ...row,
                                  execution_mode: (event.target as HTMLSelectElement).value as
                                    | "soft"
                                    | "barrier",
                                }
                              : row
                          ),
                        })
                      }
                      className="h-10 w-full rounded-lg border border-slate-700 bg-slate-950/80 px-3 text-sm text-slate-100 outline-none focus:border-amber-400"
                    >
                      <option value="soft">soft</option>
                      <option value="barrier">barrier</option>
                    </select>
                  </label>
                </div>
              </div>
            ))}
          </div>
          <InlineHint>
            `soft` phases prefer the current open phase first. `barrier` phases hold later phases
            closed until earlier required work is complete.
          </InlineHint>
          {effectiveBlueprint.review_stages.map((stage, index) => {
            const modelDraft = reviewModels[stage.stage_id] || { provider: "", model: "" };
            return (
              <div
                key={stage.stage_id}
                className="rounded-xl border border-slate-800 bg-slate-900/70 p-3"
              >
                <div className="mb-3 flex items-center justify-between gap-2">
                  <div className="text-sm font-semibold text-slate-100">
                    {stage.title || `Review stage ${index + 1}`}
                  </div>
                  <button
                    className="tcp-btn-danger h-7 px-2 text-xs"
                    onClick={() =>
                      updateBlueprint({
                        review_stages: effectiveBlueprint.review_stages.filter(
                          (row) => row.stage_id !== stage.stage_id
                        ),
                      })
                    }
                  >
                    Remove
                  </button>
                </div>
                <div className="grid gap-3 md:grid-cols-2">
                  <LabeledInput
                    label="Title"
                    value={stage.title}
                    onInput={(value) =>
                      updateBlueprint({
                        review_stages: effectiveBlueprint.review_stages.map((row) =>
                          row.stage_id === stage.stage_id ? { ...row, title: value } : row
                        ),
                      })
                    }
                  />
                  <label className="block text-sm">
                    <div className="mb-1 font-medium text-slate-200">Stage kind</div>
                    <select
                      value={stage.stage_kind}
                      onInput={(event) =>
                        updateBlueprint({
                          review_stages: effectiveBlueprint.review_stages.map((row) =>
                            row.stage_id === stage.stage_id
                              ? {
                                  ...row,
                                  stage_kind: (event.target as HTMLSelectElement).value as
                                    | "review"
                                    | "test"
                                    | "approval",
                                }
                              : row
                          ),
                        })
                      }
                      className="h-10 w-full rounded-lg border border-slate-700 bg-slate-950/80 px-3 text-sm text-slate-100 outline-none focus:border-amber-400"
                    >
                      <option value="review">review</option>
                      <option value="test">test</option>
                      <option value="approval">approval</option>
                    </select>
                  </label>
                </div>
                <div className="grid gap-3 md:grid-cols-3">
                  <LabeledInput
                    label="Targets"
                    value={stage.target_ids.join(", ")}
                    onInput={(value) =>
                      updateBlueprint({
                        review_stages: effectiveBlueprint.review_stages.map((row) =>
                          row.stage_id === stage.stage_id
                            ? { ...row, target_ids: splitCsv(value) }
                            : row
                        ),
                      })
                    }
                    placeholder={stageIds.join(", ")}
                  />
                  <LabeledInput
                    label="Phase"
                    value={stage.phase_id || ""}
                    onInput={(value) =>
                      updateBlueprint({
                        review_stages: effectiveBlueprint.review_stages.map((row) =>
                          row.stage_id === stage.stage_id ? { ...row, phase_id: value } : row
                        ),
                      })
                    }
                  />
                  <LabeledInput
                    label="Lane"
                    value={stage.lane || ""}
                    onInput={(value) =>
                      updateBlueprint({
                        review_stages: effectiveBlueprint.review_stages.map((row) =>
                          row.stage_id === stage.stage_id ? { ...row, lane: value } : row
                        ),
                      })
                    }
                  />
                </div>
                <InlineHint>
                  Targets are the stages this review or gate is checking. Put the review in the
                  phase where that checkpoint should happen.
                </InlineHint>
                <LabeledTextArea
                  label="Prompt"
                  value={stage.prompt}
                  onInput={(value) =>
                    updateBlueprint({
                      review_stages: effectiveBlueprint.review_stages.map((row) =>
                        row.stage_id === stage.stage_id ? { ...row, prompt: value } : row
                      ),
                    })
                  }
                  rows={4}
                />
                <InlineHint>
                  Use the prompt to define what must be checked and what should trigger approve,
                  rework, or fail.
                </InlineHint>
                <div className="grid gap-3 md:grid-cols-2">
                  <LabeledInput
                    label="Checklist"
                    value={(stage.checklist || []).join(", ")}
                    onInput={(value) =>
                      updateBlueprint({
                        review_stages: effectiveBlueprint.review_stages.map((row) =>
                          row.stage_id === stage.stage_id
                            ? { ...row, checklist: splitCsv(value) }
                            : row
                        ),
                      })
                    }
                    placeholder="comma-separated"
                  />
                  <LabeledInput
                    label="MCP servers override"
                    value={(stage.mcp_servers_override || []).join(", ")}
                    onInput={(value) =>
                      updateBlueprint({
                        review_stages: effectiveBlueprint.review_stages.map((row) =>
                          row.stage_id === stage.stage_id
                            ? { ...row, mcp_servers_override: splitCsv(value) }
                            : row
                        ),
                      })
                    }
                    placeholder={mcpServers.map((server) => server.name).join(", ")}
                  />
                </div>
                <InlineHint>
                  Checklist items should be concrete pass/fail checks, not broad wishes.
                </InlineHint>
                <ProviderModelSelector
                  providerLabel="Model provider"
                  modelLabel="Model"
                  draft={modelDraft}
                  providers={providers}
                  onChange={(draft) =>
                    setReviewModels((current) => ({
                      ...current,
                      [stage.stage_id]: draft,
                    }))
                  }
                />
              </div>
            );
          })}
        </Section>
      ) : null}

      {activeTab === "compile" ? (
        <Section
          title="Compile & Run"
          subtitle="Validate the mission graph before launch."
          icon="binary"
        >
          <div className="flex flex-wrap items-center gap-2">
            <button
              className="tcp-btn h-8 px-3 text-xs"
              disabled={busy === "preview"}
              onClick={() => void compilePreview()}
            >
              <i data-lucide="refresh-cw"></i>
              {busy === "preview" ? "Compiling..." : "Compile preview"}
            </button>
            <button
              className="tcp-btn-primary h-8 px-3 text-xs"
              disabled={busy === "apply"}
              onClick={() => void saveMission()}
            >
              <i data-lucide={editingAutomation ? "save" : "arrow-up-circle"}></i>
              {busy === "apply"
                ? "Saving..."
                : editingAutomation
                  ? "Save automation"
                  : runAfterCreate
                    ? "Create and run"
                    : "Create draft"}
            </button>
            {!editingAutomation ? (
              <label className="ml-2 inline-flex items-center gap-2 text-xs text-slate-300">
                <input
                  type="checkbox"
                  checked={runAfterCreate}
                  onChange={(event) =>
                    setRunAfterCreate((event.target as HTMLInputElement).checked)
                  }
                />
                Run immediately after create
              </label>
            ) : null}
            {editingAutomation && onClearEditing ? (
              <button className="tcp-btn h-8 px-3 text-xs" onClick={() => onClearEditing()}>
                Cancel edit
              </button>
            ) : null}
          </div>

          {preview ? (
            <>
              <div className="grid gap-3 lg:grid-cols-2">
                <div className="rounded-lg border border-slate-800 bg-slate-900/70 p-3">
                  <div className="mb-2 text-sm font-semibold text-slate-100">Validation</div>
                  {Array.isArray(preview?.validation) && preview.validation.length ? (
                    <div className="grid gap-2">
                      {preview.validation.map((message: any, index: number) => (
                        <div
                          key={`${message?.code || "message"}-${index}`}
                          className={`rounded-lg border px-3 py-2 text-xs ${
                            String(message?.severity || "").toLowerCase() === "warning"
                              ? "border-amber-500/40 bg-amber-500/10 text-amber-200"
                              : "border-slate-700 bg-slate-950/60 text-slate-200"
                          }`}
                        >
                          <div className="font-medium">
                            {String(message?.code || message?.severity || "validation")}
                          </div>
                          <div className="mt-1">{String(message?.message || "")}</div>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <div className="tcp-subtle text-xs">No validation warnings.</div>
                  )}
                </div>
                <div className="rounded-lg border border-slate-800 bg-slate-900/70 p-3">
                  <div className="mb-2 text-sm font-semibold text-slate-100">
                    Compiled automation
                  </div>
                  <div className="grid gap-1 text-xs text-slate-300">
                    <div>name: {String(preview?.automation?.name || "—")}</div>
                    <div>
                      nodes:{" "}
                      {Array.isArray(preview?.automation?.flow?.nodes)
                        ? preview.automation.flow.nodes.length
                        : 0}
                    </div>
                    <div>
                      agents:{" "}
                      {Array.isArray(preview?.automation?.agents)
                        ? preview.automation.agents.length
                        : 0}
                    </div>
                    <div>
                      max parallel:{" "}
                      {String(preview?.automation?.execution?.max_parallel_agents ?? "—")}
                    </div>
                  </div>
                </div>
              </div>
              <div className="rounded-lg border border-slate-800 bg-slate-900/70 p-3">
                <div className="mb-2 text-sm font-semibold text-slate-100">Node preview</div>
                <div className="grid gap-2">
                  {(Array.isArray(preview?.node_previews) ? preview.node_previews : []).map(
                    (node: any) => (
                      <div
                        key={String(node?.node_id || "")}
                        className="rounded-lg border border-slate-800 bg-slate-950/70 p-3 text-xs text-slate-300"
                      >
                        <div className="flex flex-wrap items-center gap-2">
                          <strong className="text-slate-100">
                            {String(node?.title || node?.node_id || "node")}
                          </strong>
                          <span className="tcp-subtle">{String(node?.node_id || "")}</span>
                          <span className="tcp-subtle">phase: {String(node?.phase_id || "—")}</span>
                          <span className="tcp-subtle">lane: {String(node?.lane || "—")}</span>
                          <span className="tcp-subtle">
                            priority: {String(node?.priority ?? "—")}
                          </span>
                        </div>
                        <div className="mt-1">
                          depends on:{" "}
                          {Array.isArray(node?.depends_on) && node.depends_on.length
                            ? node.depends_on.join(", ")
                            : "none"}
                        </div>
                        <div className="mt-1">
                          tools:{" "}
                          {Array.isArray(node?.tool_allowlist) && node.tool_allowlist.length
                            ? node.tool_allowlist.join(", ")
                            : "default"}
                        </div>
                        <div className="mt-1">
                          MCP:{" "}
                          {Array.isArray(node?.mcp_servers) && node.mcp_servers.length
                            ? node.mcp_servers.join(", ")
                            : "default"}
                        </div>
                      </div>
                    )
                  )}
                </div>
              </div>
            </>
          ) : (
            <div className="tcp-subtle text-xs">
              Compile the mission to inspect validation, compiled nodes, and execution shape.
            </div>
          )}
        </Section>
      ) : null}
    </div>
  );
}
