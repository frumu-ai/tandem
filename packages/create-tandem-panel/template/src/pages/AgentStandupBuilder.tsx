import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { renderIcons } from "../app/icons.js";
import { EmptyState } from "./ui";

type StandupTemplateOption = {
  templateId: string;
  displayName: string;
  role: string;
  modelLabel: string;
};

const SCHEDULE_PRESETS = [
  { label: "Every hour", intervalSeconds: 3600 },
  { label: "Every morning", cron: "0 9 * * *" },
  { label: "Every evening", cron: "0 18 * * *" },
  { label: "Daily at midnight", cron: "0 0 * * *" },
  { label: "Weekly Monday", cron: "0 9 * * 1" },
  { label: "Manual only", cron: "" },
] as const;

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

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

function standupScheduleToAutomationSchedule(selectedPreset: string, customCron: string) {
  const trimmedCron = String(customCron || "").trim();
  if (trimmedCron) {
    return { type: "cron", cron_expression: trimmedCron, timezone: "UTC" };
  }
  const preset = SCHEDULE_PRESETS.find((row) => row.label === selectedPreset);
  if (preset?.intervalSeconds) {
    return { type: "interval", interval_seconds: preset.intervalSeconds };
  }
  if (preset?.cron) {
    return { type: "cron", cron_expression: preset.cron, timezone: "UTC" };
  }
  return { type: "manual" };
}

function normalizeStandupTemplateOption(row: any): StandupTemplateOption | null {
  const templateId = String(row?.template_id || row?.templateID || row?.id || "").trim();
  if (!templateId) return null;
  const displayName = String(row?.display_name || row?.displayName || row?.name || "").trim();
  const role = String(row?.role || "worker").trim() || "worker";
  const defaultModel = row?.default_model || row?.defaultModel || {};
  const provider = String(defaultModel?.provider_id || defaultModel?.providerId || "").trim();
  const modelId = String(defaultModel?.model_id || defaultModel?.modelId || "").trim();
  return {
    templateId,
    displayName: displayName || templateId,
    role,
    modelLabel: provider && modelId ? `${provider}/${modelId}` : "",
  };
}

export function AgentStandupBuilder({
  client,
  toast,
}: {
  client: any;
  toast: (kind: "ok" | "info" | "warn" | "err", text: string) => void;
}) {
  const queryClient = useQueryClient();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [name, setName] = useState("Daily Engineering Standup");
  const [workspaceRoot, setWorkspaceRoot] = useState("");
  const [schedulePreset, setSchedulePreset] = useState("Every morning");
  const [customCron, setCustomCron] = useState("");
  const [reportPathTemplate, setReportPathTemplate] = useState("docs/standups/{{date}}.md");
  const [participantTemplateIds, setParticipantTemplateIds] = useState<string[]>([]);
  const [preview, setPreview] = useState<any>(null);

  const templatesQuery = useQuery({
    queryKey: ["teams", "standup", "templates"],
    queryFn: () =>
      client?.agentTeams?.listTemplates?.().catch(() => ({ templates: [] })) ??
      Promise.resolve({ templates: [] }),
    refetchInterval: 12000,
  });
  const healthQuery = useQuery({
    queryKey: ["teams", "standup", "health"],
    queryFn: () => client?.health?.().catch(() => ({})) ?? Promise.resolve({}),
    refetchInterval: 30000,
  });

  useEffect(() => {
    const defaultWorkspaceRoot = String(
      (healthQuery.data as any)?.workspaceRoot || (healthQuery.data as any)?.workspace_root || ""
    ).trim();
    if (!defaultWorkspaceRoot) return;
    setWorkspaceRoot((current) => current || defaultWorkspaceRoot);
  }, [healthQuery.data]);

  const templates = useMemo(
    () =>
      toArray(templatesQuery.data, "templates")
        .map(normalizeStandupTemplateOption)
        .filter((row): row is StandupTemplateOption => !!row),
    [templatesQuery.data]
  );

  const composeMutation = useMutation({
    mutationFn: async () => {
      const trimmedName = String(name || "").trim();
      const trimmedWorkspaceRoot = String(workspaceRoot || "").trim();
      if (!trimmedName) throw new Error("Standup name is required.");
      const workspaceError = validateWorkspaceRootInput(trimmedWorkspaceRoot);
      if (workspaceError) throw new Error(workspaceError);
      if (!participantTemplateIds.length) {
        throw new Error("Select at least one participant template.");
      }
      const response = await client?.agentTeams?.composeStandup?.({
        name: trimmedName,
        workspaceRoot: trimmedWorkspaceRoot,
        schedule: standupScheduleToAutomationSchedule(schedulePreset, customCron),
        participantTemplateIds,
        reportPathTemplate: String(reportPathTemplate || "").trim() || undefined,
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

  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    renderIcons(root);
  }, [
    name,
    workspaceRoot,
    schedulePreset,
    customCron,
    reportPathTemplate,
    participantTemplateIds.join(","),
    templates.length,
    composeMutation.isPending,
    createMutation.isPending,
    !!preview,
  ]);

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
          <h3 className="mt-1 text-lg font-semibold text-white">
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
        <input
          className="tcp-input"
          placeholder="/absolute/workspace/root"
          value={workspaceRoot}
          onInput={(event) => setWorkspaceRoot((event.target as HTMLInputElement).value)}
        />
        <select
          className="tcp-input"
          value={schedulePreset}
          onInput={(event) => setSchedulePreset((event.target as HTMLSelectElement).value)}
        >
          {SCHEDULE_PRESETS.map((preset) => (
            <option key={preset.label} value={preset.label}>
              {preset.label}
            </option>
          ))}
        </select>
        <input
          className="tcp-input font-mono text-sm"
          placeholder="Custom cron (optional)"
          value={customCron}
          onInput={(event) => setCustomCron((event.target as HTMLInputElement).value)}
        />
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
          <i data-lucide="file-search"></i>
          {composeMutation.isPending ? "Composing…" : "Preview Standup Workflow"}
        </button>
        <button
          type="button"
          className="tcp-btn-primary"
          disabled={createMutation.isPending || !templates.length}
          onClick={() => createMutation.mutate()}
        >
          <i data-lucide="rocket"></i>
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
    </div>
  );
}
