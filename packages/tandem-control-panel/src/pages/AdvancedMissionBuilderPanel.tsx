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
          <LabeledInput
            label="Workspace root"
            value={blueprint.workspace_root}
            onInput={(value) => updateBlueprint({ workspace_root: value })}
          />
          <LabeledTextArea
            label="Mission goal"
            value={blueprint.goal}
            onInput={(value) => updateBlueprint({ goal: value })}
            placeholder="Describe the shared objective all participants are working toward."
          />
          <LabeledTextArea
            label="Shared context"
            value={blueprint.shared_context || ""}
            onInput={(value) => updateBlueprint({ shared_context: value })}
            placeholder="Shared constraints, references, context, and operator guidance."
          />
          <LabeledInput
            label="Success criteria"
            value={blueprint.success_criteria.join(", ")}
            onInput={(value) => updateBlueprint({ success_criteria: splitCsv(value) })}
            placeholder="comma-separated"
          />
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
        </Section>
      ) : null}

      {activeTab === "workstreams" ? (
        <Section
          title="Workstreams"
          subtitle="Scoped sub-objectives, dependencies, tools, MCP, and output contracts."
        >
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
