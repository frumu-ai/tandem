import { useState, useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { motion, AnimatePresence } from "framer-motion";
import {
  X,
  Play,
  Pause,
  Square,
  RefreshCw,
  CheckCircle,
  AlertCircle,
  Loader2,
  Sparkles,
} from "lucide-react";
import { Button } from "@/components/ui";
import { BudgetMeter } from "./BudgetMeter";
import { TaskBoard } from "./TaskBoard";
import { ModelSelector } from "@/components/chat/ModelSelector";
import { DEFAULT_ORCHESTRATOR_CONFIG } from "./types";
import type { OrchestratorConfig, RunSnapshot, Run, Task, OrchestratorEvent } from "./types";

interface OrchestratorPanelProps {
  onClose: () => void;
  runId?: string | null;
}

interface OrchestratorModelSelection {
  model?: string | null;
  provider?: string | null;
}

export function OrchestratorPanel({ onClose, runId: initialRunId }: OrchestratorPanelProps) {
  const [runId, setRunId] = useState<string | null>(initialRunId || null);
  const [snapshot, setSnapshot] = useState<RunSnapshot | null>(null);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [runConfig, setRunConfig] = useState<OrchestratorConfig | null>(null);
  const [taskRuntime, setTaskRuntime] = useState<
    Record<string, { status: string; detail?: string }>
  >({});
  const [isLoading, setIsLoading] = useState(false);
  const [isReadOnly, setIsReadOnly] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Objective input for creating a new run
  const [objective, setObjective] = useState("");

  // Model selection
  const [selectedModel, setSelectedModel] = useState<string | undefined>(undefined);
  const [selectedProvider, setSelectedProvider] = useState<string | undefined>(undefined);
  const [runModel, setRunModel] = useState<string | undefined>(undefined);
  const [runProvider, setRunProvider] = useState<string | undefined>(undefined);

  // Revision feedback
  const [showRevisionInput, setShowRevisionInput] = useState(false);
  const [revisionFeedback, setRevisionFeedback] = useState("");

  // Task detail modal
  const [selectedTask, setSelectedTask] = useState<Task | null>(null);

  // Sync internal runId state when parent changes selection
  useEffect(() => {
    if (initialRunId && initialRunId !== runId) {
      setRunId(initialRunId);
      // Reset transient state so poller repopulates fresh
      setSnapshot(null);
      setTasks([]);
      setError(null);
      setRunModel(undefined);
      setRunProvider(undefined);
    }
  }, [initialRunId]);

  // Load current model/provider for an existing run so paused-resume can swap models safely.
  useEffect(() => {
    if (!runId) {
      setRunModel(undefined);
      setRunProvider(undefined);
      return;
    }

    let isMounted = true;
    const loadRunModel = async () => {
      try {
        const selection = await invoke<OrchestratorModelSelection>("orchestrator_get_run_model", {
          runId,
        });
        if (!isMounted) return;

        const model = selection.model ?? undefined;
        const provider = selection.provider ?? undefined;

        setRunModel(model);
        setRunProvider(provider);

        if (model) setSelectedModel(model);
        if (provider) setSelectedProvider(provider);
      } catch {
        // Best-effort only; run may not be fully rehydrated yet.
      }
    };

    loadRunModel();
    return () => {
      isMounted = false;
    };
  }, [runId, snapshot?.status]);

  // Poll for updates when a run is active
  useEffect(() => {
    if (!runId) return;

    let isMounted = true;
    const pollStatus = async () => {
      if (!isMounted) return;

      try {
        // Try to get from active engine first
        const snap = await invoke<RunSnapshot>("orchestrator_get_run", { runId });
        if (!isMounted) return;

        setSnapshot(snap);
        setIsReadOnly(false);

        const taskList = await invoke<Task[]>("orchestrator_list_tasks", { runId });
        if (!isMounted) return;
        setTasks(taskList);

        const config = await invoke<OrchestratorConfig>("orchestrator_get_config", { runId });
        if (!isMounted) return;
        setRunConfig(config);
      } catch (e) {
        // If failed, try to load from disk (historical run)
        if (!isMounted) return;

        try {
          const run = await invoke<Run>("orchestrator_load_run", { runId });
          if (!isMounted) return;

          // Convert Run to Snapshot for display
          setSnapshot({
            run_id: run.run_id,
            status: run.status,
            objective: run.objective,
            task_count: run.tasks.length,
            tasks_completed: run.tasks.filter((t) => t.state === "done").length,
            tasks_failed: run.tasks.filter((t) => t.state === "failed").length,
            budget: run.budget,
            current_task_id: run.tasks.find((t) => t.state === "in_progress")?.id,
            error_message: run.error_message,
            created_at: run.started_at,
            updated_at: run.ended_at || new Date().toISOString(),
          });

          setTasks(run.tasks);
          setRunConfig(run.config);
          setIsReadOnly(true);
        } catch (diskError) {
          console.error("Failed to load run:", e, diskError);
        }
      }
    };

    // Initial poll
    pollStatus();

    // Set up interval - allow polling even for read-only to catch state changes if it becomes active (unlikely but safe)
    // or just to refresh if something changes externally
    const interval = setInterval(pollStatus, 1000);
    return () => {
      isMounted = false;
      clearInterval(interval);
    };
  }, [runId]);

  // Listen for orchestrator events
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;

    const setupListener = async () => {
      unlisten = await listen<OrchestratorEvent>("orchestrator-event", (event) => {
        if (event.payload.type === "task_trace") {
          const taskId = event.payload.task_id as string | undefined;
          const stage = event.payload.stage as string | undefined;
          const detail = event.payload.detail as string | undefined;
          if (!taskId || !stage) return;

          setTaskRuntime((prev) => {
            const next = { ...prev };

            if (stage === "EXEC_FINISHED") {
              delete next[taskId];
              return next;
            }

            const status =
              stage === "TASK_CREATED"
                ? "Queued"
                : stage === "SCHEDULED"
                  ? "Scheduled"
                  : stage === "PERMIT_REQUESTED"
                    ? "Waiting permit"
                    : stage === "PERMIT_ACQUIRED"
                      ? "Running"
                      : stage === "EXEC_STARTED"
                        ? "Running"
                        : stage === "FIRST_TOOL_CALL"
                          ? "Tool started"
                          : stage === "TOOL_CALL_FINISHED"
                            ? "Tool finished"
                            : stage;

            next[taskId] = { status, detail };
            return next;
          });
        }
      });
    };

    setupListener();

    return () => {
      unlisten?.();
    };
  }, []);

  const handleCreateRun = async () => {
    if (!objective.trim()) {
      setError("Please enter an objective");
      return;
    }

    setIsLoading(true);
    setError(null);

    try {
      const config: OrchestratorConfig = {
        ...DEFAULT_ORCHESTRATOR_CONFIG,
        max_total_tokens: 200_000,
        max_tokens_per_step: 25_000,
        max_parallel_tasks: 4,
        llm_parallel: 3,
        fs_write_parallel: 1,
        shell_parallel: 1,
        network_parallel: 2,
      };

      const newRunId = await invoke<string>("orchestrator_create_run", {
        objective,
        config,
        model: selectedModel,
        provider: selectedProvider,
      });

      setRunId(newRunId);
      setRunConfig(config);

      // Start the orchestrator to begin planning
      await invoke("orchestrator_start", { runId: newRunId });
    } catch (e) {
      console.error("Failed to create orchestrator run:", e);
      setError(`Failed to create run: ${e}`);
    } finally {
      setIsLoading(false);
    }
  };

  const handleApprove = async () => {
    if (!runId) return;
    setIsLoading(true);
    try {
      await invoke("orchestrator_approve", { runId });
    } catch (e) {
      setError(`Failed to approve: ${e}`);
    } finally {
      setIsLoading(false);
    }
  };

  const handleRequestRevision = async () => {
    if (!runId || !revisionFeedback.trim()) return;
    setIsLoading(true);
    try {
      await invoke("orchestrator_request_revision", { runId, feedback: revisionFeedback });
      setShowRevisionInput(false);
      setRevisionFeedback("");
    } catch (e) {
      setError(`Failed to request revision: ${e}`);
    } finally {
      setIsLoading(false);
    }
  };

  const handlePause = async () => {
    if (!runId) return;
    try {
      await invoke("orchestrator_pause", { runId });
    } catch (e) {
      setError(`Failed to pause: ${e}`);
    }
  };

  const handleResume = async () => {
    if (!runId) return;
    try {
      await invoke("orchestrator_resume", { runId });
    } catch (e) {
      setError(`Failed to resume: ${e}`);
    }
  };

  const handleCancel = async () => {
    if (!runId) return;
    try {
      await invoke("orchestrator_cancel", { runId });
    } catch (e) {
      setError(`Failed to cancel: ${e}`);
    }
  };

  const handleSetResumeModel = async () => {
    if (!runId || !selectedModel || !selectedProvider) return;
    setIsLoading(true);
    try {
      const selection = await invoke<OrchestratorModelSelection>("orchestrator_set_resume_model", {
        runId,
        model: selectedModel,
        provider: selectedProvider,
      });

      const model = selection.model ?? selectedModel;
      const provider = selection.provider ?? selectedProvider;
      setRunModel(model);
      setRunProvider(provider);
      setSelectedModel(model);
      setSelectedProvider(provider);
    } catch (e) {
      setError(`Failed to set resume model: ${e}`);
    } finally {
      setIsLoading(false);
    }
  };

  const handleRestart = async () => {
    if (!runId) return;
    setIsLoading(true);
    try {
      await invoke("orchestrator_restart_run", { runId });
    } catch (e) {
      setError(`Failed to restart: ${e}`);
    } finally {
      setIsLoading(false);
    }
  };

  const handleNewRun = () => {
    if (runId && snapshot) {
      const confirmed = window.confirm(
        "Start a fresh run? This clears the current orchestration from view."
      );
      if (!confirmed) return;
    }
    setRunId(null);
    setSnapshot(null);
    setTasks([]);
    setObjective("");
    setRunModel(undefined);
    setRunProvider(undefined);
    setError(null);
  };

  const runStatus = snapshot?.status;
  const isActive = runStatus === "planning" || runStatus === "executing";
  const isAwaitingApproval = runStatus === "awaiting_approval";
  const isPaused = runStatus === "paused";
  const isCompleted = runStatus === "completed";
  const isFailed = runStatus === "failed";
  const isCancelled = runStatus === "cancelled";
  const canAdjustResumeModel = isPaused || isCancelled || isFailed;
  const canShowRuntimeControls = Boolean(snapshot && (!isReadOnly || isPaused || isActive));
  const runningCount = tasks.filter((t) => t.state === "in_progress").length;
  const hasRunningTasks = runningCount > 0;
  const hasFailedTasks = tasks.some((t) => t.state === "failed");
  const canPause = runStatus === "executing" && hasRunningTasks && !isReadOnly;
  const hasModelSelection = Boolean(selectedModel && selectedProvider);
  const resumeModelChanged =
    canAdjustResumeModel &&
    hasModelSelection &&
    (selectedModel !== runModel || selectedProvider !== runProvider);
  const canApplyResumeModel = Boolean(
    canAdjustResumeModel && hasModelSelection && resumeModelChanged && !isLoading
  );
  const canResume = isPaused && !resumeModelChanged;
  const canCancel = Boolean(
    snapshot &&
    !isCompleted &&
    !isFailed &&
    !isCancelled &&
    (runStatus === "planning" || hasRunningTasks || !hasFailedTasks)
  );
  const showRetryFailedInline = runStatus === "executing" && hasFailedTasks && !hasRunningTasks;
  const showTerminalControls = isCompleted || isFailed || isCancelled || showRetryFailedInline;
  const terminalPrimaryLabel = isCancelled
    ? "Resume Run"
    : isFailed || showRetryFailedInline
      ? "Retry Failed Tasks"
      : "Restart Run";

  const maxParallelTasks =
    runConfig?.max_parallel_tasks ?? DEFAULT_ORCHESTRATOR_CONFIG.max_parallel_tasks ?? 1;

  const tasksForDisplay = useMemo(
    () =>
      tasks.map((t) => ({
        ...t,
        runtime_status: taskRuntime[t.id]?.status,
        runtime_detail: taskRuntime[t.id]?.detail,
      })),
    [tasks, taskRuntime]
  );

  return (
    <div className="flex h-full flex-col bg-surface">
      <div className="flex h-full flex-col">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-border px-4 py-3">
          <div className="flex items-center gap-2">
            <Sparkles className="h-5 w-5 text-primary" />
            <h3 className="font-semibold text-text">Orchestrator</h3>
            {snapshot && (
              <div className="flex items-center gap-2">
                {isReadOnly && (
                  <span className="rounded bg-surface-elevated px-1.5 py-0.5 text-[10px] font-medium text-text-muted border border-border">
                    Read Only
                  </span>
                )}
                <span
                  className={`rounded-full px-2 py-0.5 text-[10px] uppercase tracking-wide ${
                    snapshot.status === "completed"
                      ? "bg-emerald-500/10 text-emerald-500"
                      : snapshot.status === "failed"
                        ? "bg-red-500/10 text-red-500"
                        : "bg-surface-elevated text-text-muted"
                  }`}
                >
                  {snapshot.status.replace("_", " ")}
                </span>
                {isActive && (
                  <span className="rounded-full bg-surface-elevated px-2 py-0.5 text-[10px] uppercase tracking-wide text-text-muted">
                    Running ({runningCount}/{maxParallelTasks})
                  </span>
                )}
              </div>
            )}
          </div>
          <button
            onClick={onClose}
            className="rounded p-1 text-text-subtle hover:bg-surface-elevated hover:text-text"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-4">
          <AnimatePresence mode="wait">
            {!runId ? (
              <motion.div
                key="new-run"
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -20 }}
                className="space-y-4"
              >
                <div>
                  <div className="flex items-center justify-between mb-2">
                    <h4 className="text-sm font-medium text-text">
                      What would you like to accomplish?
                    </h4>
                    <ModelSelector
                      currentModel={selectedModel}
                      align="right"
                      side="bottom"
                      onModelSelect={(modelId, providerId) => {
                        setSelectedModel(modelId);
                        setSelectedProvider(providerId);
                      }}
                    />
                  </div>
                  <textarea
                    value={objective}
                    onChange={(e) => setObjective(e.target.value)}
                    placeholder="Describe your objective in detail. The orchestrator will break it down into tasks and execute them..."
                    className="w-full rounded-lg border border-border bg-surface-elevated p-3 text-sm text-text placeholder:text-text-muted focus:border-primary focus:outline-none min-h-[120px] resize-none"
                  />
                </div>

                <Button
                  onClick={handleCreateRun}
                  disabled={isLoading || !objective.trim()}
                  className="w-full"
                >
                  {isLoading ? (
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  ) : (
                    <Play className="mr-2 h-4 w-4" />
                  )}
                  Start Orchestration
                </Button>
              </motion.div>
            ) : snapshot ? (
              <motion.div
                key="run-active"
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -20 }}
                className="space-y-4"
              >
                {/* Objective */}
                <div className="rounded-lg bg-surface-elevated p-3">
                  <div className="text-[10px] uppercase tracking-wide text-text-subtle mb-1">
                    Objective
                  </div>
                  <p className="text-sm text-text">{snapshot.objective}</p>
                </div>

                {/* Budget */}
                <BudgetMeter budget={snapshot.budget} />

                {/* Controls */}
                {(canShowRuntimeControls || showTerminalControls) && (
                  <div className="rounded-lg border border-border bg-surface-elevated/40 p-3 space-y-2">
                    <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                      Controls
                    </div>

                    {canAdjustResumeModel && (
                      <div className="space-y-2">
                        <div className="flex items-center gap-2">
                          <ModelSelector
                            currentModel={selectedModel}
                            className="flex-1"
                            align="left"
                            side="bottom"
                            onModelSelect={(modelId, providerId) => {
                              setSelectedModel(modelId);
                              setSelectedProvider(providerId);
                            }}
                          />
                          <Button
                            variant="secondary"
                            size="sm"
                            onClick={handleSetResumeModel}
                            disabled={!canApplyResumeModel}
                          >
                            Apply Model
                          </Button>
                        </div>
                        <p className="text-xs text-text-muted">
                          Current run model:{" "}
                          {runModel
                            ? `${runProvider ? `${runProvider}/` : ""}${runModel}`
                            : "unknown"}
                          {resumeModelChanged ? " (apply selection before resume)" : ""}
                        </p>
                      </div>
                    )}

                    {canShowRuntimeControls && (canPause || canResume || canCancel) && (
                      <div className="flex gap-2">
                        {canPause && (
                          <Button variant="secondary" onClick={handlePause} className="flex-1">
                            <Pause className="mr-1 h-4 w-4" />
                            Pause
                          </Button>
                        )}
                        {canResume && (
                          <Button
                            variant="secondary"
                            onClick={handleResume}
                            className="flex-1"
                            disabled={isLoading}
                          >
                            <Play className="mr-1 h-4 w-4" />
                            Resume Pending Tasks
                          </Button>
                        )}
                        {canCancel && (
                          <Button variant="danger" onClick={handleCancel} className="flex-1">
                            <Square className="mr-1 h-4 w-4" />
                            Cancel
                          </Button>
                        )}
                      </div>
                    )}

                    {showTerminalControls && (
                      <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                        <Button variant="secondary" onClick={handleRestart}>
                          <Play className="mr-1 h-4 w-4" />
                          {terminalPrimaryLabel}
                        </Button>
                        <Button variant="secondary" onClick={handleNewRun} className="w-full">
                          <RefreshCw className="mr-1 h-4 w-4" />
                          Start Fresh Run
                        </Button>
                      </div>
                    )}

                    <p className="text-xs text-text-muted">
                      Resume continues pending tasks.{" "}
                      {isCancelled
                        ? "Resume Run continues this cancelled run from remaining work."
                        : isFailed || showRetryFailedInline
                          ? "Retry Failed Tasks reruns failed tasks and unfinished work."
                          : "Restart Run retries failed or in-progress tasks."}{" "}
                      Start Fresh Run clears this run from view.
                    </p>
                  </div>
                )}

                {/* Status-specific content */}
                {isAwaitingApproval && (
                  <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-4 space-y-3">
                    <div className="flex items-center gap-2 text-amber-200">
                      <AlertCircle className="h-4 w-4" />
                      <span className="text-sm font-medium">Plan Ready for Review</span>
                    </div>
                    <p className="text-xs text-amber-200/80">
                      Review the tasks below and approve to start execution, or request changes.
                    </p>

                    {showRevisionInput ? (
                      <div className="space-y-2">
                        <textarea
                          value={revisionFeedback}
                          onChange={(e) => setRevisionFeedback(e.target.value)}
                          placeholder="Describe what changes you'd like..."
                          className="w-full rounded-lg border border-border bg-surface p-2 text-sm text-text placeholder:text-text-muted focus:border-primary focus:outline-none min-h-[80px] resize-none"
                        />
                        <div className="flex gap-2">
                          <Button
                            variant="secondary"
                            size="sm"
                            onClick={() => setShowRevisionInput(false)}
                          >
                            Cancel
                          </Button>
                          <Button
                            size="sm"
                            onClick={handleRequestRevision}
                            disabled={!revisionFeedback.trim() || isLoading}
                          >
                            Submit Feedback
                          </Button>
                        </div>
                      </div>
                    ) : (
                      <div className="flex gap-2">
                        <Button
                          variant="secondary"
                          onClick={() => setShowRevisionInput(true)}
                          disabled={isLoading}
                          className="flex-1"
                        >
                          <RefreshCw className="mr-1 h-4 w-4" />
                          Request Changes
                        </Button>
                        <Button onClick={handleApprove} disabled={isLoading} className="flex-1">
                          <CheckCircle className="mr-1 h-4 w-4" />
                          Approve & Execute
                        </Button>
                      </div>
                    )}
                  </div>
                )}

                {isCompleted && (
                  <div className="rounded-lg border border-emerald-500/30 bg-emerald-500/10 p-4">
                    <div className="flex items-center gap-2 text-emerald-200">
                      <CheckCircle className="h-4 w-4" />
                      <span className="text-sm font-medium">Completed Successfully</span>
                    </div>
                    <p className="mt-1 text-xs text-emerald-200/80">
                      All tasks have been executed. Review the results below.
                    </p>
                  </div>
                )}

                {isFailed && (
                  <div className="rounded-lg border border-red-500/30 bg-red-500/10 p-4">
                    <div className="flex items-center gap-2 text-red-200">
                      <AlertCircle className="h-4 w-4" />
                      <span className="text-sm font-medium">Execution Failed</span>
                    </div>
                    {snapshot.error_message && (
                      <p className="mt-1 text-xs text-red-200/80">{snapshot.error_message}</p>
                    )}
                  </div>
                )}

                {/* Task Board or Loading State */}
                {snapshot.status === "planning" && tasks.length === 0 ? (
                  <div className="flex flex-col items-center justify-center rounded-lg border border-border bg-surface py-12 text-center">
                    <Loader2 className="mb-4 h-8 w-8 animate-spin text-primary" />
                    <h3 className="text-lg font-medium text-text">Generating Plan</h3>
                    <p className="mt-1 max-w-sm text-sm text-text-muted">
                      The orchestrator is analyzing your request and breaking it down into
                      executable tasks...
                    </p>
                  </div>
                ) : (
                  <TaskBoard
                    tasks={tasksForDisplay}
                    currentTaskId={snapshot.current_task_id}
                    onTaskClick={(task) => setSelectedTask(task)}
                  />
                )}
              </motion.div>
            ) : (
              <div className="flex h-32 items-center justify-center text-text-subtle">
                <Loader2 className="h-5 w-5 animate-spin" />
              </div>
            )}
          </AnimatePresence>

          {/* Error display */}
          {error && (
            <div className="mt-4 rounded-lg border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-300">
              {error}
            </div>
          )}
        </div>
      </div>

      {/* Task Detail Modal */}
      <AnimatePresence>
        {selectedTask && (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
            onClick={() => setSelectedTask(null)}
          >
            <motion.div
              initial={{ scale: 0.95, y: 20 }}
              animate={{ scale: 1, y: 0 }}
              exit={{ scale: 0.95, y: 20 }}
              onClick={(e) => e.stopPropagation()}
              className="w-full max-w-2xl rounded-lg border border-border bg-surface shadow-xl overflow-hidden"
            >
              {/* Header */}
              <div className="flex items-center justify-between border-b border-border px-6 py-4">
                <div className="flex items-center gap-3">
                  <span className="text-lg font-semibold text-text">{selectedTask.title}</span>
                  <span className="rounded-full bg-surface-elevated px-2 py-0.5 text-xs text-text-muted">
                    {selectedTask.id}
                  </span>
                </div>
                <button
                  onClick={() => setSelectedTask(null)}
                  className="rounded-lg p-1 hover:bg-surface-elevated transition-colors"
                >
                  <X className="h-5 w-5 text-text-muted" />
                </button>
              </div>

              {/* Content */}
              <div className="max-h-[70vh] overflow-y-auto p-6 space-y-4">
                {/* Description */}
                <div>
                  <h4 className="text-sm font-medium text-text-subtle mb-2">Description</h4>
                  <p className="text-sm text-text whitespace-pre-wrap">
                    {selectedTask.description || "No description provided"}
                  </p>
                </div>

                {/* State */}
                <div>
                  <h4 className="text-sm font-medium text-text-subtle mb-2">Status</h4>
                  <span className="inline-flex items-center gap-1.5 rounded-full bg-surface-elevated px-3 py-1 text-sm">
                    {selectedTask.state.replace("_", " ").toUpperCase()}
                  </span>
                </div>

                {/* Dependencies */}
                {selectedTask.dependencies.length > 0 && (
                  <div>
                    <h4 className="text-sm font-medium text-text-subtle mb-2">Dependencies</h4>
                    <div className="flex flex-wrap gap-2">
                      {selectedTask.dependencies.map((depId) => (
                        <span
                          key={depId}
                          className="rounded-full bg-surface-elevated px-3 py-1 text-sm text-text-muted"
                        >
                          {depId}
                        </span>
                      ))}
                    </div>
                  </div>
                )}

                {/* Error */}
                {selectedTask.error_message && (
                  <div>
                    <h4 className="text-sm font-medium text-red-400 mb-2">Error</h4>
                    <p className="text-sm text-red-300 bg-red-500/10 rounded-lg p-3 border border-red-500/30">
                      {selectedTask.error_message}
                    </p>
                  </div>
                )}
              </div>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
