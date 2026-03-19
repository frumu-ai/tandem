import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { useEngineStream } from "../features/stream/useEngineStream";
import { AnimatedPage, Badge, PageHeader, PanelCard, SplitView } from "../ui/index";
import { EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

function safeString(value: unknown) {
  return String(value || "").trim();
}

function formatJson(value: unknown) {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return "{}";
  }
}

function statusTone(enabled: boolean) {
  return enabled ? "ok" : "ghost";
}

function workflowEventType(event: any) {
  return safeString(event?.event_type || event?.type || event?.event || "workflow.event");
}

export function PacksPage({ api, toast }: AppPageProps) {
  const queryClient = useQueryClient();
  const [path, setPath] = useState("");
  const [selectedPack, setSelectedPack] = useState("");
  const [selectedWorkflow, setSelectedWorkflow] = useState("");
  const [simulateEventType, setSimulateEventType] = useState("context.task.created");
  const [simulatePayload, setSimulatePayload] = useState('{\n  "event_id": "demo-task-1"\n}');
  const [liveEvents, setLiveEvents] = useState<Array<{ at: number; data: any }>>([]);

  const listQuery = useQuery({
    queryKey: ["packs", "list"],
    queryFn: () => api("/api/engine/packs", { method: "GET" }).catch(() => ({ packs: [] })),
    refetchInterval: 15000,
  });

  const workflowsQuery = useQuery({
    queryKey: ["workflows", "list"],
    queryFn: () => api("/api/engine/workflows", { method: "GET" }).catch(() => ({ workflows: [] })),
    refetchInterval: 15000,
  });

  const hooksQuery = useQuery({
    queryKey: ["workflows", "hooks"],
    queryFn: () =>
      api("/api/engine/workflow-hooks", { method: "GET" }).catch(() => ({ hooks: [] })),
    refetchInterval: 15000,
  });

  const packs = toArray(listQuery.data, "packs");
  const workflows = toArray(workflowsQuery.data, "workflows");
  const hooks = toArray(hooksQuery.data, "hooks");

  const activePackId = useMemo(() => {
    if (selectedPack) return selectedPack;
    return safeString(packs[0]?.pack_id || packs[0]?.name);
  }, [packs, selectedPack]);

  const activeWorkflowId = useMemo(() => {
    if (selectedWorkflow) return selectedWorkflow;
    return safeString(workflows[0]?.workflow_id);
  }, [selectedWorkflow, workflows]);

  const workflowRunsQuery = useQuery({
    queryKey: ["workflows", "runs", activeWorkflowId],
    queryFn: () =>
      api(
        `/api/engine/workflows/runs${activeWorkflowId ? `?workflow_id=${encodeURIComponent(activeWorkflowId)}&limit=20` : "?limit=20"}`,
        { method: "GET" }
      ).catch(() => ({ runs: [] })),
    refetchInterval: 6000,
  });
  const workflowRuns = toArray(workflowRunsQuery.data, "runs");

  const packDetailQuery = useQuery({
    queryKey: ["packs", "detail", activePackId],
    enabled: !!activePackId,
    queryFn: () => api(`/api/engine/packs/${encodeURIComponent(activePackId)}`, { method: "GET" }),
  });

  const workflowDetailQuery = useQuery({
    queryKey: ["workflows", "detail", activeWorkflowId],
    enabled: !!activeWorkflowId,
    queryFn: () =>
      api(`/api/engine/workflows/${encodeURIComponent(activeWorkflowId)}`, { method: "GET" }),
  });

  const installMutation = useMutation({
    mutationFn: () =>
      api("/api/engine/packs/install", {
        method: "POST",
        body: JSON.stringify({ path, source: { kind: "control-panel" } }),
      }),
    onSuccess: async () => {
      toast("ok", "Pack installed.");
      setPath("");
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["packs"] }),
        queryClient.invalidateQueries({ queryKey: ["workflows"] }),
      ]);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const runWorkflowMutation = useMutation({
    mutationFn: (workflowId: string) =>
      api(`/api/engine/workflows/${encodeURIComponent(workflowId)}/run`, {
        method: "POST",
      }),
    onSuccess: async (payload) => {
      toast("ok", `Workflow ${safeString(payload?.run?.workflow_id || activeWorkflowId)} started.`);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["workflows"] }),
        queryClient.invalidateQueries({ queryKey: ["workflows", "runs"] }),
      ]);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const simulateMutation = useMutation({
    mutationFn: async () => {
      let properties = {};
      const trimmed = safeString(simulatePayload);
      if (trimmed) {
        properties = JSON.parse(trimmed);
      }
      return api("/api/engine/workflows/simulate", {
        method: "POST",
        body: JSON.stringify({
          event_type: simulateEventType,
          properties,
        }),
      });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const hookToggleMutation = useMutation({
    mutationFn: ({ bindingId, enabled }: { bindingId: string; enabled: boolean }) =>
      api(`/api/engine/workflow-hooks/${encodeURIComponent(bindingId)}`, {
        method: "PATCH",
        body: JSON.stringify({ enabled }),
      }),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["workflows", "hooks"] }),
        queryClient.invalidateQueries({ queryKey: ["workflows", "detail"] }),
      ]);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const selectedPackPayload = packDetailQuery.data?.pack || null;
  const selectedWorkflowPayload = workflowDetailQuery.data?.workflow || null;
  const selectedWorkflowHooks = toArray(workflowDetailQuery.data, "hooks");
  const simulation = simulateMutation.data?.simulation || null;

  useEngineStream(
    `/api/engine/workflows/events${activeWorkflowId ? `?workflow_id=${encodeURIComponent(activeWorkflowId)}` : ""}`,
    (event) => {
      try {
        const data = JSON.parse(event.data);
        if (data?.status === "ready") return;
        setLiveEvents((prev) => [...prev.slice(-59), { at: Date.now(), data }]);
      } catch {
        // ignore malformed workflow stream events
      }
    },
    {
      enabled: true,
    }
  );

  return (
    <AnimatedPage className="grid gap-4">
      <PageHeader
        eyebrow="Operations"
        title="Pack library and workflow lab"
        subtitle="Inspect installed packs, see workflow extensions, toggle hooks, simulate lifecycle events, and run workflow definitions without leaving the control panel."
        badges={
          <>
            <Badge tone="info">{packs.length} installed packs</Badge>
            <Badge tone="warn">{workflows.length} workflows</Badge>
            <Badge tone="ghost">{hooks.length} hooks</Badge>
          </>
        }
      />

      <SplitView
        main={
          <div className="grid gap-4">
            <PanelCard
              title="Installed packs"
              subtitle="Select a pack to inspect its manifest, declared workflow contents, and hook contributions."
            >
              <div className="grid gap-2">
                {packs.length ? (
                  packs.map((pack: any, index: number) => {
                    const packId = safeString(pack?.pack_id || pack?.name || index);
                    const active = packId === activePackId;
                    return (
                      <button
                        key={packId}
                        type="button"
                        className={`tcp-list-item text-left ${active ? "border-amber-400/70" : ""}`}
                        onClick={() => setSelectedPack(packId)}
                      >
                        <div className="mb-1 flex items-center justify-between gap-2">
                          <strong>{safeString(pack?.name || packId)}</strong>
                          <span className="tcp-badge-info">{safeString(pack?.version || "-")}</span>
                        </div>
                        <div className="flex flex-wrap gap-2 text-xs">
                          <span className="tcp-subtle">
                            {safeString(pack?.pack_type || "pack")}
                          </span>
                          <span className="tcp-subtle">{safeString(pack?.install_path || "")}</span>
                        </div>
                      </button>
                    );
                  })
                ) : (
                  <EmptyState text="No packs are installed yet." />
                )}
              </div>
            </PanelCard>

            <PanelCard
              title="Workflow registry"
              subtitle="This is the live workflow registry resolved from built-ins, installed packs, and workspace-local overrides."
            >
              <div className="grid gap-2">
                {workflows.length ? (
                  workflows.map((workflow: any) => {
                    const workflowId = safeString(workflow?.workflow_id);
                    const active = workflowId === activeWorkflowId;
                    const sourceKind = safeString(workflow?.source?.kind || "unknown");
                    return (
                      <button
                        key={workflowId}
                        type="button"
                        className={`tcp-list-item text-left ${active ? "border-amber-400/70" : ""}`}
                        onClick={() => setSelectedWorkflow(workflowId)}
                      >
                        <div className="mb-1 flex items-center justify-between gap-2">
                          <strong>{safeString(workflow?.name || workflowId)}</strong>
                          <span className={`tcp-badge-${sourceKind === "pack" ? "warn" : "info"}`}>
                            {sourceKind}
                          </span>
                        </div>
                        <div className="tcp-subtle text-xs">
                          {workflowId} · {toArray(workflow?.steps, "steps").length} steps
                        </div>
                      </button>
                    );
                  })
                ) : (
                  <EmptyState text="No workflows are currently registered." />
                )}
              </div>
            </PanelCard>
          </div>
        }
        aside={
          <div className="grid gap-4">
            <PanelCard
              title="Install pack"
              subtitle="Install a local pack path and reload workflow contributions automatically."
            >
              <div className="grid gap-3">
                <input
                  className="tcp-input"
                  value={path}
                  onInput={(e) => setPath((e.target as HTMLInputElement).value)}
                  placeholder="/path/to/pack.zip"
                />
                <button
                  className="tcp-btn-primary"
                  disabled={!path.trim() || installMutation.isPending}
                  onClick={() => installMutation.mutate()}
                >
                  <i data-lucide="download"></i>
                  Install from path
                </button>
              </div>
            </PanelCard>

            <PanelCard
              title="Pack inspector"
              subtitle="Shows which workflows and hooks a pack contributes, alongside its current capability surface."
            >
              {selectedPackPayload ? (
                <div className="grid gap-3">
                  <div className="tcp-list-item">
                    <div className="mb-1 flex items-center justify-between gap-2">
                      <strong>
                        {safeString(selectedPackPayload?.installed?.name || activePackId)}
                      </strong>
                      <Badge tone="info">
                        {safeString(selectedPackPayload?.installed?.version || "-")}
                      </Badge>
                    </div>
                    <div className="flex flex-wrap gap-2 text-xs">
                      <Badge tone="ghost">
                        {Number(selectedPackPayload?.workflow_extensions?.workflow_count || 0)}{" "}
                        workflows
                      </Badge>
                      <Badge tone="ghost">
                        {Number(selectedPackPayload?.workflow_extensions?.workflow_hook_count || 0)}{" "}
                        hooks
                      </Badge>
                      <Badge tone="ghost">
                        {Number(selectedPackPayload?.risk?.required_capabilities_count || 0)}{" "}
                        required caps
                      </Badge>
                    </div>
                  </div>
                  <div className="grid gap-2">
                    <div className="tcp-subtle text-xs uppercase tracking-[0.24em]">
                      Workflow Entry Points
                    </div>
                    {toArray(selectedPackPayload?.workflow_extensions, "workflow_entrypoints")
                      .length ? (
                      toArray(selectedPackPayload?.workflow_extensions, "workflow_entrypoints").map(
                        (entry: any, index: number) => (
                          <div
                            key={`${safeString(entry)}-${index}`}
                            className="tcp-list-item text-sm"
                          >
                            {safeString(entry)}
                          </div>
                        )
                      )
                    ) : (
                      <EmptyState text="This pack does not declare workflow entrypoints." />
                    )}
                  </div>
                  <details className="tcp-list-item">
                    <summary className="cursor-pointer font-medium">Manifest</summary>
                    <pre className="tcp-subtle mt-3 overflow-x-auto text-xs">
                      {formatJson(selectedPackPayload?.manifest || {})}
                    </pre>
                  </details>
                </div>
              ) : (
                <EmptyState text="Select an installed pack to inspect workflow extensions." />
              )}
            </PanelCard>
          </div>
        }
      />

      <SplitView
        main={
          <PanelCard
            title="Workflow viewer"
            subtitle="Structured view of workflow steps and source provenance for the selected workflow."
            actions={
              <button
                className="tcp-btn"
                disabled={!activeWorkflowId || runWorkflowMutation.isPending}
                onClick={() => activeWorkflowId && runWorkflowMutation.mutate(activeWorkflowId)}
              >
                <i data-lucide="play"></i>
                Run workflow
              </button>
            }
          >
            {selectedWorkflowPayload ? (
              <div className="grid gap-3">
                <div className="tcp-list-item">
                  <div className="mb-1 flex items-center justify-between gap-2">
                    <strong>{safeString(selectedWorkflowPayload?.name || activeWorkflowId)}</strong>
                    <Badge tone="info">
                      {safeString(selectedWorkflowPayload?.source?.kind || "unknown")}
                    </Badge>
                  </div>
                  <div className="tcp-subtle text-sm">
                    {safeString(selectedWorkflowPayload?.description || "No description provided.")}
                  </div>
                </div>
                <div className="grid gap-2">
                  {toArray(selectedWorkflowPayload, "steps").length ? (
                    toArray(selectedWorkflowPayload, "steps").map((step: any, index: number) => (
                      <div key={safeString(step?.step_id || index)} className="tcp-list-item">
                        <div className="mb-1 text-xs uppercase tracking-[0.24em] text-slate-400">
                          Step {index + 1}
                        </div>
                        <div className="font-medium">{safeString(step?.action || "unknown")}</div>
                        {step?.with ? (
                          <pre className="tcp-subtle mt-2 overflow-x-auto text-xs">
                            {formatJson(step.with)}
                          </pre>
                        ) : null}
                      </div>
                    ))
                  ) : (
                    <EmptyState text="This workflow does not define any linear steps." />
                  )}
                </div>
              </div>
            ) : (
              <EmptyState text="Select a workflow to inspect its current resolved definition." />
            )}
          </PanelCard>
        }
        aside={
          <div className="grid gap-4">
            <PanelCard
              title="Hook manager"
              subtitle="Enable or disable hook bindings on the live registry and inspect the attached actions."
            >
              <div className="grid gap-2">
                {selectedWorkflowHooks.length ? (
                  selectedWorkflowHooks.map((hook: any) => {
                    const bindingId = safeString(hook?.binding_id);
                    const enabled = !!hook?.enabled;
                    return (
                      <div key={bindingId} className="tcp-list-item">
                        <div className="mb-2 flex items-start justify-between gap-2">
                          <div>
                            <div className="font-medium">
                              {safeString(hook?.event || bindingId)}
                            </div>
                            <div className="tcp-subtle text-xs">{bindingId}</div>
                          </div>
                          <button
                            className="tcp-btn"
                            disabled={hookToggleMutation.isPending}
                            onClick={() =>
                              hookToggleMutation.mutate({
                                bindingId,
                                enabled: !enabled,
                              })
                            }
                          >
                            {enabled ? "Disable" : "Enable"}
                          </button>
                        </div>
                        <div className="mb-2 flex flex-wrap gap-2">
                          <Badge tone={statusTone(enabled)}>
                            {enabled ? "Enabled" : "Disabled"}
                          </Badge>
                          <Badge tone="ghost">{toArray(hook, "actions").length} actions</Badge>
                        </div>
                        <div className="grid gap-1">
                          {toArray(hook, "actions").map((action: any, index: number) => (
                            <div key={`${bindingId}-${index}`} className="tcp-subtle text-xs">
                              {safeString(action?.action || action)}
                            </div>
                          ))}
                        </div>
                      </div>
                    );
                  })
                ) : (
                  <EmptyState text="The selected workflow has no registered hooks." />
                )}
              </div>
            </PanelCard>

            <PanelCard
              title="Recent workflow runs"
              subtitle="Latest workflow executions for the selected workflow, including hook-triggered runs."
            >
              <div className="grid gap-2">
                {workflowRuns.length ? (
                  workflowRuns.map((run: any, index: number) => {
                    const status = safeString(run?.status || "unknown");
                    const tone =
                      status === "completed"
                        ? "ok"
                        : status === "failed"
                          ? "err"
                          : status === "running"
                            ? "warn"
                            : "ghost";
                    return (
                      <div key={safeString(run?.run_id || index)} className="tcp-list-item">
                        <div className="mb-1 flex items-center justify-between gap-2">
                          <strong>{safeString(run?.run_id || `run-${index + 1}`)}</strong>
                          <Badge tone={tone}>{status}</Badge>
                        </div>
                        <div className="tcp-subtle text-xs">
                          trigger: {safeString(run?.trigger_event || "manual")}
                        </div>
                        <div className="tcp-subtle text-xs">
                          actions: {toArray(run, "actions").length}
                        </div>
                      </div>
                    );
                  })
                ) : (
                  <EmptyState text="No workflow runs have been recorded yet." />
                )}
              </div>
            </PanelCard>
          </div>
        }
      />

      <SplitView
        main={
          <PanelCard
            title="Workflow testing mode"
            subtitle="Simulate a source event to see which hooks will match before triggering real actions."
            actions={
              <button
                className="tcp-btn-primary"
                disabled={!simulateEventType.trim() || simulateMutation.isPending}
                onClick={() => simulateMutation.mutate()}
              >
                <i data-lucide="flask-conical"></i>
                Simulate
              </button>
            }
          >
            <div className="grid gap-3">
              <input
                className="tcp-input"
                value={simulateEventType}
                onInput={(e) => setSimulateEventType((e.target as HTMLInputElement).value)}
                placeholder="context.task.created"
              />
              <textarea
                className="tcp-input min-h-[180px] font-mono text-xs"
                value={simulatePayload}
                onInput={(e) => setSimulatePayload((e.target as HTMLTextAreaElement).value)}
                spellCheck={false}
              />
              {simulation ? (
                <div className="grid gap-3">
                  <div className="flex flex-wrap gap-2">
                    {toArray(simulation, "canonical_events").map(
                      (eventName: any, index: number) => (
                        <Badge key={`${safeString(eventName)}-${index}`} tone="info">
                          {safeString(eventName)}
                        </Badge>
                      )
                    )}
                  </div>
                  <div className="grid gap-2">
                    {toArray(simulation, "matched_bindings").length ? (
                      toArray(simulation, "matched_bindings").map((binding: any) => (
                        <div key={safeString(binding?.binding_id)} className="tcp-list-item">
                          <div className="font-medium">{safeString(binding?.binding_id)}</div>
                          <div className="tcp-subtle text-xs">
                            {safeString(binding?.event)} · {toArray(binding, "actions").length}{" "}
                            actions
                          </div>
                        </div>
                      ))
                    ) : (
                      <EmptyState text="No workflow hooks matched this event." />
                    )}
                  </div>
                </div>
              ) : (
                <EmptyState text="Run a simulation to inspect canonical events, matched bindings, and planned actions." />
              )}
            </div>
          </PanelCard>
        }
        aside={
          <PanelCard
            title="Triggered actions"
            subtitle="Preview of actions the engine plans to execute for the simulated event."
          >
            <div className="grid gap-2">
              {toArray(simulation, "planned_actions").length ? (
                toArray(simulation, "planned_actions").map((action: any, index: number) => (
                  <div
                    key={`${safeString(action?.action || action)}-${index}`}
                    className="tcp-list-item"
                  >
                    <div className="font-medium">{safeString(action?.action || action)}</div>
                    {action?.with ? (
                      <pre className="tcp-subtle mt-2 overflow-x-auto text-xs">
                        {formatJson(action.with)}
                      </pre>
                    ) : null}
                  </div>
                ))
              ) : (
                <EmptyState text="No planned actions yet." />
              )}
            </div>
          </PanelCard>
        }
      />

      <PanelCard
        title="Live workflow events"
        subtitle="Real-time `workflow.*` events for the selected workflow, including run start, action completion, and failures."
        actions={
          <button className="tcp-btn" onClick={() => setLiveEvents([])}>
            <i data-lucide="trash-2"></i>
            Clear stream
          </button>
        }
      >
        <div className="grid max-h-[28rem] gap-2 overflow-auto rounded-2xl border border-slate-700/60 bg-black/20 p-2">
          {liveEvents.length ? (
            [...liveEvents].reverse().map((item, index) => {
              const type = workflowEventType(item.data);
              const tone = type.endsWith(".failed")
                ? "err"
                : type.endsWith(".completed")
                  ? "ok"
                  : type.endsWith(".started")
                    ? "warn"
                    : "info";
              return (
                <div key={`${item.at}-${index}`} className="tcp-list-item">
                  <div className="mb-1 flex items-center justify-between gap-2">
                    <strong>{type}</strong>
                    <Badge tone={tone}>{new Date(item.at).toLocaleTimeString()}</Badge>
                  </div>
                  <div className="tcp-subtle text-xs">
                    run: {safeString(item.data?.properties?.runID || "n/a")}
                  </div>
                  <div className="tcp-subtle text-xs">
                    action:{" "}
                    {safeString(
                      item.data?.properties?.action || item.data?.properties?.actionID || "-"
                    )}
                  </div>
                </div>
              );
            })
          ) : (
            <EmptyState text="Run a workflow or trigger a hook to watch live workflow events here." />
          )}
        </div>
      </PanelCard>
    </AnimatedPage>
  );
}
