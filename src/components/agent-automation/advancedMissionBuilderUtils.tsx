import {
  AutomationV2Spec,
  BuilderModelDraft,
  MissionBlueprint,
  MissionBuilderReviewStage,
  MissionBuilderWorkstream,
  ProviderInfo,
  UserProject,
} from "@/components/orchestrate/types";

import React, { useState, useMemo } from "react";
import { Button } from "@/components/ui/Button";

export function toModelDraft(policy: unknown): BuilderModelDraft {
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

export function workstreamModelDrafts(blueprint: MissionBlueprint) {
  const drafts: Record<string, BuilderModelDraft> = {};
  for (const workstream of blueprint.workstreams) {
    drafts[workstream.workstream_id] = toModelDraft(workstream.model_override || null);
  }
  return drafts;
}

export function newWorkstream(index: number): MissionBuilderWorkstream {
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

export function newReviewStage(index: number): MissionBuilderReviewStage {
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

export function newPhase(index: number) {
  return {
    phase_id: `phase_${index}`,
    title: `Phase ${index}`,
    description: "",
    execution_mode: (index === 1 ? "soft" : "barrier") as "soft" | "barrier",
  };
}

export function newMilestone(index: number) {
  return {
    milestone_id: `milestone_${index}`,
    title: `Milestone ${index}`,
    description: "",
    phase_id: "",
    required_stage_ids: [],
  };
}

export function defaultBlueprint(activeProject: UserProject | null): MissionBlueprint {
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

export function mergeJsonObjects(
  base: Record<string, unknown> | null | undefined,
  patch: Record<string, unknown> | null | undefined
): Record<string, unknown> | null {
  if (!base && !patch) return null;
  return {
    ...(base || {}),
    ...(patch || {}),
  };
}

export function normalizeWorkstream(
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

export function normalizeReviewStage(
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

export function normalizeMissionBlueprint(
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

export function extractMissionBlueprintFromAutomation(
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

export function baseBlueprintTeam(activeProject: UserProject | null): MissionBlueprint["team"] {
  return defaultBlueprint(activeProject).team;
}

export function toModelPolicy(draft: BuilderModelDraft) {
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

export function splitCsv(raw: string) {
  return String(raw || "")
    .split(",")
    .map((value) => value.trim())
    .filter(Boolean);
}

export function ScopeSelector({
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

export function parseOptionalInt(raw: string) {
  const trimmed = String(raw || "").trim();
  if (!trimmed) return undefined;
  const value = Number.parseInt(trimmed, 10);
  return Number.isFinite(value) && value > 0 ? value : undefined;
}

export function parseOptionalFloat(raw: string) {
  const trimmed = String(raw || "").trim();
  if (!trimmed) return undefined;
  const value = Number.parseFloat(trimmed);
  return Number.isFinite(value) && value > 0 ? value : undefined;
}

export function modelOptions(providers: ProviderInfo[], providerId: string) {
  return providers.find((provider) => provider.id === providerId)?.models ?? [];
}

export function BuilderCard({
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
