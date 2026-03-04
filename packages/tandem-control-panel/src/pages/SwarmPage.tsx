import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { useMemo, useState } from "react";
import { normalizeMessages } from "../features/chat/messages";
import { saveStoredSessionId } from "../features/chat/session";
import { useEngineStream } from "../features/stream/useEngineStream";
import { PageCard, EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

type BlackboardMode = "docked" | "expanded" | "fullscreen";

function normalizeTasks(runPayload: any) {
  const blackboardTasks = Array.isArray(runPayload?.blackboard?.tasks)
    ? runPayload.blackboard.tasks
    : [];
  if (blackboardTasks.length) {
    return blackboardTasks.map((task: any) => ({
      taskId: task?.id,
      title: task?.payload?.title || task?.task_type || task?.id,
      status: task?.status,
      stepStatus: task?.status,
      sessionId: "",
      workflowId: task?.workflow_id || "",
      assignedAgent: task?.assigned_agent || task?.lease_owner || "",
      taskRev: task?.task_rev || 0,
      dependsOn: Array.isArray(task?.depends_on_task_ids) ? task.depends_on_task_ids : [],
      artifactIds: Array.isArray(task?.artifact_ids) ? task.artifact_ids : [],
      decisionIds: Array.isArray(task?.decision_ids) ? task.decision_ids : [],
      updatedTs: Number(task?.updated_ts || 0),
    }));
  }
  if (Array.isArray(runPayload?.tasks)) return runPayload.tasks;
  return [];
}

function statusBadgeClass(status: string) {
  const s = String(status || "")
    .trim()
    .toLowerCase();
  if (s === "done" || s === "completed" || s === "active") return "tcp-badge-ok";
  if (s === "failed" || s === "error" || s === "cancelled" || s === "canceled")
    return "tcp-badge-err";
  if (s === "running" || s === "in_progress" || s === "runnable") return "tcp-badge-warn";
  return "tcp-badge-info";
}

export function SwarmPage({ api, toast, navigate }: AppPageProps) {
  const queryClient = useQueryClient();
  const [workspaceRoot, setWorkspaceRoot] = useState("");
  const [objective, setObjective] = useState("Ship a small feature end-to-end");
  const [maxTasks, setMaxTasks] = useState("3");
  const [selectedRunId, setSelectedRunId] = useState("");
  const [blackboardMode, setBlackboardMode] = useState<BlackboardMode>("docked");
  const [expandedTaskTitles, setExpandedTaskTitles] = useState<Record<string, boolean>>({});

  const statusQuery = useQuery({
    queryKey: ["swarm", "status"],
    queryFn: () => api("/api/swarm/status"),
    refetchInterval: 5000,
  });
  const runsQuery = useQuery({
    queryKey: ["swarm", "runs", workspaceRoot],
    queryFn: () =>
      api(
        `/api/swarm/runs?workspace=${encodeURIComponent(workspaceRoot || statusQuery.data?.workspaceRoot || "")}`
      ),
    enabled: !!statusQuery.data,
    refetchInterval: 6000,
  });

  const runs = Array.isArray(runsQuery.data?.runs) ? runsQuery.data.runs : [];
  const hiddenCount = Number(runsQuery.data?.hiddenCount || 0);
  const runId = selectedRunId || String(statusQuery.data?.runId || runs[0]?.run_id || "");

  const runQuery = useQuery({
    queryKey: ["swarm", "run", runId],
    enabled: !!runId,
    queryFn: () => api(`/api/swarm/run/${encodeURIComponent(runId)}`),
    refetchInterval: 4500,
  });

  const tasks = normalizeTasks(runQuery.data);
  const runEvents = Array.isArray(runQuery.data?.events) ? runQuery.data.events : [];
  const blackboard = runQuery.data?.blackboard || null;
  const blackboardPatches = Array.isArray(runQuery.data?.blackboardPatches)
    ? runQuery.data.blackboardPatches
    : [];
  const replayDrift = runQuery.data?.replay?.drift || {};
  const runStatus = String(runQuery.data?.run?.status || "")
    .trim()
    .toLowerCase();
  const lastErrorText = String(statusQuery.data?.lastError || "").trim();

  const sessionsByStepId = useMemo(() => {
    const map = new Map<string, string>();
    for (const evt of runEvents) {
      if (
        String(evt?.type || "")
          .trim()
          .toLowerCase() !== "step_completed"
      )
        continue;
      const stepId = String(evt?.step_id || "").trim();
      const payload = evt?.payload && typeof evt.payload === "object" ? evt.payload : {};
      const sessionId = String(payload?.session_id || "").trim();
      if (stepId && sessionId) map.set(stepId, sessionId);
    }
    return map;
  }, [runEvents]);

  const latestOutput = useMemo(() => {
    let latest: any = null;
    let latestTs = 0;
    for (const evt of runEvents) {
      if (
        String(evt?.type || "")
          .trim()
          .toLowerCase() !== "step_completed"
      )
        continue;
      const payload = evt?.payload && typeof evt.payload === "object" ? evt.payload : {};
      const sessionId = String(payload?.session_id || "").trim();
      if (!sessionId) continue;
      const ts = Number(evt?.ts_ms || 0);
      if (!latest || ts >= latestTs) {
        latest = { event: evt, sessionId };
        latestTs = ts;
      }
    }
    return latest;
  }, [runEvents]);

  const outputSessionQuery = useQuery({
    queryKey: ["swarm", "run-output-session", latestOutput?.sessionId || ""],
    enabled: !!latestOutput?.sessionId,
    queryFn: () =>
      api(`/api/engine/session/${encodeURIComponent(String(latestOutput?.sessionId || ""))}`),
    refetchInterval: 6000,
  });

  const latestAssistantOutput = useMemo(() => {
    const messages = normalizeMessages(outputSessionQuery.data, "Assistant");
    for (let i = messages.length - 1; i >= 0; i -= 1) {
      if (messages[i]?.role === "assistant" && String(messages[i]?.text || "").trim()) {
        return String(messages[i].text || "").trim();
      }
    }
    return "";
  }, [outputSessionQuery.data]);

  const shouldShowLastError =
    !!lastErrorText &&
    !(
      runStatus === "completed" && /all steps are done; marking run completed/i.test(lastErrorText)
    );

  useEngineStream(
    runId ? `/api/swarm/events?runId=${encodeURIComponent(runId)}` : "",
    () => {
      queryClient.invalidateQueries({ queryKey: ["swarm", "status"] });
      if (runId) queryClient.invalidateQueries({ queryKey: ["swarm", "run", runId] });
    },
    { enabled: !!runId }
  );

  const startMutation = useMutation({
    mutationFn: () =>
      api("/api/swarm/start", {
        method: "POST",
        body: JSON.stringify({
          workspaceRoot: workspaceRoot || statusQuery.data?.workspaceRoot || "",
          objective,
          maxTasks: Number(maxTasks || 3),
        }),
      }),
    onSuccess: async (payload) => {
      const id = String(payload?.runId || "");
      if (id) setSelectedRunId(id);
      toast("ok", "Swarm run started.");
      await queryClient.invalidateQueries({ queryKey: ["swarm"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const actionMutation = useMutation({
    mutationFn: ({ path, body }: { path: string; body: any }) =>
      api(path, { method: "POST", body: JSON.stringify(body) }),
    onSuccess: async (payload: any, vars) => {
      if (vars?.path === "/api/swarm/continue" || vars?.path === "/api/swarm/resume") {
        const why = String(payload?.whyNextStep || "").trim();
        const selected = String(payload?.selectedStepId || "").trim();
        const started = payload?.started === true ? "executor started" : "executor already active";
        const executorState = String(payload?.executorState || "").trim();
        const executorReason = String(payload?.executorReason || "").trim();
        toast(
          "ok",
          selected ? `${started}; next step ${selected}` : why ? `${started}; ${why}` : started
        );
        if (executorState === "error" && executorReason) {
          toast("err", executorReason);
        }
      }
      if (vars?.path === "/api/swarm/runs/hide") {
        setSelectedRunId("");
        toast(
          "ok",
          `Hidden ${Array.isArray(vars?.body?.runIds) ? vars.body.runIds.length : 1} run(s).`
        );
      }
      if (vars?.path === "/api/swarm/runs/hide_completed") {
        setSelectedRunId("");
        toast("ok", `Hidden completed runs (${Number(payload?.hiddenNow || 0)}).`);
      }
      await queryClient.invalidateQueries({ queryKey: ["swarm"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const activeTasks = useMemo(
    () =>
      tasks.filter((t: any) =>
        ["running", "in_progress", "runnable"].includes(
          String(t?.stepStatus || t?.status || "").toLowerCase()
        )
      ),
    [tasks]
  );

  const agentLanes = useMemo(() => {
    const map = new Map<string, any[]>();
    for (const task of tasks) {
      const lane = String(task?.assignedAgent || "unassigned");
      const rows = map.get(lane) || [];
      rows.push(task);
      map.set(lane, rows);
    }
    return Array.from(map.entries())
      .map(([agent, rows]) => ({
        agent,
        running: rows.filter((t) => String(t?.status || "") === "in_progress").length,
        done: rows.filter((t) => String(t?.status || "") === "done").length,
        total: rows.length,
      }))
      .sort((a, b) => b.total - a.total);
  }, [tasks]);

  const workflows = useMemo(() => {
    const map = new Map<string, any[]>();
    for (const task of tasks) {
      const wf = String(task?.workflowId || "default");
      const rows = map.get(wf) || [];
      rows.push(task);
      map.set(wf, rows);
    }
    return Array.from(map.entries())
      .map(([workflowId, rows]) => ({
        workflowId,
        total: rows.length,
        done: rows.filter((t) => String(t?.status || "") === "done").length,
        blocked: rows.filter((t) => String(t?.status || "") === "blocked").length,
      }))
      .sort((a, b) => b.total - a.total)
      .slice(0, 12);
  }, [tasks]);

  const driftFlags = useMemo(
    () =>
      Object.entries(replayDrift || {})
        .filter(([, value]) => value === true)
        .map(([key]) => key),
    [replayDrift]
  );

  const toggleTaskTitle = (taskId: string) => {
    setExpandedTaskTitles((prev) => ({ ...prev, [taskId]: !prev[taskId] }));
  };

  const renderBlackboardPanel = (fullscreen = false) => (
    <div
      className={
        fullscreen
          ? "fixed inset-4 z-50 overflow-auto rounded-2xl border border-slate-600/70 bg-slate-950/95 p-4"
          : "rounded-xl border border-slate-700/60 bg-black/20 p-3"
      }
    >
      <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <div>
          <div className="font-medium">Blackboard</div>
          <div className="tcp-subtle text-xs">
            run: {String(runQuery.data?.run?.status || "unknown")} | current step:{" "}
            {String(runQuery.data?.run?.current_step_id || activeTasks[0]?.taskId || "n/a")}
          </div>
          <div className="tcp-subtle text-xs">
            why_next_step: {String(runQuery.data?.run?.why_next_step || "n/a")}
          </div>
        </div>
        <div className="flex items-center gap-2">
          <button
            className={`tcp-btn h-7 px-2 text-xs ${blackboardMode === "docked" ? "border-amber-400/70" : ""}`}
            onClick={() => setBlackboardMode("docked")}
          >
            Docked
          </button>
          <button
            className={`tcp-btn h-7 px-2 text-xs ${blackboardMode === "expanded" ? "border-amber-400/70" : ""}`}
            onClick={() => setBlackboardMode("expanded")}
          >
            Expanded
          </button>
          <button
            className={`tcp-btn h-7 px-2 text-xs ${blackboardMode === "fullscreen" ? "border-amber-400/70" : ""}`}
            onClick={() => setBlackboardMode("fullscreen")}
          >
            Fullscreen
          </button>
          {fullscreen ? (
            <button
              className="tcp-btn-danger h-7 px-2 text-xs"
              onClick={() => setBlackboardMode("expanded")}
            >
              Exit
            </button>
          ) : null}
        </div>
      </div>

      <div className="mb-3 grid gap-2 md:grid-cols-3">
        <div className="rounded-lg border border-slate-700/60 bg-slate-900/40 p-2 text-xs">
          <div className="font-medium">Run status</div>
          <div className="tcp-subtle">{String(runQuery.data?.run?.status || "unknown")}</div>
        </div>
        <div className="rounded-lg border border-slate-700/60 bg-slate-900/40 p-2 text-xs">
          <div className="font-medium">Task progress</div>
          <div className="tcp-subtle">
            active {activeTasks.length} / total {tasks.length}
          </div>
        </div>
        <div className="rounded-lg border border-slate-700/60 bg-slate-900/40 p-2 text-xs">
          <div className="font-medium">Revision</div>
          <div className="tcp-subtle">{String(blackboard?.revision ?? 0)}</div>
        </div>
      </div>

      <div className="mb-3 grid gap-3 lg:grid-cols-2">
        <div className="rounded-lg border border-slate-700/60 bg-slate-900/30 p-3">
          <div className="mb-1 text-sm font-medium">Decision Lineage</div>
          <div className="grid max-h-[200px] gap-2 overflow-auto text-xs">
            {Array.isArray(blackboard?.decisions) && blackboard.decisions.length ? (
              blackboard.decisions.slice(-12).map((row: any, idx: number) => (
                <div
                  key={String(row?.id || idx)}
                  className="rounded border border-slate-700/50 p-2"
                >
                  <div className="font-medium">
                    {String(row?.title || row?.id || `decision-${idx}`)}
                  </div>
                  <div className="tcp-subtle">{String(row?.content || row?.summary || "")}</div>
                </div>
              ))
            ) : (
              <EmptyState text="No decisions yet." />
            )}
          </div>
        </div>

        <div className="rounded-lg border border-slate-700/60 bg-slate-900/30 p-3">
          <div className="mb-1 text-sm font-medium">Agent Activity Lanes</div>
          <div className="grid max-h-[200px] gap-2 overflow-auto text-xs">
            {agentLanes.length ? (
              agentLanes.map((lane) => (
                <div key={lane.agent} className="rounded border border-slate-700/50 p-2">
                  <div className="font-medium">{lane.agent}</div>
                  <div className="tcp-subtle">
                    running {lane.running} | done {lane.done} | total {lane.total}
                  </div>
                </div>
              ))
            ) : (
              <EmptyState text="No agent activity yet." />
            )}
          </div>
        </div>
      </div>

      <div className="mb-3 grid gap-3 lg:grid-cols-2">
        <div className="rounded-lg border border-slate-700/60 bg-slate-900/30 p-3">
          <div className="mb-1 text-sm font-medium">Workflow Graph View</div>
          <div className="grid max-h-[180px] gap-2 overflow-auto text-xs">
            {workflows.length ? (
              workflows.map((wf) => (
                <div key={wf.workflowId} className="rounded border border-slate-700/50 p-2">
                  <div className="font-medium">{wf.workflowId}</div>
                  <div className="tcp-subtle">
                    done {wf.done}/{wf.total} | blocked {wf.blocked}
                  </div>
                </div>
              ))
            ) : (
              <EmptyState text="No workflow data yet." />
            )}
          </div>
        </div>

        <div className="rounded-lg border border-slate-700/60 bg-slate-900/30 p-3">
          <div className="mb-1 text-sm font-medium">Artifact Lineage</div>
          <div className="grid max-h-[180px] gap-2 overflow-auto text-xs">
            {Array.isArray(blackboard?.artifacts) && blackboard.artifacts.length ? (
              blackboard.artifacts.slice(-12).map((artifact: any, idx: number) => (
                <div
                  key={String(artifact?.id || idx)}
                  className="rounded border border-slate-700/50 p-2"
                >
                  <div className="font-medium">{String(artifact?.id || `artifact-${idx}`)}</div>
                  <div className="tcp-subtle">
                    {String(artifact?.kind || "artifact")} |{" "}
                    {String(artifact?.uri || artifact?.path || "")}
                  </div>
                </div>
              ))
            ) : (
              <EmptyState text="No artifacts yet." />
            )}
          </div>
        </div>
      </div>

      <div className="grid gap-3 lg:grid-cols-2">
        <div className="rounded-lg border border-slate-700/60 bg-slate-900/30 p-3">
          <div className="mb-1 text-sm font-medium">Alerts and Drift</div>
          <div className="grid gap-1 text-xs">
            <div className="tcp-subtle">
              open questions:{" "}
              {Array.isArray(blackboard?.open_questions) ? blackboard.open_questions.length : 0}
            </div>
            <div className="tcp-subtle">
              replay drift: {driftFlags.length ? driftFlags.join(", ") : "none"}
            </div>
          </div>
        </div>

        <div className="rounded-lg border border-slate-700/60 bg-slate-900/30 p-3">
          <div className="mb-1 text-sm font-medium">Patch Feed (Debug)</div>
          <div className="grid max-h-[180px] gap-1 overflow-auto text-xs">
            {blackboardPatches.length ? (
              blackboardPatches.slice(-20).map((patch: any, idx: number) => (
                <div
                  key={String(patch?.seq || idx)}
                  className="tcp-subtle rounded border border-slate-700/40 p-1"
                >
                  #{String(patch?.seq || "?")} {String(patch?.op || "unknown")}
                </div>
              ))
            ) : (
              <EmptyState text="No patch data yet." />
            )}
          </div>
        </div>
      </div>
    </div>
  );

  return (
    <>
      <div className="grid min-w-0 gap-4 overflow-x-hidden xl:grid-cols-[1.05fr_1fr]">
        <PageCard title="Swarm Context Runs" subtitle="Create, monitor, and control live runs">
          <div className="mb-3 grid gap-2 md:grid-cols-[1fr_140px_auto]">
            <input
              className="tcp-input"
              placeholder="workspace root"
              value={workspaceRoot || statusQuery.data?.workspaceRoot || ""}
              onInput={(e) => setWorkspaceRoot((e.target as HTMLInputElement).value)}
            />
            <input
              className="tcp-input"
              type="number"
              min="1"
              value={maxTasks}
              onInput={(e) => setMaxTasks((e.target as HTMLInputElement).value)}
            />
            <button
              className="tcp-btn-primary"
              onClick={() => startMutation.mutate()}
              disabled={startMutation.isPending}
            >
              New Run
            </button>
          </div>
          <textarea
            className="tcp-input mb-3 min-h-[84px]"
            value={objective}
            onInput={(e) => setObjective((e.target as HTMLTextAreaElement).value)}
          />

          <div className="mb-3 flex flex-wrap gap-2">
            <button
              className="tcp-btn"
              disabled={!runId}
              onClick={() => actionMutation.mutate({ path: "/api/swarm/approve", body: { runId } })}
            >
              Approve
            </button>
            <button
              className="tcp-btn"
              disabled={!runId}
              onClick={() =>
                actionMutation.mutate({ path: "/api/swarm/continue", body: { runId } })
              }
            >
              Continue
            </button>
            <button
              className="tcp-btn"
              disabled={!runId}
              onClick={() => actionMutation.mutate({ path: "/api/swarm/pause", body: { runId } })}
            >
              Pause
            </button>
            <button
              className="tcp-btn"
              disabled={!runId}
              onClick={() => actionMutation.mutate({ path: "/api/swarm/resume", body: { runId } })}
            >
              Resume
            </button>
            <button
              className="tcp-btn-danger"
              disabled={!runId}
              onClick={() => actionMutation.mutate({ path: "/api/swarm/cancel", body: { runId } })}
            >
              Cancel
            </button>
          </div>
          <div className="mb-2 flex flex-wrap items-center justify-between gap-2 text-xs">
            <span className="tcp-subtle">
              visible runs: {runs.length}
              {hiddenCount > 0 ? ` | hidden: ${hiddenCount}` : ""}
            </span>
            <button
              className="tcp-btn h-7 px-2 text-xs"
              onClick={() =>
                actionMutation.mutate({
                  path: "/api/swarm/runs/hide_completed",
                  body: { workspace: workspaceRoot || statusQuery.data?.workspaceRoot || "" },
                })
              }
            >
              Hide Completed
            </button>
          </div>
          <div className="mb-3 grid gap-2 md:grid-cols-3">
            <div className="rounded-lg border border-slate-700/60 bg-slate-900/20 p-2 text-xs">
              <div className="font-medium">Resolved model</div>
              <div className="tcp-subtle">
                {String(statusQuery.data?.resolvedModelProvider || "n/a")} /{" "}
                {String(statusQuery.data?.resolvedModelId || "n/a")}
              </div>
            </div>
            <div className="rounded-lg border border-slate-700/60 bg-slate-900/20 p-2 text-xs">
              <div className="font-medium">Model source</div>
              <div className="tcp-subtle">
                {String(statusQuery.data?.modelResolutionSource || "none")}
              </div>
            </div>
            <div className="rounded-lg border border-slate-700/60 bg-slate-900/20 p-2 text-xs">
              <div className="font-medium">Executor</div>
              <div className="tcp-subtle">
                {String(statusQuery.data?.executorState || "idle")}
                {String(statusQuery.data?.executorReason || "").trim()
                  ? `: ${String(statusQuery.data?.executorReason || "")}`
                  : ""}
              </div>
            </div>
          </div>
          {shouldShowLastError ? (
            <div className="mb-3 rounded-lg border border-rose-400/40 bg-rose-950/25 p-2 text-xs text-rose-200">
              last error: {lastErrorText}
            </div>
          ) : null}

          <div className="grid max-h-[46vh] gap-2 overflow-auto">
            <AnimatePresence initial={false}>
              {runs.map((run: any) => {
                const id = String(run?.run_id || run?.runId || "");
                const active = id === runId;
                return (
                  <motion.div
                    key={id}
                    className={`tcp-list-item min-w-0 overflow-hidden ${active ? "border-amber-400/60" : ""}`}
                    initial={{ opacity: 0, y: 6 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: -6 }}
                  >
                    <div className="flex min-w-0 items-start gap-2">
                      <button
                        className="min-w-0 flex-1 text-left"
                        onClick={() => setSelectedRunId(id)}
                      >
                        <div className="flex min-w-0 items-start justify-between gap-2">
                          <span
                            className="block min-w-0 flex-1 text-sm font-medium leading-snug"
                            style={{ overflowWrap: "anywhere", wordBreak: "break-word" }}
                            title={String(run?.objective || id)}
                          >
                            {String(run?.objective || id)}
                          </span>
                          <span
                            className={`${statusBadgeClass(String(run?.status || "unknown"))} shrink-0`}
                          >
                            {String(run?.status || "unknown")}
                          </span>
                        </div>
                        <div className="tcp-subtle text-xs" style={{ overflowWrap: "anywhere" }}>
                          {id}
                        </div>
                      </button>
                      <button
                        className="tcp-btn h-7 shrink-0 px-2 text-xs"
                        onClick={() =>
                          actionMutation.mutate({
                            path: "/api/swarm/runs/hide",
                            body: { runIds: [id] },
                          })
                        }
                      >
                        Hide
                      </button>
                    </div>
                  </motion.div>
                );
              })}
            </AnimatePresence>
            {!runs.length ? <EmptyState text="No runs yet." /> : null}
          </div>
        </PageCard>

        <PageCard title="Task Board" subtitle="Animated run graph + task statuses">
          <div className="mb-3 rounded-xl border border-slate-700/60 bg-black/20 p-3">
            <svg viewBox="0 0 700 180" className="h-[160px] w-full">
              {tasks.slice(0, 8).map((task: any, index: number) => {
                const x = 60 + index * 82;
                const y = 90 + Math.sin(index * 0.9) * 20;
                const isActive =
                  String(task?.stepStatus || task?.status || "")
                    .toLowerCase()
                    .includes("progress") ||
                  String(task?.stepStatus || task?.status || "")
                    .toLowerCase()
                    .includes("running");
                return (
                  <g key={String(task?.taskId || task?.step_id || index)}>
                    {index > 0 ? (
                      <line
                        x1={x - 82}
                        y1={90}
                        x2={x}
                        y2={y}
                        stroke="rgba(148,163,184,.35)"
                        strokeWidth="1.4"
                      />
                    ) : null}
                    <motion.circle
                      cx={x}
                      cy={y}
                      r={isActive ? 10 : 8}
                      fill={isActive ? "rgba(245,158,11,.85)" : "rgba(71,85,105,.85)"}
                      animate={isActive ? { r: [9, 12, 9], opacity: [0.8, 1, 0.8] } : {}}
                      transition={{ repeat: isActive ? Infinity : 0, duration: 1.2 }}
                    />
                  </g>
                );
              })}
            </svg>
            <div className="tcp-subtle text-xs">
              Active tasks: {activeTasks.length} / {tasks.length}
            </div>
          </div>

          <div className="mb-3 rounded-xl border border-slate-700/60 bg-slate-900/30 p-3">
            <div className="mb-1 flex items-center justify-between gap-2">
              <div className="font-medium">Run Output</div>
              {latestOutput?.sessionId ? (
                <button
                  className="tcp-btn h-7 px-2 text-xs"
                  onClick={() => {
                    saveStoredSessionId(String(latestOutput.sessionId));
                    navigate("chat");
                  }}
                >
                  Open Session
                </button>
              ) : null}
            </div>
            {latestOutput?.sessionId ? (
              <>
                <div className="tcp-subtle text-xs">
                  step: {String(latestOutput?.event?.step_id || "n/a")} | session:{" "}
                  {String(latestOutput.sessionId)}
                </div>
                <div className="mt-2 tcp-code max-h-40 overflow-auto whitespace-pre-wrap break-words">
                  {latestAssistantOutput || "No assistant output text found in this session yet."}
                </div>
              </>
            ) : (
              <div className="tcp-subtle text-xs">
                No completed step output session yet. Run at least one step to populate results.
              </div>
            )}
          </div>

          <div className="grid max-h-[40vh] gap-2 overflow-auto">
            {tasks.length ? (
              tasks.map((task: any, index: number) => {
                const stepId = String(task?.taskId || task?.step_id || `step-${index}`);
                const sessionId = String(
                  sessionsByStepId.get(stepId) || task?.sessionId || task?.session_id || ""
                );
                const rawTitle = String(task?.title || stepId);
                const isExpanded = Boolean(expandedTaskTitles[stepId]);
                const shouldClamp = rawTitle.length > 180;
                const displayTitle =
                  shouldClamp && !isExpanded ? `${rawTitle.slice(0, 180).trimEnd()}...` : rawTitle;
                return (
                  <div key={stepId} className="tcp-list-item">
                    <div className="mb-1 flex items-start justify-between gap-2">
                      <strong
                        className="min-w-0 flex-1 text-sm leading-snug"
                        style={{ overflowWrap: "anywhere", wordBreak: "break-word" }}
                        title={rawTitle}
                      >
                        {displayTitle}
                      </strong>
                      <span
                        className={statusBadgeClass(
                          String(task?.stepStatus || task?.status || "pending")
                        )}
                      >
                        {String(task?.stepStatus || task?.status || "pending")}
                      </span>
                    </div>
                    {shouldClamp ? (
                      <button
                        className="tcp-btn mb-1 h-6 px-2 text-[11px]"
                        onClick={() => toggleTaskTitle(stepId)}
                      >
                        {isExpanded ? "Less" : "More"}
                      </button>
                    ) : null}
                    <div className="tcp-subtle text-xs" style={{ overflowWrap: "anywhere" }}>
                      {stepId}
                    </div>
                    {task?.workflowId ? (
                      <div className="tcp-subtle text-xs">workflow: {String(task.workflowId)}</div>
                    ) : null}
                    {task?.assignedAgent ? (
                      <div className="tcp-subtle text-xs">agent: {String(task.assignedAgent)}</div>
                    ) : null}
                    <div className="mt-2 flex gap-2">
                      <button
                        className="tcp-btn h-7 px-2 text-xs"
                        onClick={() =>
                          navigator.clipboard?.writeText(`runId=${runId}\nstepId=${stepId}`)
                        }
                      >
                        Copy IDs
                      </button>
                      <button
                        className="tcp-btn h-7 px-2 text-xs"
                        onClick={() =>
                          actionMutation.mutate({
                            path: "/api/swarm/retry",
                            body: { runId, stepId },
                          })
                        }
                      >
                        Retry
                      </button>
                      {sessionId ? (
                        <button
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={() => {
                            saveStoredSessionId(sessionId);
                            navigate("chat");
                          }}
                        >
                          Open Session
                        </button>
                      ) : null}
                    </div>
                  </div>
                );
              })
            ) : (
              <EmptyState text="No task data yet." />
            )}
          </div>

          {blackboard && blackboardMode === "docked" ? renderBlackboardPanel(false) : null}
        </PageCard>

        {blackboard && blackboardMode === "expanded" ? (
          <div className="xl:col-span-2">{renderBlackboardPanel(false)}</div>
        ) : null}
      </div>

      {blackboard && blackboardMode === "fullscreen" ? renderBlackboardPanel(true) : null}
    </>
  );
}
