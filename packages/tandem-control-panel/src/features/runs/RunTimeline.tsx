import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../../lib/api";
import { Badge, EmptyState, IconButton, LoadingState, PanelCard } from "../../ui/index.tsx";
import { useEngineStream } from "../stream/useEngineStream";
import {
  DEFAULT_TIMELINE_LIMIT,
  buildRunTimeline,
  legacyRunTimelineRequestPath,
  runTimelinePageEventCount,
  runTimelineRequestPath,
} from "../../../lib/runs/run-timeline.js";

type RunTimelineEntry = {
  id: string;
  eventId?: string;
  runId?: string;
  source: string;
  eventType: string;
  title: string;
  summary: string;
  tone: "running" | "ok" | "failed" | "info";
  actor?: string;
  phase?: string;
  sequence?: number;
  occurredAtMs?: number;
  sessionId?: string;
  nodeId?: string;
  tenantOrg?: string;
  tenantWorkspace?: string;
};

type UseRunTimelineOptions = {
  runId: string;
  enabled?: boolean;
  limit?: number;
  stream?: boolean;
};

type RunTimelineProps = {
  entries: RunTimelineEntry[];
  loading?: boolean;
  loadingMore?: boolean;
  error?: string;
  title?: string;
  subtitle?: string;
  className?: string;
  hasMore?: boolean;
  onRefresh?: () => void;
  onLoadMore?: () => void;
};

type TimelineRequestOptions = {
  beforeSeq?: number | null;
  limit: number;
  tail?: number | boolean;
};

function formatTime(ms = 0) {
  if (!ms) return "n/a";
  return new Date(ms).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function toneBadge(tone: RunTimelineEntry["tone"]) {
  if (tone === "ok") return "ok";
  if (tone === "failed") return "err";
  if (tone === "running") return "warn";
  return "info";
}

function liveRunStreamPath(runId: string) {
  return runId ? `/api/engine/run/${encodeURIComponent(runId)}/events` : "";
}

function firstSequence(entries: RunTimelineEntry[]) {
  return entries.reduce((min, entry) => {
    const seq = Number(entry.sequence || 0);
    return seq > 0 ? Math.min(min, seq) : min;
  }, Number.POSITIVE_INFINITY);
}

async function fetchRunTimelinePage(runId: string, options: TimelineRequestOptions) {
  let canonicalPage: any = null;
  try {
    canonicalPage = await api(runTimelineRequestPath(runId, options));
    if (runTimelinePageEventCount(canonicalPage) > 0) return canonicalPage;
  } catch (canonicalError) {
    try {
      return await api(legacyRunTimelineRequestPath(runId, options));
    } catch {
      throw canonicalError;
    }
  }

  try {
    const legacyPage = await api(legacyRunTimelineRequestPath(runId, options));
    return runTimelinePageEventCount(legacyPage) > 0 ? legacyPage : canonicalPage;
  } catch {
    return canonicalPage;
  }
}

export function useRunTimeline({
  runId,
  enabled = true,
  limit = DEFAULT_TIMELINE_LIMIT,
  stream = true,
}: UseRunTimelineOptions) {
  const [pages, setPages] = useState<any[]>([]);
  const [liveEvents, setLiveEvents] = useState<any[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState("");
  const activeRunIdRef = useRef(runId);
  activeRunIdRef.current = runId;

  const loadPage = useCallback(
    async (cursorSeq: number | null = null, mode: "replace" | "prepend" = "replace") => {
      if (!runId || !enabled) return;
      const requestedRunId = runId;
      const isCurrentRun = () => activeRunIdRef.current === requestedRunId;
      setError("");
      if (mode === "prepend") setLoadingMore(true);
      else setLoading(true);
      try {
        const page = await fetchRunTimelinePage(requestedRunId, {
          beforeSeq: mode === "prepend" ? cursorSeq : null,
          limit,
          tail: limit,
        });
        if (!isCurrentRun()) return;
        setPages((prev) => (mode === "prepend" ? [page, ...prev] : [page]));
      } catch (err: any) {
        if (!isCurrentRun()) return;
        setError(err instanceof Error ? err.message : String(err));
        if (mode === "replace") setPages([]);
      } finally {
        if (isCurrentRun()) {
          setLoading(false);
          setLoadingMore(false);
        }
      }
    },
    [enabled, limit, runId]
  );

  useEffect(() => {
    setPages([]);
    setLiveEvents([]);
    setError("");
    if (enabled && runId) void loadPage(null, "replace");
  }, [enabled, loadPage, runId]);

  useEngineStream(
    stream && runId ? liveRunStreamPath(runId) : "",
    (message) => {
      try {
        const payload = JSON.parse(String(message?.data || "{}"));
        const type = String(payload?.type || payload?.event_type || "");
        if (!payload || type === "run.stream.connected" || payload.status === "ready") return;
        setLiveEvents((prev) => [...prev, payload].slice(-limit));
      } catch {
        return;
      }
    },
    { enabled: enabled && stream && !!runId }
  );

  const entries = useMemo(
    () => buildRunTimeline({ persistedPages: pages, liveEvents, limit: limit * 4 }),
    [liveEvents, limit, pages]
  );
  const persistedEntries = useMemo(
    () => buildRunTimeline({ persistedPages: pages }),
    [pages]
  );
  const firstPage = pages[0];
  const firstPersistedSeq = firstSequence(persistedEntries);
  const hasMore =
    runTimelinePageEventCount(firstPage) >= limit &&
    Number.isFinite(firstPersistedSeq) &&
    firstPersistedSeq > 1;
  const loadMore = useCallback(() => {
    const beforeSeq = firstSequence(persistedEntries);
    if (!Number.isFinite(beforeSeq)) return;
    void loadPage(beforeSeq, "prepend");
  }, [loadPage, persistedEntries]);

  return {
    entries,
    error,
    hasMore,
    loading,
    loadingMore,
    refresh: () => void loadPage(0, "replace"),
    loadMore,
  };
}

export function RunTimeline({
  entries,
  loading = false,
  loadingMore = false,
  error = "",
  title = "Event Timeline",
  subtitle,
  className = "",
  hasMore = false,
  onRefresh,
  onLoadMore,
}: RunTimelineProps) {
  return (
    <PanelCard
      className={className}
      title={title}
      subtitle={subtitle}
      actions={
        onRefresh ? (
          <IconButton title="Refresh timeline" aria-label="Refresh timeline" onClick={onRefresh}>
            <i data-lucide="refresh-cw"></i>
          </IconButton>
        ) : null
      }
    >
      {error ? (
        <div className="mb-3">
          <Badge tone="warn">{error}</Badge>
        </div>
      ) : null}
      {loading && !entries.length ? (
        <LoadingState title="Loading timeline" text="Fetching run events" />
      ) : entries.length ? (
        <div className="grid gap-3">
          <ol className="grid max-h-[34rem] gap-2 overflow-auto pr-1">
            {entries.map((entry) => (
              <li
                key={entry.id}
                className="grid gap-3 rounded-lg border border-white/10 bg-black/20 p-3 md:grid-cols-[8rem_minmax(0,1fr)]"
              >
                <div className="min-w-0 text-xs text-tcp-text-tertiary">
                  <div className="font-mono text-[11px]">{formatTime(entry.occurredAtMs)}</div>
                  <div className="mt-2 flex flex-wrap gap-1">
                    <Badge tone={toneBadge(entry.tone)}>{entry.source}</Badge>
                    {entry.sequence ? <Badge tone="ghost">#{entry.sequence}</Badge> : null}
                  </div>
                </div>
                <div className="min-w-0">
                  <div className="mb-1 flex flex-wrap items-center gap-2">
                    <h4 className="min-w-0 truncate text-sm font-semibold text-tcp-text-primary">
                      {entry.title}
                    </h4>
                    <Badge tone={toneBadge(entry.tone)}>{entry.tone}</Badge>
                  </div>
                  <p className="line-clamp-2 text-xs text-tcp-text-secondary">{entry.summary}</p>
                  <div className="mt-2 flex flex-wrap gap-2 text-[11px] text-tcp-text-tertiary">
                    {entry.actor ? <span>actor: {entry.actor}</span> : null}
                    {entry.phase ? <span>phase: {entry.phase}</span> : null}
                    {entry.nodeId ? <span>node: {entry.nodeId}</span> : null}
                    {entry.tenantOrg || entry.tenantWorkspace ? (
                      <span>
                        scope: {[entry.tenantOrg, entry.tenantWorkspace].filter(Boolean).join("/")}
                      </span>
                    ) : null}
                    {entry.eventId ? <span className="font-mono">event: {entry.eventId}</span> : null}
                  </div>
                </div>
              </li>
            ))}
          </ol>
          {hasMore && onLoadMore ? (
            <button
              type="button"
              className="tcp-btn h-8 w-fit px-3 text-xs"
              disabled={loadingMore}
              onClick={onLoadMore}
            >
              <i data-lucide={loadingMore ? "loader-2" : "chevrons-down"}></i>
              {loadingMore ? "Loading" : "Load Older"}
            </button>
          ) : null}
        </div>
      ) : (
        <EmptyState title="No events yet" text="Run events will appear once the runtime records them." />
      )}
    </PanelCard>
  );
}
