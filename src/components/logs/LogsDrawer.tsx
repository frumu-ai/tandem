import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { List, type RowComponentProps, useListCallbackRef } from "react-window";
import { FileText, Pause, Play, ScrollText, Search, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui";
import { cn } from "@/lib/utils";
import {
  listAppLogFiles,
  onLogStreamEvent,
  startLogStream,
  stopLogStream,
  type LogFileInfo,
  type LogSource,
} from "@/lib/tauri";

const MAX_RENDER_LINES = 5000;
const DEFAULT_TAIL_LINES = 500;

type Level = "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR" | "STDOUT" | "STDERR" | "UNKNOWN";
type LevelFilter = "ALL" | Exclude<Level, "UNKNOWN">;

type ParsedLine = {
  ts?: string;
  level: Level;
  target?: string;
  msg: string;
};

type LineItem = {
  id: number;
  raw: string;
  parsed: ParsedLine;
};

function parseLine(raw: string): ParsedLine {
  const trimmed = raw.replace(/\r?\n$/, "");

  const sidecar = trimmed.match(/^(STDOUT|STDERR)\s*(?:[:-])?\s*(.*)$/);
  if (sidecar) {
    const level = sidecar[1] as "STDOUT" | "STDERR";
    return { level, msg: sidecar[2] ?? "" };
  }

  // Common tracing format:
  // 2026-02-09T15:30:55.123Z  INFO module::path: message...
  const m = trimmed.match(/^(\S+)\s+(TRACE|DEBUG|INFO|WARN|ERROR)\s+([^:]+):\s*(.*)$/);
  if (m) {
    return {
      ts: m[1],
      level: m[2] as Level,
      target: m[3],
      msg: m[4] ?? "",
    };
  }

  // Fallback: [LEVEL] ... or LEVEL ...
  const m2 = trimmed.match(/^\[?(TRACE|DEBUG|INFO|WARN|ERROR)\]?\s+(.*)$/);
  if (m2) {
    return { level: m2[1] as Level, msg: m2[2] ?? "" };
  }

  return { level: "UNKNOWN", msg: trimmed };
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

    // ResizeObserver is supported in modern Chromium/WebKit; Tauri ships a modern runtime.
    const RO = globalThis.ResizeObserver;
    if (!RO) return;
    const ro = new RO((entries) => {
      const h = entries[0]?.contentRect?.height;
      if (typeof h === "number" && h > 0) setHeight(h);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  return { ref, height };
}

function pickNewest(files: LogFileInfo[]): string | null {
  if (!files || files.length === 0) return null;
  const sorted = [...files].sort((a, b) => b.modified_ms - a.modified_ms);
  return sorted[0]?.name ?? null;
}

export function LogsDrawer({ onClose }: { onClose: () => void }) {
  const [tab, setTab] = useState<LogSource>("tandem");
  const [files, setFiles] = useState<LogFileInfo[]>([]);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [paused, setPaused] = useState(false);
  const [search, setSearch] = useState("");
  const [levelFilter, setLevelFilter] = useState<LevelFilter>("ALL");
  const [follow, setFollow] = useState(true);
  const [lines, setLines] = useState<LineItem[]>([]);
  const [dropped, setDropped] = useState(0);
  const [streamId, setStreamId] = useState<string | null>(null);
  const [selectedLine, setSelectedLine] = useState<LineItem | null>(null);

  const streamIdRef = useRef<string | null>(null);
  const nextLineIdRef = useRef(1);
  const pendingRawLinesRef = useRef<string[]>([]);
  const [listApi, setListApi] = useListCallbackRef(null);

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
    async (next: { source: LogSource; fileName?: string }) => {
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

      if (tab === "sidecar") {
        void startCurrentStream({ source: "sidecar" });
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

  const visibleLines = useMemo(() => {
    const q = search.trim().toLowerCase();
    return lines.filter((l) => {
      if (levelFilter !== "ALL") {
        const lv = l.parsed.level;
        if (lv !== levelFilter) return false;
      }
      if (!q) return true;
      return l.raw.toLowerCase().includes(q);
    });
  }, [lines, levelFilter, search]);

  // Keep the view pinned to the bottom when follow is enabled.
  useEffect(() => {
    if (!follow) return;
    if (paused) return;
    if (visibleLines.length === 0) return;
    listApi?.scrollToRow({ index: visibleLines.length - 1, align: "end", behavior: "instant" });
  }, [follow, paused, visibleLines.length, listApi]);

  const headerSubtitle = useMemo(() => {
    if (paused) return "Paused";
    if (tab === "sidecar") return "Sidecar stdout/stderr (live)";
    return selectedFile ? `Tandem logs: ${selectedFile}` : "Tandem logs";
  }, [paused, tab, selectedFile]);

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

          <span className="font-mono text-[12px] text-text whitespace-pre">
            {p.msg || item.raw}
          </span>
        </button>
      </div>
    );
  };

  const copyText = useCallback(async (text: string) => {
    const value = text.replace(/\r?\n$/, "");
    try {
      await globalThis.navigator?.clipboard?.writeText(value);
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
    }
  }, []);

  return (
    <div className="fixed inset-y-0 right-0 z-50 w-full sm:w-[560px] border-l border-border bg-surface shadow-xl">
      <div className="flex h-full flex-col">
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

            <button
              onClick={onClose}
              className="rounded p-1 text-text-subtle hover:bg-surface-elevated hover:text-text"
              title="Close"
            >
              <X className="h-4 w-4" />
            </button>
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
                onClick={() => setTab("sidecar")}
                className={cn(
                  "inline-flex items-center gap-2 rounded-lg border px-3 py-1.5 text-xs transition-colors",
                  tab === "sidecar"
                    ? "border-primary/40 bg-primary/10 text-text"
                    : "border-border bg-surface-elevated text-text-subtle hover:text-text"
                )}
              >
                <span className="terminal-text text-[10px] text-primary/90">OC</span>
                Sidecar
              </button>

              <div className="flex-1" />

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
                      .join("\n")
                  )
                }
                className="inline-flex items-center gap-2 rounded-lg border border-border bg-surface-elevated px-3 py-1.5 text-xs text-text-subtle transition-colors hover:text-text"
                title="Copy the last 200 lines (from the current filtered view)"
              >
                Copy last 200
              </button>
            </div>

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
          </div>
        </div>

        {/* List */}
        <div ref={listContainerRef} className="relative flex-1 overflow-hidden">
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
              style={{ height: listHeight, width: "100%", overflowX: "auto", overflowY: "auto" }}
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
                onClick={() => {
                  setFollow(true);
                  listApi?.scrollToRow({
                    index: visibleLines.length - 1,
                    align: "end",
                    behavior: "instant",
                  });
                }}
                className="pointer-events-auto rounded-full border border-border bg-surface-elevated/90 px-3 py-1 text-[10px] text-text-subtle shadow-lg shadow-black/30 backdrop-blur-sm hover:text-text"
                title="Jump to bottom and resume following"
              >
                Jump to bottom to follow
              </button>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="border-t border-border px-4 py-3">
          <div className="flex items-center justify-between gap-3">
            <div className="text-xs text-text-subtle">
              Tip: scroll up to pause following; use “Jump to bottom to follow” to resume.
            </div>
            <div className="flex items-center gap-2">
              <Button
                variant="secondary"
                onClick={() => (selectedLine ? copyText(selectedLine.raw) : undefined)}
                className="h-8 px-3 text-xs"
                disabled={!selectedLine}
                title={selectedLine ? "Copy selected line" : "Click a line above to select it"}
              >
                Copy
              </Button>
              <Button
                variant="secondary"
                onClick={() => {
                  pendingRawLinesRef.current = [];
                  setLines([]);
                  setDropped(0);
                  setSelectedLine(null);
                }}
                className="h-8 px-3 text-xs"
              >
                Clear
              </Button>
            </div>
          </div>

          <div className="mt-2 rounded-lg border border-border bg-surface-elevated px-3 py-2">
            <div className="text-[10px] uppercase tracking-wide text-text-subtle">
              {selectedLine
                ? "Selected line (full text)"
                : "Click a line to preview/copy full text"}
            </div>
            <div className="mt-1 max-h-24 overflow-auto whitespace-pre font-mono text-[11px] text-text">
              {selectedLine?.raw?.replace(/\r?\n$/, "") ?? ""}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
