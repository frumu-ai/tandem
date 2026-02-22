import React, { useCallback, useEffect, useRef, useState } from "react";
import {
  Terminal,
  CheckCircle2,
  XCircle,
  Clock,
  ChevronDown,
  ChevronRight,
  Copy,
  Shield,
  Play,
  Ban,
  FileEdit,
  FileText,
  Search,
  Cpu,
} from "lucide-react";
import { Button } from "@/components/ui";
import { cn } from "@/lib/utils";
import {
  onSidecarEvent,
  approveTool,
  denyTool,
  listToolExecutions,
  type StreamEvent,
} from "@/lib/tauri";

// ---------------------------------------------------------------------------
// Types & Tool Categorization
// ---------------------------------------------------------------------------

type ToolCategory = "shell" | "file_read" | "file_write" | "search" | "batch" | "memory" | "other";

interface ToolCategoryInfo {
  icon: typeof Terminal;
  color: string;
  bgColor: string;
  borderColor: string;
}

const TOOL_CATEGORIES: Record<ToolCategory, ToolCategoryInfo> = {
  shell: {
    icon: Terminal,
    color: "text-emerald-400",
    bgColor: "bg-emerald-500/10",
    borderColor: "border-emerald-500/30",
  },
  file_write: {
    icon: FileEdit,
    color: "text-amber-400",
    bgColor: "bg-amber-500/10",
    borderColor: "border-amber-500/30",
  },
  file_read: {
    icon: FileText,
    color: "text-sky-400",
    bgColor: "bg-sky-500/10",
    borderColor: "border-sky-500/30",
  },
  search: {
    icon: Search,
    color: "text-purple-400",
    bgColor: "bg-purple-500/10",
    borderColor: "border-purple-500/30",
  },
  batch: {
    icon: Cpu,
    color: "text-cyan-300",
    bgColor: "bg-cyan-500/12",
    borderColor: "border-cyan-500/35",
  },
  memory: {
    icon: Shield,
    color: "text-fuchsia-300",
    bgColor: "bg-fuchsia-500/12",
    borderColor: "border-fuchsia-500/35",
  },
  other: {
    icon: Cpu,
    color: "text-text-subtle",
    bgColor: "bg-surface-elevated",
    borderColor: "border-border",
  },
};

function categorizeTool(tool: string): ToolCategory {
  const t = tool.toLowerCase();

  // Memory events
  if (t === "memory.store" || t === "memory.lookup" || t.startsWith("memory.")) {
    return "memory";
  }

  // Batch execution wrapper
  if (t === "batch" || t.includes("batch")) {
    return "batch";
  }

  // Shell commands
  if (
    t.includes("bash") ||
    t.includes("shell") ||
    t.includes("command") ||
    t.includes("exec") ||
    t === "run_command"
  ) {
    return "shell";
  }

  // File write operations
  if (
    t.includes("write") ||
    t.includes("edit") ||
    t.includes("replace") ||
    t.includes("create") ||
    t === "multi_replace_file_content" ||
    t === "replace_file_content" ||
    t === "write_to_file"
  ) {
    return "file_write";
  }

  // File read operations
  if (t.includes("read") || t.includes("view") || t === "view_file" || t === "view_code_item") {
    return "file_read";
  }

  // Search operations
  if (
    t.includes("search") ||
    t.includes("grep") ||
    t.includes("find") ||
    t.includes("glob") ||
    t === "codebase_search"
  ) {
    return "search";
  }

  return "other";
}

type EntryStatus = "pending" | "running" | "completed" | "failed";

interface ConsoleEntry {
  /** part_id from the SSE event */
  id: string;
  tool: string;
  args: Record<string, unknown>;
  status: EntryStatus;
  result?: string;
  error?: string;
  timestamp: Date;
  sessionId: string;
  messageId?: string;
  category: ToolCategory;
}

interface PendingApproval {
  requestId: string;
  sessionId: string;
  tool?: string;
  args?: Record<string, unknown>;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function formatCommand(tool: string, args: Record<string, unknown>): string {
  if (tool === "memory.lookup") {
    const chunks = Number(args.chunks_total ?? 0);
    const latency = Number(args.latency_ms ?? 0);
    const status = String(args.status ?? "unknown");
    return `${chunks} chunks (${latency}ms) [${status}]`;
  }
  if (tool === "memory.store") {
    const role = String(args.role ?? "unknown");
    const s = Number(args.session_chunks_stored ?? 0);
    const p = Number(args.project_chunks_stored ?? 0);
    const status = String(args.status ?? "unknown");
    return `${role} S${s}/P${p} [${status}]`;
  }

  // For shell commands, extract the actual command
  const shellCmd = args.command ?? args.cmd ?? args.input ?? args.script ?? args.code;
  if (typeof shellCmd === "string") return shellCmd;

  // For file operations, show the file path
  if (args.targetFile || args.filePath || args.absolutePath) {
    const file = (args.targetFile ?? args.filePath ?? args.absolutePath) as string;
    const parts = file.split(/[/\\]/);
    return parts[parts.length - 1] || file;
  }

  // For search operations, show the query
  if (args.query) {
    return `"${args.query}"`;
  }

  // Default: show tool name
  return tool;
}

function truncate(s: string, max: number): string {
  return s.length > max ? s.slice(0, max) + "…" : s;
}

function statusIcon(status: EntryStatus) {
  switch (status) {
    case "pending":
      return <Clock className="h-3.5 w-3.5 text-amber-400" />;
    case "running":
      return (
        <div className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-primary border-t-transparent" />
      );
    case "completed":
      return <CheckCircle2 className="h-3.5 w-3.5 text-emerald-400" />;
    case "failed":
      return <XCircle className="h-3.5 w-3.5 text-red-400" />;
  }
}

function statusLabel(status: EntryStatus): string {
  switch (status) {
    case "pending":
      return "Pending approval";
    case "running":
      return "Running";
    case "completed":
      return "Completed";
    case "failed":
      return "Failed";
  }
}

// ---------------------------------------------------------------------------
// Console Entry Card
// ---------------------------------------------------------------------------

const ConsoleCard = React.memo(function ConsoleCard({
  entry,
  approval,
  onApprove,
  onDeny,
}: {
  entry: ConsoleEntry;
  approval?: PendingApproval;
  onApprove: (
    requestId: string,
    sessionId: string,
    tool?: string,
    args?: Record<string, unknown>
  ) => void;
  onDeny: (
    requestId: string,
    sessionId: string,
    tool?: string,
    args?: Record<string, unknown>
  ) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [copied, setCopied] = useState(false);

  const categoryInfo = TOOL_CATEGORIES[entry.category];
  const CategoryIcon = categoryInfo.icon;
  const command = formatCommand(entry.tool, entry.args);
  const hasOutput = !!entry.result || !!entry.error;

  const copyOutput = () => {
    const text = entry.error || entry.result || "";
    globalThis.navigator?.clipboard?.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };

  const copyArgs = () => {
    try {
      const text = JSON.stringify(entry.args, null, 2);
      globalThis.navigator?.clipboard?.writeText(text).then(() => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      });
    } catch {
      // ignore
    }
  };

  return (
    <div
      className={cn(
        "rounded-lg border overflow-hidden",
        categoryInfo.borderColor,
        categoryInfo.bgColor
      )}
    >
      {/* Header */}
      <div
        className="flex items-center gap-2 px-3 py-2 cursor-pointer hover:bg-surface/50 transition-colors"
        onClick={() => setExpanded((v) => !v)}
      >
        {expanded ? (
          <ChevronDown className="h-3.5 w-3.5 text-text-subtle flex-shrink-0" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5 text-text-subtle flex-shrink-0" />
        )}

        <CategoryIcon className={cn("h-3.5 w-3.5 flex-shrink-0", categoryInfo.color)} />
        {statusIcon(entry.status)}

        <div className="flex-1 min-w-0 flex items-center gap-2">
          <code className="text-[10px] text-text-muted font-mono uppercase tracking-wide">
            {entry.tool}
          </code>
          <code className="text-xs text-text font-mono truncate">{truncate(command, 100)}</code>
        </div>

        <span className="text-[10px] text-text-subtle whitespace-nowrap">
          {statusLabel(entry.status)}
        </span>

        <span className="text-[10px] text-text-muted tabular-nums whitespace-nowrap">
          {entry.timestamp.toLocaleTimeString()}
        </span>
      </div>

      {/* Inline approval */}
      {approval && entry.status === "pending" && (
        <div className="flex items-center gap-3 border-t border-border bg-amber-500/5 px-3 py-2">
          <Shield className="h-4 w-4 text-amber-400 flex-shrink-0" />
          <span className="text-xs text-text-subtle flex-1">
            AI requests permission to run this tool
          </span>
          <Button
            variant="secondary"
            size="sm"
            className="h-7 px-3 text-xs gap-1.5"
            onClick={(e) => {
              e.stopPropagation();
              onDeny(approval.requestId, approval.sessionId, approval.tool, approval.args);
            }}
          >
            <Ban className="h-3 w-3" />
            Deny
          </Button>
          <Button
            size="sm"
            className="h-7 px-3 text-xs gap-1.5"
            onClick={(e) => {
              e.stopPropagation();
              onApprove(approval.requestId, approval.sessionId, approval.tool, approval.args);
            }}
          >
            <Play className="h-3 w-3" />
            Run
          </Button>
        </div>
      )}

      {/* Expandable details */}
      {expanded && (
        <div className="border-t border-border">
          <div className="flex items-center justify-between px-3 py-1.5 bg-surface">
            <span className="text-[10px] uppercase tracking-wide text-text-subtle">
              {hasOutput ? (entry.error ? "Error" : "Output") : "Tool Details"}
            </span>
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                if (hasOutput) {
                  copyOutput();
                } else {
                  copyArgs();
                }
              }}
              className="flex items-center gap-1 rounded px-2 py-0.5 text-[10px] text-text-subtle hover:text-text hover:bg-surface-elevated transition-colors"
            >
              <Copy className="h-3 w-3" />
              {copied ? "Copied!" : "Copy"}
            </button>
          </div>
          <div className="max-h-60 overflow-auto px-3 py-2 bg-background">
            {hasOutput ? (
              <pre
                className={cn(
                  "font-mono text-[11px] leading-relaxed whitespace-pre-wrap break-all",
                  entry.error ? "text-red-300" : "text-text-muted"
                )}
              >
                {entry.error || entry.result}
              </pre>
            ) : (
              <div className="space-y-2">
                <div>
                  <div className="text-[10px] uppercase tracking-wide text-text-subtle mb-1">
                    Tool
                  </div>
                  <code className="text-xs text-text">{entry.tool}</code>
                </div>
                <div>
                  <div className="text-[10px] uppercase tracking-wide text-text-subtle mb-1">
                    Arguments
                  </div>
                  <pre className="font-mono text-[11px] leading-relaxed text-text-muted whitespace-pre-wrap break-all">
                    {JSON.stringify(entry.args, null, 2)}
                  </pre>
                </div>
                {entry.status === "running" && (
                  <div className="text-xs text-text-subtle italic">Waiting for output...</div>
                )}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
});

// ---------------------------------------------------------------------------
// ConsoleTab (main export)
// ---------------------------------------------------------------------------

interface ConsoleTabProps {
  sessionId?: string | null;
  sessionIds?: string[];
  autoScroll?: boolean;
}

export function ConsoleTab({ sessionId, sessionIds, autoScroll = true }: ConsoleTabProps) {
  const [entries, setEntries] = useState<ConsoleEntry[]>([]);
  const [approvals, setApprovals] = useState<Map<string, PendingApproval>>(() => new Map());
  const [isLoadingHistory, setIsLoadingHistory] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const syntheticIdSeqRef = useRef(0);
  const recentMemoryEventRef = useRef<Map<string, number>>(new Map());
  const [isFollowing, setIsFollowing] = useState(autoScroll);
  const [showJumpToLatest, setShowJumpToLatest] = useState(false);
  const sessionScope = React.useMemo(() => {
    const merged = new Set<string>();
    for (const id of sessionIds ?? []) {
      const next = id?.trim();
      if (next) merged.add(next);
    }
    const primary = sessionId?.trim();
    if (primary) merged.add(primary);
    return merged;
  }, [sessionId, sessionIds]);
  const sessionScopeKey = React.useMemo(
    () => Array.from(sessionScope).sort().join("|"),
    [sessionScope]
  );
  const sessionScopeSet = React.useMemo(
    () => new Set(sessionScopeKey ? sessionScopeKey.split("|") : []),
    [sessionScopeKey]
  );
  const nextSyntheticId = useCallback((prefix: string) => {
    syntheticIdSeqRef.current += 1;
    return `${prefix}:${syntheticIdSeqRef.current}`;
  }, []);
  const shouldSkipDuplicateMemoryEvent = useCallback((signature: string) => {
    const now = Date.now();
    const cache = recentMemoryEventRef.current;
    const previous = cache.get(signature);
    cache.set(signature, now);

    if (cache.size > 300) {
      for (const [key, seenAt] of cache) {
        if (now - seenAt > 10_000) {
          cache.delete(key);
        }
      }
    }

    return previous !== undefined && now - previous < 750;
  }, []);

  const rowsToEntries = useCallback((rows: Awaited<ReturnType<typeof listToolExecutions>>) => {
    const seen = new Set<string>();
    return rows
      .filter((row) => {
        if (seen.has(row.id)) return false;
        seen.add(row.id);
        return true;
      })
      .map((row) => {
        const status: EntryStatus =
          row.status === "pending" || row.status === "running" || row.status === "completed"
            ? row.status
            : "failed";
        const resultText =
          row.result == null
            ? undefined
            : typeof row.result === "string"
              ? row.result
              : JSON.stringify(row.result, null, 2);
        const timestamp = row.ended_at_ms ?? row.started_at_ms;

        return {
          id: row.id,
          tool: row.tool,
          args:
            row.args && typeof row.args === "object" && !Array.isArray(row.args)
              ? (row.args as Record<string, unknown>)
              : {},
          status,
          result: resultText,
          error: row.error,
          timestamp: new Date(timestamp),
          sessionId: row.session_id,
          messageId: row.message_id,
          category: categorizeTool(row.tool),
        } satisfies ConsoleEntry;
      });
  }, []);

  // Load historical tool executions when session changes
  useEffect(() => {
    setEntries([]);
    setApprovals(new Map());
    setIsFollowing(autoScroll);
    setShowJumpToLatest(false);

    if (sessionScopeSet.size === 0) {
      setIsLoadingHistory(false);
      return;
    }

    let cancelled = false;
    const loadHistory = async () => {
      setIsLoadingHistory(true);
      try {
        const scopeIds = Array.from(sessionScopeSet);
        const allRows = (
          await Promise.all(scopeIds.map((id) => listToolExecutions(id, 400)))
        ).flat();
        const rows = allRows.sort((a, b) => {
          const aTs = a.ended_at_ms ?? a.started_at_ms;
          const bTs = b.ended_at_ms ?? b.started_at_ms;
          return aTs - bTs;
        });
        if (cancelled) return;

        const toolEntries = rowsToEntries(rows);

        setEntries(toolEntries);
        setApprovals(new Map());
      } catch (err) {
        console.error("[ConsoleTab] History load failed:", err);
        if (!cancelled) {
          setEntries([]);
          setApprovals(new Map());
        }
      } finally {
        if (!cancelled) {
          setIsLoadingHistory(false);
        }
      }
    };

    void loadHistory();

    return () => {
      cancelled = true;
    };
  }, [autoScroll, rowsToEntries, sessionScopeKey, sessionScopeSet]);

  // Fallback polling: keep console in sync from persisted tool history if SSE lags/disconnects.
  useEffect(() => {
    if (sessionScopeSet.size === 0) return;
    let cancelled = false;

    const poll = async () => {
      try {
        const scopeIds = Array.from(sessionScopeSet);
        const allRows = (await Promise.all(scopeIds.map((id) => listToolExecutions(id, 400))))
          .flat()
          .sort((a, b) => {
            const aTs = a.ended_at_ms ?? a.started_at_ms;
            const bTs = b.ended_at_ms ?? b.started_at_ms;
            return aTs - bTs;
          });
        if (cancelled) return;

        const polled = rowsToEntries(allRows);
        setEntries((prev) => {
          const byId = new Map(prev.map((entry) => [entry.id, entry]));
          for (const entry of polled) {
            byId.set(entry.id, entry);
          }
          return Array.from(byId.values()).sort(
            (a, b) => a.timestamp.getTime() - b.timestamp.getTime()
          );
        });
      } catch {
        // best-effort fallback; keep SSE-driven state intact
      }
    };

    const timer = setInterval(() => void poll(), 3000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [rowsToEntries, sessionScopeKey, sessionScopeSet]);

  const isNearBottom = useCallback(() => {
    const el = listRef.current;
    if (!el) return true;
    const remaining = el.scrollHeight - el.scrollTop - el.clientHeight;
    return remaining <= 24;
  }, []);

  const scrollToLatest = useCallback((behavior: "auto" | "smooth" = "smooth") => {
    const el = listRef.current;
    if (el) {
      el.scrollTo({ top: el.scrollHeight, behavior });
    } else {
      bottomRef.current?.scrollIntoView({ behavior });
    }
    setIsFollowing(true);
    setShowJumpToLatest(false);
  }, []);

  // Auto-scroll to bottom when new entries arrive
  useEffect(() => {
    if (!autoScroll || !isFollowing) return;
    scrollToLatest("smooth");
  }, [autoScroll, entries.length, isFollowing, scrollToLatest]);

  // -----------------------------------------------------------------------
  // SSE Listener
  // -----------------------------------------------------------------------
  const handleEvent = useCallback(
    (event: StreamEvent) => {
      if (sessionScopeSet.size === 0) return;

      switch (event.type) {
        case "tool_start": {
          if (!sessionScopeSet.has(event.session_id)) break;
          const category = categorizeTool(event.tool);

          setEntries((prev) => {
            // Avoid duplicates
            if (prev.some((e) => e.id === event.part_id)) {
              return prev;
            }
            return [
              ...prev,
              {
                id: event.part_id,
                tool: event.tool,
                args: event.args as Record<string, unknown>,
                status: "running",
                timestamp: new Date(),
                sessionId: event.session_id,
                messageId: event.message_id,
                category,
              },
            ];
          });
          break;
        }

        case "tool_end": {
          if (!sessionScopeSet.has(event.session_id)) break;
          setEntries((prev) => {
            let matchedId = event.part_id;
            const hasExact = prev.some((e) => e.id === event.part_id);
            if (!hasExact) {
              for (let i = prev.length - 1; i >= 0; i -= 1) {
                const candidate = prev[i];
                if (
                  candidate.sessionId === event.session_id &&
                  candidate.status === "running" &&
                  candidate.tool.toLowerCase() === event.tool.toLowerCase() &&
                  (!event.message_id || candidate.messageId === event.message_id)
                ) {
                  matchedId = candidate.id;
                  break;
                }
              }
            }

            return prev.map((e) =>
              e.id === matchedId
                ? {
                    ...e,
                    status: event.error ? "failed" : "completed",
                    result: event.result ? String(event.result) : undefined,
                    error: event.error ?? undefined,
                  }
                : e
            );
          });
          // Remove any approval for this tool
          setApprovals((prev) => {
            const next = new Map(prev);
            for (const [key, val] of next) {
              if (val.sessionId === event.session_id) {
                next.delete(key);
              }
            }
            return next;
          });
          break;
        }

        case "run_finished":
        case "session_idle":
        case "session_error": {
          const sid = event.session_id;
          if (!sessionScopeSet.has(sid)) break;
          const terminalError =
            event.type === "session_error"
              ? event.error
              : event.type === "run_finished" && event.status !== "completed"
                ? event.error || `run_${event.status}`
                : "interrupted";
          setEntries((prev) => {
            const next = prev.map((e) =>
              e.sessionId === sid && e.status === "running"
                ? {
                    ...e,
                    status: "failed" as const,
                    error: e.error || terminalError,
                  }
                : e
            );
            const hadRunning = prev.some((e) => e.sessionId === sid && e.status === "running");
            if (!hadRunning && terminalError) {
              next.push({
                id: nextSyntheticId(`session.error:${sid}`),
                tool: "session.error",
                args: {
                  status: event.type === "run_finished" ? (event.status ?? "unknown") : event.type,
                  error: terminalError,
                },
                status: "failed" as const,
                error: terminalError,
                timestamp: new Date(),
                sessionId: sid,
                category: "other",
              });
            }
            return next;
          });
          break;
        }

        case "permission_asked": {
          if (!sessionScopeSet.has(event.session_id)) break;
          if (!event.tool) return;

          const category = categorizeTool(event.tool);

          // Add a pending entry if we don't have one for this request yet
          setEntries((prev) => {
            // Check if there's already a running entry for this tool in this session
            const existingRunning = prev.find(
              (e) => e.sessionId === event.session_id && e.status === "running"
            );
            if (existingRunning) {
              return prev.map((e) =>
                e.id === existingRunning.id ? { ...e, status: "pending" } : e
              );
            }

            // Add a new pending entry
            if (prev.some((e) => e.id === event.request_id)) return prev;
            return [
              ...prev,
              {
                id: event.request_id,
                tool: event.tool || "unknown",
                args: (event.args as Record<string, unknown>) || {},
                status: "pending",
                timestamp: new Date(),
                sessionId: event.session_id,
                category,
              },
            ];
          });

          setApprovals((prev) => {
            const next = new Map(prev);
            next.set(event.request_id, {
              requestId: event.request_id,
              sessionId: event.session_id,
              tool: event.tool,
              args: event.args as Record<string, unknown>,
            });
            return next;
          });
          break;
        }

        case "memory_retrieval": {
          if (!sessionScopeSet.has(event.session_id)) break;
          const eventStatus = event.status ?? "unknown";
          const isFailure = eventStatus === "error_fallback" || eventStatus === "degraded_disabled";
          const signature = [
            "memory_retrieval",
            event.session_id,
            event.query_hash ?? "none",
            eventStatus,
            String(event.used),
            String(event.chunks_total),
          ].join(":");
          if (shouldSkipDuplicateMemoryEvent(signature)) break;
          setEntries((prev) => [
            ...prev,
            {
              id: nextSyntheticId(
                `memory.lookup:${event.session_id}:${event.query_hash ?? "none"}:${eventStatus}`
              ),
              tool: "memory.lookup",
              args: {
                status: eventStatus,
                used: event.used,
                chunks_total: event.chunks_total,
                session_chunks: event.session_chunks,
                history_chunks: event.history_chunks,
                project_fact_chunks: event.project_fact_chunks,
                latency_ms: event.latency_ms,
                query_hash: event.query_hash,
                embedding_status: event.embedding_status,
                embedding_reason: event.embedding_reason,
              },
              status: isFailure ? "failed" : "completed",
              result: `lookup used=${event.used} chunks=${event.chunks_total} latency=${event.latency_ms}ms`,
              error: isFailure ? `lookup status=${eventStatus}` : undefined,
              timestamp: new Date(),
              sessionId: event.session_id,
              category: "search",
            },
          ]);
          break;
        }

        case "memory_storage": {
          if (!sessionScopeSet.has(event.session_id)) break;
          const eventStatus = event.status ?? "unknown";
          const isFailure = eventStatus === "error" || Boolean(event.error);
          const signature = [
            "memory_storage",
            event.session_id,
            event.message_id ?? "none",
            event.role,
            eventStatus,
            String(event.session_chunks_stored),
            String(event.project_chunks_stored),
            event.error ?? "",
          ].join(":");
          if (shouldSkipDuplicateMemoryEvent(signature)) break;
          setEntries((prev) => [
            ...prev,
            {
              id: nextSyntheticId(
                `memory.store:${event.session_id}:${event.message_id ?? "none"}:${event.role}:${eventStatus}`
              ),
              tool: "memory.store",
              args: {
                role: event.role,
                session_chunks_stored: event.session_chunks_stored,
                project_chunks_stored: event.project_chunks_stored,
                status: eventStatus,
                message_id: event.message_id,
              },
              status: isFailure ? "failed" : "completed",
              result: `store role=${event.role} session=${event.session_chunks_stored} project=${event.project_chunks_stored}`,
              error: event.error,
              timestamp: new Date(),
              sessionId: event.session_id,
              messageId: event.message_id,
              category: "other",
            },
          ]);
          break;
        }
      }
    },
    [nextSyntheticId, sessionScopeSet, shouldSkipDuplicateMemoryEvent]
  );

  useEffect(() => {
    if (!sessionScopeKey) return;

    let unlistenFn: (() => void) | null = null;
    const setup = async () => {
      unlistenFn = await onSidecarEvent(handleEvent);
    };
    void setup();
    return () => {
      unlistenFn?.();
    };
  }, [handleEvent, sessionScopeKey]);

  // -----------------------------------------------------------------------
  // Approval handlers
  // -----------------------------------------------------------------------
  const handleApprove = async (
    requestId: string,
    sid: string,
    tool?: string,
    args?: Record<string, unknown>
  ) => {
    try {
      await approveTool(sid, requestId, { tool, args });
      // Update entry status
      setEntries((prev) => prev.map((e) => (e.id === requestId ? { ...e, status: "running" } : e)));
      setApprovals((prev) => {
        const next = new Map(prev);
        next.delete(requestId);
        return next;
      });
    } catch (err) {
      console.error("[Console] Approve failed:", err);
    }
  };

  const handleDeny = async (
    requestId: string,
    sid: string,
    tool?: string,
    args?: Record<string, unknown>
  ) => {
    try {
      await denyTool(sid, requestId, { tool, args });
      setEntries((prev) =>
        prev.map((e) =>
          e.id === requestId ? { ...e, status: "failed", error: "Denied by user" } : e
        )
      );
      setApprovals((prev) => {
        const next = new Map(prev);
        next.delete(requestId);
        return next;
      });
    } catch (err) {
      console.error("[Console] Deny failed:", err);
    }
  };

  // Match approvals to entries
  const getApproval = (entry: ConsoleEntry): PendingApproval | undefined => {
    // Direct match by request_id
    const direct = approvals.get(entry.id);
    if (direct) return direct;
    // Check all approvals for same session
    for (const [, val] of approvals) {
      if (val.sessionId === entry.sessionId && entry.status === "pending") {
        return val;
      }
    }
    return undefined;
  };

  // -----------------------------------------------------------------------
  // Render
  // -----------------------------------------------------------------------

  if (sessionScopeSet.size === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-3 text-text-subtle">
        <Terminal className="h-8 w-8 opacity-40" />
        <p className="text-sm">Select a session to view tool calls.</p>
        <p className="text-xs text-text-muted max-w-xs text-center">
          Console history and live tool execution events are scoped to the active session set.
        </p>
      </div>
    );
  }

  if (isLoadingHistory && entries.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-3 text-text-subtle">
        <Terminal className="h-8 w-8 opacity-40" />
        <p className="text-sm">Loading tool history...</p>
      </div>
    );
  }

  if (entries.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-3 text-text-subtle">
        <Terminal className="h-8 w-8 opacity-40" />
        <p className="text-sm">Tool executions will appear here.</p>
        <p className="text-xs text-text-muted max-w-xs text-center">
          When the AI runs tools, you'll see them here with live status, details, and output.
        </p>
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* Header bar */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-border">
        <span className="text-xs text-text-subtle">
          <span className="font-mono tabular-nums">{entries.length}</span> event
          {entries.length !== 1 ? "s" : ""}
        </span>
        <button
          type="button"
          onClick={() => {
            setEntries([]);
            setApprovals(new Map());
          }}
          className="text-[10px] text-text-muted hover:text-text transition-colors"
        >
          Clear
        </button>
      </div>

      {/* Scrollable tool list */}
      <div className="relative min-h-0 flex-1">
        <div
          ref={listRef}
          className="h-full overflow-y-auto px-3 py-2 space-y-2"
          onScroll={() => {
            if (!autoScroll) return;
            const nearBottom = isNearBottom();
            setIsFollowing(nearBottom);
            setShowJumpToLatest(!nearBottom);
          }}
        >
          {entries.map((entry, idx) => (
            <ConsoleCard
              key={`${entry.id}:${idx}`}
              entry={entry}
              approval={getApproval(entry)}
              onApprove={handleApprove}
              onDeny={handleDeny}
            />
          ))}
          <div ref={bottomRef} />
        </div>
        {autoScroll && showJumpToLatest ? (
          <button
            type="button"
            onClick={() => scrollToLatest("smooth")}
            className="absolute bottom-3 left-1/2 -translate-x-1/2 inline-flex items-center gap-1 rounded-full border border-primary/40 bg-primary/15 px-3 py-1 text-xs text-primary hover:bg-primary/25 transition-colors"
          >
            <ChevronDown className="h-3.5 w-3.5" />
            Jump to latest
          </button>
        ) : null}
      </div>
    </div>
  );
}
