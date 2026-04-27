import { useEffect, useMemo, useState } from "react";
import { Button, Input } from "@/components/ui";
import { detectBrowserTimezone } from "@/components/agent-automation/timezone";
import {
  agentTeamListTemplates,
  automationsV2Update,
  automationsV2RunNow,
  missionBuilderApply,
  missionBuilderPreview,
  type McpServerRecord,
  type AutomationV2Spec,
  type JsonObject,
  type MissionBlueprint,
  type MissionBuilderCompilePreview,
  type MissionBuilderReviewStage,
  type MissionBuilderWorkstream,
  type ProviderInfo,
  type UserProject,
} from "@/lib/tauri";

type BuilderModelDraft = { provider: string; model: string };

interface AdvancedMissionBuilderProps {
  activeProject: UserProject | null;
  providers: ProviderInfo[];
  mcpServers: McpServerRecord[];
  toolIds: string[];
  editingAutomation?: AutomationV2Spec | null;
  onRefreshAutomations: () => Promise<void>;
  onShowAutomations: () => void;
  onShowRuns: () => void;
  onClearEditing?: () => void;
  onOpenMcpExtensions?: () => void;
  blueprintMetadataPatch?: JsonObject | null;
}

function toModelDraft(policy: unknown): BuilderModelDraft {
  const row = (policy as Record<string, unknown> | null) || null;
  const defaultModel =
    (row?.default_model as Record<string, unknown> | undefined) ||
    (row?.defaultModel as Record<string, unknown> | undefined) ||
    null;
  return {
    provider: String(defaultModel?.provider_id || defaultModel?.providerId || "").trim(),
    model: String(defaultModel?.model_id || defaultModel?.modelId || "").trim(),
  };
}

function workstreamModelDrafts(blueprint: MissionBlueprint) {
  const drafts: Record<string, BuilderModelDraft> = {};
  for (const workstream of blueprint.workstreams) {
    drafts[workstream.workstream_id] = toModelDraft(workstream.model_override || null);
  }
  return drafts;
}

function newWorkstream(index: number): MissionBuilderWorkstream {
  return {
    workstream_id: `workstream_${index}_${crypto.randomUUID().slice(0, 8)}`,
    title: `Workstream ${index}`,
    objective: "",
    role: "worker",
    priority: index,
    phase_id: "",
    lane: "",
    milestone: "",
    prompt: "",
    depends_on: [],
    input_refs: [],
    output_contract: { kind: "report_markdown", summary_guidance: "" },
    tool_allowlist_override: [],
    mcp_servers_override: [],
  };
}

function newReviewStage(index: number): MissionBuilderReviewStage {
  return {
    stage_id: `review_${index}_${crypto.randomUUID().slice(0, 8)}`,
    stage_kind: "approval",
    title: `Gate ${index}`,
    priority: index,
    phase_id: "",
    lane: "",
    milestone: "",
    target_ids: [],
    prompt: "",
    checklist: [],
    tool_allowlist_override: [],
    mcp_servers_override: [],
    gate: {
      required: true,
      decisions: ["approve", "rework", "cancel"],
      rework_targets: [],
      instructions: "",
    },
  };
}

function newPhase(index: number) {
  return {
    phase_id: `phase_${index}`,
    title: `Phase ${index}`,
    description: "",
    execution_mode: (index === 1 ? "soft" : "barrier") as "soft" | "barrier",
  };
}

function newMilestone(index: number) {
  return {
    milestone_id: `milestone_${index}`,
    title: `Milestone ${index}`,
    description: "",
    phase_id: "",
    required_stage_ids: [],
  };
}

function defaultBlueprint(activeProject: UserProject | null): MissionBlueprint {
  return {
    mission_id: `mission_${crypto.randomUUID().slice(0, 8)}`,
    title: "",
    goal: "",
    success_criteria: [],
    shared_context: "",
    workspace_root: activeProject?.path || "",
    orchestrator_template_id: "",
    phases: [newPhase(1)],
    milestones: [],
    team: {
      allowed_mcp_servers: [],
      max_parallel_agents: 4,
      orchestrator_only_tool_calls: false,
    },
    workstreams: [newWorkstream(1)],
    review_stages: [],
    metadata: null,
  };
}

function mergeJsonObjects(
  base: Record<string, unknown> | null | undefined,
  patch: Record<string, unknown> | null | undefined
): Record<string, unknown> | null {
  if (!base && !patch) return null;
  return {
    ...(base || {}),
    ...(patch || {}),
  };
}

function normalizeWorkstream(
  workstream: Partial<MissionBuilderWorkstream> | null | undefined,
  index: number
): MissionBuilderWorkstream {
  const base = newWorkstream(index + 1);
  return {
    ...base,
    ...workstream,
    workstream_id: String(workstream?.workstream_id || base.workstream_id).trim(),
    title: String(workstream?.title || base.title).trim(),
    objective: String(workstream?.objective || "").trim(),
    role: String(workstream?.role || base.role).trim(),
    prompt: String(workstream?.prompt || "").trim(),
    phase_id: String(workstream?.phase_id || "").trim(),
    lane: String(workstream?.lane || "").trim(),
    milestone: String(workstream?.milestone || "").trim(),
    depends_on: Array.isArray(workstream?.depends_on) ? workstream?.depends_on.filter(Boolean) : [],
    input_refs: Array.isArray(workstream?.input_refs) ? workstream?.input_refs.filter(Boolean) : [],
    tool_allowlist_override: Array.isArray(workstream?.tool_allowlist_override)
      ? workstream.tool_allowlist_override.filter(Boolean)
      : [],
    mcp_servers_override: Array.isArray(workstream?.mcp_servers_override)
      ? workstream.mcp_servers_override.filter(Boolean)
      : [],
    output_contract: {
      kind: String(workstream?.output_contract?.kind || base.output_contract.kind).trim(),
      summary_guidance: String(
        workstream?.output_contract?.summary_guidance || base.output_contract.summary_guidance || ""
      ).trim(),
      schema: workstream?.output_contract?.schema || null,
    },
  };
}

function normalizeReviewStage(
  stage: Partial<MissionBuilderReviewStage> | null | undefined,
  index: number
): MissionBuilderReviewStage {
  const base = newReviewStage(index + 1);
  return {
    ...base,
    ...stage,
    stage_id: String(stage?.stage_id || base.stage_id).trim(),
    stage_kind: (stage?.stage_kind || base.stage_kind) as MissionBuilderReviewStage["stage_kind"],
    title: String(stage?.title || base.title).trim(),
    phase_id: String(stage?.phase_id || "").trim(),
    lane: String(stage?.lane || "").trim(),
    milestone: String(stage?.milestone || "").trim(),
    target_ids: Array.isArray(stage?.target_ids) ? stage.target_ids.filter(Boolean) : [],
    checklist: Array.isArray(stage?.checklist) ? stage.checklist.filter(Boolean) : [],
    prompt: String(stage?.prompt || "").trim(),
    tool_allowlist_override: Array.isArray(stage?.tool_allowlist_override)
      ? stage.tool_allowlist_override.filter(Boolean)
      : [],
    mcp_servers_override: Array.isArray(stage?.mcp_servers_override)
      ? stage.mcp_servers_override.filter(Boolean)
      : [],
    gate:
      stage?.stage_kind === "approval" || (stage?.gate && typeof stage.gate === "object")
        ? {
            required: stage?.gate?.required ?? true,
            decisions: stage?.gate?.decisions || ["approve", "rework", "cancel"],
            rework_targets: Array.isArray(stage?.gate?.rework_targets)
              ? stage.gate.rework_targets.filter(Boolean)
              : [],
            instructions: String(stage?.gate?.instructions || "").trim(),
          }
        : stage?.gate || null,
  };
}

function normalizeMissionBlueprint(
  raw: Partial<MissionBlueprint> | null | undefined,
  activeProject: UserProject | null
): MissionBlueprint {
  const base = defaultBlueprint(activeProject);
  const team = ((raw?.team as Record<string, unknown> | undefined) ||
    {}) as MissionBlueprint["team"];
  const workstreams = Array.isArray(raw?.workstreams) ? raw.workstreams : [];
  const reviewStages = Array.isArray(raw?.review_stages) ? raw.review_stages : [];
  const phases = Array.isArray(raw?.phases) ? raw.phases : base.phases || [];
  const milestones = Array.isArray(raw?.milestones) ? raw.milestones : [];
  return {
    ...base,
    ...raw,
    mission_id: String(raw?.mission_id || base.mission_id).trim(),
    title: String(raw?.title || "").trim(),
    goal: String(raw?.goal || "").trim(),
    success_criteria: Array.isArray(raw?.success_criteria)
      ? raw.success_criteria.filter(Boolean)
      : [],
    shared_context: String(raw?.shared_context || "").trim(),
    workspace_root: String(
      raw?.workspace_root || activeProject?.path || base.workspace_root
    ).trim(),
    orchestrator_template_id: String(raw?.orchestrator_template_id || "").trim(),
    phases: phases.map((phase, index) => ({
      phase_id: String(phase?.phase_id || `phase_${index + 1}`).trim(),
      title: String(phase?.title || phase?.phase_id || `Phase ${index + 1}`).trim(),
      description: String(phase?.description || "").trim(),
      execution_mode: (phase?.execution_mode || "soft") as "soft" | "barrier",
    })),
    milestones: milestones.map((milestone, index) => ({
      milestone_id: String(milestone?.milestone_id || `milestone_${index + 1}`).trim(),
      title: String(milestone?.title || milestone?.milestone_id || `Milestone ${index + 1}`).trim(),
      description: String(milestone?.description || "").trim(),
      phase_id: String(milestone?.phase_id || "").trim(),
      required_stage_ids: Array.isArray(milestone?.required_stage_ids)
        ? milestone.required_stage_ids.filter(Boolean)
        : [],
    })),
    team: {
      allowed_template_ids: Array.isArray(team.allowed_template_ids)
        ? team.allowed_template_ids.filter(Boolean)
        : [],
      default_model_policy:
        (team.default_model_policy as Record<string, unknown> | undefined) || undefined,
      allowed_mcp_servers: Array.isArray(team.allowed_mcp_servers)
        ? team.allowed_mcp_servers.filter(Boolean)
        : [],
      max_parallel_agents:
        typeof team.max_parallel_agents === "number"
          ? team.max_parallel_agents
          : base.team.max_parallel_agents,
      mission_budget: (team.mission_budget as Record<string, unknown> | undefined) || undefined,
      orchestrator_only_tool_calls: Boolean(team.orchestrator_only_tool_calls),
    },
    workstreams: (workstreams.length ? workstreams : base.workstreams).map((workstream, index) =>
      normalizeWorkstream(workstream, index)
    ),
    review_stages: reviewStages.map((stage, index) => normalizeReviewStage(stage, index)),
    metadata: (raw?.metadata as Record<string, unknown> | null | undefined) || null,
  };
}

function extractMissionBlueprintFromAutomation(
  automation: AutomationV2Spec | null | undefined,
  activeProject: UserProject | null
): MissionBlueprint | null {
  const metadata = (automation?.metadata as Record<string, unknown> | undefined) || {};
  const direct =
    (metadata.mission_blueprint as Partial<MissionBlueprint> | undefined) ||
    (metadata.missionBlueprint as Partial<MissionBlueprint> | undefined) ||
    null;
  if (direct && typeof direct === "object") {
    return normalizeMissionBlueprint(direct, activeProject);
  }
  const mission = (metadata.mission as Record<string, unknown> | undefined) || {};
  const derivedWorkstreams =
    Array.isArray(metadata.workstreams) && metadata.workstreams.length > 0
      ? (metadata.workstreams as MissionBuilderWorkstream[])
      : (automation?.flow?.nodes || [])
          .filter((node) => {
            const metadata = (node.metadata as Record<string, unknown> | undefined) || {};
            const builder = (metadata.builder as Record<string, unknown> | undefined) || {};
            const stageKind = String(node.stage_kind || "")
              .trim()
              .toLowerCase();
            return stageKind === "" || stageKind === "workstream" || builder.role || builder.prompt;
          })
          .map((node) => {
            const nodeMetadata = (node.metadata as Record<string, unknown> | undefined) || {};
            const builder = (nodeMetadata.builder as Record<string, unknown> | undefined) || {};
            const agent =
              automation?.agents?.find((entry) => entry.agent_id === node.agent_id) || null;
            return {
              workstream_id: String(node.node_id || "").trim(),
              title: String(builder.title || node.node_id || "Workstream").trim(),
              objective: String(node.objective || "").trim(),
              role: String(builder.role || "worker").trim(),
              priority:
                typeof builder.priority === "number"
                  ? builder.priority
                  : builder.priority
                    ? Number.parseInt(String(builder.priority), 10) || undefined
                    : undefined,
              phase_id: String(builder.phase_id || "").trim(),
              lane: String(builder.lane || "").trim(),
              milestone: String(builder.milestone || "").trim(),
              template_id: String(agent?.template_id || "").trim() || undefined,
              prompt: String(builder.prompt || "").trim(),
              depends_on: Array.isArray(node.depends_on)
                ? node.depends_on.map((row) => String(row || "").trim()).filter(Boolean)
                : [],
              input_refs: Array.isArray(node.input_refs)
                ? node.input_refs.map((row) => ({
                    from_step_id: String(row.from_step_id || "").trim(),
                    alias: String(row.alias || row.from_step_id || "").trim(),
                  }))
                : [],
              output_contract: {
                kind: String(node.output_contract?.kind || "report_markdown").trim(),
                summary_guidance: String(node.output_contract?.summary_guidance || "").trim(),
                schema: node.output_contract?.schema || null,
              },
              model_override:
                (agent?.model_policy as Record<string, unknown> | undefined) || undefined,
              tool_allowlist_override: Array.isArray(agent?.tool_policy?.allowlist)
                ? agent.tool_policy.allowlist.filter(Boolean)
                : [],
              mcp_servers_override: Array.isArray(agent?.mcp_policy?.allowed_servers)
                ? agent.mcp_policy.allowed_servers.filter(Boolean)
                : [],
            } satisfies MissionBuilderWorkstream;
          });
  const derivedReviewStages =
    Array.isArray(metadata.review_stages) && metadata.review_stages.length > 0
      ? (metadata.review_stages as MissionBuilderReviewStage[])
      : (automation?.flow?.nodes || [])
          .filter((node) => {
            const stageKind = String(node.stage_kind || "")
              .trim()
              .toLowerCase();
            return stageKind === "review" || stageKind === "test" || stageKind === "approval";
          })
          .map((node) => {
            const nodeMetadata = (node.metadata as Record<string, unknown> | undefined) || {};
            const builder = (nodeMetadata.builder as Record<string, unknown> | undefined) || {};
            const agent =
              automation?.agents?.find((entry) => entry.agent_id === node.agent_id) || null;
            return {
              stage_id: String(node.node_id || "").trim(),
              stage_kind: (String(node.stage_kind || "review")
                .trim()
                .toLowerCase() || "review") as MissionBuilderReviewStage["stage_kind"],
              title: String(builder.title || node.node_id || "Stage").trim(),
              priority:
                typeof builder.priority === "number"
                  ? builder.priority
                  : builder.priority
                    ? Number.parseInt(String(builder.priority), 10) || undefined
                    : undefined,
              phase_id: String(builder.phase_id || "").trim(),
              lane: String(builder.lane || "").trim(),
              milestone: String(builder.milestone || "").trim(),
              target_ids: Array.isArray(node.depends_on)
                ? node.depends_on.map((row) => String(row || "").trim()).filter(Boolean)
                : [],
              role: String(builder.role || "").trim() || undefined,
              template_id: String(agent?.template_id || "").trim() || undefined,
              prompt: String(node.objective || builder.prompt || "").trim(),
              checklist: Array.isArray(builder.checklist)
                ? builder.checklist.map((row) => String(row || "").trim()).filter(Boolean)
                : [],
              model_override:
                (agent?.model_policy as Record<string, unknown> | undefined) || undefined,
              tool_allowlist_override: Array.isArray(agent?.tool_policy?.allowlist)
                ? agent.tool_policy.allowlist.filter(Boolean)
                : [],
              mcp_servers_override: Array.isArray(agent?.mcp_policy?.allowed_servers)
                ? agent.mcp_policy.allowed_servers.filter(Boolean)
                : [],
              gate: (node.gate as MissionBuilderReviewStage["gate"]) || null,
            } satisfies MissionBuilderReviewStage;
          });
  const derivedPhaseIds = Array.from(
    new Set(
      [...derivedWorkstreams, ...derivedReviewStages]
        .map((row) => String(row.phase_id || "").trim())
        .filter(Boolean)
    )
  );
  const derivedMilestoneIds = Array.from(
    new Set(
      [...derivedWorkstreams, ...derivedReviewStages]
        .map((row) => String(row.milestone || "").trim())
        .filter(Boolean)
    )
  );
  const fallbackPhases =
    Array.isArray(mission.phases) && mission.phases.length > 0
      ? (mission.phases as MissionBlueprint["phases"])
      : derivedPhaseIds.map((phaseId, index) => ({
          phase_id: phaseId,
          title: phaseId.replace(/_/g, " ") || `Phase ${index + 1}`,
          description: "",
          execution_mode: "soft" as const,
        }));
  const fallbackMilestones =
    Array.isArray(mission.milestones) && mission.milestones.length > 0
      ? (mission.milestones as MissionBlueprint["milestones"])
      : derivedMilestoneIds.map((milestoneId, index) => ({
          milestone_id: milestoneId,
          title: milestoneId.replace(/_/g, " ") || `Milestone ${index + 1}`,
          description: "",
          phase_id: "",
          required_stage_ids: [],
        }));
  const builderKind = String(metadata.builder_kind || metadata.builderKind || "").trim();
  const looksLikeAdvancedAutomation =
    builderKind === "mission_blueprint" ||
    derivedWorkstreams.length > 0 ||
    derivedReviewStages.length > 0 ||
    Object.keys(mission).length > 0;
  if (!looksLikeAdvancedAutomation) return null;
  return normalizeMissionBlueprint(
    {
      mission_id: String(mission.mission_id || automation?.automation_id || "").trim(),
      title: String(mission.title || automation?.name || "").trim(),
      goal: String(mission.goal || automation?.description || "").trim(),
      success_criteria: Array.isArray(mission.success_criteria)
        ? mission.success_criteria.map((row) => String(row || "").trim()).filter(Boolean)
        : [],
      shared_context: String(mission.shared_context || "").trim(),
      workspace_root: String(automation?.workspace_root || activeProject?.path || "").trim(),
      orchestrator_template_id: String(mission.orchestrator_template_id || "").trim(),
      phases: fallbackPhases,
      milestones: fallbackMilestones,
      team: {
        ...baseBlueprintTeam(activeProject),
        ...((mission.team as MissionBlueprint["team"]) || {}),
        allowed_mcp_servers: Array.isArray(
          ((mission.team as MissionBlueprint["team"] | undefined)?.allowed_mcp_servers ||
            metadata.allowed_mcp_servers) as string[] | undefined
        )
          ? (
              (((mission.team as MissionBlueprint["team"] | undefined)?.allowed_mcp_servers ||
                metadata.allowed_mcp_servers) as string[]) || []
            ).filter(Boolean)
          : [],
        max_parallel_agents:
          (mission.team as MissionBlueprint["team"] | undefined)?.max_parallel_agents ||
          automation?.execution?.max_parallel_agents ||
          baseBlueprintTeam(activeProject).max_parallel_agents,
      },
      workstreams: derivedWorkstreams,
      review_stages: derivedReviewStages,
    },
    activeProject
  );
}

function baseBlueprintTeam(activeProject: UserProject | null): MissionBlueprint["team"] {
  return defaultBlueprint(activeProject).team;
}

function toModelPolicy(draft: BuilderModelDraft) {
  const provider = draft.provider.trim();
  const model = draft.model.trim();
  if (!provider || !model) return undefined;
  return {
    default_model: {
      provider_id: provider,
      model_id: model,
    },
  };
}

function splitCsv(raw: string) {
  return String(raw || "")
    .split(",")
    .map((value) => value.trim())
    .filter(Boolean);
}

function ScopeSelector({
  label,
  helper,
  options,
  selected,
  inheritSummary,
  emptySummary,
  onChange,
}: {
  label: string;
  helper: string;
  options: string[];
  selected: string[];
  inheritSummary: string;
  emptySummary: string;
  onChange: (next: string[]) => void;
}) {
  const [query, setQuery] = useState("");
  const normalizedQuery = query.trim().toLowerCase();
  const filteredOptions = useMemo(
    () =>
      options.filter((option) =>
        normalizedQuery ? option.toLowerCase().includes(normalizedQuery) : true
      ),
    [normalizedQuery, options]
  );
  const summary = selected.length
    ? `${selected.length} override${selected.length === 1 ? "" : "s"} selected`
    : inheritSummary || emptySummary;

  return (
    <div className="rounded-lg border border-border bg-surface px-3 py-3">
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="text-sm font-medium text-text">{label}</div>
          <div className="mt-1 text-xs text-text-muted">{helper}</div>
        </div>
        <Button size="sm" variant="secondary" onClick={() => onChange([])}>
          Inherit
        </Button>
      </div>
      <div className="mt-2 text-xs text-text-subtle">{summary}</div>
      <input
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        placeholder={`Search ${label.toLowerCase()}`}
        className="mt-3 h-10 w-full rounded-lg border border-border bg-surface-elevated/40 px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
      />
      <div className="mt-3 max-h-44 space-y-2 overflow-y-auto rounded-lg border border-border bg-surface-elevated/30 p-2">
        {filteredOptions.length ? (
          filteredOptions.map((option) => {
            const checked = selected.includes(option);
            return (
              <label
                key={option}
                className="flex items-center justify-between gap-3 rounded-md px-2 py-2 text-sm text-text hover:bg-surface"
              >
                <span className="truncate">{option}</span>
                <input
                  type="checkbox"
                  checked={checked}
                  onChange={(event) =>
                    onChange(
                      event.target.checked
                        ? [...selected, option].sort()
                        : selected.filter((value) => value !== option)
                    )
                  }
                  className="h-4 w-4 rounded border-border bg-surface"
                />
              </label>
            );
          })
        ) : (
          <div className="px-2 py-3 text-xs text-text-muted">{emptySummary}</div>
        )}
      </div>
    </div>
  );
}

function parseOptionalInt(raw: string) {
  const trimmed = String(raw || "").trim();
  if (!trimmed) return undefined;
  const value = Number.parseInt(trimmed, 10);
  return Number.isFinite(value) && value > 0 ? value : undefined;
}

function parseOptionalFloat(raw: string) {
  const trimmed = String(raw || "").trim();
  if (!trimmed) return undefined;
  const value = Number.parseFloat(trimmed);
  return Number.isFinite(value) && value > 0 ? value : undefined;
}

function modelOptions(providers: ProviderInfo[], providerId: string) {
  return providers.find((provider) => provider.id === providerId)?.models ?? [];
}

function BuilderCard({
  title,
  subtitle,
  children,
  actions,
}: {
  title: string;
  subtitle?: string;
  children: React.ReactNode;
  actions?: React.ReactNode;
}) {
  return (
    <section className="rounded-xl border border-border bg-surface p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 className="text-sm font-semibold text-text">{title}</h3>
          {subtitle ? <p className="mt-1 text-xs text-text-muted">{subtitle}</p> : null}
        </div>
        {actions}
      </div>
      <div className="mt-4">{children}</div>
    </section>
  );
}

export function AdvancedMissionBuilder({
  activeProject,
  providers,
  mcpServers,
  toolIds,
  editingAutomation = null,
  onRefreshAutomations,
  onShowAutomations,
  onShowRuns,
  onClearEditing,
  onOpenMcpExtensions,
  blueprintMetadataPatch = null,
}: AdvancedMissionBuilderProps) {
  const [blueprint, setBlueprint] = useState<MissionBlueprint>(() =>
    defaultBlueprint(activeProject)
  );
  const [teamModel, setTeamModel] = useState<BuilderModelDraft>({ provider: "", model: "" });
  const [workstreamModels, setWorkstreamModels] = useState<Record<string, BuilderModelDraft>>({});
  const [preview, setPreview] = useState<MissionBuilderCompilePreview | null>(null);
  const [templates, setTemplates] = useState<Array<{ template_id: string; role: string }>>([]);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [runAfterCreate, setRunAfterCreate] = useState(true);

  const missionBudget = blueprint.team.mission_budget || {};

  useEffect(() => {
    setBlueprint((current) => ({
      ...current,
      workspace_root: activeProject?.path || current.workspace_root,
    }));
  }, [activeProject?.id]);

  useEffect(() => {
    const savedBlueprint = extractMissionBlueprintFromAutomation(editingAutomation, activeProject);
    if (!editingAutomation?.automation_id || !savedBlueprint) {
      setBlueprint({
        ...defaultBlueprint(activeProject),
        metadata: mergeJsonObjects(null, blueprintMetadataPatch),
      });
      setTeamModel({ provider: "", model: "" });
      setWorkstreamModels({});
      setPreview(null);
      setRunAfterCreate(true);
      return;
    }
    setBlueprint(savedBlueprint);
    setTeamModel(toModelDraft(savedBlueprint.team?.default_model_policy || null));
    setWorkstreamModels(workstreamModelDrafts(savedBlueprint));
    setPreview(null);
    setRunAfterCreate(false);
    setError(null);
  }, [editingAutomation?.automation_id, activeProject?.id]);

  useEffect(() => {
    if (editingAutomation?.automation_id) return;
    setBlueprint((current) => ({
      ...current,
      metadata: mergeJsonObjects(
        (current.metadata as Record<string, unknown> | null | undefined) || null,
        blueprintMetadataPatch
      ),
    }));
  }, [blueprintMetadataPatch, editingAutomation?.automation_id]);

  useEffect(() => {
    void agentTeamListTemplates()
      .then((rows) =>
        setTemplates(
          rows.map((row) => ({
            template_id: row.template_id,
            role: row.role,
          }))
        )
      )
      .catch(() => setTemplates([]));
  }, []);

  const allStageIds = useMemo(
    () => [
      ...blueprint.workstreams.map((row) => row.workstream_id),
      ...blueprint.review_stages.map((row) => row.stage_id),
    ],
    [blueprint]
  );
  const phaseIds = useMemo(
    () => (blueprint.phases || []).map((phase) => phase.phase_id).filter(Boolean),
    [blueprint.phases]
  );
  const milestoneIds = useMemo(
    () => (blueprint.milestones || []).map((milestone) => milestone.milestone_id).filter(Boolean),
    [blueprint.milestones]
  );

  const updateBlueprint = (patch: Partial<MissionBlueprint>) => {
    setBlueprint((current) => ({ ...current, ...patch }));
    setPreview(null);
  };

  const updateWorkstream = (workstreamId: string, patch: Partial<MissionBuilderWorkstream>) => {
    setBlueprint((current) => ({
      ...current,
      workstreams: current.workstreams.map((row) =>
        row.workstream_id === workstreamId ? { ...row, ...patch } : row
      ),
    }));
    setPreview(null);
  };

  const updateReviewStage = (stageId: string, patch: Partial<MissionBuilderReviewStage>) => {
    setBlueprint((current) => ({
      ...current,
      review_stages: current.review_stages.map((row) =>
        row.stage_id === stageId ? { ...row, ...patch } : row
      ),
    }));
    setPreview(null);
  };

  const effectiveBlueprint = useMemo<MissionBlueprint>(() => {
    const nextWorkstreams = blueprint.workstreams.map((workstream) => ({
      ...workstream,
      model_override: toModelPolicy(
        workstreamModels[workstream.workstream_id] || { provider: "", model: "" }
      ),
    }));
    return {
      ...blueprint,
      phases: blueprint.phases || [],
      milestones: blueprint.milestones || [],
      team: {
        ...blueprint.team,
        default_model_policy: toModelPolicy(teamModel),
      },
      workstreams: nextWorkstreams,
    };
  }, [blueprint, teamModel, workstreamModels]);
  const previewGraphColumns = useMemo(() => {
    if (!preview) return [];
    const nodeLookup = new Map(preview.node_previews.map((node) => [node.node_id, node]));
    const downstreamCounts = new Map<string, number>();
    const downstreamIds = new Map<string, string[]>();
    for (const node of preview.node_previews) {
      downstreamCounts.set(node.node_id, 0);
      downstreamIds.set(node.node_id, []);
    }
    for (const node of preview.node_previews) {
      for (const upstreamId of node.depends_on) {
        downstreamCounts.set(upstreamId, (downstreamCounts.get(upstreamId) || 0) + 1);
        downstreamIds.set(upstreamId, [...(downstreamIds.get(upstreamId) || []), node.node_id]);
      }
    }
    const phaseLookup = new Map(
      (effectiveBlueprint.phases || []).map((phase) => [
        phase.phase_id,
        {
          phaseId: phase.phase_id,
          title: phase.title || phase.phase_id,
          executionMode: phase.execution_mode || "soft",
        },
      ])
    );
    const grouped = new Map<
      string,
      {
        phaseId: string;
        title: string;
        executionMode: string;
        lanes: Array<{
          laneId: string;
          title: string;
          inboundHandoffs: number;
          outboundHandoffs: number;
          nodes: Array<
            (typeof preview.node_previews)[number] & {
              downstreamCount: number;
              downstreamNodeIds: string[];
              dependencyEdges: Array<{
                nodeId: string;
                title: string;
                crossLane: boolean;
                crossPhase: boolean;
              }>;
              downstreamEdges: Array<{
                nodeId: string;
                title: string;
                crossLane: boolean;
                crossPhase: boolean;
              }>;
            }
          >;
        }>;
      }
    >();
    for (const node of preview.node_previews) {
      const phaseId = node.phase_id || "unassigned";
      const phaseMeta = phaseLookup.get(phaseId) || {
        phaseId,
        title: phaseId === "unassigned" ? "Unassigned" : phaseId,
        executionMode: "n/a",
      };
      const existing = grouped.get(phaseId) || { ...phaseMeta, lanes: [] };
      const laneId = node.lane || "unassigned";
      const laneTitle = node.lane || "Unassigned Lane";
      let lane = existing.lanes.find((entry) => entry.laneId === laneId);
      if (!lane) {
        lane = { laneId, title: laneTitle, inboundHandoffs: 0, outboundHandoffs: 0, nodes: [] };
        existing.lanes.push(lane);
      }
      const dependencyEdges = node.depends_on.map((dependencyId) => {
        const dependencyNode = nodeLookup.get(dependencyId);
        const dependencyLane = dependencyNode?.lane || "unassigned";
        const dependencyPhase = dependencyNode?.phase_id || "unassigned";
        const crossLane = dependencyLane !== laneId;
        const crossPhase = dependencyPhase !== phaseId;
        if (crossLane) lane!.inboundHandoffs += 1;
        return {
          nodeId: dependencyId,
          title: dependencyNode?.title || dependencyId,
          crossLane,
          crossPhase,
        };
      });
      const nodeDownstreamEdges = (downstreamIds.get(node.node_id) || []).map((downstreamId) => {
        const downstreamNode = nodeLookup.get(downstreamId);
        const downstreamLane = downstreamNode?.lane || "unassigned";
        const downstreamPhase = downstreamNode?.phase_id || "unassigned";
        const crossLane = downstreamLane !== laneId;
        const crossPhase = downstreamPhase !== phaseId;
        if (crossLane) lane!.outboundHandoffs += 1;
        return {
          nodeId: downstreamId,
          title: downstreamNode?.title || downstreamId,
          crossLane,
          crossPhase,
        };
      });
      lane.nodes.push({
        ...node,
        downstreamCount: downstreamCounts.get(node.node_id) || 0,
        downstreamNodeIds: downstreamIds.get(node.node_id) || [],
        dependencyEdges,
        downstreamEdges: nodeDownstreamEdges,
      });
      grouped.set(phaseId, existing);
    }
    return Array.from(grouped.values())
      .map((column) => ({
        ...column,
        lanes: column.lanes
          .map((lane) => ({
            ...lane,
            nodes: lane.nodes.sort(
              (a, b) =>
                (a.priority ?? 0) - (b.priority ?? 0) ||
                a.title.localeCompare(b.title) ||
                a.node_id.localeCompare(b.node_id)
            ),
          }))
          .sort((a, b) => a.title.localeCompare(b.title)),
      }))
      .sort((a, b) => {
        const aIndex = (effectiveBlueprint.phases || []).findIndex(
          (phase) => phase.phase_id === a.phaseId
        );
        const bIndex = (effectiveBlueprint.phases || []).findIndex(
          (phase) => phase.phase_id === b.phaseId
        );
        const normalizedA = aIndex === -1 ? Number.MAX_SAFE_INTEGER : aIndex;
        const normalizedB = bIndex === -1 ? Number.MAX_SAFE_INTEGER : bIndex;
        return normalizedA - normalizedB || a.title.localeCompare(b.title);
      });
  }, [preview, effectiveBlueprint.phases]);
  const previewGraphSummary = useMemo(() => {
    if (!preview) return null;

    // Bolt Optimization: Calculate fanOut map in O(N) instead of O(N^2)
    const fanOutMap = new Map<string, number>();
    for (const node of preview.node_previews) {
      for (const upstreamId of node.depends_on) {
        fanOutMap.set(upstreamId, (fanOutMap.get(upstreamId) || 0) + 1);
      }
    }

    const rootCount = preview.node_previews.filter((node) => node.depends_on.length === 0).length;
    const terminalCount = preview.node_previews.filter(
      (node) => (fanOutMap.get(node.node_id) || 0) === 0
    ).length;

    let crossLaneEdges = 0;
    let crossPhaseEdges = 0;
    const nodeLookup = new Map(preview.node_previews.map((node) => [node.node_id, node]));
    for (const node of preview.node_previews) {
      for (const upstreamId of node.depends_on) {
        const upstream = nodeLookup.get(upstreamId);
        if ((upstream?.lane || "unassigned") !== (node.lane || "unassigned")) crossLaneEdges += 1;
        if ((upstream?.phase_id || "unassigned") !== (node.phase_id || "unassigned")) {
          crossPhaseEdges += 1;
        }
      }
    }
    const highFanIn = preview.node_previews.filter((node) => node.depends_on.length >= 3).length;
    const highFanOut = preview.node_previews.filter(
      (node) => (fanOutMap.get(node.node_id) || 0) >= 3
    ).length;
    return { rootCount, terminalCount, crossLaneEdges, crossPhaseEdges, highFanIn, highFanOut };
  }, [preview]);

  const compilePreview = async () => {
    setBusyKey("preview");
    setError(null);
    try {
      const timezone = detectBrowserTimezone();
      const response = await missionBuilderPreview({
        blueprint: effectiveBlueprint,
        schedule: { type: "manual", timezone, misfire_policy: "run_once" },
      });
      setPreview(response);
    } catch (compileError) {
      setError(compileError instanceof Error ? compileError.message : String(compileError));
    } finally {
      setBusyKey(null);
    }
  };

  const createDraft = async () => {
    setBusyKey("apply");
    setError(null);
    try {
      const timezone = detectBrowserTimezone();
      if (editingAutomation?.automation_id) {
        const compiled = await missionBuilderPreview({
          blueprint: effectiveBlueprint,
          schedule: editingAutomation.schedule,
        });
        await automationsV2Update(editingAutomation.automation_id, {
          name: compiled.automation.name,
          description: compiled.automation.description || null,
          schedule: compiled.automation.schedule,
          agents: compiled.automation.agents,
          flow: compiled.automation.flow,
          execution: compiled.automation.execution,
          workspace_root: compiled.automation.workspace_root,
          metadata: {
            ...((editingAutomation.metadata as Record<string, unknown> | undefined) || {}),
            ...((compiled.automation.metadata as Record<string, unknown> | undefined) || {}),
          },
        });
        await onRefreshAutomations();
        onShowAutomations();
        onClearEditing?.();
        setPreview(compiled);
        return;
      }
      const response = await missionBuilderApply({
        blueprint: effectiveBlueprint,
        creator_id: "desktop",
        schedule: { type: "manual", timezone, misfire_policy: "run_once" },
      });
      const automationId = String(response.automation?.automation_id || "").trim();
      await onRefreshAutomations();
      if (runAfterCreate && automationId) {
        await automationsV2RunNow(automationId);
        onShowRuns();
      } else {
        onShowAutomations();
      }
      setBlueprint(defaultBlueprint(activeProject));
      setTeamModel({ provider: "", model: "" });
      setWorkstreamModels({});
      setPreview(null);
    } catch (applyError) {
      setError(applyError instanceof Error ? applyError.message : String(applyError));
    } finally {
      setBusyKey(null);
    }
  };

  return (
    <div className="space-y-4">
      {error ? (
        <div className="rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-200">
          {error}
        </div>
      ) : null}

      <BuilderCard title="Mission" subtitle="One shared brief for the whole workload.">
        <div className="grid gap-3 lg:grid-cols-2">
          <Input
            label="Mission Title"
            value={blueprint.title}
            onChange={(event) => updateBlueprint({ title: event.target.value })}
          />
          <Input
            label="Mission ID"
            value={blueprint.mission_id}
            onChange={(event) => updateBlueprint({ mission_id: event.target.value })}
          />
        </div>
        <div className="mt-3 grid gap-3 lg:grid-cols-2">
          <Input
            label="Workspace Root"
            value={blueprint.workspace_root}
            onChange={(event) => updateBlueprint({ workspace_root: event.target.value })}
          />
          <Input
            label="Success Criteria"
            value={blueprint.success_criteria.join(", ")}
            onChange={(event) =>
              updateBlueprint({ success_criteria: splitCsv(event.target.value) })
            }
          />
        </div>
        <label className="mt-3 block text-sm font-medium text-text">
          Mission Goal
          <textarea
            value={blueprint.goal}
            onChange={(event) => updateBlueprint({ goal: event.target.value })}
            className="mt-2 min-h-[120px] w-full rounded-lg border border-border bg-surface px-3 py-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            placeholder="Describe the global objective all agents are working toward."
          />
        </label>
        <label className="mt-3 block text-sm font-medium text-text">
          Shared Context
          <textarea
            value={blueprint.shared_context || ""}
            onChange={(event) => updateBlueprint({ shared_context: event.target.value })}
            className="mt-2 min-h-[120px] w-full rounded-lg border border-border bg-surface px-3 py-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            placeholder="Shared constraints, context, references, or operator instructions."
          />
        </label>
      </BuilderCard>

      <BuilderCard title="Team" subtitle="Orchestrator, defaults, and mission-wide controls.">
        <div className="grid gap-3 lg:grid-cols-2">
          <label className="block text-sm font-medium text-text">
            Orchestrator Template
            <select
              value={blueprint.orchestrator_template_id || ""}
              onChange={(event) =>
                updateBlueprint({ orchestrator_template_id: event.target.value || undefined })
              }
              className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            >
              <option value="">None</option>
              {templates.map((template) => (
                <option key={template.template_id} value={template.template_id}>
                  {template.template_id} ({template.role})
                </option>
              ))}
            </select>
          </label>
          <Input
            label="Max Parallel Agents"
            type="number"
            min={1}
            max={16}
            value={String(blueprint.team.max_parallel_agents || 4)}
            onChange={(event) =>
              updateBlueprint({
                team: {
                  ...blueprint.team,
                  max_parallel_agents: Math.max(
                    1,
                    Number.parseInt(event.target.value || "4", 10) || 4
                  ),
                },
              })
            }
          />
        </div>
        <div className="mt-3 grid gap-3 lg:grid-cols-2">
          <label className="block text-sm font-medium text-text">
            Default Provider
            <select
              value={teamModel.provider}
              onChange={(event) =>
                setTeamModel({
                  provider: event.target.value,
                  model: modelOptions(providers, event.target.value)[0] || "",
                })
              }
              className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            >
              <option value="">Engine default</option>
              {providers.map((provider) => (
                <option key={provider.id} value={provider.id}>
                  {provider.id}
                </option>
              ))}
            </select>
          </label>
          <label className="block text-sm font-medium text-text">
            Default Model
            <select
              value={teamModel.model}
              onChange={(event) =>
                setTeamModel((current) => ({ ...current, model: event.target.value }))
              }
              className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            >
              <option value="">Engine default</option>
              {modelOptions(providers, teamModel.provider).map((modelId) => (
                <option key={modelId} value={modelId}>
                  {modelId}
                </option>
              ))}
            </select>
          </label>
        </div>
        <div className="mt-3 rounded-lg border border-border bg-surface-elevated/40 p-3">
          <div className="text-sm font-medium text-text">Guardrails</div>
          <div className="mt-1 text-xs text-text-muted">
            Hard mission-wide ceilings for spend, runtime, and token burn. Leaving a field blank
            keeps the engine default.
          </div>
          <div className="mt-3 grid gap-3 lg:grid-cols-4">
            <Input
              label="Token Ceiling"
              type="number"
              min={1}
              value={missionBudget.max_tokens ? String(missionBudget.max_tokens) : ""}
              onChange={(event) =>
                updateBlueprint({
                  team: {
                    ...blueprint.team,
                    mission_budget: {
                      ...missionBudget,
                      max_tokens: parseOptionalInt(event.target.value) ?? null,
                    },
                  },
                })
              }
            />
            <Input
              label="Cost Ceiling (USD)"
              type="number"
              min={0}
              step="0.01"
              value={missionBudget.max_cost_usd ? String(missionBudget.max_cost_usd) : ""}
              onChange={(event) =>
                updateBlueprint({
                  team: {
                    ...blueprint.team,
                    mission_budget: {
                      ...missionBudget,
                      max_cost_usd: parseOptionalFloat(event.target.value) ?? null,
                    },
                  },
                })
              }
            />
            <Input
              label="Runtime Ceiling (ms)"
              type="number"
              min={1}
              value={missionBudget.max_duration_ms ? String(missionBudget.max_duration_ms) : ""}
              onChange={(event) =>
                updateBlueprint({
                  team: {
                    ...blueprint.team,
                    mission_budget: {
                      ...missionBudget,
                      max_duration_ms: parseOptionalInt(event.target.value) ?? null,
                    },
                  },
                })
              }
            />
            <Input
              label="Tool Call Ceiling"
              type="number"
              min={1}
              value={missionBudget.max_tool_calls ? String(missionBudget.max_tool_calls) : ""}
              onChange={(event) =>
                updateBlueprint({
                  team: {
                    ...blueprint.team,
                    mission_budget: {
                      ...missionBudget,
                      max_tool_calls: parseOptionalInt(event.target.value) ?? null,
                    },
                  },
                })
              }
            />
          </div>
        </div>
        <div className="mt-3 rounded-lg border border-border bg-surface-elevated/40 p-3">
          <div className="flex items-center justify-between gap-2">
            <div>
              <div className="text-sm font-medium text-text">Allowed MCP Servers</div>
              <div className="text-xs text-text-muted">
                Mission-wide MCP access inherited by workstreams unless overridden.
              </div>
            </div>
            {onOpenMcpExtensions ? (
              <Button size="sm" variant="secondary" onClick={onOpenMcpExtensions}>
                Manage MCP
              </Button>
            ) : null}
          </div>
          <div className="mt-3 grid gap-2 lg:grid-cols-2">
            {mcpServers.map((server) => {
              const checked = (blueprint.team.allowed_mcp_servers || []).includes(server.name);
              return (
                <label
                  key={server.name}
                  className="flex items-start gap-2 rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text"
                >
                  <input
                    type="checkbox"
                    checked={checked}
                    onChange={() =>
                      updateBlueprint({
                        team: {
                          ...blueprint.team,
                          allowed_mcp_servers: checked
                            ? (blueprint.team.allowed_mcp_servers || []).filter(
                                (row) => row !== server.name
                              )
                            : [...(blueprint.team.allowed_mcp_servers || []), server.name],
                        },
                      })
                    }
                  />
                  <span>
                    <span className="block font-medium">{server.name}</span>
                    <span className="block text-xs text-text-muted">
                      {server.connected ? "connected" : "disconnected"} |{" "}
                      {server.enabled ? "enabled" : "disabled"}
                    </span>
                  </span>
                </label>
              );
            })}
          </div>
        </div>
        <div className="mt-3 grid gap-4 lg:grid-cols-2">
          <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
            <div className="flex items-center justify-between gap-2">
              <div>
                <div className="text-sm font-medium text-text">Phases</div>
                <div className="text-xs text-text-muted">
                  Phase controls which stage of the mission work belongs to.
                </div>
              </div>
              <Button
                size="sm"
                variant="secondary"
                onClick={() =>
                  updateBlueprint({
                    phases: [
                      ...(blueprint.phases || []),
                      newPhase((blueprint.phases || []).length + 1),
                    ],
                  })
                }
              >
                Add Phase
              </Button>
            </div>
            <div className="mt-3 space-y-3">
              {(blueprint.phases || []).map((phase, index) => (
                <div
                  key={`${phase.phase_id}-${index}`}
                  className="rounded-lg border border-border bg-surface px-3 py-3"
                >
                  <div className="grid gap-3 lg:grid-cols-2">
                    <Input
                      label="Phase ID"
                      value={phase.phase_id}
                      onChange={(event) =>
                        updateBlueprint({
                          phases: (blueprint.phases || []).map((row, rowIndex) =>
                            rowIndex === index ? { ...row, phase_id: event.target.value } : row
                          ),
                        })
                      }
                    />
                    <Input
                      label="Title"
                      value={phase.title}
                      onChange={(event) =>
                        updateBlueprint({
                          phases: (blueprint.phases || []).map((row, rowIndex) =>
                            rowIndex === index ? { ...row, title: event.target.value } : row
                          ),
                        })
                      }
                    />
                  </div>
                  <div className="mt-3 grid gap-3 lg:grid-cols-[1fr_auto]">
                    <label className="block text-sm font-medium text-text">
                      Execution Mode
                      <select
                        value={phase.execution_mode || "soft"}
                        onChange={(event) =>
                          updateBlueprint({
                            phases: (blueprint.phases || []).map((row, rowIndex) =>
                              rowIndex === index
                                ? {
                                    ...row,
                                    execution_mode: event.target.value as "soft" | "barrier",
                                  }
                                : row
                            ),
                          })
                        }
                        className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                      >
                        <option value="soft">soft</option>
                        <option value="barrier">barrier</option>
                      </select>
                    </label>
                    <div className="flex items-end">
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() =>
                          updateBlueprint({
                            phases: (blueprint.phases || []).filter(
                              (_, rowIndex) => rowIndex !== index
                            ),
                          })
                        }
                      >
                        Remove
                      </Button>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          </div>
          <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
            <div className="flex items-center justify-between gap-2">
              <div>
                <div className="text-sm font-medium text-text">Milestones</div>
                <div className="text-xs text-text-muted">
                  Milestones define checkpoint promotions and expected stage coverage.
                </div>
              </div>
              <Button
                size="sm"
                variant="secondary"
                onClick={() =>
                  updateBlueprint({
                    milestones: [
                      ...(blueprint.milestones || []),
                      newMilestone((blueprint.milestones || []).length + 1),
                    ],
                  })
                }
              >
                Add Milestone
              </Button>
            </div>
            <div className="mt-3 space-y-3">
              {(blueprint.milestones || []).map((milestone, index) => (
                <div
                  key={`${milestone.milestone_id}-${index}`}
                  className="rounded-lg border border-border bg-surface px-3 py-3"
                >
                  <div className="grid gap-3 lg:grid-cols-2">
                    <Input
                      label="Milestone ID"
                      value={milestone.milestone_id}
                      onChange={(event) =>
                        updateBlueprint({
                          milestones: (blueprint.milestones || []).map((row, rowIndex) =>
                            rowIndex === index ? { ...row, milestone_id: event.target.value } : row
                          ),
                        })
                      }
                    />
                    <Input
                      label="Title"
                      value={milestone.title}
                      onChange={(event) =>
                        updateBlueprint({
                          milestones: (blueprint.milestones || []).map((row, rowIndex) =>
                            rowIndex === index ? { ...row, title: event.target.value } : row
                          ),
                        })
                      }
                    />
                  </div>
                  <div className="mt-3 grid gap-3 lg:grid-cols-2">
                    <Input
                      label="Phase ID"
                      value={milestone.phase_id || ""}
                      onChange={(event) =>
                        updateBlueprint({
                          milestones: (blueprint.milestones || []).map((row, rowIndex) =>
                            rowIndex === index ? { ...row, phase_id: event.target.value } : row
                          ),
                        })
                      }
                    />
                    <Input
                      label="Required Stage IDs"
                      value={(milestone.required_stage_ids || []).join(", ")}
                      onChange={(event) =>
                        updateBlueprint({
                          milestones: (blueprint.milestones || []).map((row, rowIndex) =>
                            rowIndex === index
                              ? { ...row, required_stage_ids: splitCsv(event.target.value) }
                              : row
                          ),
                        })
                      }
                    />
                  </div>
                  <div className="mt-3 flex justify-end">
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={() =>
                        updateBlueprint({
                          milestones: (blueprint.milestones || []).filter(
                            (_, rowIndex) => rowIndex !== index
                          ),
                        })
                      }
                    >
                      Remove
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      </BuilderCard>

      <BuilderCard
        title="Workstreams"
        subtitle="Role-based lanes with explicit dependencies and handoffs."
        actions={
          <Button
            size="sm"
            variant="secondary"
            onClick={() =>
              updateBlueprint({
                workstreams: [
                  ...blueprint.workstreams,
                  newWorkstream(blueprint.workstreams.length + 1),
                ],
              })
            }
          >
            Add Workstream
          </Button>
        }
      >
        <div className="space-y-4">
          {blueprint.workstreams.map((workstream, index) => {
            const modelDraft = workstreamModels[workstream.workstream_id] || {
              provider: "",
              model: "",
            };
            return (
              <div
                key={workstream.workstream_id}
                className="rounded-lg border border-border bg-surface-elevated/40 p-3"
              >
                <div className="flex items-center justify-between gap-3">
                  <div className="text-sm font-medium text-text">Lane {index + 1}</div>
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() =>
                      updateBlueprint({
                        workstreams: blueprint.workstreams.filter(
                          (row) => row.workstream_id !== workstream.workstream_id
                        ),
                      })
                    }
                  >
                    Remove
                  </Button>
                </div>
                <div className="mt-3 grid gap-3 lg:grid-cols-2">
                  <Input
                    label="Title"
                    value={workstream.title}
                    onChange={(event) =>
                      updateWorkstream(workstream.workstream_id, { title: event.target.value })
                    }
                  />
                  <Input
                    label="Workstream ID"
                    value={workstream.workstream_id}
                    onChange={(event) =>
                      updateWorkstream(workstream.workstream_id, {
                        workstream_id: event.target.value,
                      })
                    }
                  />
                </div>
                <div className="mt-3 grid gap-3 lg:grid-cols-4">
                  <Input
                    label="Role"
                    value={workstream.role}
                    onChange={(event) =>
                      updateWorkstream(workstream.workstream_id, { role: event.target.value })
                    }
                  />
                  <label className="block text-sm font-medium text-text">
                    Template
                    <select
                      value={workstream.template_id || ""}
                      onChange={(event) =>
                        updateWorkstream(workstream.workstream_id, {
                          template_id: event.target.value || undefined,
                        })
                      }
                      className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                    >
                      <option value="">None</option>
                      {templates.map((template) => (
                        <option key={template.template_id} value={template.template_id}>
                          {template.template_id} ({template.role})
                        </option>
                      ))}
                    </select>
                  </label>
                  <Input
                    label="Depends On"
                    value={(workstream.depends_on || []).join(", ")}
                    onChange={(event) =>
                      updateWorkstream(workstream.workstream_id, {
                        depends_on: splitCsv(event.target.value),
                      })
                    }
                  />
                </div>
                <div className="mt-3 grid gap-3 lg:grid-cols-4">
                  <Input
                    label="Priority"
                    type="number"
                    value={
                      workstream.priority === null || workstream.priority === undefined
                        ? ""
                        : String(workstream.priority)
                    }
                    onChange={(event) =>
                      updateWorkstream(workstream.workstream_id, {
                        priority: parseOptionalInt(event.target.value),
                      })
                    }
                  />
                  <Input
                    label="Phase ID"
                    value={workstream.phase_id || ""}
                    list={`phase-options-${workstream.workstream_id}`}
                    onChange={(event) =>
                      updateWorkstream(workstream.workstream_id, { phase_id: event.target.value })
                    }
                  />
                  <Input
                    label="Lane"
                    value={workstream.lane || ""}
                    onChange={(event) =>
                      updateWorkstream(workstream.workstream_id, { lane: event.target.value })
                    }
                  />
                  <Input
                    label="Milestone"
                    value={workstream.milestone || ""}
                    list={`milestone-options-${workstream.workstream_id}`}
                    onChange={(event) =>
                      updateWorkstream(workstream.workstream_id, { milestone: event.target.value })
                    }
                  />
                  <datalist id={`phase-options-${workstream.workstream_id}`}>
                    {phaseIds.map((phaseId) => (
                      <option key={phaseId} value={phaseId} />
                    ))}
                  </datalist>
                  <datalist id={`milestone-options-${workstream.workstream_id}`}>
                    {milestoneIds.map((milestoneId) => (
                      <option key={milestoneId} value={milestoneId} />
                    ))}
                  </datalist>
                </div>
                <div className="mt-3 grid gap-3 lg:grid-cols-2">
                  <label className="block text-sm font-medium text-text">
                    Model Provider
                    <select
                      value={modelDraft.provider}
                      onChange={(event) =>
                        setWorkstreamModels((current) => ({
                          ...current,
                          [workstream.workstream_id]: {
                            provider: event.target.value,
                            model: modelOptions(providers, event.target.value)[0] || "",
                          },
                        }))
                      }
                      className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                    >
                      <option value="">Team default</option>
                      {providers.map((provider) => (
                        <option key={provider.id} value={provider.id}>
                          {provider.id}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label className="block text-sm font-medium text-text">
                    Model
                    <select
                      value={modelDraft.model}
                      onChange={(event) =>
                        setWorkstreamModels((current) => ({
                          ...current,
                          [workstream.workstream_id]: {
                            provider: modelDraft.provider,
                            model: event.target.value,
                          },
                        }))
                      }
                      className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                    >
                      <option value="">Team default</option>
                      {modelOptions(providers, modelDraft.provider).map((modelId) => (
                        <option key={modelId} value={modelId}>
                          {modelId}
                        </option>
                      ))}
                    </select>
                  </label>
                </div>
                <div className="mt-3 grid gap-3 lg:grid-cols-2">
                  <Input
                    label="Output Contract"
                    value={workstream.output_contract.kind}
                    onChange={(event) =>
                      updateWorkstream(workstream.workstream_id, {
                        output_contract: {
                          ...workstream.output_contract,
                          kind: event.target.value,
                        },
                      })
                    }
                  />
                  <Input
                    label="Output Guidance"
                    value={workstream.output_contract.summary_guidance || ""}
                    onChange={(event) =>
                      updateWorkstream(workstream.workstream_id, {
                        output_contract: {
                          ...workstream.output_contract,
                          summary_guidance: event.target.value,
                        },
                      })
                    }
                  />
                </div>
                <div className="mt-3 grid gap-3 lg:grid-cols-2">
                  <ScopeSelector
                    label="Allowed Tools"
                    helper="Override the engine-default tool scope for this workstream."
                    options={toolIds}
                    selected={workstream.tool_allowlist_override || []}
                    inheritSummary="Inheriting engine-default tool policy."
                    emptySummary="No tool catalog loaded yet."
                    onChange={(next) =>
                      updateWorkstream(workstream.workstream_id, {
                        tool_allowlist_override: next,
                      })
                    }
                  />
                  <ScopeSelector
                    label="MCP Servers"
                    helper="Override the mission-level MCP selection for this workstream."
                    options={mcpServers.map((server) => server.name).sort()}
                    selected={workstream.mcp_servers_override || []}
                    inheritSummary={
                      (blueprint.team.allowed_mcp_servers || []).length
                        ? `Inheriting ${(blueprint.team.allowed_mcp_servers || []).length} mission MCP server(s).`
                        : "No mission-level MCP servers selected."
                    }
                    emptySummary="No MCP servers discovered."
                    onChange={(next) =>
                      updateWorkstream(workstream.workstream_id, {
                        mcp_servers_override: next,
                      })
                    }
                  />
                </div>
                <label className="mt-3 block text-sm font-medium text-text">
                  Objective
                  <textarea
                    value={workstream.objective}
                    onChange={(event) =>
                      updateWorkstream(workstream.workstream_id, { objective: event.target.value })
                    }
                    className="mt-2 min-h-[96px] w-full rounded-lg border border-border bg-surface px-3 py-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                  />
                </label>
                <label className="mt-3 block text-sm font-medium text-text">
                  Prompt
                  <textarea
                    value={workstream.prompt}
                    onChange={(event) =>
                      updateWorkstream(workstream.workstream_id, { prompt: event.target.value })
                    }
                    className="mt-2 min-h-[120px] w-full rounded-lg border border-border bg-surface px-3 py-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                  />
                </label>
              </div>
            );
          })}
        </div>
      </BuilderCard>

      <BuilderCard
        title="Review & Gates"
        subtitle="Reviewer, tester, and approval checkpoints."
        actions={
          <Button
            size="sm"
            variant="secondary"
            onClick={() =>
              updateBlueprint({
                review_stages: [
                  ...blueprint.review_stages,
                  newReviewStage(blueprint.review_stages.length + 1),
                ],
              })
            }
          >
            Add Stage
          </Button>
        }
      >
        <div className="space-y-4">
          {blueprint.review_stages.map((stage) => (
            <div
              key={stage.stage_id}
              className="rounded-lg border border-border bg-surface-elevated/40 p-3"
            >
              <div className="flex items-center justify-between gap-3">
                <div className="text-sm font-medium text-text">{stage.title || stage.stage_id}</div>
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() =>
                    updateBlueprint({
                      review_stages: blueprint.review_stages.filter(
                        (row) => row.stage_id !== stage.stage_id
                      ),
                    })
                  }
                >
                  Remove
                </Button>
              </div>
              <div className="mt-3 grid gap-3 lg:grid-cols-3">
                <Input
                  label="Stage ID"
                  value={stage.stage_id}
                  onChange={(event) =>
                    updateReviewStage(stage.stage_id, { stage_id: event.target.value })
                  }
                />
                <Input
                  label="Title"
                  value={stage.title}
                  onChange={(event) =>
                    updateReviewStage(stage.stage_id, { title: event.target.value })
                  }
                />
                <label className="block text-sm font-medium text-text">
                  Stage Kind
                  <select
                    value={stage.stage_kind}
                    onChange={(event) =>
                      updateReviewStage(stage.stage_id, {
                        stage_kind: event.target.value as MissionBuilderReviewStage["stage_kind"],
                      })
                    }
                    className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                  >
                    <option value="review">Review</option>
                    <option value="test">Test</option>
                    <option value="approval">Approval</option>
                  </select>
                </label>
              </div>
              <div className="mt-3 grid gap-3 lg:grid-cols-4">
                <Input
                  label="Target IDs"
                  value={stage.target_ids.join(", ")}
                  onChange={(event) =>
                    updateReviewStage(stage.stage_id, { target_ids: splitCsv(event.target.value) })
                  }
                />
                <Input
                  label="Role"
                  value={stage.role || ""}
                  onChange={(event) =>
                    updateReviewStage(stage.stage_id, { role: event.target.value })
                  }
                />
                <label className="block text-sm font-medium text-text">
                  Template
                  <select
                    value={stage.template_id || ""}
                    onChange={(event) =>
                      updateReviewStage(stage.stage_id, {
                        template_id: event.target.value || undefined,
                      })
                    }
                    className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                  >
                    <option value="">None</option>
                    {templates.map((template) => (
                      <option key={template.template_id} value={template.template_id}>
                        {template.template_id} ({template.role})
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              <div className="mt-3 grid gap-3 lg:grid-cols-2">
                <ScopeSelector
                  label="Allowed Tools"
                  helper="Override the engine-default tool scope for this checkpoint stage."
                  options={toolIds}
                  selected={stage.tool_allowlist_override || []}
                  inheritSummary="Inheriting engine-default tool policy."
                  emptySummary="No tool catalog loaded yet."
                  onChange={(next) =>
                    updateReviewStage(stage.stage_id, {
                      tool_allowlist_override: next,
                    })
                  }
                />
                <ScopeSelector
                  label="MCP Servers"
                  helper="Override the mission-level MCP selection for this checkpoint stage."
                  options={mcpServers.map((server) => server.name).sort()}
                  selected={stage.mcp_servers_override || []}
                  inheritSummary={
                    (blueprint.team.allowed_mcp_servers || []).length
                      ? `Inheriting ${(blueprint.team.allowed_mcp_servers || []).length} mission MCP server(s).`
                      : "No mission-level MCP servers selected."
                  }
                  emptySummary="No MCP servers discovered."
                  onChange={(next) =>
                    updateReviewStage(stage.stage_id, {
                      mcp_servers_override: next,
                    })
                  }
                />
              </div>
              <div className="mt-3 grid gap-3 lg:grid-cols-4">
                <Input
                  label="Priority"
                  type="number"
                  value={
                    stage.priority === null || stage.priority === undefined
                      ? ""
                      : String(stage.priority)
                  }
                  onChange={(event) =>
                    updateReviewStage(stage.stage_id, {
                      priority: parseOptionalInt(event.target.value),
                    })
                  }
                />
                <Input
                  label="Phase ID"
                  value={stage.phase_id || ""}
                  list={`review-phase-options-${stage.stage_id}`}
                  onChange={(event) =>
                    updateReviewStage(stage.stage_id, { phase_id: event.target.value })
                  }
                />
                <Input
                  label="Lane"
                  value={stage.lane || ""}
                  onChange={(event) =>
                    updateReviewStage(stage.stage_id, { lane: event.target.value })
                  }
                />
                <Input
                  label="Milestone"
                  value={stage.milestone || ""}
                  list={`review-milestone-options-${stage.stage_id}`}
                  onChange={(event) =>
                    updateReviewStage(stage.stage_id, { milestone: event.target.value })
                  }
                />
                <datalist id={`review-phase-options-${stage.stage_id}`}>
                  {phaseIds.map((phaseId) => (
                    <option key={phaseId} value={phaseId} />
                  ))}
                </datalist>
                <datalist id={`review-milestone-options-${stage.stage_id}`}>
                  {milestoneIds.map((milestoneId) => (
                    <option key={milestoneId} value={milestoneId} />
                  ))}
                </datalist>
              </div>
              <label className="mt-3 block text-sm font-medium text-text">
                Prompt / Instructions
                <textarea
                  value={stage.prompt}
                  onChange={(event) =>
                    updateReviewStage(stage.stage_id, { prompt: event.target.value })
                  }
                  className="mt-2 min-h-[96px] w-full rounded-lg border border-border bg-surface px-3 py-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                />
              </label>
              {stage.stage_kind === "approval" ? (
                <div className="mt-3 grid gap-3 lg:grid-cols-2">
                  <Input
                    label="Rework Targets"
                    value={stage.gate?.rework_targets?.join(", ") || ""}
                    onChange={(event) =>
                      updateReviewStage(stage.stage_id, {
                        gate: {
                          required: true,
                          decisions: ["approve", "rework", "cancel"],
                          instructions: stage.gate?.instructions || "",
                          rework_targets: splitCsv(event.target.value),
                        },
                      })
                    }
                  />
                  <Input
                    label="Gate Instructions"
                    value={stage.gate?.instructions || ""}
                    onChange={(event) =>
                      updateReviewStage(stage.stage_id, {
                        gate: {
                          required: true,
                          decisions: ["approve", "rework", "cancel"],
                          instructions: event.target.value,
                          rework_targets: stage.gate?.rework_targets || [],
                        },
                      })
                    }
                  />
                </div>
              ) : null}
            </div>
          ))}
          {!blueprint.review_stages.length ? (
            <div className="rounded-lg border border-border bg-surface px-3 py-4 text-sm text-text-muted">
              No review or approval stages configured yet.
            </div>
          ) : null}
        </div>
      </BuilderCard>

      <BuilderCard
        title="Compile"
        subtitle={
          editingAutomation?.automation_id
            ? "Validate the graph, inspect the compiled plan, then save changes back into this automation."
            : "Validate the graph, inspect the compiled plan, then create the draft."
        }
        actions={
          <div className="flex gap-2">
            <Button
              size="sm"
              variant="secondary"
              loading={busyKey === "preview"}
              onClick={() => void compilePreview()}
            >
              Compile Preview
            </Button>
            <Button
              size="sm"
              variant="primary"
              loading={busyKey === "apply"}
              onClick={() => void createDraft()}
            >
              {editingAutomation?.automation_id ? "Save Automation" : "Create Draft"}
            </Button>
            {editingAutomation?.automation_id && onClearEditing ? (
              <Button size="sm" variant="secondary" onClick={onClearEditing}>
                Cancel Edit
              </Button>
            ) : null}
          </div>
        }
      >
        {!editingAutomation?.automation_id ? (
          <label className="inline-flex items-center gap-2 text-sm text-text-muted">
            <input
              type="checkbox"
              checked={runAfterCreate}
              onChange={(event) => setRunAfterCreate(event.target.checked)}
            />
            Run immediately after draft creation
          </label>
        ) : (
          <div className="text-sm text-text-muted">
            Editing existing automation: {editingAutomation.name || editingAutomation.automation_id}
          </div>
        )}
        {preview ? (
          <div className="mt-4 grid gap-4 lg:grid-cols-[1.1fr_0.9fr]">
            <div className="space-y-3">
              <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                <div className="text-sm font-medium text-text">Validation</div>
                <div className="mt-2 space-y-2">
                  {preview.validation.map((message, index) => (
                    <div
                      key={`${message.code}-${index}`}
                      className={`rounded-lg px-3 py-2 text-sm ${
                        message.severity === "error"
                          ? "border border-red-500/40 bg-red-500/10 text-red-200"
                          : message.severity === "warning"
                            ? "border border-amber-500/40 bg-amber-500/10 text-amber-200"
                            : "border border-border bg-surface text-text-muted"
                      }`}
                    >
                      <div className="font-medium">{message.code}</div>
                      <div>{message.message}</div>
                    </div>
                  ))}
                  {!preview.validation.length ? (
                    <div className="text-sm text-text-muted">No validation issues.</div>
                  ) : null}
                </div>
              </div>
              <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                <div className="text-sm font-medium text-text">Compiled Nodes</div>
                <div className="mt-3 space-y-2">
                  {preview.node_previews.map((node) => (
                    <div
                      key={node.node_id}
                      className="rounded-lg border border-border bg-surface px-3 py-2"
                    >
                      <div className="flex items-center justify-between gap-3">
                        <div className="text-sm font-medium text-text">{node.title}</div>
                        <span className="text-[10px] uppercase tracking-wide text-text-subtle">
                          {node.stage_kind}
                        </span>
                      </div>
                      <div className="mt-1 text-xs text-text-muted">Agent: {node.agent_id}</div>
                      <div className="mt-1 text-xs text-text-muted">
                        Phase: {node.phase_id || "unassigned"} | Priority: {node.priority ?? 0}
                      </div>
                      <div className="mt-1 text-xs text-text-muted">
                        Lane: {node.lane || "none"} | Milestone: {node.milestone || "none"}
                      </div>
                      {node.depends_on.length ? (
                        <div className="mt-1 text-xs text-text-muted">
                          Depends on: {node.depends_on.join(", ")}
                        </div>
                      ) : null}
                      <div className="mt-1 text-xs text-text-muted">
                        Tools:{" "}
                        {node.tool_allowlist.length
                          ? node.tool_allowlist.join(", ")
                          : "engine default"}
                      </div>
                      <div className="mt-1 text-xs text-text-muted">
                        MCP: {node.mcp_servers.length ? node.mcp_servers.join(", ") : "none"}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
              <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                <div className="text-sm font-medium text-text">Graph Preview</div>
                <div className="mt-1 text-xs text-text-muted">
                  Grouped by phase so fan-out, fan-in, and promotion shape are visible before
                  launch.
                </div>
                {previewGraphSummary ? (
                  <div className="mt-3 grid gap-2 sm:grid-cols-3 xl:grid-cols-6">
                    {[
                      ["roots", previewGraphSummary.rootCount],
                      ["terminals", previewGraphSummary.terminalCount],
                      ["cross-lane", previewGraphSummary.crossLaneEdges],
                      ["cross-phase", previewGraphSummary.crossPhaseEdges],
                      ["high fan-in", previewGraphSummary.highFanIn],
                      ["high fan-out", previewGraphSummary.highFanOut],
                    ].map(([label, value]) => (
                      <div
                        key={String(label)}
                        className="rounded-lg border border-border bg-surface px-3 py-2"
                      >
                        <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                          {label}
                        </div>
                        <div className="mt-1 text-sm font-medium text-text">{value}</div>
                      </div>
                    ))}
                  </div>
                ) : null}
                <div className="mt-3 grid gap-3 xl:grid-cols-3">
                  {previewGraphColumns.map((column) => (
                    <div
                      key={column.phaseId}
                      className="rounded-lg border border-border bg-surface px-3 py-3"
                    >
                      <div className="flex items-center justify-between gap-2">
                        <div className="text-sm font-medium text-text">{column.title}</div>
                        <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                          {column.executionMode}
                        </div>
                      </div>
                      <div className="mt-3 space-y-2">
                        {column.lanes.map((lane) => (
                          <div
                            key={`${column.phaseId}-${lane.laneId}`}
                            className="rounded-lg border border-border bg-surface-elevated/20 p-2"
                          >
                            <div className="mb-2 flex items-center justify-between gap-2">
                              <div>
                                <div className="text-xs font-medium uppercase tracking-wide text-text-subtle">
                                  {lane.title}
                                </div>
                                <div className="mt-1 flex flex-wrap gap-2 text-[10px] uppercase tracking-wide text-text-subtle">
                                  <span>
                                    {lane.nodes.length} node{lane.nodes.length === 1 ? "" : "s"}
                                  </span>
                                  <span>in {lane.inboundHandoffs}</span>
                                  <span>out {lane.outboundHandoffs}</span>
                                </div>
                              </div>
                            </div>
                            <div className="space-y-2">
                              {lane.nodes.map((node) => (
                                <div
                                  key={`${column.phaseId}-${lane.laneId}-${node.node_id}`}
                                  className="rounded-lg border border-border bg-surface px-3 py-2"
                                >
                                  <div className="flex items-center justify-between gap-2">
                                    <div className="text-sm font-medium text-text">
                                      {node.title}
                                    </div>
                                    <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                                      p{node.priority ?? 0}
                                    </div>
                                  </div>
                                  <div className="mt-1 text-xs text-text-subtle">
                                    {node.node_id}
                                    {node.milestone ? ` | milestone ${node.milestone}` : ""}
                                  </div>
                                  <div className="mt-2 flex flex-wrap gap-2 text-[11px] text-text-muted">
                                    <span className="rounded border border-border px-2 py-1">
                                      fan-in {node.depends_on.length}
                                    </span>
                                    <span className="rounded border border-border px-2 py-1">
                                      fan-out {node.downstreamCount}
                                    </span>
                                    <span className="rounded border border-border px-2 py-1">
                                      {node.depends_on.length ? "dependent" : "root"}
                                    </span>
                                  </div>
                                  <div className="mt-2 grid gap-2 lg:grid-cols-2">
                                    <div className="rounded border border-border bg-surface-elevated/30 px-2 py-2">
                                      <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                                        Upstream
                                      </div>
                                      <div className="mt-2 flex flex-wrap gap-2">
                                        {node.dependencyEdges.length ? (
                                          node.dependencyEdges.map((edge) => (
                                            <span
                                              key={`${node.node_id}-dep-${edge.nodeId}`}
                                              className={`rounded border px-2 py-1 text-[11px] ${
                                                edge.crossPhase
                                                  ? "border-amber-500/40 bg-amber-500/10 text-amber-200"
                                                  : edge.crossLane
                                                    ? "border-sky-500/40 bg-sky-500/10 text-sky-200"
                                                    : "border-border bg-surface text-text-muted"
                                              }`}
                                            >
                                              {edge.title}
                                            </span>
                                          ))
                                        ) : (
                                          <span className="text-[11px] text-emerald-300">root</span>
                                        )}
                                      </div>
                                    </div>
                                    <div className="rounded border border-border bg-surface-elevated/30 px-2 py-2">
                                      <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                                        Downstream
                                      </div>
                                      <div className="mt-2 flex flex-wrap gap-2">
                                        {node.downstreamEdges.length ? (
                                          node.downstreamEdges.map((edge) => (
                                            <span
                                              key={`${node.node_id}-out-${edge.nodeId}`}
                                              className={`rounded border px-2 py-1 text-[11px] ${
                                                edge.crossPhase
                                                  ? "border-amber-500/40 bg-amber-500/10 text-amber-200"
                                                  : edge.crossLane
                                                    ? "border-sky-500/40 bg-sky-500/10 text-sky-200"
                                                    : "border-border bg-surface text-text-muted"
                                              }`}
                                            >
                                              {edge.title}
                                            </span>
                                          ))
                                        ) : (
                                          <span className="text-[11px] text-text-muted">
                                            terminal
                                          </span>
                                        )}
                                      </div>
                                    </div>
                                  </div>
                                </div>
                              ))}
                            </div>
                          </div>
                        ))}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
              <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                <div className="text-sm font-medium text-text">Compiled Mission Spec</div>
                <div className="mt-3 grid gap-2 text-xs text-text-muted sm:grid-cols-2">
                  <div>Mission ID: {preview.mission_spec?.mission_id || "—"}</div>
                  <div>Entrypoint: {preview.mission_spec?.entrypoint || "—"}</div>
                  <div>Title: {preview.mission_spec?.title || "—"}</div>
                  <div>Goal: {preview.mission_spec?.goal || "—"}</div>
                  <div>Phases: {(effectiveBlueprint.phases || []).length}</div>
                  <div>Milestones: {(effectiveBlueprint.milestones || []).length}</div>
                </div>
                {preview.mission_spec?.success_criteria?.length ? (
                  <div className="mt-3 flex flex-wrap gap-2">
                    {preview.mission_spec.success_criteria.map((item, index) => (
                      <span
                        key={`${item}-${index}`}
                        className="rounded-full border border-border bg-surface px-2 py-1 text-[11px] text-text-muted"
                      >
                        {item}
                      </span>
                    ))}
                  </div>
                ) : null}
              </div>
              <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                <div className="text-sm font-medium text-text">Compiled Work Items</div>
                <div className="mt-3 space-y-2">
                  {preview.work_items.length ? (
                    preview.work_items.map((item) => {
                      const metadata =
                        (item.metadata as Record<string, unknown> | null | undefined) || null;
                      return (
                        <div
                          key={item.work_item_id}
                          className="rounded-lg border border-border bg-surface px-3 py-2"
                        >
                          <div className="flex items-center justify-between gap-3">
                            <div className="text-sm font-medium text-text">
                              {item.title || item.work_item_id}
                            </div>
                            <span className="text-[10px] uppercase tracking-wide text-text-subtle">
                              {item.status}
                            </span>
                          </div>
                          <div className="mt-1 text-xs text-text-muted">
                            Work item: {item.work_item_id}
                          </div>
                          {item.detail ? (
                            <div className="mt-1 text-xs text-text-muted">{item.detail}</div>
                          ) : null}
                          <div className="mt-1 text-xs text-text-muted">
                            Assigned agent: {item.assigned_agent || "—"}
                          </div>
                          <div className="mt-1 text-xs text-text-muted">
                            Depends on:{" "}
                            {item.depends_on?.length ? item.depends_on.join(", ") : "none"}
                          </div>
                          {metadata ? (
                            <div className="mt-2 flex flex-wrap gap-2 text-[11px] text-text-subtle">
                              <span className="rounded border border-border px-2 py-1">
                                phase {String(metadata.phase_id || "—")}
                              </span>
                              <span className="rounded border border-border px-2 py-1">
                                lane {String(metadata.lane || "—")}
                              </span>
                              <span className="rounded border border-border px-2 py-1">
                                milestone {String(metadata.milestone || "—")}
                              </span>
                              <span className="rounded border border-border px-2 py-1">
                                stage {String(metadata.stage_kind || "—")}
                              </span>
                            </div>
                          ) : null}
                        </div>
                      );
                    })
                  ) : (
                    <div className="text-sm text-text-muted">No compiled work items returned.</div>
                  )}
                </div>
              </div>
            </div>
            <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
              <div className="text-sm font-medium text-text">Mission Brief Preview</div>
              <pre className="mt-3 overflow-x-auto whitespace-pre-wrap text-xs text-text-muted">
                {preview.node_previews[0]?.inherited_brief ||
                  "Compile preview to inspect the inherited brief."}
              </pre>
              <div className="mt-4 text-sm font-medium text-text">Available Stage IDs</div>
              <div className="mt-2 flex flex-wrap gap-2">
                {allStageIds.map((id) => (
                  <span
                    key={id}
                    className="rounded border border-border bg-surface px-2 py-1 text-xs text-text-muted"
                  >
                    {id}
                  </span>
                ))}
              </div>
              <div className="mt-4 text-sm font-medium text-text">Configured Phases</div>
              <div className="mt-2 flex flex-wrap gap-2">
                {(effectiveBlueprint.phases || []).map((phase) => (
                  <span
                    key={phase.phase_id}
                    className="rounded border border-border bg-surface px-2 py-1 text-xs text-text-muted"
                  >
                    {phase.phase_id} ({phase.execution_mode || "soft"})
                  </span>
                ))}
                {!(effectiveBlueprint.phases || []).length ? (
                  <span className="text-xs text-text-muted">No phases configured.</span>
                ) : null}
              </div>
            </div>
          </div>
        ) : (
          <div className="mt-4 rounded-lg border border-border bg-surface px-3 py-6 text-center text-sm text-text-muted">
            Compile the mission to inspect validation, graph shape, and inherited briefing.
          </div>
        )}
      </BuilderCard>
    </div>
  );
}
