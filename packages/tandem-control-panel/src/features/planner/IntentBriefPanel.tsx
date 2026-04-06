import { useState, useEffect } from "react";
import { Badge } from "../../ui/index.tsx";
import { ProviderModelSelector } from "../../components/ProviderModelSelector";
import { WorkspaceDirectoryPicker } from "../../components/WorkspaceDirectoryPicker";
import {
  buildDefaultKnowledgeOperatorPreferences,
  buildKnowledgeRolloutGuidance,
} from "./plannerShared";
import { renderIcons } from "../../app/icons.js";

export type PlannerTargetSurface = "automation" | "mission" | "coding" | "orchestrator";
export type PlannerHorizon = "same_day" | "multi_day" | "weekly" | "monthly" | "mixed";

export type IntentBriefDraft = {
  goal: string;
  workspaceRoot: string;
  targetSurface: PlannerTargetSurface;
  planningHorizon: PlannerHorizon;
  outputExpectations: string;
  constraints: string;
  plannerProvider: string;
  plannerModel: string;
  selectedMcpServers: string[];
};

type ProviderOption = {
  id: string;
  models: string[];
  configured?: boolean;
};

const TARGET_SURFACE_OPTIONS: Array<{ id: PlannerTargetSurface; label: string; detail: string }> = [
  {
    id: "automation",
    label: "Workflow automation",
    detail: "Package the result into a governed automation draft.",
  },
  {
    id: "mission",
    label: "Mission execution",
    detail: "Shape a mission-style plan with multi-agent handoff and runtime review.",
  },
  {
    id: "coding",
    label: "Coding workflow",
    detail: "Turn intent into repo-aware work that can publish into coding flows.",
  },
  {
    id: "orchestrator",
    label: "Orchestrator",
    detail: "Prepare an execution-ready plan for orchestration and approvals.",
  },
];

const HORIZON_OPTIONS: Array<{ id: PlannerHorizon; label: string; detail: string }> = [
  {
    id: "same_day",
    label: "Same day",
    detail: "Split work into waves that should complete in a single day.",
  },
  {
    id: "multi_day",
    label: "Multi-day",
    detail: "Break the mission into a plan that spans several days.",
  },
  {
    id: "weekly",
    label: "Weekly recurring",
    detail: "Plan recurring work that should execute on a weekly cadence.",
  },
  {
    id: "monthly",
    label: "Monthly recurring",
    detail: "Plan recurring synthesis or review across a monthly cycle.",
  },
  {
    id: "mixed",
    label: "Mixed cadence",
    detail: "Blend one-time setup work with recurring follow-on work.",
  },
];

const INTENT_STARTERS = [
  "Plan a week-long multi-agent launch workflow",
  "Turn this outcome into a phased mission with approvals",
  "Break this into daily work waves and recurring follow-up",
];

function toggleSelected(values: string[], nextValue: string) {
  return values.includes(nextValue)
    ? values.filter((value) => value !== nextValue)
    : [...values, nextValue].sort((left, right) => left.localeCompare(right));
}

function safeString(value: unknown) {
  return String(value || "").trim();
}

export function IntentBriefPanel({
  draft,
  onChange,
  providerOptions,
  plannerCanUseLlm,
  basePlannerLabel,
  availableMcpServers,
  workspaceRootError = "",
  workspaceBrowserOpen,
  workspaceBrowserDir,
  workspaceBrowserSearch,
  workspaceBrowserParentDir,
  workspaceBrowserCurrentDir,
  workspaceBrowserDirectories,
  onOpenWorkspaceBrowser,
  onCloseWorkspaceBrowser,
  onClearWorkspaceRoot,
  onWorkspaceBrowserSearchChange,
  onWorkspaceBrowserParent,
  onWorkspaceBrowserDirectory,
  onSelectWorkspaceDirectory,
  onReset,
  disabled = false,
}: {
  draft: IntentBriefDraft;
  onChange: (next: IntentBriefDraft) => void;
  providerOptions: ProviderOption[];
  plannerCanUseLlm: boolean;
  basePlannerLabel: string;
  availableMcpServers: Array<{
    name: string;
    connected: boolean;
    transport?: string;
    lastError?: string;
  }>;
  workspaceRootError?: string;
  workspaceBrowserOpen: boolean;
  workspaceBrowserDir: string;
  workspaceBrowserSearch: string;
  workspaceBrowserParentDir: string;
  workspaceBrowserCurrentDir: string;
  workspaceBrowserDirectories: any[];
  onOpenWorkspaceBrowser: () => void;
  onCloseWorkspaceBrowser: () => void;
  onClearWorkspaceRoot: () => void;
  onWorkspaceBrowserSearchChange: (value: string) => void;
  onWorkspaceBrowserParent: () => void;
  onWorkspaceBrowserDirectory: (path: string) => void;
  onSelectWorkspaceDirectory: () => void;
  onReset: () => void;
  disabled?: boolean;
}) {
  const [showAdvanced, setShowAdvanced] = useState(false);

  useEffect(() => {
    try {
      renderIcons();
    } catch {}
  });

  const selectedTarget = TARGET_SURFACE_OPTIONS.find((option) => option.id === draft.targetSurface);
  const selectedHorizon = HORIZON_OPTIONS.find((option) => option.id === draft.planningHorizon);
  const knowledgeDefaults = buildDefaultKnowledgeOperatorPreferences(draft.goal).knowledge;
  const knowledgeRollout = buildKnowledgeRolloutGuidance(draft.goal).rollout;
  const knowledgeSubject = safeString(knowledgeDefaults.subject || draft.goal);

  return (
    <div className="grid gap-4 min-h-0">
      <div className="rounded-2xl border border-emerald-500/10 bg-emerald-500/5 p-3">
        <div className="flex flex-wrap items-center gap-2">
          <Badge tone="ok">planner workbench</Badge>
          <Badge tone={plannerCanUseLlm ? "ok" : "warn"}>
            {plannerCanUseLlm ? "model ready" : "model missing"}
          </Badge>
          <div className="flex-1" />
          <button
            type="button"
            className="tcp-btn h-6 px-2 text-[10px]"
            onClick={onReset}
            title="Start a new mission and clear the current planner state"
            disabled={disabled}
          >
            <i data-lucide="plus" className="mr-1 h-3 w-3"></i>
            New mission
          </button>
        </div>
        <p className="tcp-subtle mt-2 text-xs">
          The planner turns your intent into a structured workflow. Describe the full system —
          connected agents, schedules, and handoffs — and the planner will decompose it.
        </p>
      </div>

      <div className="rounded-2xl border border-cyan-500/10 bg-cyan-500/5 p-3">
        <div className="text-xs uppercase tracking-wide text-cyan-200/80">Knowledge defaults</div>
        <div className="mt-2 flex flex-wrap gap-2">
          <Badge tone="info">project scope</Badge>
          <Badge tone="info">preflight reuse</Badge>
          <Badge tone="info">promoted trust floor</Badge>
          {!knowledgeSubject ? <Badge tone="warn">subject inferred later</Badge> : null}
        </div>
        {knowledgeSubject ? (
          <div className="mt-3 rounded border border-white/5 bg-black/20 p-2 text-xs tcp-subtle break-words">
            <strong className="text-emerald-500/80 uppercase tracking-wide mr-1">Subject:</strong>
            <span className="line-clamp-3">{knowledgeSubject}</span>
          </div>
        ) : null}
        <p className="tcp-subtle mt-2 text-xs">
          The planner will start from project-scoped promoted knowledge and reuse prior work before
          it recomputes. Raw working notes stay local unless they are promoted later.
        </p>
      </div>

      <div className="rounded-2xl border border-amber-500/15 bg-amber-500/5 p-3">
        <div className="text-xs uppercase tracking-wide text-amber-200/80">Rollout guardrails</div>
        <div className="mt-2 flex flex-wrap gap-2">
          <Badge tone="warn">project-first pilot</Badge>
          <Badge tone="warn">promoted only</Badge>
          <Badge tone="warn">approved_default rare</Badge>
        </div>
        <ul className="tcp-subtle mt-2 space-y-1 text-xs">
          {knowledgeRollout.guardrails.map((item: string) => (
            <li key={item}>• {item}</li>
          ))}
        </ul>
      </div>

      <WorkspaceDirectoryPicker
        value={draft.workspaceRoot}
        error={workspaceRootError}
        open={workspaceBrowserOpen}
        browseDir={workspaceBrowserDir}
        search={workspaceBrowserSearch}
        parentDir={workspaceBrowserParentDir}
        currentDir={workspaceBrowserCurrentDir}
        directories={workspaceBrowserDirectories}
        helperText="This folder is required. The planner uses it to inspect the repo and generate the plan."
        onOpen={onOpenWorkspaceBrowser}
        onClose={onCloseWorkspaceBrowser}
        onClear={onClearWorkspaceRoot}
        onSearchChange={onWorkspaceBrowserSearchChange}
        onBrowseParent={onWorkspaceBrowserParent}
        onBrowseDirectory={onWorkspaceBrowserDirectory}
        onSelectDirectory={onSelectWorkspaceDirectory}
      />

      <label className="grid gap-2">
        <span className="text-xs uppercase tracking-wide text-slate-500">Intent</span>
        <textarea
          className="tcp-input min-h-32 text-sm"
          value={draft.goal}
          onInput={(event) =>
            onChange({ ...draft, goal: (event.target as HTMLTextAreaElement).value })
          }
          placeholder="Describe the long-horizon outcome you want, including what success looks like."
          disabled={disabled}
        />
        <div className="flex flex-wrap gap-2">
          {INTENT_STARTERS.map((starter) => (
            <button
              key={starter}
              type="button"
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() => onChange({ ...draft, goal: starter })}
              disabled={disabled}
            >
              {starter}
            </button>
          ))}
        </div>
        <span className="tcp-subtle text-xs">
          Keep this high-level. The planner should turn the goal into structure after the first
          draft.
        </span>
      </label>

      <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div>
            <div className="text-xs uppercase tracking-wide text-slate-500">Optional guidance</div>
            <div className="tcp-subtle mt-1 text-xs">
              Leave this collapsed if you want the planner to infer the setup from your intent.
            </div>
          </div>
          <button
            type="button"
            className="tcp-btn h-8 px-3 text-xs"
            onClick={() => setShowAdvanced((current) => !current)}
            disabled={disabled}
          >
            {showAdvanced ? "Hide guidance" : "Add guidance"}
          </button>
        </div>

        <div className="mt-3 flex flex-wrap gap-2">
          <span className="tcp-badge-info">{selectedTarget?.label || "Mission execution"}</span>
          <span className="tcp-badge-info">{selectedHorizon?.label || "Mixed cadence"}</span>
          <span className="tcp-badge-info">
            {draft.selectedMcpServers.length
              ? `${draft.selectedMcpServers.length} MCP source${
                  draft.selectedMcpServers.length === 1 ? "" : "s"
                }`
              : "all connected MCP sources"}
          </span>
        </div>

        {showAdvanced ? (
          <div className="mt-4 grid gap-4">
            <div className="grid gap-2">
              <div className="text-xs uppercase tracking-wide text-slate-500">Target surface</div>
              <select
                className="tcp-input text-sm"
                value={draft.targetSurface}
                onChange={(event) =>
                  onChange({ ...draft, targetSurface: event.target.value as PlannerTargetSurface })
                }
                disabled={disabled}
              >
                {TARGET_SURFACE_OPTIONS.map((option) => (
                  <option key={option.id} value={option.id}>
                    {option.label}
                  </option>
                ))}
              </select>
              <div className="tcp-subtle text-xs">{selectedTarget?.detail}</div>
            </div>

            <div className="grid gap-2">
              <div className="text-xs uppercase tracking-wide text-slate-500">Planning horizon</div>
              <select
                className="tcp-input text-sm"
                value={draft.planningHorizon}
                onChange={(event) =>
                  onChange({ ...draft, planningHorizon: event.target.value as PlannerHorizon })
                }
                disabled={disabled}
              >
                {HORIZON_OPTIONS.map((option) => (
                  <option key={option.id} value={option.id}>
                    {option.label}
                  </option>
                ))}
              </select>
              <div className="tcp-subtle text-xs">{selectedHorizon?.detail}</div>
            </div>

            <label className="grid gap-2">
              <span className="text-xs uppercase tracking-wide text-slate-500">
                Outputs or constraints
              </span>
              <textarea
                className="tcp-input min-h-28 text-sm"
                value={[draft.outputExpectations, draft.constraints].filter(Boolean).join("\n\n")}
                onInput={(event) =>
                  onChange({
                    ...draft,
                    outputExpectations: (event.target as HTMLTextAreaElement).value,
                    constraints: "",
                  })
                }
                placeholder="Optional: add anything the planner must produce or respect."
                disabled={disabled}
              />
            </label>

            <div className="rounded-xl border border-white/10 bg-black/20 p-3">
              <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
                <div>
                  <div className="text-xs uppercase tracking-wide text-slate-500">
                    Planner model
                  </div>
                  <div className="tcp-subtle text-xs">
                    Base model: {basePlannerLabel || "not configured"}
                  </div>
                </div>
              </div>
              <ProviderModelSelector
                providerLabel="Planner provider"
                modelLabel="Planner model"
                draft={{ provider: draft.plannerProvider, model: draft.plannerModel }}
                providers={providerOptions}
                onChange={({ provider, model }) =>
                  onChange({ ...draft, plannerProvider: provider, plannerModel: model })
                }
                inheritLabel="Workspace default"
                disabled={disabled}
              />
            </div>

            <div className="rounded-xl border border-white/10 bg-black/20 p-3">
              <div className="mb-3">
                <div className="text-xs uppercase tracking-wide text-slate-500">MCP sources</div>
                <div className="tcp-subtle mt-1 text-xs">
                  Optional: limit which connected systems the planner should reference.
                </div>
              </div>
              <div className="flex flex-wrap gap-2">
                {availableMcpServers.length ? (
                  availableMcpServers.map((server) => {
                    const selected = draft.selectedMcpServers.includes(server.name);
                    return (
                      <button
                        key={server.name}
                        type="button"
                        className={
                          selected ? "tcp-btn-primary h-8 px-3 text-xs" : "tcp-btn h-8 px-3 text-xs"
                        }
                        onClick={() =>
                          onChange({
                            ...draft,
                            selectedMcpServers: toggleSelected(
                              draft.selectedMcpServers,
                              server.name
                            ),
                          })
                        }
                        disabled={disabled || !server.connected}
                        title={server.lastError || server.transport || ""}
                      >
                        {server.name}
                      </button>
                    );
                  })
                ) : (
                  <div className="tcp-subtle text-xs">No MCP servers are currently connected.</div>
                )}
              </div>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
