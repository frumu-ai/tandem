import { useState, useEffect } from "react";
import { X, Pause, Play, Square, FileText } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui";
import type { RalphStateSnapshot, IterationRecord } from "./types";

interface RalphPanelProps {
  runId: string;
  onClose: () => void;
}

export function RalphPanel({ runId, onClose }: RalphPanelProps) {
  const [status, setStatus] = useState<RalphStateSnapshot | null>(null);
  const [history, setHistory] = useState<IterationRecord[]>([]);
  const [contextInput, setContextInput] = useState("");
  const [isLoading, setIsLoading] = useState(false);

  // Poll status every 1s when active
  useEffect(() => {
    const pollStatus = async () => {
      try {
        const s = await invoke<RalphStateSnapshot>("ralph_status", { runId });
        setStatus(s);

        // Load history if active or completed
        if (s.status === "running" || s.status === "paused" || s.status === "completed") {
          const h = await invoke<IterationRecord[]>("ralph_history", { runId, limit: 10 });
          setHistory(h);
        }
      } catch (error) {
        console.error("Failed to poll Ralph status:", error);
      }
    };

    // Initial poll
    pollStatus();

    // Set up interval
    const interval = setInterval(pollStatus, 1000);
    return () => clearInterval(interval);
  }, [runId]);

  const handlePause = async () => {
    setIsLoading(true);
    try {
      await invoke("ralph_pause", { runId });
    } catch (error) {
      console.error("Failed to pause Ralph loop:", error);
    } finally {
      setIsLoading(false);
    }
  };

  const handleResume = async () => {
    setIsLoading(true);
    try {
      await invoke("ralph_resume", { runId });
    } catch (error) {
      console.error("Failed to resume Ralph loop:", error);
    } finally {
      setIsLoading(false);
    }
  };

  const handleCancel = async () => {
    setIsLoading(true);
    try {
      await invoke("ralph_cancel", { runId });
    } catch (error) {
      console.error("Failed to cancel Ralph loop:", error);
    } finally {
      setIsLoading(false);
    }
  };

  const handleAddContext = async () => {
    if (!contextInput.trim()) return;

    setIsLoading(true);
    try {
      await invoke("ralph_add_context", { runId, text: contextInput });
      setContextInput("");
    } catch (error) {
      console.error("Failed to add context:", error);
    } finally {
      setIsLoading(false);
    }
  };

  const formatDuration = (ms?: number) => {
    if (!ms) return "N/A";
    if (ms < 1000) return `${ms}ms`;
    return `${(ms / 1000).toFixed(1)}s`;
  };

  return (
    <div className="fixed inset-y-0 right-0 z-50 w-96 border-l border-border bg-surface shadow-xl">
      <div className="flex h-full flex-col">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-border px-4 py-3">
          <h3 className="font-semibold text-text">Ralph Loop</h3>
          <button
            onClick={onClose}
            className="rounded p-1 text-text-subtle hover:bg-surface-elevated hover:text-text"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-4">
          {status ? (
            <div className="space-y-6">
              {/* Status Overview */}
              <div className="grid grid-cols-2 gap-3">
                <div className="rounded-lg bg-surface-elevated p-3">
                  <div className="text-[10px] uppercase tracking-wide text-text-subtle">Status</div>
                  <div className="mt-1 font-medium capitalize text-text">{status.status}</div>
                </div>
                <div className="rounded-lg bg-surface-elevated p-3">
                  <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                    Iteration
                  </div>
                  <div className="mt-1 font-medium text-text">
                    {status.iteration} / {status.total_iterations}
                  </div>
                </div>
                <div className="rounded-lg bg-surface-elevated p-3">
                  <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                    Last Duration
                  </div>
                  <div className="mt-1 font-medium text-text">
                    {formatDuration(status.last_duration_ms)}
                  </div>
                </div>
                <div className="rounded-lg bg-surface-elevated p-3">
                  <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                    Files Modified
                  </div>
                  <div className="mt-1 font-medium text-text">{status.files_modified_count}</div>
                </div>
              </div>

              {/* Struggle Warning */}
              {status.struggle_detected && (
                <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-200">
                  <strong>Struggle detected:</strong> The loop may be stuck. Consider adding context
                  or adjusting your approach.
                </div>
              )}

              {/* Control Buttons */}
              <div className="flex gap-2">
                {status.status === "running" && (
                  <Button
                    variant="secondary"
                    onClick={handlePause}
                    disabled={isLoading}
                    className="flex-1"
                  >
                    <Pause className="mr-1 h-4 w-4" />
                    Pause
                  </Button>
                )}
                {status.status === "paused" && (
                  <Button
                    variant="secondary"
                    onClick={handleResume}
                    disabled={isLoading}
                    className="flex-1"
                  >
                    <Play className="mr-1 h-4 w-4" />
                    Resume
                  </Button>
                )}
                {(status.status === "running" || status.status === "paused") && (
                  <Button
                    variant="danger"
                    onClick={handleCancel}
                    disabled={isLoading}
                    className="flex-1"
                  >
                    <Square className="mr-1 h-4 w-4" />
                    Cancel
                  </Button>
                )}
              </div>

              {/* Add Context */}
              <div className="space-y-2">
                <label className="text-sm font-medium text-text">
                  Add Context for Next Iteration
                </label>
                <textarea
                  value={contextInput}
                  onChange={(e) => setContextInput(e.target.value)}
                  className="w-full rounded-lg border border-border bg-surface-elevated p-3 text-sm text-text placeholder:text-text-muted focus:border-primary focus:outline-none"
                  rows={3}
                  placeholder="Enter additional context or instructions..."
                />
                <Button
                  onClick={handleAddContext}
                  disabled={!contextInput.trim() || isLoading}
                  className="w-full"
                >
                  Add Context
                </Button>
              </div>

              {/* History */}
              <div className="space-y-2">
                <h4 className="text-sm font-medium text-text">Recent History</h4>
                {history.length === 0 ? (
                  <div className="rounded-lg bg-surface-elevated p-4 text-center text-sm text-text-subtle">
                    No iterations yet
                  </div>
                ) : (
                  <div className="space-y-2">
                    {[...history]
                      .reverse()
                      .slice(0, 5)
                      .map((record) => (
                        <div
                          key={record.iteration}
                          className="rounded-lg border border-border bg-surface-elevated p-3 text-sm"
                        >
                          <div className="flex items-center justify-between">
                            <span className="font-medium text-text">
                              Iteration {record.iteration}
                            </span>
                            <span className="text-xs text-text-subtle">
                              {formatDuration(record.duration_ms)}
                            </span>
                          </div>
                          <div className="mt-1 text-xs text-text-subtle">
                            {record.files_modified.length} files modified
                            {record.completion_detected && (
                              <span className="ml-2 text-emerald-400">(completion detected)</span>
                            )}
                          </div>
                          {record.errors.length > 0 && (
                            <div className="mt-1 text-xs text-red-400">
                              {record.errors.length} error(s)
                            </div>
                          )}
                        </div>
                      ))}
                  </div>
                )}
              </div>
            </div>
          ) : (
            <div className="flex h-32 items-center justify-center text-text-subtle">Loading...</div>
          )}
        </div>

        {/* Footer */}
        <div className="border-t border-border p-4">
          <button
            onClick={() => invoke("open_file", { path: ".opencode/tandem/ralph/history.json" })}
            className="flex w-full items-center justify-center gap-2 rounded-lg border border-border bg-surface-elevated px-4 py-2 text-sm text-text-subtle transition-colors hover:bg-surface hover:text-text"
          >
            <FileText className="h-4 w-4" />
            View History File
          </button>
        </div>
      </div>
    </div>
  );
}
