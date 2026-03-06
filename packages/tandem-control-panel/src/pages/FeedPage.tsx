import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useEngineStream } from "../features/stream/useEngineStream";
import { api } from "../lib/api";
import { AnimatedPage, DetailDrawer, FilterChip, PanelCard, Toolbar } from "../ui/index.tsx";
import { EmptyState, formatJson } from "./ui";
import type { AppPageProps } from "./pageTypes";

function eventTypeOf(data: any) {
  return data?.type || data?.event || "event";
}

function isNoisyEventType(type: string) {
  const normalized = String(type || "")
    .trim()
    .toLowerCase();
  return [
    "server.connected",
    "engine.lifecycle.ready",
    "engine.heartbeat",
    "engine.lifecycle.heartbeat",
  ].includes(normalized);
}

function isWorkflowContextStreamEvent(data: any) {
  return (
    String(data?.type || data?.event || "").trim() === "context.run.stream" &&
    String(data?.run_id || data?.runId || "")
      .trim()
      .startsWith("workflow-")
  );
}

export function FeedPage({ client, toast, navigate }: AppPageProps) {
  const [events, setEvents] = useState<Array<{ at: number; data: any }>>([]);
  const [filter, setFilter] = useState("");
  const [group, setGroup] = useState("all");
  const [hideNoise, setHideNoise] = useState(true);
  const [selectedEvent, setSelectedEvent] = useState<any | null>(null);
  const selectedWorkflowContextRunId = useMemo(() => {
    if (!selectedEvent || !isWorkflowContextStreamEvent(selectedEvent.data)) return "";
    return String(selectedEvent.data?.run_id || selectedEvent.data?.runId || "").trim();
  }, [selectedEvent]);
  const workflowContextDetail = useQuery({
    queryKey: ["feed", "workflow-context-run", selectedWorkflowContextRunId],
    enabled: !!selectedWorkflowContextRunId,
    queryFn: () =>
      api(`/api/engine/context/runs/${encodeURIComponent(selectedWorkflowContextRunId)}`).catch(
        () => ({ run: null })
      ),
  });
  const workflowContextBlackboard = useQuery({
    queryKey: ["feed", "workflow-context-blackboard", selectedWorkflowContextRunId],
    enabled: !!selectedWorkflowContextRunId,
    queryFn: () =>
      api(
        `/api/engine/context/runs/${encodeURIComponent(selectedWorkflowContextRunId)}/blackboard`
      ).catch(() => ({ blackboard: null })),
  });

  useEngineStream(
    "/api/engine/global/event",
    (event) => {
      try {
        const data = JSON.parse(event.data);
        setEvents((prev) => [...prev.slice(-299), { at: Date.now(), data }]);
      } catch {
        // ignore malformed events
      }
    },
    {
      enabled: true,
    }
  );

  const groupedTypes = useMemo(() => {
    const counts = new Map<string, number>();
    for (const item of events) {
      const key = eventTypeOf(item.data);
      if (hideNoise && isNoisyEventType(key)) continue;
      counts.set(key, (counts.get(key) || 0) + 1);
    }
    return [...counts.entries()].sort((a, b) => b[1] - a[1]);
  }, [events, hideNoise]);

  const filtered = useMemo(() => {
    const term = filter.trim().toLowerCase();
    return events
      .filter((item) => {
        const type = eventTypeOf(item.data);
        if (hideNoise && isNoisyEventType(type)) return false;
        if (group !== "all" && type !== group) return false;
        if (!term) return true;
        return `${type} ${JSON.stringify(item.data || {})}`.toLowerCase().includes(term);
      })
      .slice(-240)
      .reverse();
  }, [events, filter, group, hideNoise]);

  async function installFromPath(path: string) {
    try {
      const payload = await api("/api/engine/packs/install", {
        method: "POST",
        body: JSON.stringify({
          path,
          source: { kind: "control_panel_feed", event: "pack.detected" },
        }),
      });
      toast("ok", `Installed ${payload?.installed?.name || "pack"}`);
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  }

  async function installFromAttachment(evt: any) {
    try {
      const payload = await api("/api/engine/packs/install-from-attachment", {
        method: "POST",
        body: JSON.stringify({
          attachment_id: String(evt?.properties?.attachment_id || evt?.attachment_id || ""),
          path: String(evt?.properties?.path || evt?.path || ""),
          connector: String(evt?.properties?.connector || evt?.connector || "") || undefined,
          channel_id: String(evt?.properties?.channel_id || evt?.channel_id || "") || undefined,
          sender_id: String(evt?.properties?.sender_id || evt?.sender_id || "") || undefined,
        }),
      });
      toast("ok", `Installed ${payload?.installed?.name || "pack"}`);
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  }

  return (
    <AnimatedPage className="grid gap-4">
      <PanelCard
        title="Live feed"
        subtitle="Filter events and inspect raw payload details."
        actions={
          <div className="flex flex-wrap items-center justify-end gap-2">
            <span className="tcp-badge-info">{events.length} buffered</span>
            <span className="tcp-badge tcp-badge-ghost">
              {group === "all" ? "All event types" : group}
            </span>
            <span className={hideNoise ? "tcp-badge-ok" : "tcp-badge-info"}>
              {hideNoise ? "noise hidden" : "noise visible"}
            </span>
            <button className="tcp-btn" onClick={() => setEvents([])}>
              <i data-lucide="trash-2"></i>
              Clear feed
            </button>
            <button className="tcp-btn" onClick={() => navigate("packs-detail")}>
              <i data-lucide="package"></i>
              Pack library
            </button>
          </div>
        }
      >
        <Toolbar className="mb-3">
          <input
            className="tcp-input min-w-[220px] flex-1"
            value={filter}
            onInput={(e) => setFilter((e.target as HTMLInputElement).value)}
            placeholder="Filter by type or payload"
          />
          <FilterChip active={group === "all"} onClick={() => setGroup("all")}>
            <i data-lucide="list"></i>
            All
          </FilterChip>
          <FilterChip active={hideNoise} onClick={() => setHideNoise((prev) => !prev)}>
            <i data-lucide={hideNoise ? "filter" : "filter-x"}></i>
            {hideNoise ? "Hide noise" : "Show noise"}
          </FilterChip>
          {groupedTypes.slice(0, 8).map(([type, count]) => (
            <FilterChip key={type} active={group === type} onClick={() => setGroup(type)}>
              <i data-lucide="activity"></i>
              {type} ({count})
            </FilterChip>
          ))}
        </Toolbar>

        <div className="grid max-h-[68vh] gap-2 overflow-auto rounded-2xl border border-slate-700/60 bg-black/20 p-2">
          {filtered.length ? (
            filtered.map((x, index) => {
              const type = eventTypeOf(x.data);
              const isPack = String(type).startsWith("pack.");
              const isWorkflowContext = isWorkflowContextStreamEvent(x.data);
              const path = String(x.data?.properties?.path || x.data?.path || "");
              const attachmentId = String(
                x.data?.properties?.attachment_id || x.data?.attachment_id || ""
              );
              return (
                <article
                  key={`${x.at}-${index}`}
                  className="tcp-list-item cursor-pointer"
                  onClick={() => setSelectedEvent(x)}
                >
                  <div className="flex items-center justify-between gap-2">
                    <strong>{type}</strong>
                    <div className="flex items-center gap-2">
                      {isWorkflowContext ? (
                        <span className="tcp-badge-warn">workflow context</span>
                      ) : null}
                      <span className="tcp-badge-info">{new Date(x.at).toLocaleTimeString()}</span>
                    </div>
                  </div>
                  <p className="tcp-subtle mt-1 text-xs">
                    {isWorkflowContext
                      ? `run: ${String(x.data?.run_id || x.data?.runId || "n/a")}`
                      : `session: ${String(x.data?.sessionID || x.data?.sessionId || "n/a")}`}
                  </p>
                  {isWorkflowContext ? (
                    <p className="tcp-subtle mt-1 text-xs">
                      kind: {String(x.data?.kind || "context_run_event")} · seq:{" "}
                      {String(x.data?.seq || "-")}
                    </p>
                  ) : null}
                  {isPack ? (
                    <div className="mt-2 flex flex-wrap gap-2">
                      <button
                        className="tcp-btn h-7 px-2 text-xs"
                        onClick={(e) => {
                          e.stopPropagation();
                          navigate("packs-detail");
                        }}
                      >
                        <i data-lucide="package"></i>
                        Open pack library
                      </button>
                      {path ? (
                        <button
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={(e) => {
                            e.stopPropagation();
                            installFromPath(path);
                          }}
                        >
                          <i data-lucide="download"></i>
                          Install from path
                        </button>
                      ) : null}
                      {path && attachmentId ? (
                        <button
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={(e) => {
                            e.stopPropagation();
                            installFromAttachment(x.data);
                          }}
                        >
                          <i data-lucide="paperclip"></i>
                          Install attachment
                        </button>
                      ) : null}
                    </div>
                  ) : null}
                  {isWorkflowContext ? (
                    <div className="mt-2 flex flex-wrap gap-2">
                      <button
                        className="tcp-btn h-7 px-2 text-xs"
                        onClick={(e) => {
                          e.stopPropagation();
                          navigate("orchestrator");
                        }}
                      >
                        <i data-lucide="workflow"></i>
                        Open orchestrator
                      </button>
                      <button
                        className="tcp-btn h-7 px-2 text-xs"
                        onClick={(e) => {
                          e.stopPropagation();
                          navigate("packs-detail");
                        }}
                      >
                        <i data-lucide="package"></i>
                        Open workflow lab
                      </button>
                    </div>
                  ) : null}
                </article>
              );
            })
          ) : (
            <EmptyState text="No events have arrived yet." />
          )}
        </div>
      </PanelCard>

      <DetailDrawer
        open={!!selectedEvent}
        onClose={() => setSelectedEvent(null)}
        title={selectedEvent ? eventTypeOf(selectedEvent.data) : "Event"}
      >
        {selectedEvent ? (
          <div className="grid gap-3">
            <div className="tcp-list-item">
              <div className="font-medium">Captured at</div>
              <div className="tcp-subtle mt-1 text-xs">
                {new Date(selectedEvent.at).toLocaleString()}
              </div>
            </div>
            {selectedWorkflowContextRunId ? (
              <div className="tcp-list-item">
                <div className="mb-1 flex items-center justify-between gap-2">
                  <strong>
                    {String(
                      workflowContextDetail.data?.run?.objective || selectedWorkflowContextRunId
                    )}
                  </strong>
                  <span className="tcp-badge-info">
                    {String(workflowContextDetail.data?.run?.status || "unknown")}
                  </span>
                </div>
                <div className="tcp-subtle text-xs">
                  tasks:{" "}
                  {Array.isArray(workflowContextBlackboard.data?.blackboard?.tasks)
                    ? workflowContextBlackboard.data.blackboard.tasks.length
                    : 0}
                  {" · "}artifacts:{" "}
                  {Array.isArray(workflowContextBlackboard.data?.blackboard?.artifacts)
                    ? workflowContextBlackboard.data.blackboard.artifacts.length
                    : 0}
                </div>
              </div>
            ) : null}
            <pre className="tcp-code">{formatJson(selectedEvent.data)}</pre>
            {selectedWorkflowContextRunId ? (
              <pre className="tcp-code">
                {formatJson({
                  run: workflowContextDetail.data?.run || null,
                  blackboard: workflowContextBlackboard.data?.blackboard || null,
                })}
              </pre>
            ) : null}
          </div>
        ) : null}
      </DetailDrawer>
    </AnimatedPage>
  );
}
