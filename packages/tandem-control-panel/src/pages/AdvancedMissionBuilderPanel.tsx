import { useEffect, useMemo, useState } from "preact/hooks";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { TandemClient } from "@frumu/tandem-client";

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
type StarterPresetId = "research" | "marketing" | "incident" | "event";

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
  if (kind === "cron") {
    return {
      type: "cron",
      cron_expression: String(cron || "").trim(),
      timezone: "UTC",
      misfire_policy: "run_once",
    };
  }
  if (kind === "interval") {
    return {
      type: "interval",
      interval_seconds: Math.max(1, Number.parseInt(String(intervalSeconds || "3600"), 10) || 3600),
      timezone: "UTC",
      misfire_policy: "run_once",
    };
  }
  return { type: "manual", timezone: "UTC", misfire_policy: "run_once" };
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

function starterBlueprint(preset: StarterPresetId, workspaceRoot: string): MissionBlueprint {
  const root = defaultBlueprint(workspaceRoot);
  switch (preset) {
    case "research":
      return {
        ...root,
        title: "Competitive research mission",
        goal: "Produce a concise evidence-based competitive brief with clear implications and next actions.",
        success_criteria: [
          "At least 5 competitors analyzed",
          "Claims are supported by evidence",
          "Final synthesis identifies gaps and recommendations",
        ],
        shared_context:
          "Audience is executive leadership. Prefer factual, source-backed claims. Avoid speculation and clearly label unknowns.",
        phases: [
          { phase_id: "research", title: "Research", execution_mode: "soft" },
          { phase_id: "synthesis", title: "Synthesis", execution_mode: "barrier" },
        ],
        milestones: [
          {
            milestone_id: "evidence_collected",
            title: "Evidence collected",
            phase_id: "research",
            required_stage_ids: ["competitors", "market-signals"],
          },
        ],
        workstreams: [
          {
            workstream_id: "competitors",
            title: "Competitor scan",
            objective: "Identify key competitors, claims, positioning, and pricing cues.",
            role: "researcher",
            prompt:
              "Act as a competitive intelligence researcher. Review available sources, extract concrete claims, and produce a structured competitor memo with evidence.",
            priority: 1,
            phase_id: "research",
            lane: "research",
            milestone: "evidence_collected",
            depends_on: [],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "report_markdown",
              summary_guidance:
                "Competitors reviewed, claims, evidence, pricing signals, and key takeaways.",
            },
          },
          {
            workstream_id: "market-signals",
            title: "Market signals",
            objective:
              "Gather recent market signals, launches, and shifts relevant to the mission.",
            role: "researcher",
            prompt:
              "Collect recent market movements, launches, and notable changes that affect the competitive landscape. Summarize only useful signals.",
            priority: 2,
            phase_id: "research",
            lane: "signals",
            milestone: "evidence_collected",
            depends_on: [],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "report_markdown",
              summary_guidance: "Recent market signals, why they matter, and confidence level.",
            },
          },
          {
            workstream_id: "synthesis",
            title: "Synthesis brief",
            objective: "Merge findings into an executive-ready competitive brief.",
            role: "analyst",
            prompt:
              "Synthesize upstream findings into one brief for executives. Highlight evidence, important implications, risks, and recommended actions.",
            priority: 1,
            phase_id: "synthesis",
            lane: "synthesis",
            depends_on: ["competitors", "market-signals"],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "brief_markdown",
              summary_guidance: "Executive summary, implications, risks, and recommended actions.",
            },
          },
        ],
        review_stages: [
          {
            stage_id: "research-review",
            stage_kind: "review",
            title: "Evidence review",
            target_ids: ["synthesis"],
            role: "reviewer",
            prompt:
              "Review the synthesis for unsupported claims, missing evidence, and vague recommendations. Approve only if evidence is clear.",
            checklist: [
              "Claims are evidence-backed",
              "Unknowns are labeled",
              "Recommendations are actionable",
            ],
            priority: 1,
            phase_id: "synthesis",
            lane: "review",
            tool_allowlist_override: [],
            mcp_servers_override: [],
          },
        ],
      };
    case "marketing":
      return {
        ...root,
        title: "Campaign planning mission",
        goal: "Build a coherent campaign plan with audience insight, messaging, and channel execution.",
        success_criteria: [
          "Audience segments are defined",
          "Messaging is aligned to the audience",
          "Channel plan and content ideas are ready for approval",
        ],
        shared_context:
          "Tone should be clear and practical. Keep the plan realistic for a small team and limited budget.",
        phases: [
          { phase_id: "strategy", title: "Strategy", execution_mode: "soft" },
          { phase_id: "planning", title: "Planning", execution_mode: "barrier" },
        ],
        milestones: [
          {
            milestone_id: "strategy_locked",
            title: "Strategy locked",
            phase_id: "strategy",
            required_stage_ids: ["audience", "messaging"],
          },
        ],
        workstreams: [
          {
            workstream_id: "audience",
            title: "Audience analysis",
            objective: "Define target segments, needs, and objections.",
            role: "strategist",
            prompt:
              "Act as a market strategist. Define priority audience segments, what they need, and what blocks adoption.",
            priority: 1,
            phase_id: "strategy",
            lane: "strategy",
            milestone: "strategy_locked",
            depends_on: [],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "report_markdown",
              summary_guidance: "Segments, needs, objections, and buying cues.",
            },
          },
          {
            workstream_id: "messaging",
            title: "Messaging framework",
            objective: "Develop campaign messaging pillars and proof points.",
            role: "copywriter",
            prompt:
              "Create a messaging framework with pillars, proof points, and sample lines that match the audience needs.",
            priority: 1,
            phase_id: "strategy",
            lane: "messaging",
            milestone: "strategy_locked",
            depends_on: [],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "brief_markdown",
              summary_guidance: "Messaging pillars, supporting proof points, and sample copy.",
            },
          },
          {
            workstream_id: "channels",
            title: "Channel and content plan",
            objective: "Create a practical channel mix and content plan.",
            role: "planner",
            prompt:
              "Using upstream audience and messaging work, create a channel plan, recommended cadence, and example content pipeline.",
            priority: 2,
            phase_id: "planning",
            lane: "planning",
            depends_on: ["audience", "messaging"],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "plan_markdown",
              summary_guidance: "Channel mix, cadence, content ideas, and execution notes.",
            },
          },
        ],
        review_stages: [
          {
            stage_id: "campaign-approval",
            stage_kind: "approval",
            title: "Campaign approval",
            target_ids: ["channels"],
            role: "approver",
            prompt:
              "Review the campaign plan for clarity, realism, audience fit, and consistency before approval.",
            checklist: [
              "Audience fit is clear",
              "Messaging is consistent",
              "Execution plan is realistic",
            ],
            priority: 1,
            phase_id: "planning",
            lane: "approval",
            tool_allowlist_override: [],
            mcp_servers_override: [],
            gate: {
              required: true,
              decisions: ["approve", "rework", "cancel"],
              rework_targets: ["channels"],
              instructions: "Approve only when the channel plan is executable and aligned.",
            },
          },
        ],
      };
    case "incident":
      return {
        ...root,
        title: "Incident response mission",
        goal: "Investigate the incident, identify likely causes, and produce a coordinated response plan.",
        success_criteria: [
          "Incident timeline is documented",
          "Likely causes are identified or narrowed",
          "Response plan includes containment and follow-up",
        ],
        shared_context:
          "Prioritize clarity, risk reduction, and actionability. Flag uncertainty explicitly and avoid overclaiming root cause.",
        phases: [
          { phase_id: "triage", title: "Triage", execution_mode: "soft" },
          { phase_id: "response", title: "Response", execution_mode: "barrier" },
        ],
        milestones: [
          {
            milestone_id: "triage-complete",
            title: "Triage complete",
            phase_id: "triage",
            required_stage_ids: ["timeline", "impact"],
          },
        ],
        workstreams: [
          {
            workstream_id: "timeline",
            title: "Timeline reconstruction",
            objective: "Build a factual timeline of the incident.",
            role: "investigator",
            prompt:
              "Reconstruct the incident timeline from available evidence. Note confidence and missing information.",
            priority: 1,
            phase_id: "triage",
            lane: "triage",
            milestone: "triage-complete",
            depends_on: [],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "report_markdown",
              summary_guidance: "Chronological timeline with evidence and confidence notes.",
            },
          },
          {
            workstream_id: "impact",
            title: "Impact assessment",
            objective: "Assess scope, affected systems, and business impact.",
            role: "analyst",
            prompt:
              "Assess scope, affected systems, business/user impact, and immediate risk level. Be specific and avoid guesses.",
            priority: 1,
            phase_id: "triage",
            lane: "impact",
            milestone: "triage-complete",
            depends_on: [],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "report_markdown",
              summary_guidance: "Affected scope, severity, and open questions.",
            },
          },
          {
            workstream_id: "response-plan",
            title: "Response plan",
            objective: "Create a containment, communication, and follow-up plan.",
            role: "coordinator",
            prompt:
              "Create a response plan covering containment, communications, decisions needed, and follow-up investigation steps.",
            priority: 1,
            phase_id: "response",
            lane: "response",
            depends_on: ["timeline", "impact"],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "plan_markdown",
              summary_guidance: "Immediate actions, owners, risks, and next decisions.",
            },
          },
        ],
        review_stages: [
          {
            stage_id: "incident-approval",
            stage_kind: "approval",
            title: "Incident checkpoint",
            target_ids: ["response-plan"],
            role: "approver",
            prompt:
              "Check whether the response plan is safe, complete enough to act on, and clear about unknowns.",
            checklist: [
              "Safety and containment are addressed",
              "Unknowns are explicit",
              "Next actions are clear",
            ],
            priority: 1,
            phase_id: "response",
            lane: "approval",
            tool_allowlist_override: [],
            mcp_servers_override: [],
            gate: {
              required: true,
              decisions: ["approve", "rework", "cancel"],
              rework_targets: ["response-plan"],
              instructions:
                "Use rework if the response plan is missing critical containment or ownership detail.",
            },
          },
        ],
      };
    case "event":
      return {
        ...root,
        title: "Event planning mission",
        goal: "Produce a coordinated event plan covering logistics, communications, and program readiness.",
        success_criteria: [
          "Logistics, comms, and program plans are complete",
          "Dependencies are clear",
          "Approval gate confirms readiness",
        ],
        shared_context:
          "Optimize for practical execution. Surface blockers, missing owners, and sequencing risks early.",
        phases: [
          { phase_id: "planning", title: "Planning", execution_mode: "soft" },
          { phase_id: "readiness", title: "Readiness", execution_mode: "barrier" },
        ],
        milestones: [],
        workstreams: [
          {
            workstream_id: "logistics",
            title: "Logistics plan",
            objective: "Define venue, timing, staffing, and operational needs.",
            role: "operator",
            prompt:
              "Create a logistics plan with venue needs, timeline, staffing, dependencies, and open risks.",
            priority: 1,
            phase_id: "planning",
            lane: "operations",
            depends_on: [],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "plan_markdown",
              summary_guidance: "Venue, timing, staffing, dependencies, and risks.",
            },
          },
          {
            workstream_id: "program",
            title: "Program plan",
            objective: "Define agenda, speakers, and content sequencing.",
            role: "planner",
            prompt:
              "Build the event program structure, content flow, and speaker/session requirements.",
            priority: 1,
            phase_id: "planning",
            lane: "program",
            depends_on: [],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "plan_markdown",
              summary_guidance: "Agenda, session plan, dependencies, and owner notes.",
            },
          },
          {
            workstream_id: "comms",
            title: "Communications plan",
            objective: "Create audience-facing communications and timeline.",
            role: "coordinator",
            prompt:
              "Create the communications timeline, key messages, and reminders needed before and during the event.",
            priority: 2,
            phase_id: "planning",
            lane: "communications",
            depends_on: ["program"],
            input_refs: [],
            tool_allowlist_override: [],
            mcp_servers_override: [],
            output_contract: {
              kind: "plan_markdown",
              summary_guidance: "Audience communications schedule, messages, and trigger points.",
            },
          },
        ],
        review_stages: [
          {
            stage_id: "event-readiness",
            stage_kind: "approval",
            title: "Readiness gate",
            target_ids: ["logistics", "program", "comms"],
            role: "approver",
            prompt:
              "Check whether the event is operationally ready and whether sequencing risks are covered.",
            checklist: [
              "Operational readiness is clear",
              "Program dependencies are covered",
              "Comms timeline is complete",
            ],
            priority: 1,
            phase_id: "readiness",
            lane: "approval",
            tool_allowlist_override: [],
            mcp_servers_override: [],
            gate: {
              required: true,
              decisions: ["approve", "rework", "cancel"],
              rework_targets: ["logistics", "program", "comms"],
              instructions: "Use rework if any lane is still missing key readiness details.",
            },
          },
        ],
      };
  }
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
  children,
}: {
  title: string;
  subtitle?: string;
  children: any;
}) {
  return (
    <div className="rounded-xl border border-slate-700/50 bg-slate-950/50 p-4">
      <div className="mb-3">
        <div className="text-sm font-semibold text-slate-100">{title}</div>
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
  onClick,
}: {
  active: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      className={`tcp-btn h-8 px-3 text-xs ${active ? "border-amber-400/60 bg-amber-400/10 text-amber-300" : ""}`}
      onClick={onClick}
      type="button"
    >
      {label}
    </button>
  );
}

function InlineHint({ children }: { children: any }) {
  return <div className="tcp-subtle -mt-1 text-xs">{children}</div>;
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
    const next = starterBlueprint(preset, blueprint.workspace_root || workspaceRoot);
    setBlueprint(next);
    setPreview(null);
    setError("");
    setActiveTab("mission");
    setTeamModel({ provider: defaultProvider, model: defaultModel });
    setWorkstreamModels({});
    setReviewModels({});
    toast("info", `Loaded ${next.title}. Review the prompts and adapt them to your mission.`);
  }

  async function compilePreview() {
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
      const message = compileError instanceof Error ? compileError.message : String(compileError);
      setError(message);
      toast("err", message);
    } finally {
      setBusy("");
    }
  }

  async function saveMission() {
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
      const message = applyError instanceof Error ? applyError.message : String(applyError);
      setError(message);
      toast("err", message);
    } finally {
      setBusy("");
    }
  }

  return (
    <div className="grid gap-4">
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
            How this works
          </button>
          <span className="tcp-subtle text-xs">Start from example:</span>
          <button
            className="tcp-btn h-8 px-3 text-xs"
            onClick={() => applyStarterPreset("research")}
          >
            Research
          </button>
          <button
            className="tcp-btn h-8 px-3 text-xs"
            onClick={() => applyStarterPreset("marketing")}
          >
            Marketing
          </button>
          <button
            className="tcp-btn h-8 px-3 text-xs"
            onClick={() => applyStarterPreset("incident")}
          >
            Incident
          </button>
          <button className="tcp-btn h-8 px-3 text-xs" onClick={() => applyStarterPreset("event")}>
            Event
          </button>
        </div>
        <div className="mt-3 flex flex-wrap gap-2">
          {(["mission", "team", "workstreams", "review", "compile"] as CreateModeTab[]).map(
            (tab) => (
              <ToggleChip
                key={tab}
                active={activeTab === tab}
                label={tab === "workstreams" ? "workstreams" : tab}
                onClick={() => setActiveTab(tab)}
              />
            )
          )}
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
                    Research competitor messaging and identify 5 positioning gaps.
                  </div>
                  <div className="mt-3">
                    <strong className="text-slate-100">Workstream prompt</strong>
                  </div>
                  <div className="mt-1">
                    Act as a competitive analyst. Review available sources, extract concrete claims,
                    compare positioning, and produce a concise findings memo with evidence and
                    recommended angles.
                  </div>
                  <div className="mt-3">
                    <strong className="text-slate-100">Output contract</strong>
                  </div>
                  <div className="mt-1">
                    A markdown memo with sections: competitors reviewed, evidence, messaging gaps,
                    and recommended actions.
                  </div>
                </div>
              </Section>

              <Section
                title="Starter examples"
                subtitle="Use these when you do not want to begin from a blank blueprint."
              >
                <div className="grid gap-2 text-sm text-slate-300">
                  <div>
                    <strong className="text-slate-100">Research:</strong> parallel evidence
                    gathering, then synthesis and review.
                  </div>
                  <div>
                    <strong className="text-slate-100">Marketing:</strong> audience and messaging
                    lanes feeding a channel plan and approval gate.
                  </div>
                  <div>
                    <strong className="text-slate-100">Incident:</strong> timeline and impact in
                    parallel, then a response plan and checkpoint.
                  </div>
                  <div>
                    <strong className="text-slate-100">Event:</strong> logistics, program, and
                    communications coordinated into readiness approval.
                  </div>
                </div>
              </Section>
            </div>
          </div>
        </div>
      ) : null}

      {activeTab === "mission" ? (
        <Section title="Mission" subtitle="Global brief, success criteria, and schedule.">
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
          <LabeledInput
            label="Workspace root"
            value={blueprint.workspace_root}
            onInput={(value) => updateBlueprint({ workspace_root: value })}
          />
          <InlineHint>
            This is the shared working directory the mission can use for files and artifacts.
          </InlineHint>
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
        <Section title="Team" subtitle="Templates, default model, concurrency, and mission limits.">
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
          <div className="grid gap-3 md:grid-cols-2">
            <label className="block text-sm">
              <div className="mb-1 font-medium text-slate-200">Default model provider</div>
              <select
                value={teamModel.provider}
                onInput={(event) =>
                  setTeamModel({
                    provider: (event.target as HTMLSelectElement).value,
                    model:
                      providers.find(
                        (provider) => provider.id === (event.target as HTMLSelectElement).value
                      )?.models?.[0] || "",
                  })
                }
                className="h-10 w-full rounded-lg border border-slate-700 bg-slate-950/80 px-3 text-sm text-slate-100 outline-none focus:border-amber-400"
              >
                <option value="">None</option>
                {providers.map((provider) => (
                  <option key={provider.id} value={provider.id}>
                    {provider.id}
                  </option>
                ))}
              </select>
            </label>
            <label className="block text-sm">
              <div className="mb-1 font-medium text-slate-200">Default model</div>
              <select
                value={teamModel.model}
                onInput={(event) =>
                  setTeamModel((current) => ({
                    ...current,
                    model: (event.target as HTMLSelectElement).value,
                  }))
                }
                className="h-10 w-full rounded-lg border border-slate-700 bg-slate-950/80 px-3 text-sm text-slate-100 outline-none focus:border-amber-400"
              >
                <option value="">None</option>
                {(
                  providers.find((provider) => provider.id === teamModel.provider)?.models || []
                ).map((model) => (
                  <option key={model} value={model}>
                    {model}
                  </option>
                ))}
              </select>
            </label>
          </div>
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
        >
          <InlineHint>
            A workstream is one scoped lane of work. Give it one responsibility, one artifact to
            produce, and only the dependencies it truly needs.
          </InlineHint>
          <div className="flex justify-end">
            <button className="tcp-btn h-8 px-3 text-xs" onClick={addWorkstream}>
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
                <div className="grid gap-3 md:grid-cols-2">
                  <label className="block text-sm">
                    <div className="mb-1 font-medium text-slate-200">Model provider</div>
                    <select
                      value={modelDraft.provider}
                      onInput={(event) =>
                        setWorkstreamModels((current) => ({
                          ...current,
                          [workstream.workstream_id]: {
                            provider: (event.target as HTMLSelectElement).value,
                            model:
                              providers.find(
                                (provider) =>
                                  provider.id === (event.target as HTMLSelectElement).value
                              )?.models?.[0] || "",
                          },
                        }))
                      }
                      className="h-10 w-full rounded-lg border border-slate-700 bg-slate-950/80 px-3 text-sm text-slate-100 outline-none focus:border-amber-400"
                    >
                      <option value="">Default</option>
                      {providers.map((provider) => (
                        <option key={provider.id} value={provider.id}>
                          {provider.id}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label className="block text-sm">
                    <div className="mb-1 font-medium text-slate-200">Model</div>
                    <select
                      value={modelDraft.model}
                      onInput={(event) =>
                        setWorkstreamModels((current) => ({
                          ...current,
                          [workstream.workstream_id]: {
                            ...(current[workstream.workstream_id] || { provider: "", model: "" }),
                            model: (event.target as HTMLSelectElement).value,
                          },
                        }))
                      }
                      className="h-10 w-full rounded-lg border border-slate-700 bg-slate-950/80 px-3 text-sm text-slate-100 outline-none focus:border-amber-400"
                    >
                      <option value="">Default</option>
                      {(
                        providers.find((provider) => provider.id === modelDraft.provider)?.models ||
                        []
                      ).map((model) => (
                        <option key={model} value={model}>
                          {model}
                        </option>
                      ))}
                    </select>
                  </label>
                </div>
              </div>
            );
          })}
        </Section>
      ) : null}

      {activeTab === "review" ? (
        <Section title="Review & Gates" subtitle="Reviewer, tester, and approval stages.">
          <InlineHint>
            Use review stages to check quality or readiness before later work is promoted. Approval
            stages are the right place for human checkpoints.
          </InlineHint>
          <div className="flex justify-between gap-2">
            <button className="tcp-btn h-8 px-3 text-xs" onClick={addReviewStage}>
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
                <div className="grid gap-3 md:grid-cols-2">
                  <label className="block text-sm">
                    <div className="mb-1 font-medium text-slate-200">Model provider</div>
                    <select
                      value={modelDraft.provider}
                      onInput={(event) =>
                        setReviewModels((current) => ({
                          ...current,
                          [stage.stage_id]: {
                            provider: (event.target as HTMLSelectElement).value,
                            model:
                              providers.find(
                                (provider) =>
                                  provider.id === (event.target as HTMLSelectElement).value
                              )?.models?.[0] || "",
                          },
                        }))
                      }
                      className="h-10 w-full rounded-lg border border-slate-700 bg-slate-950/80 px-3 text-sm text-slate-100 outline-none focus:border-amber-400"
                    >
                      <option value="">Default</option>
                      {providers.map((provider) => (
                        <option key={provider.id} value={provider.id}>
                          {provider.id}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label className="block text-sm">
                    <div className="mb-1 font-medium text-slate-200">Model</div>
                    <select
                      value={modelDraft.model}
                      onInput={(event) =>
                        setReviewModels((current) => ({
                          ...current,
                          [stage.stage_id]: {
                            ...(current[stage.stage_id] || { provider: "", model: "" }),
                            model: (event.target as HTMLSelectElement).value,
                          },
                        }))
                      }
                      className="h-10 w-full rounded-lg border border-slate-700 bg-slate-950/80 px-3 text-sm text-slate-100 outline-none focus:border-amber-400"
                    >
                      <option value="">Default</option>
                      {(
                        providers.find((provider) => provider.id === modelDraft.provider)?.models ||
                        []
                      ).map((model) => (
                        <option key={model} value={model}>
                          {model}
                        </option>
                      ))}
                    </select>
                  </label>
                </div>
              </div>
            );
          })}
        </Section>
      ) : null}

      {activeTab === "compile" ? (
        <Section title="Compile & Run" subtitle="Validate the mission graph before launch.">
          <div className="flex flex-wrap items-center gap-2">
            <button
              className="tcp-btn h-8 px-3 text-xs"
              disabled={busy === "preview"}
              onClick={() => void compilePreview()}
            >
              {busy === "preview" ? "Compiling..." : "Compile preview"}
            </button>
            <button
              className="tcp-btn-primary h-8 px-3 text-xs"
              disabled={busy === "apply"}
              onClick={() => void saveMission()}
            >
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
