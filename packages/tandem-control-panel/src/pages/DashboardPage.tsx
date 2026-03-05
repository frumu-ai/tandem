import { useQuery } from "@tanstack/react-query";
import { PageCard, EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

export function DashboardPage(props: AppPageProps) {
  const { api, client, navigate } = props;

  const health = useQuery({
    queryKey: ["dashboard", "health"],
    queryFn: () => api("/api/system/health"),
    refetchInterval: 15000,
  });
  const sessions = useQuery({
    queryKey: ["dashboard", "sessions"],
    queryFn: () => client.sessions.list({ pageSize: 8 }).catch(() => []),
    refetchInterval: 15000,
  });
  const routines = useQuery({
    queryKey: ["dashboard", "routines"],
    queryFn: () => client.routines.list().catch(() => ({ routines: [] })),
    refetchInterval: 20000,
  });
  const swarm = useQuery({
    queryKey: ["dashboard", "swarm"],
    queryFn: () => api("/api/swarm/status").catch(() => ({ status: "unknown" })),
    refetchInterval: 6000,
  });

  const sessionRows = toArray(sessions.data, "sessions");
  const routineRows = toArray(routines.data, "routines");

  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <PageCard title="System Health" subtitle="Engine + provider status">
        <div className="grid gap-2 text-sm">
          <div className="tcp-list-item flex items-center justify-between">
            <span>Engine</span>
            <span
              className={
                health.data?.engine?.ready || health.data?.engine?.healthy
                  ? "tcp-badge-ok"
                  : "tcp-badge-warn"
              }
            >
              {health.data?.engine?.ready || health.data?.engine?.healthy ? "healthy" : "unknown"}
            </span>
          </div>
          <div className="tcp-list-item flex items-center justify-between">
            <span>Provider</span>
            <span className={props.providerStatus.ready ? "tcp-badge-ok" : "tcp-badge-warn"}>
              {props.providerStatus.ready
                ? `${props.providerStatus.defaultProvider}/${props.providerStatus.defaultModel}`
                : "setup required"}
            </span>
          </div>
          <div className="tcp-list-item flex items-center justify-between">
            <span>Swarm</span>
            <span className="tcp-badge-info">{String(swarm.data?.status || "unknown")}</span>
          </div>
        </div>
      </PageCard>

      <PageCard title="Quick Actions" subtitle="Jump to frequent tasks">
        <div className="grid grid-cols-2 gap-2">
          <button className="tcp-btn" onClick={() => navigate("chat")}>
            <i data-lucide="message-square"></i>
            Open Chat
          </button>
          <button className="tcp-btn" onClick={() => navigate("orchestrator")}>
            <i data-lucide="workflow"></i>
            Open Orchestrator
          </button>
          <button className="tcp-btn" onClick={() => navigate("agents")}>
            <i data-lucide="users"></i>
            Automations
          </button>
          <button className="tcp-btn" onClick={() => navigate("settings")}>
            <i data-lucide="settings"></i>
            Settings
          </button>
        </div>
      </PageCard>

      <PageCard title="Recent Sessions" subtitle="Latest chat sessions">
        <div className="grid gap-2">
          {sessionRows.length ? (
            sessionRows.map((session: any) => (
              <button
                key={String(session.id || session.session_id || Math.random())}
                className="tcp-list-item text-left"
                onClick={() => navigate("chat")}
              >
                <div className="font-medium inline-flex items-center gap-1.5">
                  <i data-lucide="messages-square"></i>
                  {String(session.title || session.id || "Session")}
                </div>
                <div className="tcp-subtle text-xs">
                  {String(session.id || session.session_id || "")}
                </div>
              </button>
            ))
          ) : (
            <EmptyState text="No sessions yet." />
          )}
        </div>
      </PageCard>

      <PageCard title="Routines" subtitle="Scheduled automations overview">
        <div className="grid gap-2">
          {routineRows.length ? (
            routineRows.slice(0, 8).map((routine: any) => (
              <div
                key={String(routine.id || routine.routine_id || Math.random())}
                className="tcp-list-item"
              >
                <div className="font-medium">{String(routine.name || routine.id || "Routine")}</div>
                <div className="tcp-subtle text-xs">
                  {String(routine.schedule || routine.status || "manual")}
                </div>
              </div>
            ))
          ) : (
            <EmptyState text="No routines configured." />
          )}
        </div>
      </PageCard>
    </div>
  );
}
