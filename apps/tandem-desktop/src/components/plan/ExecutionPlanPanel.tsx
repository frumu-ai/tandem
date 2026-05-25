import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
  Play,
  X,
  ChevronDown,
  ChevronRight,
  FileText,
  Terminal,
  Trash2,
  Loader2,
  AlertCircle,
} from "lucide-react";
import { DiffViewer } from "./DiffViewer";
import { type StagedOperation } from "@/lib/tauri";
import { cn } from "@/lib/utils";

interface ExecutionPlanPanelProps {
  operations: StagedOperation[];
  onExecute: () => Promise<void>;
  onRemove: (operationId: string) => Promise<void>;
  onClear: () => Promise<void>;
  isExecuting: boolean;
}

export function ExecutionPlanPanel({
  operations,
  onExecute,
  onRemove,
  onClear,
  isExecuting,
}: ExecutionPlanPanelProps) {
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);

  const toggleExpanded = (id: string) => {
    setExpandedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const handleExecute = async () => {
    try {
      setError(null);
      await onExecute();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to execute plan");
    }
  };

  const handleRemove = async (id: string) => {
    try {
      setError(null);
      await onRemove(id);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to remove operation");
    }
  };

  const handleClear = async () => {
    try {
      setError(null);
      await onClear();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to clear staging area");
    }
  };

  const getIcon = (tool: string) => {
    const toolLower = tool.toLowerCase();
    if (toolLower.includes("bash") || toolLower.includes("shell") || toolLower === "command") {
      return <Terminal className="h-4 w-4" />;
    }
    return <FileText className="h-4 w-4" />;
  };

  const getTypeColor = (tool: string) => {
    const toolLower = tool.toLowerCase();
    if (toolLower === "write") return "text-amber-400";
    if (toolLower === "delete") return "text-red-400";
    if (toolLower.includes("bash") || toolLower.includes("shell")) return "text-green-400";
    return "text-blue-400";
  };

  if (operations.length === 0) {
    return null;
  }

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: 20 }}
      className="fixed bottom-4 right-4 w-[500px] max-h-[600px] bg-surface-elevated border border-border rounded-xl shadow-2xl z-50 flex flex-col"
    >
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-border">
        <div className="flex items-center gap-2">
          <div className="h-2 w-2 rounded-full bg-primary animate-pulse" />
          <h3 className="font-semibold text-text">Execution Plan</h3>
          <span className="text-xs px-2 py-0.5 bg-primary/10 text-primary rounded-full">
            {operations.length} {operations.length === 1 ? "change" : "changes"}
          </span>
        </div>
        <button
          onClick={handleClear}
          disabled={isExecuting}
          className="text-text-muted hover:text-text transition-colors disabled:opacity-50"
          title="Clear all"
        >
          <X className="h-4 w-4" />
        </button>
      </div>

      {/* Error banner */}
      {error && (
        <div className="flex items-center gap-2 bg-error/10 px-4 py-2 text-sm text-error">
          <AlertCircle className="h-4 w-4" />
          {error}
          <button onClick={() => setError(null)} className="ml-auto text-error/70 hover:text-error">
            ×
          </button>
        </div>
      )}

      {/* Operations list */}
      <div className="flex-1 overflow-y-auto p-2 space-y-2">
        <AnimatePresence>
          {operations.map((op) => {
            const isExpanded = expandedIds.has(op.id);
            const canShowDiff = op.tool === "write" && op.before_snapshot && op.proposed_content;

            return (
              <motion.div
                key={op.id}
                initial={{ opacity: 0, scale: 0.95 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.95 }}
                className="bg-surface rounded-lg border border-border overflow-hidden"
              >
                {/* Operation header */}
                <div className="flex items-center gap-3 p-3">
                  <div className={cn("flex-shrink-0", getTypeColor(op.tool))}>
                    {getIcon(op.tool)}
                  </div>
                  <div className="flex-1 min-w-0">
                    <p className="text-sm font-medium text-text truncate">{op.description}</p>
                    <p className="text-xs text-text-muted">
                      {op.tool} • {new Date(op.timestamp).toLocaleTimeString()}
                    </p>
                  </div>
                  {canShowDiff && (
                    <button
                      onClick={() => toggleExpanded(op.id)}
                      className="flex-shrink-0 p-1 hover:bg-surface-elevated rounded transition-colors"
                    >
                      {isExpanded ? (
                        <ChevronDown className="h-4 w-4 text-text-muted" />
                      ) : (
                        <ChevronRight className="h-4 w-4 text-text-muted" />
                      )}
                    </button>
                  )}
                  <button
                    onClick={() => handleRemove(op.id)}
                    disabled={isExecuting}
                    className="flex-shrink-0 p-1 hover:bg-error/10 hover:text-error rounded transition-colors disabled:opacity-50"
                    title="Remove from plan"
                  >
                    <Trash2 className="h-4 w-4" />
                  </button>
                </div>

                {/* Expanded diff view */}
                {isExpanded && canShowDiff && (
                  <motion.div
                    initial={{ height: 0, opacity: 0 }}
                    animate={{ height: "auto", opacity: 1 }}
                    exit={{ height: 0, opacity: 0 }}
                    className="border-t border-border p-3"
                  >
                    <DiffViewer
                      oldValue={op.before_snapshot?.content || ""}
                      newValue={op.proposed_content || ""}
                      oldTitle="Current"
                      newTitle="Proposed"
                      splitView={false}
                    />
                  </motion.div>
                )}

                {/* Command preview for bash operations */}
                {isExpanded && (op.tool === "bash" || op.tool === "shell") && (
                  <motion.div
                    initial={{ height: 0, opacity: 0 }}
                    animate={{ height: "auto", opacity: 1 }}
                    exit={{ height: 0, opacity: 0 }}
                    className="border-t border-border p-3"
                  >
                    <div className="bg-surface-elevated rounded p-3 font-mono text-sm text-text">
                      <span className="text-success">$</span>{" "}
                      {(op.args.command as string) || "unknown command"}
                    </div>
                  </motion.div>
                )}
              </motion.div>
            );
          })}
        </AnimatePresence>
      </div>

      {/* Footer with actions */}
      <div className="flex items-center gap-2 px-4 py-3 border-t border-border bg-surface">
        <button
          onClick={handleExecute}
          disabled={isExecuting || operations.length === 0}
          className="flex-1 flex items-center justify-center gap-2 px-4 py-2 bg-primary text-white rounded-lg hover:bg-primary/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {isExecuting ? (
            <>
              <Loader2 className="h-4 w-4 animate-spin" />
              Executing...
            </>
          ) : (
            <>
              <Play className="h-4 w-4" />
              Execute Plan
            </>
          )}
        </button>
        <button
          onClick={handleClear}
          disabled={isExecuting}
          className="px-4 py-2 border border-border rounded-lg hover:bg-surface-elevated transition-colors disabled:opacity-50"
        >
          Clear All
        </button>
      </div>
    </motion.div>
  );
}
