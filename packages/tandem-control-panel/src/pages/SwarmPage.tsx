import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { useMemo, useState } from "react";
import { useEngineStream } from "../features/stream/useEngineStream";
import { PageCard, EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

function normalizeTasks(runPayload: any) {
  if (Array.isArray(runPayload?.tasks)) return runPayload.tasks;
  return [];
}

export function SwarmPage({ api, toast, navigate }: AppPageProps) {
  const queryClient = useQueryClient();
  const [workspaceRoot, setWorkspaceRoot] = useState("");
  const [objective, setObjective] = useState("Ship a small feature end-to-end");
  const [maxTasks, setMaxTasks] = useState("3");
  const [selectedRunId, setSelectedRunId] = useState("");

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
  const runId = selectedRunId || String(statusQuery.data?.runId || runs[0]?.run_id || "");

  const runQuery = useQuery({
    queryKey: ["swarm", "run", runId],
    enabled: !!runId,
    queryFn: () => api(`/api/swarm/run/${encodeURIComponent(runId)}`),
    refetchInterval: 4500,
  });

  const tasks = normalizeTasks(runQuery.data);

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
    onSuccess: async () => {
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

  return (
    <div className="grid gap-4 xl:grid-cols-[1.05fr_1fr]">
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

        <div className="grid max-h-[46vh] gap-2 overflow-auto">
          <AnimatePresence initial={false}>
            {runs.map((run: any) => {
              const id = String(run?.run_id || run?.runId || "");
              const active = id === runId;
              return (
                <motion.button
                  key={id}
                  className={`tcp-list-item text-left ${active ? "border-amber-400/60" : ""}`}
                  onClick={() => setSelectedRunId(id)}
                  initial={{ opacity: 0, y: 6 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -6 }}
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="truncate font-medium">{String(run?.objective || id)}</span>
                    <span className="tcp-badge-info">{String(run?.status || "unknown")}</span>
                  </div>
                  <div className="tcp-subtle text-xs">{id}</div>
                </motion.button>
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

        <div className="grid max-h-[40vh] gap-2 overflow-auto">
          {tasks.length ? (
            tasks.map((task: any, index: number) => {
              const stepId = String(task?.taskId || task?.step_id || `step-${index}`);
              const sessionId = String(task?.sessionId || task?.session_id || "");
              return (
                <div key={stepId} className="tcp-list-item">
                  <div className="mb-1 flex items-center justify-between gap-2">
                    <strong>{String(task?.title || stepId)}</strong>
                    <span className="tcp-badge-warn">
                      {String(task?.stepStatus || task?.status || "pending")}
                    </span>
                  </div>
                  <div className="tcp-subtle text-xs">{stepId}</div>
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
                        actionMutation.mutate({ path: "/api/swarm/retry", body: { runId, stepId } })
                      }
                    >
                      Retry
                    </button>
                    {sessionId ? (
                      <button className="tcp-btn h-7 px-2 text-xs" onClick={() => navigate("chat")}>
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
      </PageCard>
    </div>
  );
}
