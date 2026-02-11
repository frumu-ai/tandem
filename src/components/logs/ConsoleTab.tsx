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
  getSessionMessages,
  type StreamEvent,
  type SessionMessage,
} from "@/lib/tauri";

// ---------------------------------------------------------------------------
// Types & Tool Categorization
// ---------------------------------------------------------------------------

type ToolCategory = "shell" | "file_read" | "file_write" | "search" | "other";

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
  other: {
    icon: Cpu,
    color: "text-text-subtle",
    bgColor: "bg-surface-elevated",
    borderColor: "border-border",
  },
};

function categorizeTool(tool: string): ToolCategory {
  const t = tool.toLowerCase();

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
// History Loading
// ---------------------------------------------------------------------------

/**
 * Extract tool execution entries from session message history.
 * Reconstructs ConsoleEntry objects from tool_start/tool_end events in message parts.
 */
function extractToolExecutions(messages: SessionMessage[]): ConsoleEntry[] {
  const entriesMap = new Map<string, ConsoleEntry>();

  messages.forEach((msg) => {
    msg.parts.forEach((part) => {
      const p = part as {
        type?: string;
        id?: string;
        tool?: string;
        name?: string;
        part_id?: string;
        args?: Record<string, unknown>;
        input?: Record<string, unknown>;
        error?: string;
        result?: unknown;
        output?: unknown;
      };
      if (!p.type) return;

      // Resolve the part identifier – live events use part_id, DB uses id
      const partId = p.part_id || p.id;

      // Resolve tool name – may be stored as 'tool' or 'name'
      const toolName = p.tool || p.name;

      // Resolve args – may be stored as 'args' or 'input'
      const toolArgs = p.args || p.input || {};

      // Resolve result – may be stored as 'result' or 'output'
      const toolResult = p.result ?? p.output;

      if (p.type === "tool_start" && toolName && partId) {
        const category = categorizeTool(toolName);
        entriesMap.set(partId, {
          id: partId,
          tool: toolName,
          args: toolArgs,
          status: "running",
          timestamp: new Date(msg.info.time.created),
          sessionId: msg.info.sessionID,
          messageId: msg.info.id,
          category,
        });
      } else if (p.type === "tool_end" && partId) {
        const entry = entriesMap.get(partId);
        if (entry) {
          entry.status = p.error ? "failed" : "completed";
          entry.result = toolResult ? String(toolResult) : undefined;
          entry.error = p.error;
        }
      } else if (p.type === "tool" && toolName) {
        // Persisted tool calls stored as a single combined 'tool' part
        const id = partId || `tool-${Math.random().toString(36).slice(2)}`;
        const category = categorizeTool(toolName);
        entriesMap.set(id, {
          id,
          tool: toolName,
          args: toolArgs,
          status: p.error ? "failed" : "completed",
          result: toolResult ? String(toolResult) : undefined,
          error: p.error,
          timestamp: new Date(msg.info.time.created),
          sessionId: msg.info.sessionID,
          messageId: msg.info.id,
          category,
        });
      }
    });
  });

  // Sort by timestamp
  return Array.from(entriesMap.values()).sort(
    (a, b) => a.timestamp.getTime() - b.timestamp.getTime()
  );
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
}

export function ConsoleTab({ sessionId }: ConsoleTabProps) {
  const [entries, setEntries] = useState<ConsoleEntry[]>([]);
  const [approvals, setApprovals] = useState<Map<string, PendingApproval>>(() => new Map());
  const [isLoadingHistory, setIsLoadingHistory] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);
  const sessionIdRef = useRef(sessionId);
  const loadedSessionRef = useRef<string | null>(null);

  useEffect(() => {
    sessionIdRef.current = sessionId;
  }, [sessionId]);

  // Load historical tool executions when session changes
  useEffect(() => {
    if (!sessionId) {
      setEntries([]);
      setApprovals(new Map());
      loadedSessionRef.current = null;
      return;
    }

    // Skip if already loaded for this session
    if (loadedSessionRef.current === sessionId || isLoadingHistory) {
      return;
    }

    const loadHistory = async () => {
      setIsLoadingHistory(true);
      try {
        const messages = await getSessionMessages(sessionId);
        const toolEntries = extractToolExecutions(messages);
        setEntries(toolEntries);
        setApprovals(new Map());
        loadedSessionRef.current = sessionId;
      } catch (err) {
        console.error("[ConsoleTab] History load failed:", err);
        // Silent fail - history loading is non-critical
        setEntries([]);
        setApprovals(new Map());
      } finally {
        setIsLoadingHistory(false);
      }
    };

    loadHistory();
  }, [sessionId, isLoadingHistory]);

  // Auto-scroll to bottom when new entries arrive
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [entries.length]);

  // -----------------------------------------------------------------------
  // SSE Listener
  // -----------------------------------------------------------------------
  const handleEvent = useCallback((event: StreamEvent) => {
    switch (event.type) {
      case "tool_start": {
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
        setEntries((prev) =>
          prev.map((e) =>
            e.id === event.part_id
              ? {
                  ...e,
                  status: event.error ? "failed" : "completed",
                  result: event.result ? String(event.result) : undefined,
                  error: event.error ?? undefined,
                }
              : e
          )
        );
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

      case "permission_asked": {
        if (!event.tool) return;

        const category = categorizeTool(event.tool);

        // Add a pending entry if we don't have one for this request yet
        setEntries((prev) => {
          // Check if there's already a running entry for this tool in this session
          const existingRunning = prev.find(
            (e) => e.sessionId === event.session_id && e.status === "running"
          );
          if (existingRunning) {
            return prev.map((e) => (e.id === existingRunning.id ? { ...e, status: "pending" } : e));
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
    }
  }, []);

  useEffect(() => {
    let unlistenFn: (() => void) | null = null;
    const setup = async () => {
      unlistenFn = await onSidecarEvent(handleEvent);
    };
    setup();
    return () => {
      unlistenFn?.();
    };
  }, [handleEvent]);

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
    <div className="flex flex-col h-full">
      {/* Header bar */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-border">
        <span className="text-xs text-text-subtle">
          <span className="font-mono tabular-nums">{entries.length}</span> tool
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
      <div className="flex-1 overflow-y-auto px-3 py-2 space-y-2">
        {entries.map((entry) => (
          <ConsoleCard
            key={entry.id}
            entry={entry}
            approval={getApproval(entry)}
            onApprove={handleApprove}
            onDeny={handleDeny}
          />
        ))}
        <div ref={bottomRef} />
      </div>
    </div>
  );
}
