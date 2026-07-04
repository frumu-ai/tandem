import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../lib/api";
import { EmptyState } from "./ui";
import { ProviderModelSelector } from "../components/ProviderModelSelector";
import { ScheduleBuilder } from "../features/automations/ScheduleBuilder";
import type { ScheduleValue } from "../features/automations/scheduleValue";
import { buildPlannerProviderOptions } from "../features/planner/plannerShared";
import { TimezoneField } from "../features/automations/TimezoneField";
import { isValidTimezone } from "../features/automations/timezone";
import { Icon } from "../ui/Icon";

type ProviderOption = {
  id: string;
  models: string[];
  configured?: boolean;
};

type ModelDraft = {
  provider: string;
  model: string;
};

type StandupTemplateOption = {
  templateId: string;
  displayName: string;
  role: string;
  modelLabel: string;
};

const RUN_ONCE_MISFIRE_POLICY = { type: "run_once" as const };

function validateWorkspaceRootInput(raw: string) {
  const value = String(raw || "").trim();
  if (!value) return "Workspace root is required.";
  if (!value.startsWith("/")) return "Workspace root must be an absolute path.";
  return "";
}

function formatAutomationV2ScheduleLabel(schedule: any) {
  const type = String(schedule?.type || "")
    .trim()
    .toLowerCase();
  if (type === "cron") {
    return String(schedule?.cron_expression || schedule?.cronExpression || "cron");
  }
  if (type === "interval") {
    const seconds = Number(schedule?.interval_seconds || schedule?.intervalSeconds || 0);
    if (!Number.isFinite(seconds) || seconds <= 0) return "interval";
    if (seconds % 3600 === 0) return `Every ${seconds / 3600}h`;
    if (seconds % 60 === 0) return `Every ${seconds / 60}m`;
    return `Every ${seconds}s`;
  }
  return "manual";
}

function standupScheduleToAutomationSchedule(scheduleValue: ScheduleValue, timezone: string) {
  const effectiveTimezone = String(timezone || "").trim() || "UTC";
  if (scheduleValue.scheduleKind === "interval") {
    return {
      type: "interval",
      interval_seconds: Math.max(
        1,
        Number.parseInt(String(scheduleValue.intervalSeconds || "3600"), 10) || 3600
      ),
      timezone: effectiveTimezone,
      misfire_policy: RUN_ONCE_MISFIRE_POLICY,
    };
  }
  if (scheduleValue.scheduleKind === "cron") {
    return {
      type: "cron",
      cron_expression: String(scheduleValue.cronExpression || "").trim() || "0 9 * * *",
      timezone: effectiveTimezone,
      misfire_policy: RUN_ONCE_MISFIRE_POLICY,
    };
  }
  return { type: "manual", timezone: effectiveTimezone, misfire_policy: RUN_ONCE_MISFIRE_POLICY };
}

function buildStandupModelPolicy(draft: ModelDraft) {
  const provider = String(draft.provider || "").trim();
  const model = String(draft.model || "").trim();
  if (!provider || !model) return null;
  return {
    default_model: {
      provider_id: provider,
      model_id: model,
    },
  };
}

function resolveDefaultStandupModel(
  providerOptions: ProviderOption[],
  providersConfig: any
): ModelDraft {
  const configuredProvider = String(providersConfig?.default || "").trim();
  const provider =
    providerOptions.find((entry) => entry.id === configuredProvider)?.id ||
    providerOptions[0]?.id ||
    "";
  if (!provider) return { provider: "", model: "" };
  const models = providerOptions.find((entry) => entry.id === provider)?.models || [];
  const model = String(
    providersConfig?.providers?.[provider]?.default_model || models[0] || ""
  ).trim();
  return { provider, model };
}

export function AgentStandupBuilder({
  client,
  toast,
  workspaceRoot,
  onWorkspaceRootChange,
  defaultWorkspaceRoot,
  templates,
  timezone,
  onTimezoneChange,
}: {
  client: any;
  toast: (kind: "ok" | "info" | "warn" | "err", text: string) => void;
  workspaceRoot: string;
  onWorkspaceRootChange: (value: string) => void;
  defaultWorkspaceRoot: string;
  templates: StandupTemplateOption[];
  timezone: string;
  onTimezoneChange: (value: string) => void;
}) {
  const queryClient = useQueryClient();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [name, setName] = useState("Daily Engineering Standup");
  const [workspaceBrowserOpen, setWorkspaceBrowserOpen] = useState(false);
  const [workspaceBrowserDir, setWorkspaceBrowserDir] = useState("");
  const [workspaceBrowserSearch, setWorkspaceBrowserSearch] = useState("");
  const [scheduleValue, setScheduleValue] = useState<ScheduleValue>({
    scheduleKind: "cron",
    cronExpression: "0 9 * * *",
    intervalSeconds: "3600",
  });
  const [reportPathTemplate, setReportPathTemplate] = useState("docs/standups/{{date}}.md");
  const [participantTemplateIds, setParticipantTemplateIds] = useState<string[]>([]);
  const [modelDraft, setModelDraft] = useState<ModelDraft>({ provider: "", model: "" });
  const [preview, setPreview] = useState<any>(null);
  const initializedModelRef = useRef(false);
  const providersCatalogQuery = useQuery({
    queryKey: ["providers", "catalog"],
    queryFn: () =>
      client?.providers?.catalog?.().catch(() => ({ all: [] })) ?? Promise.resolve({ all: [] }),
    refetchInterval: 60000,
  });
  const providersConfigQuery = useQuery({
    queryKey: ["providers", "config"],
    queryFn: () => client?.providers?.config?.().catch(() => ({})) ?? Promise.resolve({}),
    refetchInterval: 60000,
  });
  const workspaceBrowserQuery = useQuery({
    queryKey: ["teams", "standup", "workspace-browser", workspaceBrowserDir],
    enabled: workspaceBrowserOpen && !!workspaceBrowserDir,
    queryFn: () =>
      api(`/api/orchestrator/workspaces/list?dir=${encodeURIComponent(workspaceBrowserDir)}`, {
        method: "GET",
      }),
    refetchInterval: workspaceBrowserOpen ? 15000 : false,
  });

  const workspaceDirectories = Array.isArray((workspaceBrowserQuery.data as any)?.directories)
    ? (workspaceBrowserQuery.data as any).directories
    : [];
  const workspaceParentDir = String((workspaceBrowserQuery.data as any)?.parent || "").trim();
  const workspaceCurrentBrowseDir = String(
    (workspaceBrowserQuery.data as any)?.dir || workspaceBrowserDir || ""
  ).trim();
  const workspaceSearchQuery = String(workspaceBrowserSearch || "")
    .trim()
    .toLowerCase();
  const filteredWorkspaceDirectories = useMemo(() => {
    if (!workspaceSearchQuery) return workspaceDirectories;
    return workspaceDirectories.filter((entry: any) => {
      const name = String(entry?.name || entry?.path || "")
        .trim()
        .toLowerCase();
      return name.includes(workspaceSearchQuery);
    });
  }, [workspaceDirectories, workspaceSearchQuery]);
  const timezoneError =
    String(timezone || "").trim().length > 0 && !isValidTimezone(timezone)
      ? "Timezone must be a valid IANA timezone like Europe/Berlin."
      : "";
  const providerOptions = useMemo<ProviderOption[]>(() => {
    return buildPlannerProviderOptions({
      providerCatalog: providersCatalogQuery.data,
      providerConfig: providersConfigQuery.data,
      defaultProvider: String(providersConfigQuery.data?.default || "").trim(),
      defaultModel: String(
        providersConfigQuery.data?.providers?.[
          String(providersConfigQuery.data?.default || "").trim()
        ]?.default_model ||
          providersConfigQuery.data?.providers?.[
            String(providersConfigQuery.data?.default || "").trim()
          ]?.defaultModel ||
          ""
      ).trim(),
    });
  }, [providersCatalogQuery.data, providersConfigQuery.data]);
  const defaultStandupModel = useMemo(
    () => resolveDefaultStandupModel(providerOptions, providersConfigQuery.data),
    [providerOptions, providersConfigQuery.data]
  );
  const standupModelPolicy = useMemo(() => buildStandupModelPolicy(modelDraft), [modelDraft]);
  const standupModelLabel = standupModelPolicy
    ? `${String(modelDraft.provider || "").trim()}/${String(modelDraft.model || "").trim()}`
    : "not selected";

  useEffect(() => {
    const allowed = new Set(templates.map((template) => template.templateId));
    setParticipantTemplateIds((current) => {
      const next = current.filter((templateId) => allowed.has(templateId));
      return next.length === current.length ? current : next;
    });
  }, [templates]);

  useEffect(() => {
    if (initializedModelRef.current) return;
    if (!defaultStandupModel.provider || !defaultStandupModel.model) return;
    setModelDraft(defaultStandupModel);
    initializedModelRef.current = true;
  }, [defaultStandupModel]);

  const composeMutation = useMutation({
    mutationFn: async () => {
      const trimmedName = String(name || "").trim();
      const trimmedWorkspaceRoot = String(workspaceRoot || "").trim();
      if (!trimmedName) throw new Error("Standup name is required.");
      const workspaceError = validateWorkspaceRootInput(trimmedWorkspaceRoot);
      if (workspaceError) throw new Error(workspaceError);
      if (timezoneError) throw new Error(timezoneError);
      if (!participantTemplateIds.length) {
        throw new Error("Select at least one participant template.");
      }
      if (!standupModelPolicy) {
        throw new Error("Choose a provider and model for this standup.");
      }
      const response = await client?.agentTeams?.composeStandup?.({
        name: trimmedName,
        workspaceRoot: trimmedWorkspaceRoot,
        schedule: standupScheduleToAutomationSchedule(scheduleValue, timezone),
        participantTemplateIds,
        reportPathTemplate: String(reportPathTemplate || "").trim() || undefined,
        modelPolicy: standupModelPolicy,
      });
      return response || null;
    },
    onSuccess: (response) => {
      setPreview(response?.automation || null);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const createMutation = useMutation({
    mutationFn: async () => {
      const automation = preview || (await composeMutation.mutateAsync())?.automation;
      if (!automation) throw new Error("Standup compose failed.");
      return client?.automationsV2?.create?.(automation);
    },
    onSuccess: async () => {
      toast("ok", "Agent standup automation created.");
      setPreview(null);
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  return (
    <div
      ref={rootRef}
      className="grid gap-4 rounded-2xl border border-emerald-500/20 bg-emerald-500/5 p-4"
    >
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="text-xs font-medium uppercase tracking-[0.24em] text-emerald-300">
            Agent Standup
          </div>
          <h3 className="mt-1 text-lg font-semibold text-tcp-text-primary">
            Build a scheduled standup from saved agents
          </h3>
          <p className="mt-1 text-sm text-slate-300">
            Choose the personalities that should participate, preview the workflow, and create the
            automation from the same place you manage those agents.
          </p>
        </div>
        <span className="tcp-badge-ok">MVP</span>
      </div>

      <div className="grid gap-3 md:grid-cols-2">
        <input
          className="tcp-input"
          placeholder="Standup name"
          value={name}
          onInput={(event) => setName((event.target as HTMLInputElement).value)}
        />
        <div className="grid gap-2 md:grid-cols-[auto_1fr_auto]">
          <button
            className="tcp-btn h-10 px-3"
            type="button"
            onClick={() => {
              const seed = String(workspaceRoot || defaultWorkspaceRoot || "/").trim();
              setWorkspaceBrowserDir(seed || "/");
              setWorkspaceBrowserSearch("");
              setWorkspaceBrowserOpen(true);
            }}
          >
            <Icon name="folder-open" />
            Browse
          </button>
          <input
            className={`tcp-input ${validateWorkspaceRootInput(workspaceRoot) ? "border-red-500/60 text-red-100" : ""}`}
            placeholder="No local directory selected. Use Browse."
            value={workspaceRoot}
            readOnly
          />
          <button
            className="tcp-btn h-10 px-3"
            type="button"
            onClick={() => onWorkspaceRootChange("")}
            disabled={!workspaceRoot}
          >
            <Icon name="x" />
            Clear
          </button>
        </div>
        <div className="md:col-span-2">
          <TimezoneField
            value={timezone}
            onChange={onTimezoneChange}
            error={timezoneError}
            label="Timezone"
            hint="Use the timezone that matches the standup's local morning or evening."
          />
        </div>
        <div className="md:col-span-2">
          <ScheduleBuilder value={scheduleValue} onChange={setScheduleValue} timezone={timezone} />
        </div>
      </div>

      <div className="grid gap-3 rounded-2xl border border-slate-800/80 bg-slate-950/40 p-4">
        <div className="flex items-start justify-between gap-3">
          <div>
            <div className="text-xs font-medium uppercase tracking-[0.24em] text-slate-500">
              Model
            </div>
            <div className="mt-1 text-sm text-slate-300">
              Pick the explicit model every standup participant and the coordinator should use.
            </div>
          </div>
          <span className="tcp-badge-info">{standupModelLabel}</span>
        </div>
        <ProviderModelSelector
          providerLabel="Provider"
          modelLabel="Model"
          draft={modelDraft}
          providers={providerOptions}
          onChange={setModelDraft}
          inheritLabel="Select provider"
          disabled={composeMutation.isPending || createMutation.isPending}
        />
        <div className="text-xs text-slate-400">
          This is prefilled from the workspace default when available, then stored directly on the
          generated standup agents so the run does not depend on implicit model resolution.
        </div>
      </div>

      <input
        className="tcp-input font-mono text-sm"
        placeholder="docs/standups/{{date}}.md"
        value={reportPathTemplate}
        onInput={(event) => setReportPathTemplate((event.target as HTMLInputElement).value)}
      />

      <div className="rounded-2xl border border-slate-800/80 bg-slate-950/40 px-4 py-3 text-sm text-slate-300">
        The markdown report path controls where the synthesized standup is written. Participant
        personalities come from the saved agents below.
      </div>

      <div className="grid gap-2">
        <div className="text-xs font-medium uppercase tracking-[0.24em] text-slate-500">
          Participants
        </div>
        {templates.length ? (
          <div className="grid gap-2 md:grid-cols-2">
            {templates.map((template) => {
              const selected = participantTemplateIds.includes(template.templateId);
              return (
                <button
                  type="button"
                  key={template.templateId}
                  className={`tcp-list-item text-left transition-all ${
                    selected ? "border-emerald-400/60 bg-emerald-400/10" : ""
                  }`}
                  onClick={() =>
                    setParticipantTemplateIds((current) =>
                      current.includes(template.templateId)
                        ? current.filter((row) => row !== template.templateId)
                        : [...current, template.templateId]
                    )
                  }
                >
                  <div className="flex items-center justify-between gap-2">
                    <strong>{template.displayName}</strong>
                    <span className="tcp-badge-info">{template.role}</span>
                  </div>
                  <div className="tcp-subtle mt-1 text-xs">{template.templateId}</div>
                  {template.modelLabel ? (
                    <div className="mt-2 text-xs text-emerald-200">{template.modelLabel}</div>
                  ) : null}
                </button>
              );
            })}
          </div>
        ) : (
          <EmptyState text="Create at least one saved agent below before composing a standup." />
        )}
      </div>

      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          className="tcp-btn"
          disabled={composeMutation.isPending || !templates.length}
          onClick={() => composeMutation.mutate()}
        >
          <Icon name="file-search" />
          {composeMutation.isPending ? "Composing…" : "Preview Standup Workflow"}
        </button>
        <button
          type="button"
          className="tcp-btn-primary"
          disabled={createMutation.isPending || !templates.length}
          onClick={() => createMutation.mutate()}
        >
          <Icon name="rocket" />
          {createMutation.isPending ? "Creating…" : "Create Standup Automation"}
        </button>
      </div>

      {preview ? (
        <div className="rounded-xl border border-slate-700/50 bg-slate-950/40 p-3">
          <div className="mb-2 flex items-center justify-between gap-2">
            <strong>{String(preview?.name || "Standup preview")}</strong>
            <span className="tcp-badge-info">
              {Array.isArray(preview?.flow?.nodes) ? preview.flow.nodes.length : 0} nodes
            </span>
          </div>
          <div className="grid gap-2 text-xs text-slate-300">
            <div>schedule: {formatAutomationV2ScheduleLabel(preview?.schedule)}</div>
            <div>timezone: {String(preview?.schedule?.timezone || timezone || "UTC")}</div>
            <div>
              model:{" "}
              {String(
                preview?.agents?.[0]?.model_policy?.default_model?.provider_id &&
                  preview?.agents?.[0]?.model_policy?.default_model?.model_id
                  ? `${preview.agents[0].model_policy.default_model.provider_id}/${preview.agents[0].model_policy.default_model.model_id}`
                  : standupModelLabel
              )}
            </div>
            <div>
              report:{" "}
              {String(preview?.metadata?.standup?.report_path_template || reportPathTemplate)}
            </div>
            <div>
              participants:{" "}
              {String(
                (
                  preview?.metadata?.standup?.participant_template_ids || participantTemplateIds
                ).join(", ")
              )}
            </div>
          </div>
        </div>
      ) : null}

      {workspaceBrowserOpen ? (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
          <button
            type="button"
            className="tcp-confirm-backdrop"
            aria-label="Close workspace directory dialog"
            onClick={() => {
              setWorkspaceBrowserOpen(false);
              setWorkspaceBrowserSearch("");
            }}
          />
          <div className="tcp-confirm-dialog max-w-2xl">
            <h3 className="tcp-confirm-title">Select Workspace Folder</h3>
            <p className="tcp-confirm-message">
              Current: {workspaceCurrentBrowseDir || workspaceBrowserDir || "n/a"}
            </p>
            <div className="mb-2 flex flex-wrap gap-2">
              <button
                className="tcp-btn"
                type="button"
                onClick={() => {
                  if (!workspaceParentDir) return;
                  setWorkspaceBrowserDir(workspaceParentDir);
                }}
                disabled={!workspaceParentDir}
              >
                <Icon name="arrow-left-to-line" />
                Up
              </button>
              <button
                className="tcp-btn-primary"
                type="button"
                onClick={() => {
                  if (!workspaceCurrentBrowseDir) return;
                  onWorkspaceRootChange(workspaceCurrentBrowseDir);
                  setWorkspaceBrowserOpen(false);
                  setWorkspaceBrowserSearch("");
                  toast("ok", `Workspace selected: ${workspaceCurrentBrowseDir}`);
                }}
                disabled={!workspaceCurrentBrowseDir}
              >
                <Icon name="badge-check" />
                Select This Folder
              </button>
              <button
                className="tcp-btn"
                type="button"
                onClick={() => {
                  setWorkspaceBrowserOpen(false);
                  setWorkspaceBrowserSearch("");
                }}
              >
                <Icon name="x" />
                Close
              </button>
            </div>
            <div className="mb-2">
              <input
                className="tcp-input"
                placeholder="Type to filter folders..."
                value={workspaceBrowserSearch}
                onInput={(event) =>
                  setWorkspaceBrowserSearch((event.target as HTMLInputElement).value)
                }
              />
            </div>
            <div className="max-h-[360px] overflow-auto rounded-lg border border-slate-700/60 bg-slate-900/20 p-2">
              {filteredWorkspaceDirectories.length ? (
                filteredWorkspaceDirectories.map((entry: any) => (
                  <button
                    key={String(entry?.path || entry?.name)}
                    className="tcp-list-item mb-1 w-full text-left"
                    type="button"
                    onClick={() => setWorkspaceBrowserDir(String(entry?.path || ""))}
                  >
                    <span className="inline-flex items-center gap-2">
                      <Icon name="folder-open" />
                      <span>{String(entry?.name || entry?.path || "")}</span>
                    </span>
                  </button>
                ))
              ) : (
                <EmptyState
                  text={
                    workspaceBrowserSearch.trim()
                      ? "No folders match your search."
                      : "No subdirectories in this folder."
                  }
                />
              )}
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
