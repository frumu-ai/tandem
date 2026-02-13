import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { List, type RowComponentProps, useListCallbackRef } from "react-window";
import {
  FileText,
  Pause,
  Play,
  ScrollText,
  Search,
  Terminal,
  Trash2,
  X,
  Maximize2,
  Minimize2,
  Copy,
} from "lucide-react";
import { ConsoleTab } from "./ConsoleTab";
import { cn } from "@/lib/utils";
import {
  listAppLogFiles,
  onLogStreamEvent,
  startLogStream,
  stopLogStream,
  type LogFileInfo,
} from "@/lib/tauri";

const MAX_RENDER_LINES = 5000;
const DEFAULT_TAIL_LINES = 500;

type Level = "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR" | "STDOUT" | "STDERR" | "UNKNOWN";
type LevelFilter = "ALL" | Exclude<Level, "UNKNOWN">;
type ProvenanceFilter = "ALL" | "CURRENT" | "LEGACY";
type Provenance = "current" | "legacy";

type ParsedLine = {
  ts?: string;
  ts_ms?: number;
  level: Level;
  target?: string;
  process?: string;
  component?: string;
  event?: string;
  correlation_id?: string;
  session_id?: string;
  msg: string;
  provenance: Provenance;
};

type LineItem = {
  id: number;
  raw: string;
  parsed: ParsedLine;
};

function parseLine(raw: string): ParsedLine {
  const trimmed = raw.replace(/\r?\n$/, "");
  const lower = trimmed.toLowerCase();
  const legacy =
    lower.includes("opencode") ||
    lower.includes("ai.frumu.tandem") ||
    lower.includes(".local\\share\\opencode") ||
    lower.includes(".local/share/opencode") ||
    lower.includes(".config\\opencode") ||
    lower.includes(".config/opencode");
  const provenance: Provenance = legacy ? "legacy" : "current";

  if (trimmed.startsWith("{") && trimmed.endsWith("}")) {
    try {
      const obj = JSON.parse(trimmed) as Record<string, unknown>;
      const fieldString = (key: string, nested?: Record<string, unknown>): string | undefined => {
        const fromNested = nested?.[key];
        if (typeof fromNested === "string" && fromNested.trim().length > 0) {
          return fromNested;
        }
        const fromTop = obj[key];
        if (typeof fromTop === "string" && fromTop.trim().length > 0) {
          return fromTop;
        }
        return undefined;
      };
      const levelRaw = typeof obj.level === "string" ? obj.level.toUpperCase() : "UNKNOWN";
      const level: Level =
        levelRaw === "TRACE" ||
        levelRaw === "DEBUG" ||
        levelRaw === "INFO" ||
        levelRaw === "WARN" ||
        levelRaw === "ERROR"
          ? (levelRaw as Level)
          : "UNKNOWN";
      const ts =
        (typeof obj.timestamp === "string" && obj.timestamp) ||
        (typeof obj.ts === "string" && obj.ts) ||
        undefined;
      const parsedTs = ts ? Date.parse(ts) : NaN;
      const target =
        (typeof obj.target === "string" && obj.target) ||
        (typeof obj.component === "string" && obj.component) ||
        undefined;
      const fields =
        obj.fields && typeof obj.fields === "object"
          ? (obj.fields as Record<string, unknown>)
          : undefined;
      const process = fieldString("process", fields);
      const component = fieldString("component", fields);
      const event = fieldString("event", fields);
      const correlation_id = fieldString("correlation_id", fields);
      const session_id = fieldString("session_id", fields);
      const message =
        (fields && typeof fields.message === "string" && fields.message) ||
        (typeof obj.message === "string" && obj.message) ||
        (fields && typeof fields.event === "string" && fields.event) ||
        trimmed;
      return {
        ts,
        ts_ms: Number.isFinite(parsedTs) ? parsedTs : undefined,
        level,
        target,
        process,
        component,
        event,
        correlation_id,
        session_id,
        msg: message,
        provenance,
      };
    } catch {
      // Fall through to text parsing
    }
  }

  const sidecar = trimmed.match(/^(STDOUT|STDERR)\s*(?:[:-])?\s*(.*)$/);
  if (sidecar) {
    const level = sidecar[1] as "STDOUT" | "STDERR";
    return { level, msg: sidecar[2] ?? "", provenance };
  }

  // Common tracing format:
  // 2026-02-09T15:30:55.123Z  INFO module::path: message...
  const m = trimmed.match(/^(\S+)\s+(TRACE|DEBUG|INFO|WARN|ERROR)\s+([^:]+):\s*(.*)$/);
  if (m) {
    const parsedTs = Date.parse(m[1]);
    return {
      ts: m[1],
      ts_ms: Number.isFinite(parsedTs) ? parsedTs : undefined,
      level: m[2] as Level,
      target: m[3],
      msg: m[4] ?? "",
      provenance,
    };
  }

  // Fallback: [LEVEL] ... or LEVEL ...
  const m2 = trimmed.match(/^\[?(TRACE|DEBUG|INFO|WARN|ERROR)\]?\s+(.*)$/);
  if (m2) {
    return { level: m2[1] as Level, msg: m2[2] ?? "", provenance };
  }

  return { level: "UNKNOWN", msg: trimmed, provenance };
}

function levelBadgeClasses(level: Level): string {
  switch (level) {
    case "ERROR":
    case "STDERR":
      return "border-red-500/30 bg-red-500/15 text-red-200";
    case "WARN":
      return "border-amber-500/30 bg-amber-500/15 text-amber-200";
    case "INFO":
    case "STDOUT":
      return "border-emerald-500/30 bg-emerald-500/15 text-emerald-200";
    case "DEBUG":
      return "border-sky-500/30 bg-sky-500/15 text-sky-200";
    case "TRACE":
      return "border-violet-500/30 bg-violet-500/15 text-violet-200";
    default:
      return "border-border bg-surface-elevated text-text-subtle";
  }
}

function formatLevel(level: Level): string {
  if (level === "STDOUT") return "OUT";
  if (level === "STDERR") return "ERR";
  if (level === "UNKNOWN") return "LOG";
  return level;
}

function useMeasuredHeight() {
  const ref = useRef<HTMLDivElement | null>(null);
  const [height, setHeight] = useState(320);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => {
      const h = el.getBoundingClientRect().height;
      if (Number.isFinite(h) && h > 0) setHeight(h);
    };
    measure();

    // ResizeObserver is supported in modern Chromium/WebKit; Tauri ships a modern runtime.
    const RO = globalThis.ResizeObserver;
    let ro: { observe: (target: any) => void; disconnect: () => void } | null = null;
    if (RO) {
      ro = new RO((entries) => {
        const h = entries[0]?.contentRect?.height;
        if (typeof h === "number" && h > 0) setHeight(h);
      });
      ro.observe(el);
    }
    const onResize = () => measure();
    globalThis.addEventListener("resize", onResize);
    return () => {
      ro?.disconnect();
      globalThis.removeEventListener("resize", onResize);
    };
  }, []);

  return { ref, height };
}

function pickNewest(files: LogFileInfo[]): string | null {
  if (!files || files.length === 0) return null;
  const sorted = [...files].sort((a, b) => b.modified_ms - a.modified_ms);
  return sorted[0]?.name ?? null;
}

export function LogsDrawer({
  onClose,
  sessionId,
}: {
  onClose: () => void;
  sessionId?: string | null;
}) {
  const [tab, setTab] = useState<"tandem" | "console">("tandem");
  const [files, setFiles] = useState<LogFileInfo[]>([]);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [paused, setPaused] = useState(false);
  const [search, setSearch] = useState("");
  const [levelFilter, setLevelFilter] = useState<LevelFilter>("ALL");
  // Default to current-runtime lines so migrated legacy logs do not dominate the view.
  const [provenanceFilter, setProvenanceFilter] = useState<ProvenanceFilter>("CURRENT");
  const [processFilter, setProcessFilter] = useState<string>("ALL");
  const [componentFilter, setComponentFilter] = useState<string>("ALL");
  const [eventFilter, setEventFilter] = useState<string>("ALL");
  const [correlationFilter, setCorrelationFilter] = useState("");
  const [sessionFilter, setSessionFilter] = useState("");
  const [currentRuntimeOnly, setCurrentRuntimeOnly] = useState(true);
  const [follow, setFollow] = useState(true);
  const [lines, setLines] = useState<LineItem[]>([]);
  const [dropped, setDropped] = useState(0);
  const [streamId, setStreamId] = useState<string | null>(null);
  const [selectedLine, setSelectedLine] = useState<LineItem | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [toastMsg, setToastMsg] = useState<string | null>(null);

  const streamIdRef = useRef<string | null>(null);
  const nextLineIdRef = useRef(1);
  const pendingRawLinesRef = useRef<string[]>([]);
  const [listApi, setListApi] = useListCallbackRef(null);
  const toastTimerRef = useRef<ReturnType<typeof globalThis.setTimeout> | null>(null);

  const { ref: listContainerRef, height: listHeight } = useMeasuredHeight();

  useEffect(() => {
    streamIdRef.current = streamId;
  }, [streamId]);

  // Listen once for stream events; we filter by stream_id locally.
  useEffect(() => {
    let unlisten: null | (() => void) = null;
    onLogStreamEvent((batch) => {
      if (!streamIdRef.current) return;
      if (batch.stream_id !== streamIdRef.current) return;

      if (typeof batch.dropped === "number") {
        setDropped((prev) => Math.max(prev, batch.dropped ?? 0));
      }

      if (batch.lines && batch.lines.length > 0) {
        pendingRawLinesRef.current.push(...batch.lines);
      }
    })
      .then((u) => {
        unlisten = u;
      })
      .catch((e) => {
        console.error("[LogsDrawer] Failed to listen for log stream events:", e);
      });

    return () => {
      try {
        unlisten?.();
      } catch {
        // ignore
      }
    };
  }, []);

  // Flush pending lines into React state at a controlled cadence.
  useEffect(() => {
    const timer = setInterval(() => {
      if (paused) return;
      const pending = pendingRawLinesRef.current;
      if (pending.length === 0) return;

      // Drain quickly; backend already batches, we just avoid render storms.
      const chunk = pending.splice(0, pending.length);
      const items: LineItem[] = chunk.map((raw) => {
        const id = nextLineIdRef.current++;
        return { id, raw, parsed: parseLine(raw) };
      });

      setLines((prev) => {
        const next = prev.concat(items);
        if (next.length <= MAX_RENDER_LINES) return next;
        return next.slice(next.length - MAX_RENDER_LINES);
      });
    }, 50);

    return () => clearInterval(timer);
  }, [paused]);

  const stopCurrentStream = useCallback(async () => {
    if (!streamIdRef.current) return;
    const id = streamIdRef.current;
    streamIdRef.current = null;
    setStreamId(null);
    pendingRawLinesRef.current = [];
    try {
      await stopLogStream(id);
    } catch (e) {
      console.warn("[LogsDrawer] Failed to stop stream:", e);
    }
  }, []);

  const startCurrentStream = useCallback(
    async (next: { source: "tandem"; fileName?: string }) => {
      await stopCurrentStream();
      setDropped(0);

      try {
        const id = await startLogStream({
          windowLabel: "main",
          source: next.source,
          fileName: next.fileName,
          tailLines: DEFAULT_TAIL_LINES,
        });
        setStreamId(id);
        streamIdRef.current = id;
      } catch (e) {
        console.error("[LogsDrawer] Failed to start log stream:", e);
      }
    },
    [stopCurrentStream]
  );

  // Load file list on open (and pick newest by default).
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const f = await listAppLogFiles();
        if (cancelled) return;
        setFiles(f);
        setSelectedFile((prev) => prev ?? pickNewest(f));
      } catch (e) {
        console.error("[LogsDrawer] Failed to list app log files:", e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Start/Restart streaming based on tab, file, and paused state.
  useEffect(() => {
    // Avoid calling setState synchronously inside effect body (lint rule); schedule a microtask.
    let cancelled = false;
    void Promise.resolve().then(() => {
      if (cancelled) return;

      if (paused) {
        void stopCurrentStream();
        return;
      }

      // Tandem file logs
      if (selectedFile) {
        void startCurrentStream({ source: "tandem", fileName: selectedFile });
      }
    });
    return () => {
      cancelled = true;
    };
  }, [tab, selectedFile, paused, startCurrentStream, stopCurrentStream]);

  // Cleanup on close/unmount.
  useEffect(() => {
    return () => {
      void Promise.resolve().then(() => {
        void stopCurrentStream();
      });
    };
  }, [stopCurrentStream]);

  const currentRuntimeCutoffMs = useMemo(() => {
    if (!currentRuntimeOnly) return undefined;

    // Identify the latest desktop startup marker in this file and use that as the runtime cutoff.
    // This avoids the previous behavior where cutoff was "drawer open time", which could hide
    // valid lines if the drawer was opened later.
    let cutoff: number | undefined;
    for (const line of lines) {
      if (
        line.parsed.event === "logging.initialized" &&
        line.parsed.process === "desktop" &&
        typeof line.parsed.ts_ms === "number"
      ) {
        if (cutoff === undefined || line.parsed.ts_ms > cutoff) {
          cutoff = line.parsed.ts_ms;
        }
      }
    }
    return cutoff;
  }, [lines, currentRuntimeOnly]);

  const visibleLines = useMemo(() => {
    const q = search.trim().toLowerCase();
    const correlationQuery = correlationFilter.trim();
    const sessionQuery = sessionFilter.trim();
    return lines.filter((l) => {
      if (levelFilter !== "ALL") {
        const lv = l.parsed.level;
        if (lv !== levelFilter) return false;
      }
      if (processFilter !== "ALL" && (l.parsed.process ?? "") !== processFilter) return false;
      if (componentFilter !== "ALL" && (l.parsed.component ?? "") !== componentFilter) return false;
      if (eventFilter !== "ALL" && (l.parsed.event ?? "") !== eventFilter) return false;
      if (correlationQuery && !(l.parsed.correlation_id ?? "").includes(correlationQuery)) {
        return false;
      }
      if (sessionQuery && !(l.parsed.session_id ?? "").includes(sessionQuery)) {
        return false;
      }
      if (provenanceFilter === "CURRENT") {
        if (l.parsed.provenance !== "current") return false;
      }
      if (typeof currentRuntimeCutoffMs === "number") {
        // Keep current-process lines only after the latest desktop startup marker.
        if (typeof l.parsed.ts_ms === "number" && l.parsed.ts_ms < currentRuntimeCutoffMs) {
          return false;
        }
      }
      if (provenanceFilter === "LEGACY" && l.parsed.provenance !== "legacy") return false;
      if (!q) return true;
      return l.raw.toLowerCase().includes(q);
    });
  }, [
    lines,
    levelFilter,
    processFilter,
    componentFilter,
    eventFilter,
    correlationFilter,
    sessionFilter,
    provenanceFilter,
    currentRuntimeOnly,
    currentRuntimeCutoffMs,
    search,
  ]);

  const processOptions = useMemo(
    () =>
      Array.from(
        new Set(lines.map((l) => l.parsed.process).filter((v): v is string => !!v))
      ).sort(),
    [lines]
  );

  const componentOptions = useMemo(
    () =>
      Array.from(
        new Set(lines.map((l) => l.parsed.component).filter((v): v is string => !!v))
      ).sort(),
    [lines]
  );

  const eventOptions = useMemo(
    () =>
      Array.from(new Set(lines.map((l) => l.parsed.event).filter((v): v is string => !!v))).sort(),
    [lines]
  );

  // Keep the view pinned to the bottom when follow is enabled.
  useEffect(() => {
    if (!follow) return;
    if (paused) return;
    if (visibleLines.length === 0) return;
    listApi?.scrollToRow({ index: visibleLines.length - 1, align: "end", behavior: "instant" });
  }, [follow, paused, visibleLines.length, listApi]);

  const headerSubtitle = useMemo(() => {
    if (paused) return "Paused";
    return selectedFile ? `Tandem logs: ${selectedFile}` : "Tandem logs";
  }, [paused, selectedFile]);

  type RowProps = { items: LineItem[] };

  const Row = ({ index, style, items }: RowComponentProps<RowProps>) => {
    const item = items[index];
    const p = item.parsed;

    return (
      <div style={style} className="px-0" title={item.raw}>
        <button
          type="button"
          onClick={() => setSelectedLine(item)}
          className={cn(
            "group flex w-max min-w-full items-center gap-2 px-3 text-left",
            index % 2 === 0 ? "bg-surface/20" : "bg-surface/0",
            "hover:bg-surface-elevated/60"
          )}
          title="Click to preview/copy the full line"
        >
          <span
            className={cn(
              "shrink-0 rounded-md border px-2 py-0.5 text-[10px] font-medium tracking-wide",
              levelBadgeClasses(p.level)
            )}
          >
            {formatLevel(p.level)}
          </span>

          {p.ts && (
            <span className="shrink-0 font-mono text-[11px] text-text-subtle tabular-nums">
              {p.ts}
            </span>
          )}

          {p.target && (
            <span className="shrink-0 font-mono text-[11px] text-text-muted">{p.target}</span>
          )}

          {p.provenance === "legacy" && (
            <span className="shrink-0 rounded-md border border-amber-500/30 bg-amber-500/10 px-1.5 py-0.5 text-[10px] text-amber-200">
              LEGACY
            </span>
          )}

          <span className="font-mono text-[12px] text-text whitespace-pre">
            {p.msg || item.raw}
          </span>
        </button>
      </div>
    );
  };

  const showToast = useCallback((msg: string) => {
    if (toastTimerRef.current) {
      globalThis.clearTimeout(toastTimerRef.current);
    }
    setToastMsg(msg);
    toastTimerRef.current = globalThis.setTimeout(() => setToastMsg(null), 1400);
  }, []);

  const copyText = useCallback(
    async (text: string, toastLabel: string = "Copied to clipboard") => {
      const value = text.replace(/\r?\n$/, "");
      try {
        const write = globalThis.navigator?.clipboard?.writeText;
        if (typeof write !== "function") {
          throw new Error("Clipboard API not available");
        }
        await write.call(globalThis.navigator!.clipboard, value);
        showToast(toastLabel);
      } catch {
        // Best-effort fallback for older webviews.
        const ta = globalThis.document.createElement("textarea");
        ta.value = value;
        ta.style.position = "fixed";
        ta.style.left = "-9999px";
        globalThis.document.body.appendChild(ta);
        ta.select();
        globalThis.document.execCommand("copy");
        globalThis.document.body.removeChild(ta);
        showToast(toastLabel);
      }
    },
    [showToast]
  );

  return (
    <div
      className={cn(
        "fixed z-50 border-border bg-surface shadow-xl h-dvh",
        expanded ? "inset-0" : "inset-y-0 right-0 w-full sm:w-[560px] border-l"
      )}
    >
      <div className="flex h-full min-h-0 flex-col">
        {/* Header */}
        <div className="border-b border-border px-4 py-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-gradient-to-br from-primary/30 to-secondary/20 ring-1 ring-white/5">
                <ScrollText className="h-4 w-4 text-primary" />
              </div>
              <div className="min-w-0">
                <h3 className="font-semibold text-text">Logs</h3>
                <div className="text-xs text-text-subtle truncate">{headerSubtitle}</div>
              </div>
            </div>

            <div className="flex items-center gap-1">
              <button
                onClick={() => setExpanded((e) => !e)}
                className="rounded p-1 text-text-subtle hover:bg-surface-elevated hover:text-text"
                title={expanded ? "Dock logs drawer" : "Expand logs to full screen"}
              >
                {expanded ? <Minimize2 className="h-4 w-4" /> : <Maximize2 className="h-4 w-4" />}
              </button>
              <button
                onClick={onClose}
                className="rounded p-1 text-text-subtle hover:bg-surface-elevated hover:text-text"
                title="Close"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
          </div>

          {/* Tabs + controls */}
          <div className="mt-3 flex flex-col gap-2">
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={() => setTab("tandem")}
                className={cn(
                  "inline-flex items-center gap-2 rounded-lg border px-3 py-1.5 text-xs transition-colors",
                  tab === "tandem"
                    ? "border-primary/40 bg-primary/10 text-text"
                    : "border-border bg-surface-elevated text-text-subtle hover:text-text"
                )}
              >
                <FileText className="h-3.5 w-3.5" />
                Tandem
              </button>
              <button
                type="button"
                onClick={() => setTab("console")}
                className={cn(
                  "inline-flex items-center gap-2 rounded-lg border px-3 py-1.5 text-xs transition-colors",
                  tab === "console"
                    ? "border-primary/40 bg-primary/10 text-text"
                    : "border-border bg-surface-elevated text-text-subtle hover:text-text"
                )}
              >
                <Terminal className="h-3.5 w-3.5" />
                Console
              </button>

              <div className="flex-1" />

              {tab !== "console" && (
                <>
                  <button
                    type="button"
                    onClick={() => setPaused((p) => !p)}
                    className={cn(
                      "inline-flex items-center gap-2 rounded-lg border px-3 py-1.5 text-xs transition-colors",
                      paused
                        ? "border-amber-500/40 bg-amber-500/10 text-amber-200"
                        : "border-border bg-surface-elevated text-text-subtle hover:text-text"
                    )}
                    title={paused ? "Resume streaming" : "Pause streaming"}
                  >
                    {paused ? <Play className="h-3.5 w-3.5" /> : <Pause className="h-3.5 w-3.5" />}
                    {paused ? "Resume" : "Pause"}
                  </button>

                  <button
                    type="button"
                    onClick={() => {
                      pendingRawLinesRef.current = [];
                      setLines([]);
                      setDropped(0);
                      setSelectedLine(null);
                    }}
                    className="inline-flex items-center gap-2 rounded-lg border border-border bg-surface-elevated px-3 py-1.5 text-xs text-text-subtle transition-colors hover:text-text"
                    title="Clear view"
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                    Clear
                  </button>

                  <button
                    type="button"
                    onClick={() =>
                      copyText(
                        visibleLines
                          .slice(-200)
                          .map((l) => l.raw.replace(/\r?\n$/, ""))
                          .join("\n"),
                        "Copied last 200 lines"
                      )
                    }
                    className="inline-flex items-center gap-2 rounded-lg border border-border bg-surface-elevated px-3 py-1.5 text-xs text-text-subtle transition-colors hover:text-text"
                    title="Copy the last 200 lines (from the current filtered view)"
                  >
                    Copy last 200
                  </button>
                </>
              )}
            </div>

            {tab !== "console" && (
              <div className="flex flex-wrap items-center gap-2">
                {tab === "tandem" && (
                  <label className="flex items-center gap-2 text-xs text-text-subtle">
                    <span className="hidden sm:inline">File</span>
                    <select
                      className="rounded-lg border border-border bg-surface-elevated px-2 py-1 text-xs text-text outline-none focus:border-primary"
                      value={selectedFile ?? ""}
                      onChange={(e) => setSelectedFile(e.target.value || null)}
                      disabled={files.length === 0}
                      title={selectedFile ?? "Select a log file"}
                    >
                      {files.length === 0 && <option value="">No logs found</option>}
                      {files
                        .slice()
                        .sort((a, b) => b.modified_ms - a.modified_ms)
                        .map((f) => (
                          <option key={f.name} value={f.name}>
                            {f.name}
                          </option>
                        ))}
                    </select>
                  </label>
                )}

                <div className="flex items-center gap-2 rounded-lg border border-border bg-surface-elevated px-2 py-1">
                  <Search className="h-3.5 w-3.5 text-text-subtle" />
                  <input
                    value={search}
                    onChange={(e) => {
                      const next = e.target.value;
                      setSearch(next);
                      if (next.trim() !== "") setFollow(false);
                    }}
                    className="w-[220px] bg-transparent text-xs text-text placeholder:text-text-subtle outline-none"
                    placeholder="Search logs..."
                  />
                </div>

                <label className="flex items-center gap-2 text-xs text-text-subtle">
                  <span className="hidden sm:inline">Level</span>
                  <select
                    className="rounded-lg border border-border bg-surface-elevated px-2 py-1 text-xs text-text outline-none focus:border-primary"
                    value={levelFilter}
                    onChange={(e) => setLevelFilter(e.target.value as LevelFilter)}
                  >
                    <option value="ALL">All</option>
                    <option value="ERROR">Error</option>
                    <option value="WARN">Warn</option>
                    <option value="INFO">Info</option>
                    <option value="DEBUG">Debug</option>
                    <option value="TRACE">Trace</option>
                    <option value="STDERR">Stderr</option>
                    <option value="STDOUT">Stdout</option>
                  </select>
                </label>

                <label className="flex items-center gap-2 text-xs text-text-subtle">
                  <span className="hidden sm:inline">Source</span>
                  <select
                    className="rounded-lg border border-border bg-surface-elevated px-2 py-1 text-xs text-text outline-none focus:border-primary"
                    value={provenanceFilter}
                    onChange={(e) => setProvenanceFilter(e.target.value as ProvenanceFilter)}
                  >
                    <option value="ALL">All sources</option>
                    <option value="CURRENT">Current runtime</option>
                    <option value="LEGACY">Legacy imported</option>
                  </select>
                </label>

                <label className="flex items-center gap-2 text-xs text-text-subtle">
                  <span className="hidden sm:inline">Process</span>
                  <select
                    className="max-w-[140px] rounded-lg border border-border bg-surface-elevated px-2 py-1 text-xs text-text outline-none focus:border-primary"
                    value={processFilter}
                    onChange={(e) => setProcessFilter(e.target.value)}
                  >
                    <option value="ALL">All</option>
                    {processOptions.map((p) => (
                      <option key={p} value={p}>
                        {p}
                      </option>
                    ))}
                  </select>
                </label>

                <label className="flex items-center gap-2 text-xs text-text-subtle">
                  <span className="hidden sm:inline">Component</span>
                  <select
                    className="max-w-[170px] rounded-lg border border-border bg-surface-elevated px-2 py-1 text-xs text-text outline-none focus:border-primary"
                    value={componentFilter}
                    onChange={(e) => setComponentFilter(e.target.value)}
                  >
                    <option value="ALL">All</option>
                    {componentOptions.map((c) => (
                      <option key={c} value={c}>
                        {c}
                      </option>
                    ))}
                  </select>
                </label>

                <label className="flex items-center gap-2 text-xs text-text-subtle">
                  <span className="hidden sm:inline">Event</span>
                  <select
                    className="max-w-[180px] rounded-lg border border-border bg-surface-elevated px-2 py-1 text-xs text-text outline-none focus:border-primary"
                    value={eventFilter}
                    onChange={(e) => setEventFilter(e.target.value)}
                  >
                    <option value="ALL">All</option>
                    {eventOptions.map((ev) => (
                      <option key={ev} value={ev}>
                        {ev}
                      </option>
                    ))}
                  </select>
                </label>

                <input
                  value={correlationFilter}
                  onChange={(e) => setCorrelationFilter(e.target.value)}
                  className="w-[170px] rounded-lg border border-border bg-surface-elevated px-2 py-1 text-xs text-text placeholder:text-text-subtle outline-none focus:border-primary"
                  placeholder="Correlation ID"
                />

                <input
                  value={sessionFilter}
                  onChange={(e) => setSessionFilter(e.target.value)}
                  className="w-[170px] rounded-lg border border-border bg-surface-elevated px-2 py-1 text-xs text-text placeholder:text-text-subtle outline-none focus:border-primary"
                  placeholder="Session ID"
                />

                <label className="flex items-center gap-1.5 text-xs text-text-subtle">
                  <input
                    type="checkbox"
                    checked={currentRuntimeOnly}
                    onChange={(e) => setCurrentRuntimeOnly(e.target.checked)}
                  />
                  Current runtime only
                </label>

                <div className="flex-1" />

                <div className="text-xs text-text-subtle">
                  <span className="font-mono tabular-nums">{visibleLines.length}</span> lines
                  {dropped > 0 && (
                    <span className="ml-2 rounded-md border border-border bg-surface-elevated px-2 py-0.5 text-[10px] text-text-muted">
                      dropped {dropped}
                    </span>
                  )}
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Console Tab - always mounted, visibility toggled via CSS */}
        <div
          style={{ display: tab === "console" ? "flex" : "none" }}
          className="min-h-0 flex-1 flex-col"
        >
          <ConsoleTab sessionId={sessionId} />
        </div>

        {/* Tandem Logs Tab - always mounted, visibility toggled via CSS */}
        <div
          style={{ display: tab === "tandem" ? "flex" : "none" }}
          className="min-h-0 flex-1 flex-col"
        >
          <div className="flex min-h-0 flex-1 flex-col">
            {/* List */}
            <div ref={listContainerRef} className="relative min-h-0 flex-1 overflow-hidden">
              {visibleLines.length === 0 ? (
                <div className="flex h-full items-center justify-center text-sm text-text-subtle">
                  {paused ? "Paused" : "Waiting for logs..."}
                </div>
              ) : (
                <List
                  listRef={setListApi}
                  rowComponent={Row}
                  rowCount={visibleLines.length}
                  rowHeight={22}
                  rowProps={{ items: visibleLines }}
                  // Allow horizontal scrolling for long log lines.
                  style={{
                    height: listHeight,
                    width: "100%",
                    overflowX: "auto",
                    overflowY: "auto",
                  }}
                  onScroll={(e) => {
                    const el = e.currentTarget as HTMLDivElement;
                    // "At bottom" tolerance so tiny pixel rounding doesn't flap.
                    const atBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 24;
                    if (atBottom) {
                      if (!follow) setFollow(true);
                    } else {
                      if (follow) setFollow(false);
                    }
                  }}
                />
              )}

              {!paused && visibleLines.length > 0 && !follow && (
                <div className="pointer-events-none absolute bottom-3 left-1/2 -translate-x-1/2">
                  <button
                    type="button"
                    className="pointer-events-auto flex items-center gap-1.5 rounded-full border border-primary/40 bg-surface-elevated/95 px-3 py-1.5 text-xs font-medium text-primary shadow-lg shadow-black/25 transition hover:border-primary/70 hover:bg-surface-elevated"
                    onClick={() => {
                      setFollow(true);
                      listApi?.scrollToRow({
                        index: visibleLines.length - 1,
                        align: "end",
                        behavior: "instant",
                      });
                    }}
                  >
                    <ScrollText className="h-3 w-3" />
                    Jump to latest
                  </button>
                </div>
              )}

              {toastMsg && (
                <div className="pointer-events-none absolute bottom-4 right-4">
                  <div className="rounded-lg border border-border bg-surface-elevated/95 px-3 py-2 text-xs text-text shadow-lg shadow-black/30 backdrop-blur-sm">
                    {toastMsg}
                  </div>
                </div>
              )}
            </div>

            {/* Selected line preview */}
            {selectedLine && (
              <div className="flex flex-col border-t border-border bg-surface">
                <div className="flex items-center justify-between px-3 py-2">
                  <span className="text-xs text-text-subtle">Selected Line</span>
                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      onClick={() => {
                        void copyText(selectedLine.raw, "Copied to clipboard");
                      }}
                      className="flex items-center gap-1 rounded px-2 py-1 text-xs text-text-subtle transition hover:bg-surface-elevated hover:text-text"
                    >
                      <Copy className="h-3 w-3" />
                      Copy
                    </button>
                    {!!selectedLine.parsed.correlation_id && (
                      <button
                        type="button"
                        onClick={() => {
                          const correlationId = selectedLine.parsed.correlation_id!;
                          const trace = lines
                            .filter((l) => l.parsed.correlation_id === correlationId)
                            .map((l) => l.raw.replace(/\r?\n$/, ""))
                            .join("\n");
                          void copyText(trace, "Copied correlation trace");
                        }}
                        className="flex items-center gap-1 rounded px-2 py-1 text-xs text-text-subtle transition hover:bg-surface-elevated hover:text-text"
                      >
                        <Copy className="h-3 w-3" />
                        Copy Correlation Trace
                      </button>
                    )}
                    <button
                      type="button"
                      onClick={() => setSelectedLine(null)}
                      className="flex items-center gap-1 rounded px-2 py-1 text-xs text-text-subtle transition hover:bg-surface-elevated hover:text-text"
                    >
                      <X className="h-3 w-3" />
                      Close
                    </button>
                  </div>
                </div>
                <div className="border-t border-border px-3 py-2 text-[11px] text-text-subtle">
                  <div className="flex flex-wrap items-center gap-3">
                    {selectedLine.parsed.process && (
                      <span>process: {selectedLine.parsed.process}</span>
                    )}
                    {selectedLine.parsed.component && (
                      <span>component: {selectedLine.parsed.component}</span>
                    )}
                    {selectedLine.parsed.event && <span>event: {selectedLine.parsed.event}</span>}
                    {selectedLine.parsed.correlation_id && (
                      <span>correlation: {selectedLine.parsed.correlation_id}</span>
                    )}
                    {selectedLine.parsed.session_id && (
                      <span>session: {selectedLine.parsed.session_id}</span>
                    )}
                  </div>
                </div>
                <div className="max-h-40 overflow-auto border-t border-border bg-background px-3 py-2">
                  <pre className="whitespace-pre-wrap break-all font-mono text-[11px] leading-relaxed text-text-muted">
                    {selectedLine.raw}
                  </pre>
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
