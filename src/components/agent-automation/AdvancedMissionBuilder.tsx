import { useEffect, useMemo, useState } from "react";
import { Button, Input } from "@/components/ui";
import {
  agentTeamListTemplates,
  automationsV2Update,
  automationsV2RunNow,
  missionBuilderApply,
  missionBuilderPreview,
  type McpServerRecord,
  type AutomationV2Spec,
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
  editingAutomation?: AutomationV2Spec | null;
  onRefreshAutomations: () => Promise<void>;
  onShowAutomations: () => void;
  onShowRuns: () => void;
  onClearEditing?: () => void;
  onOpenMcpExtensions?: () => void;
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
  editingAutomation = null,
  onRefreshAutomations,
  onShowAutomations,
  onShowRuns,
  onClearEditing,
  onOpenMcpExtensions,
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
    const metadata = (editingAutomation?.metadata as Record<string, unknown> | undefined) || {};
    const savedBlueprint =
      (metadata.mission_blueprint as MissionBlueprint | undefined) ||
      (metadata.missionBlueprint as MissionBlueprint | undefined) ||
      null;
    if (!editingAutomation?.automation_id || !savedBlueprint) {
      setBlueprint(defaultBlueprint(activeProject));
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

  const compilePreview = async () => {
    setBusyKey("preview");
    setError(null);
    try {
      const response = await missionBuilderPreview({
        blueprint: effectiveBlueprint,
        schedule: { type: "manual", timezone: "UTC", misfire_policy: "run_once" },
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
        schedule: { type: "manual", timezone: "UTC", misfire_policy: "run_once" },
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
                  <Input
                    label="Allowed Tools"
                    value={(workstream.tool_allowlist_override || []).join(", ")}
                    onChange={(event) =>
                      updateWorkstream(workstream.workstream_id, {
                        tool_allowlist_override: splitCsv(event.target.value),
                      })
                    }
                  />
                  <Input
                    label="MCP Servers"
                    value={(workstream.mcp_servers_override || []).join(", ")}
                    onChange={(event) =>
                      updateWorkstream(workstream.workstream_id, {
                        mcp_servers_override: splitCsv(event.target.value),
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
                <Input
                  label="Allowed Tools"
                  value={(stage.tool_allowlist_override || []).join(", ")}
                  onChange={(event) =>
                    updateReviewStage(stage.stage_id, {
                      tool_allowlist_override: splitCsv(event.target.value),
                    })
                  }
                />
                <Input
                  label="MCP Servers"
                  value={(stage.mcp_servers_override || []).join(", ")}
                  onChange={(event) =>
                    updateReviewStage(stage.stage_id, {
                      mcp_servers_override: splitCsv(event.target.value),
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
