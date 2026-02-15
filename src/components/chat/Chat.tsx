import { startTransition, useState, useRef, useEffect, useCallback, type ReactNode } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { useVirtualizer } from "@tanstack/react-virtual";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Message, type MessageProps } from "./Message";
import { ChatInput, type FileAttachment } from "./ChatInput";
import {
  PermissionToastContainer,
  type PermissionRequest,
} from "@/components/permissions/PermissionToast";
import { ExecutionPlanPanel } from "@/components/plan/ExecutionPlanPanel";
import { PlanViewer } from "@/components/plan/PlanViewer";
import { PlanSelector } from "@/components/plan/PlanSelector";
import { PlanActionButtons } from "./PlanActionButtons";
import { QuestionDialog } from "./QuestionDialog";
import { useStagingArea } from "@/hooks/useStagingArea";
import { usePlans } from "@/hooks/usePlans";
import {
  FolderOpen,
  Sparkles,
  Link2,
  AlertCircle,
  Loader2,
  Settings as SettingsIcon,
  ChevronDown,
} from "lucide-react";
import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/utils";
import {
  startSidecar,
  getSidecarStatus,
  getSidecarStartupHealth,
  createSession,
  sendMessageAndStartRun,
  cancelGeneration,
  onSidecarEventV2,
  queueMessage,
  queueList,
  queueRemove,
  queueSendNext,
  approveTool,
  denyTool,
  listQuestions,
  replyQuestion,
  rejectQuestion,
  getSessionMessages,
  listToolExecutions,
  type ToolExecutionRow,
  undoViaCommand,
  isGitRepo,
  getToolGuidance,
  getProvidersConfig,
  setProvidersConfig,
  type ProvidersConfig,
  type StreamEvent,
  type StreamEventEnvelopeV2,
  type QueuedMessage,
  type SidecarState,
  type SidecarStartupHealth,
  type TodoItem,
  type QuestionRequestEvent,
  type FileAttachmentInput,
  startPlanSession,
  ralphStatus,
  type RalphStateSnapshot,
} from "@/lib/tauri";
import { RalphPanel } from "@/components/ralph";
import { LogsDrawer } from "@/components/logs";
import { PythonSetupWizard } from "@/components/python";

interface ChatProps {
  workspacePath: string | null;
  sessionId?: string | null;
  onSessionCreated?: (sessionId: string) => void;
  onSidecarConnected?: () => void;
  usePlanMode?: boolean;
  onPlanModeChange?: (enabled: boolean) => void;
  onToggleTaskSidebar?: () => void;
  executePendingTasksTrigger?: number;
  onGeneratingChange?: (isGenerating: boolean) => void;
  pendingTasks?: TodoItem[];
  fileToAttach?: FileAttachment | null;
  onFileAttached?: () => void;
  selectedAgent?: string;
  onAgentChange?: (agent: string | undefined) => void;
  onFileOpen?: (filePath: string) => void;
  hasConfiguredProvider?: boolean;
  activeProviderId?: string;
  activeProviderLabel?: string;
  activeModelLabel?: string;
  onOpenSettings?: () => void;
  onProviderChange?: () => void;
  onOpenPacks?: () => void;
  onOpenExtensions?: (tab?: "skills" | "plugins" | "mcp" | "modes") => void;
  draftMessage?: string;
  onDraftMessageConsumed?: () => void;
  activeOrchestrationCount?: number;
  activeChatRunningCount?: number;
}

function startupPhaseLabel(phase?: string | null): string {
  switch ((phase || "").toLowerCase()) {
    case "migration":
      return "Scanning legacy data";
    case "storage_init":
      return "Loading sessions and storage";
    case "config_init":
      return "Loading provider config";
    case "registry_init":
      return "Initializing registries";
    case "engine_loop_init":
      return "Starting runtime loop";
    case "ready":
      return "Ready";
    case "failed":
      return "Startup failed";
    default:
      return "Starting sidecar";
  }
}

function startupPhaseDetail(phase?: string | null): string {
  switch ((phase || "").toLowerCase()) {
    case "migration":
      return "Checking legacy data and migration markers.";
    case "storage_init":
      return "Loading session history and workspace metadata.";
    case "config_init":
      return "Loading providers, models, and runtime config.";
    case "registry_init":
      return "Preparing tools, plugins, and registries.";
    case "engine_loop_init":
      return "Starting message processing and event pipeline.";
    case "ready":
      return "Engine startup complete.";
    case "failed":
      return "Startup failed. Open logs for details.";
    default:
      return "Preparing engine startup components.";
  }
}

function startupPhaseProgress(phase?: string | null): number {
  switch ((phase || "").toLowerCase()) {
    case "migration":
      return 12;
    case "storage_init":
      return 36;
    case "config_init":
      return 56;
    case "registry_init":
      return 72;
    case "engine_loop_init":
      return 88;
    case "ready":
      return 100;
    case "failed":
      return 100;
    default:
      return 6;
  }
}

function stringifyPermissionValue(value: unknown): string | undefined {
  if (typeof value === "string" && value.trim().length > 0) {
    return value;
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  if (Array.isArray(value) && value.length > 0) {
    return value
      .map((item) => stringifyPermissionValue(item))
      .filter((item): item is string => Boolean(item))
      .join(", ");
  }
  return undefined;
}

function buildPermissionReason(
  tool?: string,
  args?: Record<string, unknown>
): { reasoning: string; path?: string; command?: string } {
  const normalizedTool = (tool || "unknown").toLowerCase();
  const pathCandidate =
    stringifyPermissionValue(args?.path) ||
    stringifyPermissionValue(args?.cwd) ||
    stringifyPermissionValue(args?.directory) ||
    stringifyPermissionValue(args?.file_path);
  const patternCandidate =
    stringifyPermissionValue(args?.pattern) ||
    stringifyPermissionValue(args?.glob) ||
    stringifyPermissionValue(args?.query);
  const commandCandidate =
    stringifyPermissionValue(args?.command) || stringifyPermissionValue(args?.cmd);

  switch (normalizedTool) {
    case "glob":
      return {
        reasoning: patternCandidate
          ? `AI wants to search files matching pattern: ${patternCandidate}`
          : "AI wants to search files in your workspace.",
        path: pathCandidate,
      };
    case "read":
    case "read_file":
      return {
        reasoning: pathCandidate
          ? `AI wants to read this file: ${pathCandidate}`
          : "AI wants to read a file in your workspace.",
        path: pathCandidate,
      };
    case "list":
    case "ls":
    case "list_directory":
      return {
        reasoning: pathCandidate
          ? `AI wants to list this directory: ${pathCandidate}`
          : "AI wants to list files in your workspace.",
        path: pathCandidate,
      };
    case "bash":
    case "run_command":
    case "shell":
      return {
        reasoning: commandCandidate
          ? `AI wants to run this command: ${commandCandidate}`
          : "AI wants to run a shell command in your workspace.",
        command: commandCandidate,
        path: pathCandidate,
      };
    default:
      return {
        reasoning: `AI requests permission for tool '${tool || "unknown"}'.`,
        path: pathCandidate,
        command: commandCandidate,
      };
  }
}

export function Chat({
  workspacePath,
  sessionId: propSessionId,
  onSessionCreated,
  onSidecarConnected,
  usePlanMode: propUsePlanMode = false,
  onPlanModeChange,
  onToggleTaskSidebar,
  executePendingTasksTrigger,
  onGeneratingChange,
  pendingTasks,
  fileToAttach,
  onFileAttached,
  selectedAgent: propSelectedAgent,
  onAgentChange: propOnAgentChange,
  onFileOpen,
  hasConfiguredProvider = true,
  activeProviderId: _activeProviderId,
  activeProviderLabel,
  activeModelLabel,
  onOpenSettings,
  onProviderChange,
  onOpenPacks,
  onOpenExtensions,
  draftMessage,
  onDraftMessageConsumed,
  activeOrchestrationCount = 0,
  activeChatRunningCount = 0,
}: ChatProps) {
  const [messages, setMessages] = useState<MessageProps[]>([]);
  const [isGenerating, setIsGenerating] = useState(false);
  const isGeneratingRef = useRef(isGenerating);
  useEffect(() => {
    isGeneratingRef.current = isGenerating;
  }, [isGenerating]);
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(propSessionId || null);
  const [allowAllTools, setAllowAllTools] = useState(false);
  const [enabledToolCategories, setEnabledToolCategories] = useState<Set<string>>(new Set());
  // Ralph Loop State
  const [loopEnabled, setLoopEnabled] = useState(false);
  const [showRalphPanel, setShowRalphPanel] = useState(false);
  const [showLogsDrawer, setShowLogsDrawer] = useState(false);
  const [showPythonWizard, setShowPythonWizard] = useState(false);
  const [ralphStatusSnapshot, setRalphStatusSnapshot] = useState<RalphStateSnapshot | undefined>(
    undefined
  );
  const [, forceUpdate] = useState({}); // Keep for critical display updates

  // Notify parent when generating state changes
  useEffect(() => {
    onGeneratingChange?.(isGenerating);
  }, [isGenerating, onGeneratingChange]);

  // If Python is blocked due to missing workspace venv, pop the wizard automatically.
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    listen<{ reason: string; workspace_path: string | null }>("python-setup-required", (event) => {
      console.warn("[PythonWizard] setup required:", event.payload?.reason);
      setShowPythonWizard(true);
    })
      .then((u) => {
        unlisten = u;
      })
      .catch((e) => {
        console.error("[PythonWizard] failed to attach listener:", e);
      });
    return () => {
      if (unlisten) unlisten();
    };
  }, []);
  const [sidecarStatus, setSidecarStatus] = useState<SidecarState>("stopped");
  const [error, setError] = useState<string | null>(null);
  const [isConnecting, setIsConnecting] = useState(false);
  const [pendingPermissions, setPendingPermissions] = useState<PermissionRequest[]>([]);
  const [pendingQuestionRequests, setPendingQuestionRequests] = useState<QuestionRequestEvent[]>(
    []
  );
  const [statusBanner, setStatusBanner] = useState<string | null>(null);
  const [streamHealth, setStreamHealth] = useState<"healthy" | "degraded" | "recovering">(
    "healthy"
  );
  const [startupHealth, setStartupHealth] = useState<SidecarStartupHealth | null>(null);
  const [engineReady, setEngineReady] = useState(false);
  const [queuedMessages, setQueuedMessages] = useState<QueuedMessage[]>([]);
  // const [activities, setActivities] = useState<ActivityItem[]>([]);
  const [isLoadingHistory, setIsLoadingHistory] = useState(false);
  const [isGitRepository, setIsGitRepository] = useState(false);
  const handledPermissionIdsRef = useRef<Set<string>>(new Set());
  const handledQuestionRequestIdsRef = useRef<Set<string>>(new Set());
  const pendingQuestionToolCallIdsRef = useRef<Set<string>>(new Set());
  const pendingQuestionToolMessageIdsRef = useRef<Set<string>>(new Set());

  // Support both new agent prop and legacy usePlanMode
  const selectedAgent =
    propSelectedAgent !== undefined ? propSelectedAgent : propUsePlanMode ? "plan" : undefined;
  const selectedModeId = selectedAgent === "general" ? "ask" : selectedAgent;
  const onAgentChange =
    propOnAgentChange ||
    ((agent: string | undefined) => {
      onPlanModeChange?.(agent === "plan");
    });
  const usePlanMode = selectedModeId === "plan";
  const hasPendingQuestionOverlay = pendingQuestionRequests.length > 0;
  const setUsePlanMode = (enabled: boolean) => {
    onAgentChange(enabled ? "plan" : undefined);
  };
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const messagesRef = useRef<MessageProps[]>([]);
  const currentAssistantMessageRef = useRef<string>("");
  const currentAssistantMessageIdRef = useRef<string | null>(null);
  const seenEventIdsRef = useRef<Set<string>>(new Set());
  const queueDrainRef = useRef(false);
  const assistantFlushFrameRef = useRef<number | null>(null);
  const generationTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const statusBannerTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastEventAtRef = useRef<number | null>(null);
  const prevPropSessionIdRef = useRef<string | null>(null);
  // Track the currently active session without causing re-renders (used by stream handlers).
  const currentSessionIdRef = useRef<string | null>(null);
  // If we try to load history before the sidecar is running, stash the session id and retry later.
  const deferredSessionLoadRef = useRef<string | null>(null);
  const engineReadyRef = useRef(false);
  useEffect(() => {
    engineReadyRef.current = engineReady;
  }, [engineReady]);

  // Staging area hook
  const {
    stagedOperations,
    isExecuting: isExecutingPlan,
    stageOperation,
    removeOperation,
    executePlan,
    clearStaging,
  } = useStagingArea();

  // Plan management hook
  const {
    plans,
    activePlan,
    setActivePlan,
    isLoading: isLoadingPlans,
    refreshPlans,
  } = usePlans(workspacePath);
  const [showPlanView, setShowPlanView] = useState(false);
  // Backend startup can run an extended 240s path on first-run/import repair.
  // Keep frontend attach wait above that to avoid false reconnect churn.
  const SIDECAR_ATTACH_TIMEOUT_MS = 250_000;

  const waitForSidecarRunning = useCallback(
    async (timeoutMs = SIDECAR_ATTACH_TIMEOUT_MS, pollIntervalMs = 500): Promise<boolean> => {
      const startedAt = Date.now();
      while (Date.now() - startedAt < timeoutMs) {
        const status = await getSidecarStatus();
        setSidecarStatus(status);
        if (status === "running") {
          return true;
        }
        await new Promise((resolve) => globalThis.setTimeout(resolve, pollIntervalMs));
      }
      return false;
    },
    []
  );

  const waitForEngineReady = useCallback(
    async (timeoutMs = 12_000, pollIntervalMs = 200): Promise<boolean> => {
      const startedAt = Date.now();
      while (Date.now() - startedAt < timeoutMs) {
        if (engineReadyRef.current) {
          return true;
        }
        const status = await getSidecarStatus();
        setSidecarStatus(status);
        if (status !== "running" && status !== "starting") {
          return false;
        }
        await new Promise((resolve) => globalThis.setTimeout(resolve, pollIntervalMs));
      }
      return engineReadyRef.current;
    },
    []
  );

  const startSidecarWithTimeout = useCallback(async () => {
    setSidecarStatus("starting");
    setEngineReady(false);
    // Let backend startup logic control timeout/retries so frontend doesn't
    // abort early while engine reports healthy-but-not-ready startup phases.
    await startSidecar();

    const isRunning = await waitForSidecarRunning(3_000, 300);
    if (!isRunning) {
      throw new Error("Tandem Engine start returned, but engine is not in running state.");
    }
  }, [waitForSidecarRunning]);

  // Handle creating a new frictionless plan
  const handleNewPlan = async () => {
    try {
      console.log("[Chat] Starting new frictionless plan session...");
      const result = await startPlanSession();

      // Force refresh of plans to pick up the new file
      await refreshPlans();

      // Update session ID to the new one
      if (onSessionCreated) {
        onSessionCreated(result.session.id);
      }

      setCurrentSessionId(result.session.id);

      // Manually find and set the active plan since we know the path
      // This might be redundant if we just wait for the refresh, but good for UX
      // const newPlan = plans.find(p => p.fullPath === result.plan_path);
      // if (newPlan) setActivePlan(newPlan);

      // Enable plan view
      setShowPlanView(true);
    } catch (e) {
      console.error("Failed to start plan session:", e);
      setError("Failed to create new plan");
    }
  };

  // Enable default tool categories on mount
  useEffect(() => {
    setEnabledToolCategories(new Set(["files", "search", "terminal"]));
  }, []);

  // Clear any queued prompts when switching sessions.
  useEffect(() => {
    setPendingQuestionRequests([]);
    pendingQuestionToolCallIdsRef.current = new Set();
    pendingQuestionToolMessageIdsRef.current = new Set();
    seenEventIdsRef.current = new Set();
  }, [currentSessionId]);

  useEffect(() => {
    if (!currentSessionId) {
      setQueuedMessages([]);
      return;
    }
    queueList(currentSessionId)
      .then(setQueuedMessages)
      .catch((e) => {
        console.warn("Failed to load queue:", e);
      });
  }, [currentSessionId]);

  // Fetch already-pending question requests so the current session
  // shows the prompt even if the SSE event was missed.
  useEffect(() => {
    if (sidecarStatus !== "running" || !currentSessionId) return;

    let cancelled = false;
    (async () => {
      try {
        const all = await listQuestions();
        if (cancelled) return;

        const relevant = all.filter(
          (r) => r.session_id === currentSessionId && r.questions && r.questions.length > 0
        );

        // Track which tool calls in history are still answerable.
        pendingQuestionToolCallIdsRef.current = new Set(
          relevant.map((r) => r.tool_call_id).filter(Boolean) as string[]
        );
        pendingQuestionToolMessageIdsRef.current = new Set(
          relevant.map((r) => r.tool_message_id).filter(Boolean) as string[]
        );

        setPendingQuestionRequests((prev) => {
          if (relevant.length === 0) return prev;

          const existing = new Set(prev.map((r) => r.request_id));
          const next = [...prev];
          for (const req of relevant) {
            if (
              handledQuestionRequestIdsRef.current.has(req.request_id) ||
              existing.has(req.request_id)
            ) {
              continue;
            }
            next.push(req);
          }
          return next;
        });
      } catch (e) {
        // Non-fatal: the SSE event will still surface prompts.
        console.warn("Failed to list pending questions:", e);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [sidecarStatus, currentSessionId]);

  const handleOpenQuestionToolCall = useCallback(
    async ({ messageId, toolCallId }: { messageId: string; toolCallId: string }) => {
      if (!currentSessionId) return;

      try {
        const all = await listQuestions();
        const match = all.find(
          (r) =>
            r.session_id === currentSessionId &&
            (r.tool_call_id === toolCallId || r.tool_message_id === messageId)
        );

        if (!match) {
          setError("No pending question found for that tool call. It may already be answered.");
          // Hide the action going forward for this tool call/message.
          pendingQuestionToolCallIdsRef.current.delete(toolCallId);
          pendingQuestionToolMessageIdsRef.current.delete(messageId);
          return;
        }

        setPendingQuestionRequests((prev) => {
          if (
            handledQuestionRequestIdsRef.current.has(match.request_id) ||
            prev.some((r) => r.request_id === match.request_id)
          ) {
            return prev;
          }

          if (match.tool_call_id) pendingQuestionToolCallIdsRef.current.add(match.tool_call_id);
          if (match.tool_message_id)
            pendingQuestionToolMessageIdsRef.current.add(match.tool_message_id);
          return [match, ...prev];
        });
      } catch (e) {
        console.error("Failed to locate pending question:", e);
        setError(`Failed to locate pending question: ${e}`);
      }
    },
    [currentSessionId]
  );

  const isQuestionToolCallPending = useCallback(
    ({ messageId, toolCallId }: { messageId: string; toolCallId: string }) => {
      return (
        pendingQuestionToolCallIdsRef.current.has(toolCallId) ||
        pendingQuestionToolMessageIdsRef.current.has(messageId)
      );
    },
    []
  );

  // Handle execute pending tasks from sidebar
  useEffect(() => {
    if (
      executePendingTasksTrigger &&
      executePendingTasksTrigger > 0 &&
      currentSessionId &&
      !isGenerating &&
      pendingTasks &&
      pendingTasks.length > 0
    ) {
      // Build a message with actual task content - use action-oriented prompts
      const taskList = pendingTasks.map((t, i) => `${i + 1}. ${t.content}`).join("\n");

      const message = `Please implement the following tasks from our plan:

${taskList}

Start with task #1 and continue through each one. IMPORTANT: After verifying each task is done, you MUST use the 'todowrite' tool to set its status to "completed" in the task list. Call 'todowrite' with status="completed" for each task ID.`;

      // Small delay to ensure state is ready
      setTimeout(() => {
        handleSend(message);
      }, 100);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [executePendingTasksTrigger]);

  // Auto-scroll to bottom smoothly (only when new messages arrive or generation ends)
  const previousMessageCountRef = useRef(0);
  const messagesContainerRef = useRef<HTMLDivElement>(null);
  const [isAtBottom, setIsAtBottom] = useState(true);
  const [followLatest, setFollowLatest] = useState(true);
  const showJumpToLatest = messages.length > 0 && !isAtBottom;

  const rowVirtualizer = useVirtualizer({
    count: messages.length,
    getScrollElement: () => messagesContainerRef.current,
    estimateSize: () => 220,
    overscan: 10,
    getItemKey: (index) => messages[index]?.id ?? index,
  });

  // Helper function to scroll to bottom
  const scrollToBottom = useCallback(
    (smooth = false) => {
      // With virtualization, we scroll to the last index
      if (messages.length > 0) {
        rowVirtualizer.scrollToIndex(messages.length - 1, {
          align: "end",
          behavior: smooth ? "smooth" : "auto",
        });
      }
    },
    [messages.length, rowVirtualizer]
  );

  // Track whether the user is near the bottom and whether auto-follow should remain enabled.
  useEffect(() => {
    const container = messagesContainerRef.current;
    if (!container) return;

    const BOTTOM_THRESHOLD_PX = 96;

    const syncScrollState = () => {
      const distanceFromBottom =
        container.scrollHeight - (container.scrollTop + container.clientHeight);
      const atBottom = distanceFromBottom <= BOTTOM_THRESHOLD_PX;
      setIsAtBottom(atBottom);
      if (atBottom) {
        setFollowLatest(true);
      } else {
        setFollowLatest(false);
      }
    };

    syncScrollState();
    container.addEventListener("scroll", syncScrollState, { passive: true });
    return () => container.removeEventListener("scroll", syncScrollState);
  }, [messages.length]);

  // Scroll when message count increases (new message added) and follow mode is enabled.
  useEffect(() => {
    const currentCount = messages.length;

    if (followLatest && currentCount > previousMessageCountRef.current) {
      scrollToBottom(false); // Instant scroll for new messages
    }

    previousMessageCountRef.current = currentCount;
  }, [followLatest, messages.length, scrollToBottom]);

  // Also scroll when generation stops (final message complete), if follow mode is enabled.
  useEffect(() => {
    if (!isGenerating && followLatest && messages.length > 0) {
      // Small delay to ensure content is rendered, then scroll
      setTimeout(() => scrollToBottom(false), 100);
    }
  }, [followLatest, isGenerating, messages.length, scrollToBottom]);

  // Scroll during active generation (streaming content) only when follow mode is enabled.
  useEffect(() => {
    if (followLatest && isGenerating && messages.length > 0) {
      scrollToBottom(false); // Keep scrolling as content streams
    }
  }, [followLatest, messages, isGenerating, scrollToBottom]);

  // Store callback in ref to avoid dependency issues
  const onSidecarConnectedRef = useRef(onSidecarConnected);
  useEffect(() => {
    onSidecarConnectedRef.current = onSidecarConnected;
  }, [onSidecarConnected]);

  // Auto-connect sidecar on mount
  useEffect(() => {
    const autoConnect = async () => {
      try {
        const status = await getSidecarStatus();
        setSidecarStatus(status);

        // Auto-start if not already running.
        // If another client already started it, wait for it to transition instead of
        // issuing duplicate start calls that can pile up behind lifecycle locks.
        if (status === "starting") {
          setIsConnecting(true);
          setEngineReady(false);
          const attached = await waitForSidecarRunning(SIDECAR_ATTACH_TIMEOUT_MS, 500);
          if (!attached) {
            try {
              await startSidecarWithTimeout();
            } catch (e) {
              setSidecarStatus("failed");
              setError(
                e instanceof Error
                  ? e.message
                  : "Tandem Engine is still starting. Please click Connect to retry."
              );
              setIsConnecting(false);
              return;
            }
          }
          const ready = await waitForEngineReady();
          if (!ready) {
            throw new Error("Tandem Engine started but did not signal readiness on event stream.");
          }
          setSidecarStatus("running");
          setStreamHealth("healthy");
          onSidecarConnectedRef.current?.();
          setIsConnecting(false);
          return;
        }

        if (status !== "running") {
          setIsConnecting(true);
          try {
            await startSidecarWithTimeout();
            const ready = await waitForEngineReady();
            if (!ready) {
              throw new Error(
                "Tandem Engine started but did not signal readiness on event stream."
              );
            }
            setSidecarStatus("running");
            setStreamHealth("healthy");
            // Notify parent that sidecar is connected
            onSidecarConnectedRef.current?.();
          } catch (e) {
            console.error("Failed to auto-start sidecar:", e);
            setSidecarStatus("failed");
            setError(
              e instanceof Error ? e.message : "Failed to connect to Tandem Engine. Retry below."
            );
            // Don't set error - user can still manually connect
          } finally {
            setIsConnecting(false);
          }
        } else {
          setEngineReady(true);
          setStreamHealth("healthy");
          // Already running, notify parent
          onSidecarConnectedRef.current?.();
        }
      } catch (e) {
        console.error("Failed to get sidecar status:", e);
        setSidecarStatus("failed");
      }
    };
    autoConnect();
  }, [startSidecarWithTimeout, waitForEngineReady, waitForSidecarRunning]); // Only run on mount behaviorally

  // Sync internal session ID with prop (only when prop truly changes)
  useEffect(() => {
    const nextSessionId = propSessionId || null;
    const prevSessionId = currentSessionIdRef.current;

    // No change → do nothing (prevents churn from unrelated state updates)
    if (nextSessionId === prevSessionId) return;

    // If we are currently generating a response for a session we just created,
    // don't let a null/undefined prop wipe it out while the parent is still updating.
    if (!nextSessionId && isGenerating && prevSessionId) {
      console.log("[Chat] Ignoring null propSessionId while generating:", prevSessionId);
      return;
    }

    console.log("[Chat] Session ID changed from", prevSessionId, "to", nextSessionId);

    setCurrentSessionId(nextSessionId);
    currentSessionIdRef.current = nextSessionId; // Update ref synchronously

    // Reset streaming state for the new session
    setIsGenerating(false);
    currentAssistantMessageRef.current = "";
    currentAssistantMessageIdRef.current = null;
    handledPermissionIdsRef.current = new Set();
    setPendingPermissions([]);
    setPendingQuestionRequests([]);

    if (generationTimeoutRef.current) {
      clearTimeout(generationTimeoutRef.current);
      generationTimeoutRef.current = null;
    }
  }, [propSessionId, isGenerating]);

  // Load session history when prop session changes (avoid transient mismatches)
  useEffect(() => {
    const nextSessionId = propSessionId || null;
    const prevPropSessionId = prevPropSessionIdRef.current;

    if (nextSessionId === prevPropSessionId) return;
    prevPropSessionIdRef.current = nextSessionId;

    // CRITICAL: If we are already active in this session (e.g. we just created it),
    // don't wipe messages or reload history, as it will interrupt the stream.
    if (nextSessionId && nextSessionId === currentSessionId) {
      console.log("[Chat] Prop sessionId matches current, skipping history reload");
      return;
    }

    if (isGenerating) {
      deferredSessionLoadRef.current = nextSessionId;
      console.log("[Chat] Deferring history load while generating:", nextSessionId);
      return;
    }

    if (nextSessionId) {
      console.log("[Chat] Loading session history for:", nextSessionId);
      setMessages([]);
      currentAssistantMessageRef.current = "";
      loadSessionHistory(nextSessionId);
    } else {
      // Switching to new chat
      console.log("[Chat] Clearing messages for new chat");
      setMessages([]);
      currentAssistantMessageRef.current = "";
      setAllowAllTools(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [propSessionId, isGenerating]);

  // Check if folder is a Git repository
  useEffect(() => {
    const checkGitRepo = async () => {
      if (workspacePath) {
        try {
          const isGit = await isGitRepo(workspacePath);
          setIsGitRepository(isGit);
          console.log(
            "[Chat] Workspace is Git repo:",
            isGit,
            "- Undo button will be",
            isGit ? "visible" : "hidden"
          );
        } catch (e) {
          console.error("[Chat] Failed to check Git status:", e);
          setIsGitRepository(false);
        }
      } else {
        setIsGitRepository(false);
      }
    };
    checkGitRepo();
  }, [workspacePath]);

  const loadSessionHistory = useCallback(
    async (sessionId: string) => {
      setIsLoadingHistory(true);
      try {
        // On startup, `propSessionId` can be set before the sidecar finishes auto-starting.
        // If we just "skip" here, the session can appear selected but never load until the user
        // changes sessions/projects (because the prop doesn't change). Defer instead.
        if (sidecarStatus !== "running") {
          console.log(
            "[LoadHistory] Sidecar not running, deferring history load for:",
            sessionId,
            "(status:",
            sidecarStatus,
            ")"
          );
          deferredSessionLoadRef.current = sessionId;
          setIsLoadingHistory(false);
          return;
        }

        const [sessionMessages, persistedToolRows] = await Promise.all([
          getSessionMessages(sessionId),
          listToolExecutions(sessionId, 500).catch(() => [] as ToolExecutionRow[]),
        ]);

        console.log("[LoadHistory] Received", sessionMessages.length, "messages from OpenCode");

        // Log FULL message structure to see all available fields
        if (sessionMessages.length > 0) {
          console.log(
            "[LoadHistory] FULL message structure sample:",
            JSON.stringify(sessionMessages[0], null, 2)
          );
        }

        console.log(
          "[LoadHistory] Messages with flags:",
          sessionMessages.map((m) => ({
            id: m.info.id,
            role: m.info.role,
            reverted: m.info.reverted,
            deleted: m.info.deleted,
            hasRevertedFlag: "reverted" in m.info,
            hasDeletedFlag: "deleted" in m.info,
          }))
        );

        const isMemoryRow = (row: ToolExecutionRow) =>
          row.tool === "memory.lookup" || row.tool === "memory.store";
        const persistedMemoryRows = persistedToolRows.filter(isMemoryRow);
        const persistedNonMemoryRows = persistedToolRows.filter((row) => !isMemoryRow(row));

        const toolRowsByMessageId = new Map<string, ToolExecutionRow[]>();
        for (const row of persistedNonMemoryRows) {
          if (!row.message_id) continue;
          const arr = toolRowsByMessageId.get(row.message_id) || [];
          arr.push(row);
          toolRowsByMessageId.set(row.message_id, arr);
        }

        // Convert session messages to our format
        const convertedMessages: MessageProps[] = [];
        let pendingRowsForAssistant: ToolExecutionRow[] = [];

        for (const msg of sessionMessages) {
          // Skip reverted or deleted messages
          if (msg.info.reverted || msg.info.deleted) {
            console.log(`[LoadHistory] Skipping reverted/deleted message: ${msg.info.id}`);
            continue;
          }

          const role = msg.info.role as "user" | "assistant" | "system";

          // Extract text content from parts
          let content = "";
          const toolCalls: MessageProps["toolCalls"] = [];
          const attachments: { name: string; type: string }[] = [];

          for (const part of msg.parts) {
            const partObj = part as Record<string, unknown>;
            if ((partObj.type === "text" || partObj.type === "reasoning") && partObj.text) {
              content += partObj.text as string;
            } else if (partObj.type === "file") {
              // Handle file attachments
              const filename = (partObj.filename as string) || "file";
              const mime = (partObj.mime as string) || "application/octet-stream";
              const url = (partObj.url as string) || "";
              const isImage =
                mime.startsWith("image/") || /\.(jpg|jpeg|png|gif|webp|svg)$/i.test(filename);

              attachments.push({
                name: filename,
                type: isImage ? "image" : "file",
                preview: isImage && url ? url : undefined,
              } as any);

              // Only add a text placeholder if there's no other text or if we want to explicitly record it
              if (role === "user") {
                content += `\n[📎 Attached file: ${filename}]\n`;
              }
            } else if (partObj.type === "tool" || partObj.type === "tool-invocation") {
              const toolName = (partObj.tool || "unknown") as string;
              // Technical tools list - we keep these visible in history now per user request
              // to see what happened during the session.
              /* 
              const technicalTools = [
                "todowrite",
                "edit",
                "write",
                "patch",
                "ls",
                "read",
                "list",
                "search",
                "bash",
                "run_command",
                "delete_file",
              ];

              // Skip finished technical tools in history to keep chat clean
              const state = partObj.state as Record<string, unknown> | undefined;
              const explicitStatus = typeof state?.status === "string" ? state.status : undefined;
              const hasOutput =
                state?.output !== undefined ||
                partObj.result !== undefined ||
                partObj.output !== undefined;
              const hasError = state?.error !== undefined || partObj.error !== undefined;
              const status =
                explicitStatus === "completed"
                  ? "completed"
                  : explicitStatus === "failed"
                    ? "failed"
                    : hasError
                      ? "failed"
                      : hasOutput
                        ? "completed"
                        : "pending";

              if (technicalTools.includes(toolName) && status === "completed") {
                continue;
              }
              */

              const state = partObj.state as Record<string, unknown> | undefined;
              const explicit = typeof state?.status === "string" ? state.status : undefined;
              const status: "pending" | "running" | "completed" | "failed" =
                explicit === "completed"
                  ? "completed"
                  : explicit === "failed" ||
                      explicit === "error" ||
                      explicit === "cancelled" ||
                      explicit === "canceled" ||
                      explicit === "denied" ||
                      explicit === "timeout"
                    ? "failed"
                    : explicit === "running" || explicit === "in_progress"
                      ? "running"
                      : "pending";

              toolCalls.push({
                id: (partObj.id || partObj.callID || "") as string,
                tool: toolName,
                args: (state?.input || partObj.args || {}) as Record<string, unknown>,
                result: state?.output ? String(state.output) : undefined,
                status,
                isTechnical: false, // technicalTools.includes(toolName), // Always show in history
              });
            }
          }

          if (role === "user") {
            pendingRowsForAssistant = toolRowsByMessageId.get(msg.info.id) || [];
          }
          if (role === "assistant" && pendingRowsForAssistant.length > 0) {
            const rehydrated = pendingRowsForAssistant.map((row) => {
              const rowStatus = (row.status || "").toLowerCase();
              const mappedStatus: "pending" | "running" | "completed" | "failed" =
                rowStatus === "completed"
                  ? "completed"
                  : rowStatus === "failed" || rowStatus === "error" || rowStatus === "cancelled"
                    ? "failed"
                    : rowStatus === "running"
                      ? "running"
                      : "pending";
              return {
                id: row.part_id || row.id,
                tool: row.tool,
                args: (row.args as Record<string, unknown>) || {},
                result:
                  row.error || row.result === undefined ? undefined : JSON.stringify(row.result),
                status: mappedStatus,
                isTechnical: false,
              };
            });

            for (const call of rehydrated) {
              if (!toolCalls.some((existing) => existing.id === call.id)) {
                toolCalls.push(call);
              }
            }
            pendingRowsForAssistant = [];
          }

          // Only add messages that have actual text content or are user messages
          // Skip assistant messages that only have tool calls (internal OpenCode operations)
          if (content.trim() || role === "user" || (toolCalls.length > 0 && role === "assistant")) {
            convertedMessages.push({
              id: msg.info.id,
              role,
              content,
              timestamp: new Date(msg.info.time.created),
              toolCalls: toolCalls.length > 0 ? toolCalls : undefined,
              attachments: attachments.length > 0 ? (attachments as any) : undefined,
            });
          } else {
            console.log(`[LoadHistory] Skipping empty/technical assistant message: ${msg.info.id}`);
          }
        }

        if (pendingRowsForAssistant.length > 0 && convertedMessages.length > 0) {
          for (let i = convertedMessages.length - 1; i >= 0; i -= 1) {
            const msg = convertedMessages[i];
            if (msg.role !== "assistant") continue;
            const existing = msg.toolCalls || [];
            const merged = [...existing];
            for (const row of pendingRowsForAssistant) {
              const id = row.part_id || row.id;
              if (merged.some((call) => call.id === id)) continue;
              merged.push({
                id,
                tool: row.tool,
                args: (row.args as Record<string, unknown>) || {},
                result:
                  row.error || row.result === undefined ? undefined : JSON.stringify(row.result),
                status:
                  row.status === "completed"
                    ? "completed"
                    : row.status === "running"
                      ? "running"
                      : row.status === "failed"
                        ? "failed"
                        : "pending",
                isTechnical: false,
              });
            }
            convertedMessages[i] = { ...msg, toolCalls: merged };
            break;
          }
        }

        const assistantIndices = convertedMessages
          .map((message, index) => ({ message, index }))
          .filter(({ message }) => message.role === "assistant")
          .map(({ index }) => index);
        const firstAssistantIdx = assistantIndices[0];
        const lastAssistantIdx = assistantIndices[assistantIndices.length - 1];
        const toNumber = (value: unknown, fallback = 0): number => {
          const numeric = Number(value);
          return Number.isFinite(numeric) ? numeric : fallback;
        };

        const lookupRowsAsc = persistedMemoryRows
          .filter((row) => row.tool === "memory.lookup")
          .slice()
          .sort(
            (a, b) =>
              (a.ended_at_ms ?? a.started_at_ms ?? 0) - (b.ended_at_ms ?? b.started_at_ms ?? 0)
          );
        for (const row of lookupRowsAsc) {
          if (assistantIndices.length === 0) break;
          const args =
            row.args && typeof row.args === "object" && !Array.isArray(row.args)
              ? (row.args as Record<string, unknown>)
              : {};
          const rowTs = row.ended_at_ms ?? row.started_at_ms ?? 0;
          const mappedIdx =
            assistantIndices.find((idx) => convertedMessages[idx].timestamp.getTime() >= rowTs) ??
            lastAssistantIdx;
          if (mappedIdx == null) continue;

          const target = convertedMessages[mappedIdx];
          const prev = target.memoryRetrieval;
          convertedMessages[mappedIdx] = {
            ...target,
            memoryRetrieval: {
              status:
                typeof args.status === "string"
                  ? (args.status as
                      | "not_attempted"
                      | "attempted_no_hits"
                      | "retrieved_used"
                      | "degraded_disabled"
                      | "error_fallback")
                  : prev?.status,
              used: Boolean(args.used ?? prev?.used ?? false),
              chunks_total: toNumber(args.chunks_total, prev?.chunks_total ?? 0),
              latency_ms: toNumber(args.latency_ms, prev?.latency_ms ?? 0),
              embedding_status:
                typeof args.embedding_status === "string"
                  ? args.embedding_status
                  : prev?.embedding_status,
              embedding_reason:
                typeof args.embedding_reason === "string"
                  ? args.embedding_reason
                  : prev?.embedding_reason,
              session_chunks_stored: prev?.session_chunks_stored ?? 0,
              project_chunks_stored: prev?.project_chunks_stored ?? 0,
            },
          };
        }

        const storageRowsAsc = persistedMemoryRows
          .filter((row) => row.tool === "memory.store")
          .slice()
          .sort(
            (a, b) =>
              (a.ended_at_ms ?? a.started_at_ms ?? 0) - (b.ended_at_ms ?? b.started_at_ms ?? 0)
          );
        for (const row of storageRowsAsc) {
          if (assistantIndices.length === 0) break;
          const args =
            row.args && typeof row.args === "object" && !Array.isArray(row.args)
              ? (row.args as Record<string, unknown>)
              : {};
          const role = typeof args.role === "string" ? args.role : "unknown";
          if (role !== "assistant") continue;

          const rowTs = row.ended_at_ms ?? row.started_at_ms ?? 0;
          let mappedIdx =
            row.message_id != null
              ? convertedMessages.findIndex(
                  (message) => message.role === "assistant" && message.id === row.message_id
                )
              : -1;
          if (mappedIdx < 0) {
            mappedIdx =
              assistantIndices
                .slice()
                .reverse()
                .find((idx) => convertedMessages[idx].timestamp.getTime() <= rowTs) ?? -1;
          }
          if (mappedIdx < 0) {
            mappedIdx = firstAssistantIdx ?? -1;
          }
          if (mappedIdx < 0) continue;

          const target = convertedMessages[mappedIdx];
          const prev = target.memoryRetrieval;
          convertedMessages[mappedIdx] = {
            ...target,
            memoryRetrieval: {
              status: prev?.status,
              used: prev?.used ?? false,
              chunks_total: prev?.chunks_total ?? 0,
              latency_ms: prev?.latency_ms ?? 0,
              embedding_status: prev?.embedding_status,
              embedding_reason: prev?.embedding_reason,
              session_chunks_stored: toNumber(
                args.session_chunks_stored,
                prev?.session_chunks_stored ?? 0
              ),
              project_chunks_stored: toNumber(
                args.project_chunks_stored,
                prev?.project_chunks_stored ?? 0
              ),
            },
          };
        }

        console.log("[LoadHistory] Converted to", convertedMessages.length, "UI messages");
        setMessages(convertedMessages);
        // Force re-render to ensure messages display correctly
        setTimeout(() => forceUpdate({}), 50);
      } catch (e) {
        console.error("Failed to load session history:", e);
        setError("Failed to load chat history");
      } finally {
        setIsLoadingHistory(false);
      }
    },
    [setError, setIsLoadingHistory, setMessages, sidecarStatus]
  );

  // If we deferred a history load while waiting for the sidecar to come up, retry once it is running.
  useEffect(() => {
    if (sidecarStatus !== "running") return;
    if (isGenerating) return;

    const deferredId = deferredSessionLoadRef.current;
    if (!deferredId) return;

    // Prefer the latest prop if it exists, but fall back to what we deferred.
    const effectiveId = propSessionId || deferredId;
    deferredSessionLoadRef.current = null;

    console.log("[Chat] Sidecar running - loading deferred session history:", effectiveId);
    setMessages([]);
    currentAssistantMessageRef.current = "";
    loadSessionHistory(effectiveId);
  }, [sidecarStatus, isGenerating, propSessionId, loadSessionHistory, setMessages]);

  // Helper to determine activity type from tool name - DISABLED
  // const getActivityType = (tool: string): ActivityItem["type"] => {
  //   const toolLower = tool.toLowerCase();
  //   if (toolLower.includes("read") || toolLower.includes("view")) return "file_read";
  //   if (toolLower.includes("write") || toolLower.includes("edit") || toolLower.includes("create"))
  //     return "file_write";
  //   if (toolLower.includes("search") || toolLower.includes("grep") || toolLower.includes("find"))
  //     return "search";
  //   if (
  //     toolLower.includes("bash") ||
  //     toolLower.includes("shell") ||
  //     toolLower.includes("command") ||
  //     toolLower.includes("exec")
  //   )
  //     return "command";
  //   if (toolLower.includes("browse") || toolLower.includes("web") || toolLower.includes("fetch"))
  //     return "browse";
  //   return "tool";
  // };

  // Helper to get a friendly title for a tool - DISABLED
  // const getActivityTitle = (tool: string, args: Record<string, unknown>): string => {
  //   const toolLower = tool.toLowerCase();

  //   // Try to extract a meaningful path or query
  //   const path = args.path || args.file || args.filename;
  //   const query = args.query || args.pattern || args.search;
  //   const command = args.command || args.cmd;

  //   if (path && typeof path === "string") {
  //     // Shorten long paths
  //     const shortPath = path.length > 40 ? "..." + path.slice(-37) : path;
  //     if (toolLower.includes("read")) return `Reading ${shortPath}`;
  //     if (toolLower.includes("write") || toolLower.includes("edit")) return `Editing ${shortPath}`;
  //     if (toolLower.includes("create")) return `Creating ${shortPath}`;
  //     if (toolLower.includes("delete")) return `Deleting ${shortPath}`;
  //     if (toolLower.includes("list")) return `Listing ${shortPath}`;
  //     return `${tool} → ${shortPath}`;
  //   }

  //   if (query && typeof query === "string") {
  //     const shortQuery = query.length > 30 ? query.slice(0, 27) + "..." : query;
  //     return `Searching: "${shortQuery}"`;
  //   }

  //   if (command && typeof command === "string") {
  //     const shortCmd = command.length > 30 ? command.slice(0, 27) + "..." : command;
  //     return `Running: ${shortCmd}`;
  //   }

  //   // Fallback to tool name
  //   return tool.replace(/_/g, " ").replace(/\b\w/g, (l) => l.toUpperCase());
  // };

  // Update ref when session changes (but this doesn't cause handleStreamEvent to recreate)
  useEffect(() => {
    currentSessionIdRef.current = currentSessionId;
  }, [currentSessionId]);

  useEffect(() => {
    messagesRef.current = messages;
  }, [messages]);

  const applyAssistantContent = useCallback((contentToApply: string) => {
    setMessages((prev) => {
      const targetId = currentAssistantMessageIdRef.current;

      if (targetId) {
        const idx = prev.findIndex((m) => m.id === targetId);
        if (idx >= 0) {
          const updated = [...prev];
          updated[idx] = { ...updated[idx], content: contentToApply };
          return updated;
        }
      }

      const lastMessage = prev[prev.length - 1];
      if (lastMessage && lastMessage.role === "assistant") {
        return [
          ...prev.slice(0, -1),
          {
            ...lastMessage,
            id: targetId || lastMessage.id,
            content: contentToApply,
          },
        ];
      }

      return [
        ...prev,
        {
          id: targetId || crypto.randomUUID(),
          role: "assistant",
          content: contentToApply,
          timestamp: new Date(),
        },
      ];
    });
  }, []);

  const flushAssistantContent = useCallback(() => {
    assistantFlushFrameRef.current = null;
    const contentToApply = currentAssistantMessageRef.current;
    startTransition(() => {
      applyAssistantContent(contentToApply);
    });
  }, [applyAssistantContent]);

  const scheduleAssistantFlush = useCallback(() => {
    if (assistantFlushFrameRef.current !== null) return;
    assistantFlushFrameRef.current = globalThis.requestAnimationFrame(flushAssistantContent);
  }, [flushAssistantContent]);

  useEffect(() => {
    return () => {
      if (assistantFlushFrameRef.current !== null) {
        globalThis.cancelAnimationFrame(assistantFlushFrameRef.current);
      }
    };
  }, []);

  useEffect(() => {
    return () => {
      if (statusBannerTimerRef.current) {
        globalThis.clearTimeout(statusBannerTimerRef.current);
      }
    };
  }, []);

  const showStatusBanner = useCallback((message: string) => {
    setStatusBanner(message);
    if (statusBannerTimerRef.current) {
      globalThis.clearTimeout(statusBannerTimerRef.current);
    }
    statusBannerTimerRef.current = globalThis.setTimeout(() => {
      setStatusBanner(null);
      statusBannerTimerRef.current = null;
    }, 2600);
  }, []);

  const finalizePendingToolCalls = useCallback((reason: string) => {
    setMessages((prev) =>
      prev.map((msg) => {
        if (
          msg.role !== "assistant" ||
          !Array.isArray(msg.toolCalls) ||
          msg.toolCalls.length === 0
        ) {
          return msg;
        }
        const nextToolCalls = msg.toolCalls.map((tc) => {
          if (tc.status !== "pending") {
            return tc;
          }
          return {
            ...tc,
            status: "failed" as const,
            result: tc.result || reason,
          };
        });
        return { ...msg, toolCalls: nextToolCalls };
      })
    );
  }, []);

  const handleApprovePermission = async (id: string, _remember?: "once" | "session" | "always") => {
    try {
      const req = pendingPermissions.find((p) => p.id === id);
      if (!req?.session_id) return;
      await approveTool(req.session_id, id, {
        tool: req?.tool,
        args: req?.args,
        messageId: req?.messageId,
      });

      // Update tool call status
      setMessages((prev) => {
        return prev.map((msg) => {
          if (msg.role === "assistant" && msg.toolCalls) {
            return {
              ...msg,
              toolCalls: msg.toolCalls.map((tc) =>
                tc.id === id ? { ...tc, status: "completed" as const } : tc
              ),
            };
          }
          return msg;
        });
      });

      // Remove from pending
      setPendingPermissions((prev) => prev.filter((p) => p.id !== id));
    } catch (e) {
      console.error("Failed to approve tool:", e);
      setError(`Failed to approve action: ${e}`);
    }
  };

  const handleStreamEvent = useCallback(
    (event: StreamEvent) => {
      const eventSessionId = (event as { session_id?: string }).session_id;
      if (!currentSessionIdRef.current && eventSessionId) {
        currentSessionIdRef.current = eventSessionId;
      }
      if (
        eventSessionId &&
        currentSessionIdRef.current &&
        eventSessionId !== currentSessionIdRef.current
      ) {
        return;
      }

      lastEventAtRef.current = Date.now();
      if (generationTimeoutRef.current) {
        clearTimeout(generationTimeoutRef.current);
      }
      generationTimeoutRef.current = setTimeout(() => {
        if (isGenerating) {
          setError("Response timed out. Try again or stop and restart the chat.");
          setIsGenerating(false);
          currentAssistantMessageRef.current = "";
        }
      }, 60000);

      switch (event.type) {
        case "content": {
          // Update the message ID ref if we have one from OpenCode
          if (event.message_id && !currentAssistantMessageIdRef.current) {
            currentAssistantMessageIdRef.current = event.message_id;
          }

          const current = currentAssistantMessageRef.current;
          const incoming = event.content || "";
          const delta = event.delta || "";

          if (event.delta) {
            // Prefer full snapshots when they are clearly cumulative.
            if (incoming && (!current || incoming.startsWith(current))) {
              currentAssistantMessageRef.current = incoming;
            } else {
              currentAssistantMessageRef.current = current + delta;
            }
          } else if (!current) {
            currentAssistantMessageRef.current = incoming;
          } else if (incoming.startsWith(current)) {
            // No explicit delta: some providers still send cumulative content.
            currentAssistantMessageRef.current = incoming;
          } else if (!current.startsWith(incoming)) {
            // No delta + non-cumulative chunk: treat as append to avoid losing tokens.
            currentAssistantMessageRef.current = current + incoming;
          }
          scheduleAssistantFlush();
          break;
        }

        case "tool_start": {
          // Technical tools are handled as transient background tasks
          // UPDATE: User wants to see all tools, so we disable technical hiding
          /*
          const technicalTools = [
            "todowrite",
            "edit",
            "write",
            "patch",
            "ls",
            "read",
            "list",
            "search",
            "bash",
            "run_command",
            "delete_file",
          ];
          const isTechnical = technicalTools.includes(event.tool);
          */
          const isTechnical = false;

          // Add tool call to the message
          setMessages((prev) => {
            const lastMessage = prev[prev.length - 1];
            if (lastMessage && lastMessage.role === "assistant") {
              const toolCalls = lastMessage.toolCalls || [];
              // Check if tool already exists (update) or is new (add)
              const existingIdx = toolCalls.findIndex((tc) => tc.id === event.part_id);
              if (existingIdx >= 0) {
                // Update existing
                const newToolCalls = [...toolCalls];
                newToolCalls[existingIdx] = {
                  ...newToolCalls[existingIdx],
                  tool: event.tool,
                  args: event.args as Record<string, unknown>,
                  status: "pending" as const,
                };
                return [...prev.slice(0, -1), { ...lastMessage, toolCalls: newToolCalls }];
              } else {
                // Add new
                return [
                  ...prev.slice(0, -1),
                  {
                    ...lastMessage,
                    toolCalls: [
                      ...toolCalls,
                      {
                        id: event.part_id,
                        tool: event.tool,
                        args: event.args as Record<string, unknown>,
                        status: "pending" as const,
                        isTechnical, // Mark as technical for transient display
                      },
                    ],
                  },
                ];
              }
            }
            return prev;
          });

          break;
        }

        case "tool_end": {
          // Technical tools are handled as transient background tasks
          /*
          const technicalTools = [
            "todowrite",
            "edit",
            "write",
            "patch",
            "ls",
            "read",
            "list",
            "search",
            "bash",
            "run_command",
            "delete_file",
          ];
          const isTechnical = technicalTools.includes(event.tool);
          */
          const isTechnical = false;

          // Update tool call with result
          setMessages((prev) => {
            const lastMessage = prev[prev.length - 1];
            if (lastMessage && lastMessage.role === "assistant" && lastMessage.toolCalls) {
              let matchedToolCallId = event.part_id;
              const hasExactMatch = lastMessage.toolCalls.some((tc) => tc.id === event.part_id);
              if (!hasExactMatch) {
                const fallbackMatch = [...lastMessage.toolCalls]
                  .reverse()
                  .find(
                    (tc) =>
                      tc.status === "pending" &&
                      tc.tool.toLowerCase() === (event.tool || "").toLowerCase()
                  );
                if (fallbackMatch) {
                  matchedToolCallId = fallbackMatch.id;
                }
              }

              // If it's a technical tool, we might want to remove it entirely once done
              if (isTechnical && !event.error) {
                const newToolCalls = lastMessage.toolCalls.filter(
                  (tc) => tc.id !== matchedToolCallId
                );
                return [...prev.slice(0, -1), { ...lastMessage, toolCalls: newToolCalls }];
              }

              const newToolCalls = lastMessage.toolCalls.map((tc) =>
                tc.id === matchedToolCallId
                  ? {
                      ...tc,
                      result: event.error || String(event.result || ""),
                      status: (event.error ? "failed" : "completed") as "failed" | "completed",
                    }
                  : tc
              );
              return [...prev.slice(0, -1), { ...lastMessage, toolCalls: newToolCalls }];
            }
            return prev;
          });
          break;
        }

        case "session_status":
          // Could update UI to show session status
          console.log("Session status:", event.status);
          break;

        case "run_finished": {
          if (!currentSessionId || event.session_id !== currentSessionId) {
            break;
          }
          if (event.status !== "completed") {
            finalizePendingToolCalls(event.error || `run_${event.status}`);
          }
          break;
        }

        case "memory_retrieval": {
          if (!currentSessionId || event.session_id !== currentSessionId) {
            break;
          }
          setMessages((prev) => {
            const lastMessage = prev[prev.length - 1];
            if (!lastMessage || lastMessage.role !== "assistant") {
              return prev;
            }
            return [
              ...prev.slice(0, -1),
              {
                ...lastMessage,
                memoryRetrieval: {
                  status: event.status,
                  used: event.used,
                  chunks_total: event.chunks_total,
                  latency_ms: event.latency_ms,
                  embedding_status: event.embedding_status,
                  embedding_reason: event.embedding_reason,
                  session_chunks_stored: lastMessage.memoryRetrieval?.session_chunks_stored ?? 0,
                  project_chunks_stored: lastMessage.memoryRetrieval?.project_chunks_stored ?? 0,
                },
              },
            ];
          });
          break;
        }

        case "memory_storage": {
          if (!currentSessionId || event.session_id !== currentSessionId) {
            break;
          }
          if (event.role !== "assistant") {
            break;
          }
          setMessages((prev) => {
            if (prev.length === 0) return prev;
            const targetIdx =
              event.message_id != null
                ? prev.findIndex((m) => m.id === event.message_id)
                : prev.length - 1;
            if (targetIdx < 0) return prev;
            const target = prev[targetIdx];
            if (target.role !== "assistant") return prev;

            const next = [...prev];
            next[targetIdx] = {
              ...target,
              memoryRetrieval: {
                status: target.memoryRetrieval?.status,
                used: target.memoryRetrieval?.used ?? false,
                chunks_total: target.memoryRetrieval?.chunks_total ?? 0,
                latency_ms: target.memoryRetrieval?.latency_ms ?? 0,
                embedding_status: target.memoryRetrieval?.embedding_status,
                embedding_reason: target.memoryRetrieval?.embedding_reason,
                session_chunks_stored: event.session_chunks_stored,
                project_chunks_stored: event.project_chunks_stored,
              },
            };
            return next;
          });
          break;
        }

        case "session_idle": {
          console.log("[StreamEvent] Session idle - completing generation");
          finalizePendingToolCalls("interrupted");

          // Capture final content before any async operations
          const finalContent = currentAssistantMessageRef.current;
          const finalMessageId = currentAssistantMessageIdRef.current;

          console.log(
            "[StreamEvent] Final content captured:",
            finalContent.length,
            "chars, messageId:",
            finalMessageId
          );

          // Clear generation timeout first
          if (generationTimeoutRef.current) {
            clearTimeout(generationTimeoutRef.current);
            generationTimeoutRef.current = null;
          }

          // Stop generating early to prevent further updates
          setIsGenerating(false);

          // Apply final content - ensure the last assistant message has the complete content
          setMessages((prev) => {
            const lastMessage = prev[prev.length - 1];
            if (lastMessage && lastMessage.role === "assistant") {
              // Use captured finalContent, or fall back to existing content if empty
              const contentToApply = finalContent || lastMessage.content;
              console.log(
                "[StreamEvent] Applying final content to assistant message:",
                contentToApply.length,
                "chars"
              );
              return [
                ...prev.slice(0, -1),
                {
                  ...lastMessage,
                  id: finalMessageId || lastMessage.id,
                  content: contentToApply,
                },
              ];
            }
            return prev;
          });

          // Wait longer before clearing references to ensure React has processed the update
          // Also move backfill check into this timeout so messagesRef is synced
          setTimeout(() => {
            if (!isGeneratingRef.current) {
              currentAssistantMessageRef.current = "";
              currentAssistantMessageIdRef.current = null;
            }

            // Check if we need to backfill from session history.
            // Besides empty content, we also treat "assistant echoes user prompt"
            // as suspicious (seen when stream events miss final assistant text).
            const lastMsg = messagesRef.current[messagesRef.current.length - 1];
            const previousMsg = messagesRef.current[messagesRef.current.length - 2];
            const echoedUserPrompt =
              lastMsg?.role === "assistant" &&
              previousMsg?.role === "user" &&
              Boolean(lastMsg.content?.trim()) &&
              lastMsg.content.trim() === previousMsg.content?.trim();
            const hasPendingTools =
              lastMsg?.role === "assistant" &&
              Array.isArray(lastMsg.toolCalls) &&
              lastMsg.toolCalls.some((tc) => tc.status === "pending");
            const needsBackfill =
              !lastMsg ||
              lastMsg.role !== "assistant" ||
              !lastMsg.content?.trim() ||
              echoedUserPrompt ||
              hasPendingTools;
            if (needsBackfill && currentSessionIdRef.current) {
              console.log("[Chat] Backfilling empty assistant content from history");
              loadSessionHistory(currentSessionIdRef.current);
            }
          }, 1000);

          // Handle deferred session loads
          if (deferredSessionLoadRef.current) {
            const deferredId = deferredSessionLoadRef.current;
            deferredSessionLoadRef.current = null;
            console.log("[Chat] Loading deferred session history:", deferredId);
            if (deferredId) {
              setMessages([]);
              currentAssistantMessageRef.current = "";
              loadSessionHistory(deferredId);
            } else {
              setMessages([]);
              currentAssistantMessageRef.current = "";
            }
          }

          // Force re-render after a longer delay to ensure React has batched all updates
          setTimeout(() => forceUpdate({}), 200);

          if (queueDrainRef.current && currentSessionIdRef.current) {
            void (async () => {
              try {
                const hasNext = await queueSendNext(currentSessionIdRef.current as string);
                const refreshed = await queueList(currentSessionIdRef.current as string);
                setQueuedMessages(refreshed);
                if (!hasNext || refreshed.length === 0) {
                  queueDrainRef.current = false;
                } else {
                  setIsGenerating(true);
                }
              } catch (e) {
                queueDrainRef.current = false;
                console.warn("Queue send-all stopped due to error:", e);
              }
            })();
          }
          break;
        }

        case "session_error": {
          console.error("[StreamEvent] Session error:", event.error);
          finalizePendingToolCalls(event.error || "interrupted");

          // Display the error to the user
          setError(`Session error: ${event.error}`);

          // Capture any content we had before the error
          const errorTimeContent = currentAssistantMessageRef.current;
          console.log("[StreamEvent] Content before error:", errorTimeContent.length, "chars");

          // Stop generation and clean up
          setIsGenerating(false);

          // DON'T clear the content ref - we want to preserve what was streamed
          // currentAssistantMessageRef.current = "";

          // Clear generation timeout
          if (generationTimeoutRef.current) {
            clearTimeout(generationTimeoutRef.current);
            generationTimeoutRef.current = null;
          }

          // Update the last assistant message - preserve existing content and append error (deduplicated)
          setMessages((prev) => {
            const lastMessage = prev[prev.length - 1];
            if (lastMessage && lastMessage.role === "assistant") {
              // Use existing content from ref, or message content, or empty string
              const existingContent = errorTimeContent || lastMessage.content || "";
              const errorText = `**Error:** ${event.error}`;

              // Only append if the error isn't already there
              if (existingContent.includes(errorText)) {
                return prev;
              }

              const errorSuffix = existingContent ? `\n\n${errorText}` : `Error: ${event.error}`;

              const updated = [...prev];
              updated[updated.length - 1] = {
                ...lastMessage,
                content: existingContent + errorSuffix,
              };
              return updated;
            }
            return prev;
          });

          // Clear refs after updating messages
          setTimeout(() => {
            currentAssistantMessageRef.current = "";
            currentAssistantMessageIdRef.current = null;
          }, 500);
          break;
        }

        case "permission_asked": {
          // Handle permission requests from OpenCode
          // Use the current assistant message ID so we can associate file snapshots with this message
          const currentMsgId = currentAssistantMessageIdRef.current;
          if (handledPermissionIdsRef.current.has(event.request_id)) {
            break;
          }
          handledPermissionIdsRef.current.add(event.request_id);

          // Check if this is a destructive operation
          const isDestructive = [
            "write",
            "write_file",
            "create_file",
            "delete",
            "delete_file",
            "bash",
            "shell",
            "run_command",
          ].includes(event.tool || "");

          // Route to staging if plan mode is enabled and operation is destructive
          if (usePlanMode && isDestructive) {
            console.log("[Permission] Routing to staging area");
            stageOperation(
              event.request_id,
              event.session_id,
              event.tool || "unknown",
              (event.args as Record<string, unknown>) || {},
              currentMsgId || undefined
            ).catch((err) => {
              console.error("[Permission] Failed to stage operation:", err);
              setError(`Failed to stage operation: ${err}`);
            });
          } else {
            // Auto-approve if allowAllTools is enabled on frontend
            if (allowAllTools) {
              approveTool(event.session_id, event.request_id, {
                tool: event.tool || undefined,
                args: (event.args as Record<string, unknown>) || undefined,
                messageId: currentMsgId || undefined,
              }).catch((e) => {
                console.error("[Permission] Auto-approve failed:", e);
                const msg = e instanceof Error ? e.message : String(e);
                setError(msg);
                if (msg.toLowerCase().includes("python")) {
                  setShowPythonWizard(true);
                }
              });
            } else {
              const summary = buildPermissionReason(
                event.tool || undefined,
                (event.args as Record<string, unknown>) || undefined
              );
              // Immediate mode: show permission toast as before
              const permissionRequest: PermissionRequest = {
                id: event.request_id,
                session_id: event.session_id,
                type: (event.tool || "unknown") as PermissionRequest["type"],
                path: summary.path,
                command: summary.command,
                reasoning: summary.reasoning,
                riskLevel:
                  event.tool === "delete_file" || event.tool === "bash" ? "high" : "medium",
                tool: event.tool || undefined,
                args: (event.args as Record<string, unknown>) || undefined,
                messageId: currentMsgId || undefined, // Associate with current message for undo
              };
              setPendingPermissions((prev) => {
                if (prev.some((p) => p.id === permissionRequest.id)) {
                  return prev;
                }
                return [...prev, permissionRequest];
              });
            }
          }
          break;
        }

        case "question_asked": {
          // Only show prompts for the active session.
          if (!currentSessionId || event.session_id !== currentSessionId) {
            break;
          }
          if (!event.questions || event.questions.length === 0) {
            break;
          }

          setPendingQuestionRequests((prev) => {
            if (
              handledQuestionRequestIdsRef.current.has(event.request_id) ||
              prev.some((r) => r.request_id === event.request_id)
            ) {
              return prev;
            }

            return [
              ...prev,
              {
                session_id: event.session_id,
                request_id: event.request_id,
                questions: event.questions,
                tool_call_id: event.tool_call_id,
                tool_message_id: event.tool_message_id,
              },
            ];
          });
          break;
        }

        case "raw": {
          // Try to extract useful activity info from raw events
          const data = event.data as Record<string, unknown>;

          if (event.event_type === "system.stream_health") {
            const status = data?.status;
            if (status === "healthy" || status === "degraded" || status === "recovering") {
              setStreamHealth(status);
              if (status === "healthy") {
                setEngineReady(true);
              }
            }
            break;
          }

          if (event.event_type === "engine.lifecycle.ready") {
            setEngineReady(true);
            setSidecarStatus("running");
            setStreamHealth("healthy");
            break;
          }

          if (event.event_type === "system.engine_restart_detected") {
            setEngineReady(false);
            setIsGenerating(false);
            finalizePendingToolCalls("interrupted: engine restarted");
            showStatusBanner("Engine restarted. Rehydrating session state...");
            if (currentSessionIdRef.current) {
              loadSessionHistory(currentSessionIdRef.current);
            }
            break;
          }

          // NOTE: We intentionally DON'T handle message.removed events here
          // because it causes issues with OpenCode auto-generating responses
          // when the conversation ends on a user message. We handle message
          // removal optimistically in handleUndo instead.

          // Handle message.updated events - these often contain tool info
          if (event.event_type === "message.updated") {
            const info = data.info as Record<string, unknown> | undefined;
            if (info) {
              console.log("Message updated:", info);
            }
          }

          // Log todo events for debugging
          if (event.event_type === "todo.updated") {
            console.log("[Todo] Received todo.updated event:", data);
          }

          // Log other raw events for debugging
          console.warn("Raw event (unhandled):", event.event_type, data);
          break;
        }
      }
    },
    [
      isGenerating,
      loadSessionHistory,
      currentSessionId,
      stageOperation,
      usePlanMode,
      allowAllTools,
      scheduleAssistantFlush,
      finalizePendingToolCalls,
      showStatusBanner,
    ]
  );

  // Listen for sidecar events
  useEffect(() => {
    let unlistenFn: (() => void) | null = null;

    const setupListener = async () => {
      unlistenFn = await onSidecarEventV2((envelope: StreamEventEnvelopeV2) => {
        if (!envelope?.event_id) {
          handleStreamEvent(envelope.payload);
          return;
        }
        if (seenEventIdsRef.current.has(envelope.event_id)) {
          return;
        }
        seenEventIdsRef.current.add(envelope.event_id);
        if (seenEventIdsRef.current.size > 5000) {
          const ids = Array.from(seenEventIdsRef.current).slice(-2000);
          seenEventIdsRef.current = new Set(ids);
        }
        handleStreamEvent(envelope.payload);
      });
    };

    setupListener();

    return () => {
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, [handleStreamEvent]);

  const connectSidecar = async () => {
    setIsConnecting(true);
    setError(null);
    setStreamHealth("recovering");
    setSidecarStatus("starting");
    setEngineReady(false);

    try {
      const status = await getSidecarStatus();
      if (status === "starting") {
        const attached = await waitForSidecarRunning(SIDECAR_ATTACH_TIMEOUT_MS, 500);
        if (!attached) {
          throw new Error("Tandem Engine is still starting and did not become ready in time.");
        }
      } else {
        await startSidecarWithTimeout();
      }
      const ready = await waitForEngineReady();
      if (!ready) {
        throw new Error("Tandem Engine started but did not signal readiness on event stream.");
      }
      setSidecarStatus("running");
      setStreamHealth("healthy");
      // Don't create a session here - it will be created when user sends first message
      // Notify parent that sidecar is connected
      onSidecarConnected?.();
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e);
      setError(`Failed to start AI: ${errorMessage}`);
      setSidecarStatus("failed");
    } finally {
      setIsConnecting(false);
    }
  };

  const handleSend = useCallback(
    async (content: string, attachments?: FileAttachment[], forceMode?: "plan" | "immediate") => {
      setError(null);

      // If sidecar isn't running, try to start it
      let currentStatus = sidecarStatus;
      if (currentStatus !== "running") {
        try {
          await connectSidecar();
          currentStatus = await getSidecarStatus();
        } catch (e) {
          console.error("Failed to connect:", e);
          return;
        }
        if (currentStatus !== "running") {
          return;
        }
      }
      if (!engineReadyRef.current) {
        const ready = await waitForEngineReady(8_000, 200);
        if (!ready) {
          setError("Tandem Engine is running but not ready to stream yet. Please retry Connect.");
          return;
        }
      }

      const effectiveModeId =
        forceMode === "immediate"
          ? "immediate"
          : forceMode === "plan"
            ? "plan"
            : selectedModeId || "immediate";
      const legacyAgent =
        effectiveModeId === "immediate"
          ? undefined
          : effectiveModeId === "ask"
            ? "general"
            : effectiveModeId;

      // Create session if needed
      let sessionId = currentSessionId;
      if (!sessionId) {
        try {
          const session = await createSession(
            undefined,
            undefined,
            undefined,
            allowAllTools,
            effectiveModeId
          );
          sessionId = session.id;
          setCurrentSessionId(session.id);
          currentSessionIdRef.current = session.id; // Update ref synchronously before events arrive
          onSessionCreated?.(session.id);
        } catch (e) {
          setError(`Failed to create session: ${e}`);
          return;
        }
      }

      if (isGeneratingRef.current) {
        try {
          await queueMessage(
            sessionId,
            content,
            attachments?.map((a) => ({
              mime: a.mime,
              filename: a.name,
              url: a.url,
            }))
          );
          const refreshed = await queueList(sessionId);
          setQueuedMessages(refreshed);
          showStatusBanner("Message queued");
        } catch (e) {
          const errorMessage = e instanceof Error ? e.message : String(e);
          setError(`Failed to queue message: ${errorMessage}`);
        }
        return;
      }

      // Inject tool guidance if tool categories are enabled
      let finalContent = content;
      if (enabledToolCategories.size > 0) {
        try {
          const guidance = await getToolGuidance(Array.from(enabledToolCategories));

          if (guidance.length > 0) {
            // Determine effective mode once for all guidance
            const effectiveMode =
              forceMode === "immediate" ? false : forceMode === "plan" ? true : usePlanMode;

            const guidanceText = guidance
              .map((g) => {
                let instructions = g.instructions;

                if (!effectiveMode) {
                  // In Immediate Mode: Skip the planning phase, just create directly
                  instructions = instructions.replace(
                    /PHASE 1: PLANNING.*?PHASE 2: EXECUTION \(After User Approval\)/s,
                    "When user requests a presentation, create the .tandem.ppt.json file directly using the write tool."
                  );
                }

                return `
=== ${g.category.toUpperCase()} CAPABILITY ENABLED ===

${instructions}

JSON Schema:
${JSON.stringify(g.json_schema, null, 2)}

Example:
${g.example}
`;
              })
              .join("\n\n");

            finalContent = `${guidanceText}\n\n===== USER REQUEST =====\n${content}`;
            console.log(
              `[ToolGuidance] Injected ${effectiveMode ? "Plan Mode" : "Immediate Mode"} guidance for: ${Array.from(enabledToolCategories).join(", ")}`
            );
          }
        } catch (e) {
          console.error("Failed to get tool guidance:", e);
          // Continue without guidance
        }
      }

      // In Plan Mode, guide the AI to use todowrite for task tracking
      // Use effective mode (respect forceMode override)
      const effectivePlanMode =
        forceMode === "immediate" ? false : forceMode === "plan" ? true : usePlanMode;

      if (effectivePlanMode) {
        finalContent = `${finalContent}
        
(Please use the todowrite tool to create a structured task list. Then, ask for user approval before starting execution/completing the tasks.)`;
        console.log("[PlanMode] Using OpenCode's Plan agent with todowrite guidance");
      }

      // Add user message
      const userMessage: MessageProps = {
        id: crypto.randomUUID(),
        role: "user",
        content,
        timestamp: new Date(),
        // Show attachments in message if any
        attachments: attachments?.map((a) => ({
          name: a.name,
          type: a.type,
          preview: a.preview,
        })),
      };
      setMessages((prev) => [...prev, userMessage]);

      // Add placeholder assistant message
      const assistantMessage: MessageProps = {
        id: crypto.randomUUID(),
        role: "assistant",
        content: "",
        timestamp: new Date(),
      };
      setMessages((prev) => [...prev, assistantMessage]);
      setIsGenerating(true);
      currentAssistantMessageRef.current = "";
      currentAssistantMessageIdRef.current = null; // Reset the message ID ref
      lastEventAtRef.current = Date.now();
      if (generationTimeoutRef.current) {
        clearTimeout(generationTimeoutRef.current);
      }
      generationTimeoutRef.current = setTimeout(() => {
        if (lastEventAtRef.current && Date.now() - lastEventAtRef.current >= 60000) {
          setError("Response timed out. Try again or stop and restart the chat.");
          setIsGenerating(false);
          currentAssistantMessageRef.current = "";
        }
      }, 60000);

      try {
        // For data URLs, split between images (inline as markdown) and text (embed in message)
        let messageContent = finalContent;
        const attachmentsToSend: FileAttachmentInput[] = [];

        if (attachments && attachments.length > 0) {
          for (const attachment of attachments) {
            if (!attachment.url) {
              console.warn("[Attachments] Skipping attachment with empty url:", attachment.name);
              continue;
            }
            const isImage =
              attachment.type.startsWith("image/") || attachment.mime.startsWith("image/");

            if (attachment.url.startsWith("data:")) {
              if (isImage) {
                // Images: Inline as Markdown image with data URL
                // This bypasses file attachment processing which can be flaky for data URLs
                messageContent += `\n![${attachment.name}](${attachment.url})\n`;
              } else {
                // Text files: Decode and embed in message
                try {
                  const base64Data = attachment.url.split(",")[1];
                  // Safe decoding of base64 text
                  const decodedContent = decodeURIComponent(
                    // eslint-disable-next-line no-undef
                    escape(atob(base64Data))
                  );
                  // Use a generic format that won't trigger OpenCode to look for files
                  messageContent += `\n\nHere is the attached content from ${attachment.name}:\n\`\`\`\n${decodedContent}\n\`\`\`\n`;
                } catch (e) {
                  console.warn(
                    `Failed to decode attachment ${attachment.name}, sending as file`,
                    e
                  );
                  // Fallback: send as attachment if decoding fails
                  attachmentsToSend.push({
                    mime: attachment.mime,
                    filename: attachment.name,
                    url: attachment.url,
                  });
                }
              }
            } else {
              // Not a data URL (e.g. file path), send as attachment
              attachmentsToSend.push({
                mime: attachment.mime,
                filename: attachment.name,
                url: attachment.url,
              });
            }
          }
        }

        // Send message and stream response, with selected agent
        await sendMessageAndStartRun(
          sessionId,
          messageContent,
          attachmentsToSend.length > 0 ? attachmentsToSend : undefined,
          legacyAgent,
          effectiveModeId
        );
      } catch (e) {
        const errorMessage = e instanceof Error ? e.message : String(e);
        setError(`Failed to send message: ${errorMessage}`);
        setIsGenerating(false);

        // Update the assistant message with error
        setMessages((prev) => {
          const lastMessage = prev[prev.length - 1];
          if (lastMessage && lastMessage.role === "assistant" && !lastMessage.content) {
            return [
              ...prev.slice(0, -1),
              {
                ...lastMessage,
                content: `Error: ${errorMessage}`,
              },
            ];
          }
          return prev;
        });
      }
    },
    [
      sidecarStatus,
      currentSessionId,
      onSessionCreated,
      connectSidecar,
      getSidecarStatus,
      createSession,
      sendMessageAndStartRun,
      setError,
      setIsGenerating,
      setMessages,
      usePlanMode,
      selectedModeId,
      enabledToolCategories,
      stagedOperations.length,
      allowAllTools,
      showStatusBanner,
    ]
  );

  const handleStop = async () => {
    if (currentSessionId) {
      try {
        await cancelGeneration(currentSessionId);
      } catch (e) {
        console.error("Failed to cancel generation:", e);
      }
    }
    setIsGenerating(false);
    if (generationTimeoutRef.current) {
      clearTimeout(generationTimeoutRef.current);
      generationTimeoutRef.current = null;
    }
  };

  const handleQueueRemove = useCallback(
    async (itemId: string) => {
      if (!currentSessionId) return;
      await queueRemove(currentSessionId, itemId);
      setQueuedMessages(await queueList(currentSessionId));
    },
    [currentSessionId]
  );

  const handleQueueSendNext = useCallback(async () => {
    if (!currentSessionId) return;
    queueDrainRef.current = false;
    const sent = await queueSendNext(currentSessionId);
    if (sent) {
      setQueuedMessages(await queueList(currentSessionId));
      setIsGenerating(true);
    }
  }, [currentSessionId]);

  const handleQueueSendAll = useCallback(async () => {
    if (!currentSessionId) return;
    queueDrainRef.current = true;
    const sent = await queueSendNext(currentSessionId);
    if (sent) {
      setQueuedMessages(await queueList(currentSessionId));
      setIsGenerating(true);
    } else {
      queueDrainRef.current = false;
    }
  }, [currentSessionId]);

  const handleUndo = useCallback(
    async (_messageId: string) => {
      if (!currentSessionId) return;

      try {
        console.log("[Undo] Executing /undo command for session:", currentSessionId);

        // Execute the /undo command which triggers Git-based file restoration
        // Note: OpenCode's /undo operates on the entire session, not individual messages
        await undoViaCommand(currentSessionId);

        console.log("[Undo] Command executed successfully, reloading session history");

        // Reload the session history to reflect the reverted state
        await loadSessionHistory(currentSessionId);

        console.log("[Undo] Session reloaded successfully");
      } catch (e) {
        const errorMessage = e instanceof Error ? e.message : String(e);
        setError(`Failed to undo: ${errorMessage}`);
        console.error("[Undo] Error:", e);
      }
    },
    [currentSessionId, loadSessionHistory]
  );

  const handleEdit = useCallback(
    async (messageId: string, newContent: string) => {
      // Find this user message
      const msgIndex = messages.findIndex((m) => m.id === messageId);
      if (msgIndex < 0) return;

      const userMessage = messages[msgIndex];
      if (userMessage.role !== "user") return;

      // Remove this message and everything after it
      setMessages((prev) => prev.slice(0, msgIndex));

      // Send the edited message
      await handleSend(newContent);
    },
    [messages, handleSend]
  );

  // Ralph Loop polling
  useEffect(() => {
    let intervalId: ReturnType<typeof setInterval>;

    const pollRalph = async () => {
      // Only poll if enabled or if we have a known active run
      if (
        !loopEnabled &&
        (!ralphStatusSnapshot ||
          ["idle", "cancelled", "completed", "error"].includes(ralphStatusSnapshot.status))
      ) {
        return;
      }

      try {
        // Find existing run ID if we have one, otherwise just poll generic status (which might return last run)
        const status = await ralphStatus(ralphStatusSnapshot?.run_id);
        setRalphStatusSnapshot(status);

        // Auto-enable if we detect a running state we didn't know about
        if (status.status === "running" || status.status === "paused") {
          setLoopEnabled(true);
        }
      } catch (_e) {
        // Silent fail for polling
      }
    };

    if (
      loopEnabled ||
      (ralphStatusSnapshot && ["running", "paused"].includes(ralphStatusSnapshot.status))
    ) {
      pollRalph(); // Initial poll
      intervalId = setInterval(pollRalph, 1000);
    }

    return () => clearInterval(intervalId);
  }, [loopEnabled, ralphStatusSnapshot]);

  const handleLoopToggle = (enabled: boolean) => {
    setLoopEnabled(enabled);
    if (enabled) {
      // Maybe automatically check status?
      ralphStatus().then(setRalphStatusSnapshot).catch(console.error);
    }
  };

  const handleRewind = useCallback(
    async (messageId: string) => {
      // Find this user message
      const msgIndex = messages.findIndex((m) => m.id === messageId);
      if (msgIndex < 0) return;

      const userMessage = messages[msgIndex];
      if (userMessage.role !== "user") return;

      // Remove this message and everything after it
      setMessages((prev) => prev.slice(0, msgIndex));

      // Re-send the user message
      await handleSend(userMessage.content);
    },
    [messages, handleSend]
  );

  const handleRegenerate = useCallback(
    async (messageId: string) => {
      // Find the user message before this assistant message
      const msgIndex = messages.findIndex((m) => m.id === messageId);
      if (msgIndex <= 0) return;

      const prevMessage = messages[msgIndex - 1];
      if (prevMessage.role !== "user") return;

      // Remove the assistant response and regenerate
      setMessages((prev) => prev.slice(0, msgIndex));

      // Resend the user message (without attachments for now)
      await handleSend(prevMessage.content);
    },
    [messages, handleSend]
  );

  const handleCopy = useCallback(async (content: string) => {
    try {
      if (typeof window !== "undefined" && window.navigator?.clipboard) {
        await window.navigator.clipboard.writeText(content);
      }
    } catch (e) {
      console.error("Failed to copy:", e);
    }
  }, []);

  const handleDenyPermission = async (id: string, _remember?: boolean) => {
    try {
      const req = pendingPermissions.find((p) => p.id === id);
      if (!req?.session_id) return;
      await denyTool(req.session_id, id, {
        tool: req?.tool,
        args: req?.args,
        messageId: req?.messageId,
      });

      // Update tool call status
      setMessages((prev) => {
        return prev.map((msg) => {
          if (msg.role === "assistant" && msg.toolCalls) {
            return {
              ...msg,
              toolCalls: msg.toolCalls.map((tc) =>
                tc.id === id ? { ...tc, status: "failed" as const, result: "Denied by user" } : tc
              ),
            };
          }
          return msg;
        });
      });

      // Remove from pending
      setPendingPermissions((prev) => prev.filter((p) => p.id !== id));
    } catch (e) {
      console.error("Failed to deny tool:", e);
      setError(`Failed to deny action: ${e}`);
    }
  };

  const removeActiveQuestionRequest = () => {
    setPendingQuestionRequests((prev) => prev.slice(1));
  };

  const handleSubmitQuestionRequest = async (answers: string[][]) => {
    const request = pendingQuestionRequests[0];
    if (!request) return;

    const normalizedQuestionContext = request.questions
      .map((q) => {
        const labels = q.options.map((o) => o.label).join(" ");
        return `${q.header} ${q.question} ${labels}`;
      })
      .join(" ")
      .toLowerCase();
    const normalizedAnswers = answers
      .flat()
      .map((a) => a.trim().toLowerCase())
      .filter(Boolean);
    const isPlanProceedDecision =
      normalizedQuestionContext.includes("plan") &&
      (normalizedQuestionContext.includes("proceed") ||
        normalizedQuestionContext.includes("saved") ||
        normalizedQuestionContext.includes("read-only"));
    const selectedImmediateExecution = normalizedAnswers.some((a) =>
      /(enable code edits|implement|execute|apply changes|go ahead|proceed|do it now|immediate|coder)/.test(
        a
      )
    );
    const selectedManualPath = normalizedAnswers.some((a) =>
      /(myself|manual|manually|i'll do it myself|i will do it myself)/.test(a)
    );

    try {
      await replyQuestion(request.request_id, answers);
      handledQuestionRequestIdsRef.current.add(request.request_id);
      if (request.tool_call_id) pendingQuestionToolCallIdsRef.current.delete(request.tool_call_id);
      if (request.tool_message_id)
        pendingQuestionToolMessageIdsRef.current.delete(request.tool_message_id);
      if (isPlanProceedDecision && selectedImmediateExecution && !selectedManualPath) {
        setUsePlanMode(false);
        showStatusBanner("Switched to Immediate mode for execution.");
      }
      removeActiveQuestionRequest();
    } catch (err) {
      console.error("Failed to reply to question request:", err);
      setError(`Failed to answer question: ${err}`);
    }
  };

  const handleRejectQuestionRequest = async () => {
    const request = pendingQuestionRequests[0];
    if (!request) return;

    try {
      await rejectQuestion(request.request_id);
      handledQuestionRequestIdsRef.current.add(request.request_id);
      if (request.tool_call_id) pendingQuestionToolCallIdsRef.current.delete(request.tool_call_id);
      if (request.tool_message_id)
        pendingQuestionToolMessageIdsRef.current.delete(request.tool_message_id);
      removeActiveQuestionRequest();
    } catch (err) {
      console.error("Failed to reject question request:", err);
      setError(`Failed to reject question: ${err}`);
    }
  };

  useEffect(() => {
    if (!usePlanMode || !activePlan || !hasPendingQuestionOverlay) return;
    const currentQuestionText =
      pendingQuestionRequests[0]?.questions?.[0]?.question?.toLowerCase() ?? "";
    if (
      currentQuestionText.includes("plan") &&
      (currentQuestionText.includes("proceed") || currentQuestionText.includes("saved")) &&
      !showPlanView
    ) {
      setShowPlanView(true);
    }
  }, [activePlan, hasPendingQuestionOverlay, pendingQuestionRequests, showPlanView, usePlanMode]);

  useEffect(() => {
    if (!isConnecting) {
      setStartupHealth(null);
      return;
    }

    let cancelled = false;
    const poll = async () => {
      try {
        const health = await getSidecarStartupHealth();
        if (!cancelled) {
          setStartupHealth(health);
        }
      } catch {
        if (!cancelled) {
          setStartupHealth(null);
        }
      }
    };

    poll();
    const interval = globalThis.setInterval(poll, 700);
    return () => {
      cancelled = true;
      globalThis.clearInterval(interval);
    };
  }, [isConnecting]);

  const needsConnection = sidecarStatus !== "running" && !isConnecting;
  const showConnectingOverlay = isConnecting;

  return (
    <div className="relative flex h-full flex-col">
      {statusBanner && (
        <div className="pointer-events-none absolute right-4 top-4 z-40 rounded-lg border border-success/40 bg-success/15 px-3 py-2 text-sm text-success shadow-lg">
          {statusBanner}
        </div>
      )}
      <AnimatePresence>
        {showConnectingOverlay && (
          <motion.div
            className="fixed inset-0 z-[120] flex items-center justify-center bg-[radial-gradient(circle_at_center,color-mix(in_srgb,var(--color-primary)_20%,transparent)_0%,color-mix(in_srgb,var(--color-background)_78%,black_22%)_68%,color-mix(in_srgb,var(--color-background)_90%,black_10%)_100%)] backdrop-blur-md"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
          >
            <motion.div
              className="w-[min(440px,88vw)] rounded-2xl border border-primary/20 bg-surface-elevated/92 p-4 shadow-2xl shadow-black/35"
              initial={{ scale: 0.97, opacity: 0 }}
              animate={{ scale: 1, opacity: 1 }}
              exit={{ scale: 0.98, opacity: 0 }}
            >
              <div className="mb-2.5 flex items-center gap-3">
                <div className="flex h-9 w-9 items-center justify-center rounded-xl border border-primary/25 bg-primary/10 shadow-lg shadow-primary/20">
                  <Loader2 className="h-5 w-5 animate-spin text-primary/90" />
                </div>
                <div>
                  <h3 className="text-base font-semibold text-text">Connecting to Tandem Engine</h3>
                  <p className="text-sm text-text-subtle">
                    {startupPhaseLabel(startupHealth?.phase)}
                    {startupHealth
                      ? ` - ${Math.max(0, Math.round((startupHealth.startup_elapsed_ms || 0) / 1000))}s`
                      : ""}
                  </p>
                </div>
              </div>
              <div className="rounded-lg border border-border bg-surface/70 px-2 py-2">
                <div className="grid grid-cols-[repeat(14,minmax(0,1fr))] gap-1">
                  {Array.from({ length: 14 }).map((_, index) => (
                    <motion.div
                      key={`connect-cell-${index}`}
                      className="h-2.5 w-full rounded-[3px] border border-primary/20 bg-primary/10"
                      animate={{
                        opacity:
                          index <
                          Math.max(
                            1,
                            Math.round((startupPhaseProgress(startupHealth?.phase) / 100) * 14)
                          )
                            ? [0.35, 0.65, 1, 0.65, 0.35]
                            : [0.12, 0.2, 0.3, 0.2, 0.12],
                        scaleY: [0.9, 1, 1.08, 1, 0.9],
                        backgroundColor: [
                          "var(--color-primary-muted)",
                          "var(--color-primary)",
                          "var(--color-secondary)",
                          "var(--color-primary)",
                          "var(--color-primary-muted)",
                        ],
                        boxShadow: [
                          "0 0 0px transparent",
                          "0 0 8px color-mix(in srgb, var(--color-primary) 55%, transparent)",
                          "0 0 12px color-mix(in srgb, var(--color-secondary) 55%, transparent)",
                          "0 0 8px color-mix(in srgb, var(--color-primary) 45%, transparent)",
                          "0 0 0px transparent",
                        ],
                      }}
                      transition={{
                        duration: 1.15,
                        repeat: Infinity,
                        repeatType: "loop",
                        ease: "easeInOut",
                        delay: index * 0.06,
                      }}
                    />
                  ))}
                </div>
                <div className="mt-1.5 flex items-center justify-between text-xs text-text-muted">
                  <span>{startupPhaseDetail(startupHealth?.phase)}</span>
                  <span>{startupPhaseProgress(startupHealth?.phase)}% estimated</span>
                </div>
              </div>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>
      {/* Header */}
      <header className="flex items-center justify-between border-b border-border px-6 py-4">
        <div className="flex items-center gap-3">
          <div>
            <h1 className="font-semibold text-text">Tandem</h1>
            {workspacePath && (
              <p className="flex items-center gap-1 text-sm text-text-muted">
                <FolderOpen className="h-3 w-3" />
                {workspacePath}
              </p>
            )}
          </div>
        </div>

        {/* Connection status */}
        <div className="flex items-center gap-4">
          {activeChatRunningCount > 0 && (
            <motion.div
              initial={{ opacity: 0, scale: 0.9 }}
              animate={{ opacity: 1, scale: 1 }}
              className="flex items-center gap-1.5 rounded-lg border border-primary/30 bg-primary/10 px-2.5 py-1"
              title="Chat sessions currently running"
            >
              <Loader2 className="h-3 w-3 animate-spin text-primary" />
              <span className="text-[10px] font-medium uppercase tracking-wide text-primary">
                {activeChatRunningCount} chat
              </span>
            </motion.div>
          )}

          {activeOrchestrationCount > 0 && (
            <motion.div
              initial={{ opacity: 0, scale: 0.9 }}
              animate={{ opacity: 1, scale: 1 }}
              className="flex items-center gap-1.5 rounded-lg border border-amber-500/30 bg-amber-500/10 px-2.5 py-1"
              title="Orchestration runs currently executing in the background"
            >
              <Loader2 className="h-3 w-3 animate-spin text-amber-400" />
              <span className="text-[10px] font-medium uppercase tracking-wide text-amber-300">
                {activeOrchestrationCount} orch
              </span>
            </motion.div>
          )}

          {/* Staged operations counter */}
          {usePlanMode && stagedOperations.length > 0 && (
            <motion.div
              initial={{ opacity: 0, scale: 0.8 }}
              animate={{ opacity: 1, scale: 1 }}
              className="flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-amber-500/10 border border-amber-500/20"
            >
              <div className="h-1.5 w-1.5 rounded-full bg-amber-500 animate-pulse" />
              <span className="text-xs font-medium text-amber-500">
                {stagedOperations.length} change{stagedOperations.length !== 1 ? "s" : ""} pending
              </span>
            </motion.div>
          )}

          <div className="flex items-center gap-2">
            <div
              className={`h-2 w-2 rounded-full ${
                sidecarStatus === "running"
                  ? "bg-primary"
                  : sidecarStatus === "starting"
                    ? "bg-warning animate-pulse"
                    : "bg-text-subtle"
              }`}
            />
            <span className="text-xs text-text-muted">
              {sidecarStatus === "running"
                ? "Connected"
                : sidecarStatus === "starting"
                  ? "Connecting..."
                  : "Disconnected"}
            </span>
          </div>
          {sidecarStatus === "running" && (
            <span
              className={cn(
                "rounded border px-2 py-0.5 text-[10px] uppercase tracking-wide",
                streamHealth === "healthy"
                  ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-300"
                  : streamHealth === "degraded"
                    ? "border-amber-500/30 bg-amber-500/10 text-amber-300"
                    : "border-sky-500/30 bg-sky-500/10 text-sky-300"
              )}
            >
              stream {streamHealth}
            </span>
          )}
        </div>
      </header>

      {/* Plan Mode info banner */}
      {usePlanMode && (
        <motion.div
          initial={{ opacity: 0, height: 0 }}
          animate={{ opacity: 1, height: "auto" }}
          exit={{ opacity: 0, height: 0 }}
          className="border-b border-border bg-surface-elevated px-4 py-2"
        >
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-text-muted">Plan Mode Active</span>
            <button
              onClick={() => setShowPlanView(!showPlanView)}
              className="text-xs text-primary hover:underline"
            >
              {showPlanView ? "Hide Plans" : "Show Plans"}
            </button>
          </div>

          {/* Inline Plan Selector */}
          <div className="mt-2">
            <PlanSelector
              plans={plans}
              activePlan={activePlan}
              onSelectPlan={setActivePlan}
              onNewPlan={handleNewPlan}
              isLoading={isLoadingPlans}
            />
          </div>
        </motion.div>
      )}

      {/* Error banner */}
      {error && (
        <div className="flex items-center gap-2 bg-error/10 px-4 py-2 text-sm text-error">
          <AlertCircle className="h-4 w-4" />
          {error}
          {error.toLowerCase().includes("python") && (
            <button
              onClick={() => setShowPythonWizard(true)}
              className="ml-2 rounded-md border border-error/30 bg-error/5 px-2 py-1 text-xs text-error hover:bg-error/10"
            >
              Open Python setup
            </button>
          )}
          <button onClick={() => setError(null)} className="ml-auto text-error/70 hover:text-error">
            ×
          </button>
        </div>
      )}

      {/* Main content area with split view support */}
      <div className="flex flex-1 overflow-hidden">
        {/* Messages area */}
        <div
          className={cn(
            "flex flex-col flex-1 overflow-hidden",
            showPlanView && activePlan && "w-1/2"
          )}
        >
          {/* Messages */}
          <div className="relative flex-1 overflow-hidden">
            <div
              ref={messagesContainerRef}
              className="h-full w-full overflow-y-auto overflow-x-hidden pb-48"
            >
              {isLoadingHistory ? (
                <motion.div
                  className="flex h-full items-center justify-center"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                >
                  <div className="flex flex-col items-center gap-3">
                    <Loader2 className="h-8 w-8 animate-spin text-primary" />
                    <p className="text-sm text-text-muted">Loading chat history...</p>
                  </div>
                </motion.div>
              ) : messages.length === 0 && !isGenerating ? (
                <EmptyState
                  needsConnection={needsConnection}
                  isConnecting={isConnecting}
                  onConnect={connectSidecar}
                  workspacePath={workspacePath}
                  onSendMessage={handleSend}
                  hasConfiguredProvider={hasConfiguredProvider}
                  onOpenSettings={onOpenSettings}
                  onOpenPacks={onOpenPacks}
                  onOpenExtensions={onOpenExtensions}
                />
              ) : (
                <div
                  style={{
                    height: `${rowVirtualizer.getTotalSize()}px`,
                    width: "100%",
                    position: "relative",
                  }}
                >
                  {rowVirtualizer.getVirtualItems().map((virtualItem) => {
                    const message = messages[virtualItem.index];
                    const isLastMessage = virtualItem.index === messages.length - 1;
                    const isAssistant = message.role === "assistant";
                    const showActionButtons =
                      usePlanMode &&
                      isLastMessage &&
                      isAssistant &&
                      !isGenerating &&
                      !hasPendingQuestionOverlay;

                    // Use content length in key ONLY for streaming messages to force re-renders
                    const isActivelyStreaming = isGenerating && isLastMessage && isAssistant;
                    return (
                      <div
                        key={virtualItem.key}
                        data-index={virtualItem.index}
                        ref={rowVirtualizer.measureElement}
                        style={{
                          position: "absolute",
                          top: 0,
                          left: 0,
                          width: "100%",
                          transform: `translateY(${virtualItem.start}px)`,
                        }}
                      >
                        <Message
                          key={message.id}
                          {...message}
                          isStreaming={isActivelyStreaming}
                          renderMode={isActivelyStreaming ? "streaming-lite" : "full"}
                          disableMountAnimation
                          onEdit={handleEdit}
                          onRewind={handleRewind}
                          onRegenerate={handleRegenerate}
                          onCopy={handleCopy}
                          onUndo={isGitRepository ? handleUndo : undefined}
                          onFileOpen={onFileOpen}
                          onOpenQuestionToolCall={handleOpenQuestionToolCall}
                          isQuestionToolCallPending={isQuestionToolCallPending}
                        />
                        {showActionButtons && (
                          <div className="ml-14 mb-4">
                            <PlanActionButtons
                              onImplement={() => {
                                // Switch to immediate mode for execution
                                setUsePlanMode(false);
                                handleSend("Please implement this plan now.");
                              }}
                              onRework={(feedback) => {
                                handleSend(`Please rework the plan with this feedback: ${feedback}

After making the changes, present the updated plan in full (including the complete JSON structure) so I can review it before implementation.`);
                              }}
                              onCancel={() => {
                                clearStaging();
                                handleSend(
                                  "Let's try a different approach. Cancel the current plan."
                                );
                              }}
                              onViewTasks={onToggleTaskSidebar}
                              disabled={isGenerating}
                              pendingTasks={pendingTasks}
                              onExecuteTasks={() => {
                                // Execute pending tasks with their specific content
                                if (pendingTasks && pendingTasks.length > 0) {
                                  console.log(
                                    "[ExecuteTasks] Switching to Immediate mode for task execution"
                                  );
                                  // Switch to immediate mode for execution
                                  setUsePlanMode(false);

                                  const taskList = pendingTasks
                                    .map((t, i) => `${i + 1}. ${t.content}`)
                                    .join("\n");
                                  const message = `EXECUTION MODE: Please implement the following approved tasks now. Create the files and write the content directly.

${taskList}

Start with task #1 and execute each one. Use the 'write' tool to create files immediately. IMPORTANT: As you finish each task, you MUST use the 'todowrite' tool to mark it as "completed".`;
                                  // Force immediate mode for this specific message
                                  handleSend(message, undefined, "immediate");
                                }
                              }}
                            />
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              )}

              {/* Streaming indicator */}
              {isGenerating && (
                <motion.div
                  className="glass border-glass rounded-2xl shadow-lg shadow-black/20 ring-1 ring-white/5 px-4 py-6 flex gap-4"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  transition={{ duration: 0.2 }}
                >
                  <div className="relative flex h-8 w-8 items-center justify-center rounded-lg bg-gradient-to-br from-secondary to-primary text-white shadow-[0_0_12px_rgba(59,130,246,0.45)]">
                    <Sparkles className="h-4 w-4 animate-pulse" />
                  </div>
                  <div className="flex items-center gap-3">
                    <span className="inline-block h-3 w-1.5 bg-primary animate-pulse" />
                    <span className="terminal-text text-text-muted">Processing</span>
                    <div className="flex gap-1">
                      <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-text-subtle [animation-delay:-0.3s]" />
                      <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-text-subtle [animation-delay:-0.15s]" />
                      <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-text-subtle" />
                    </div>
                  </div>
                </motion.div>
              )}

              <div ref={messagesEndRef} />
            </div>

            {showJumpToLatest && (
              <div className="absolute bottom-4 left-0 right-0 z-20 flex justify-center px-5 pointer-events-none">
                <button
                  type="button"
                  className="pointer-events-auto inline-flex items-center gap-2 rounded-full border border-primary/40 bg-surface-elevated/95 px-4 py-2 text-xs font-medium text-primary shadow-lg shadow-black/25 transition hover:border-primary/70 hover:bg-surface-elevated animate-pulse"
                  onClick={() => {
                    setFollowLatest(true);
                    scrollToBottom(true);
                  }}
                  aria-label="Jump to latest message"
                >
                  <ChevronDown className="h-4 w-4" />
                  Jump to latest
                </button>
              </div>
            )}
          </div>

          {/* Input or Configuration Prompt */}
          {!hasConfiguredProvider ? (
            <div className="border-t border-border bg-surface p-4">
              <div className="flex flex-col items-center justify-center gap-3 rounded-xl border border-dashed border-yellow-500/50 bg-yellow-500/5 p-6 text-center">
                <div className="flex items-center gap-2 text-yellow-500">
                  <AlertCircle className="h-5 w-5" />
                  <p className="font-semibold">Setup Required</p>
                </div>
                <p className="text-sm text-text-muted">
                  Configure an AI provider (OpenAI, Anthropic, etc.) to start chatting.
                </p>
                {onOpenSettings && (
                  <Button onClick={onOpenSettings} variant="primary" className="mt-2 text-white">
                    <SettingsIcon className="mr-2 h-4 w-4" />
                    Open Settings
                  </Button>
                )}
              </div>
            </div>
          ) : (
            <>
              {queuedMessages.length > 0 && (
                <div className="border-t border-border bg-surface/70 px-4 py-3">
                  <div className="mx-auto flex w-full max-w-5xl items-start justify-between gap-3 rounded-lg border border-border bg-surface-elevated p-3">
                    <div className="min-w-0 flex-1">
                      <p className="text-xs font-semibold text-text">
                        Queued messages ({queuedMessages.length})
                      </p>
                      <div className="mt-1 space-y-1">
                        {queuedMessages.slice(0, 3).map((q) => (
                          <div
                            key={q.id}
                            className="flex items-center gap-2 text-xs text-text-muted"
                          >
                            <span className="truncate">{q.content || "(attachment only)"}</span>
                            <button
                              type="button"
                              className="text-text-subtle hover:text-error"
                              onClick={() => handleQueueRemove(q.id)}
                            >
                              remove
                            </button>
                          </div>
                        ))}
                        {queuedMessages.length > 3 && (
                          <p className="text-[11px] text-text-subtle">
                            +{queuedMessages.length - 3} more
                          </p>
                        )}
                      </div>
                    </div>
                    <div className="flex shrink-0 gap-2">
                      <Button size="sm" variant="secondary" onClick={handleQueueSendNext}>
                        Send next
                      </Button>
                      <Button size="sm" onClick={handleQueueSendAll}>
                        Send all
                      </Button>
                    </div>
                  </div>
                </div>
              )}
              <ChatInput
                onSend={handleSend}
                onStop={handleStop}
                isGenerating={isGenerating}
                disabled={!workspacePath}
                placeholder={
                  workspacePath
                    ? needsConnection
                      ? "Type to connect and start chatting..."
                      : "Ask Tandem anything..."
                    : "Select a folder to start chatting"
                }
                draftMessage={draftMessage}
                onDraftMessageConsumed={onDraftMessageConsumed}
                selectedAgent={selectedModeId}
                onAgentChange={onAgentChange}
                externalAttachment={fileToAttach}
                onExternalAttachmentProcessed={onFileAttached}
                enabledToolCategories={enabledToolCategories}
                onToolCategoriesChange={setEnabledToolCategories}
                allowAllTools={allowAllTools}
                onAllowAllToolsChange={setAllowAllTools}
                allowAllToolsLocked={false}
                activeProviderLabel={activeProviderLabel}
                activeModelLabel={activeModelLabel}
                loopEnabled={loopEnabled}
                onLoopToggle={handleLoopToggle}
                loopStatus={ralphStatusSnapshot}
                onLoopPanelOpen={() => setShowRalphPanel(true)}
                onLogsOpen={() => setShowLogsDrawer(true)}
                onModelSelect={async (modelId, providerIdRaw) => {
                  // Update the global provider config to switch to this model
                  try {
                    // Normalize provider ID (sidecar uses 'opencode', tandem use 'opencode_zen')
                    const providerId =
                      providerIdRaw === "opencode" ? "opencode_zen" : providerIdRaw;
                    const providerIdForSidecar =
                      providerId === "opencode_zen" ? "opencode" : providerId;

                    const config = await getProvidersConfig();

                    // Determine which top-level provider to use
                    const knownProviders = [
                      "openai",
                      "anthropic",
                      "openrouter",
                      "opencode_zen",
                      "ollama",
                    ];
                    const isKnownTopLevel = knownProviders.includes(providerId);

                    // Always persist the selected model (provider+model) so the backend can route
                    // to arbitrary OpenCode providers (including user-defined ones).
                    let updated: ProvidersConfig = {
                      ...config,
                      selected_model: { provider_id: providerIdForSidecar, model_id: modelId },
                    };

                    if (isKnownTopLevel) {
                      // For known providers, keep the existing behavior: enable one, disable others.
                      updated = {
                        ...updated,
                        openrouter: {
                          ...config.openrouter,
                          enabled: providerId === "openrouter",
                          default: providerId === "openrouter",
                        },
                        opencode_zen: {
                          ...config.opencode_zen,
                          enabled: providerId === "opencode_zen",
                          default: providerId === "opencode_zen",
                        },
                        anthropic: {
                          ...config.anthropic,
                          enabled: providerId === "anthropic",
                          default: providerId === "anthropic",
                        },
                        openai: {
                          ...config.openai,
                          enabled: providerId === "openai",
                          default: providerId === "openai",
                        },
                        ollama: {
                          ...config.ollama,
                          enabled: providerId === "ollama",
                          default: providerId === "ollama",
                        },
                        custom: config.custom,
                      };

                      // Update model for known providers (for display + defaults)
                      if (providerId === "opencode_zen") updated.opencode_zen.model = modelId;
                      if (providerId === "openrouter") updated.openrouter.model = modelId;
                      if (providerId === "anthropic") updated.anthropic.model = modelId;
                      if (providerId === "openai") updated.openai.model = modelId;
                      if (providerId === "ollama") updated.ollama.model = modelId;
                    }

                    await setProvidersConfig(updated);
                    // Trigger refresh in parent to update labels
                    onProviderChange?.();
                  } catch (e) {
                    console.error("Failed to update model selection:", e);
                  }
                }}
              />
            </>
          )}

          {/* Permission requests - only show in immediate mode */}
          {!usePlanMode && (
            <PermissionToastContainer
              requests={pendingPermissions}
              onApprove={handleApprovePermission}
              onDeny={handleDenyPermission}
            />
          )}

          {/* Question dialog */}
          <QuestionDialog
            key={pendingQuestionRequests[0]?.request_id ?? "no-request"}
            request={pendingQuestionRequests[0] ?? null}
            onSubmit={handleSubmitQuestionRequest}
            onReject={handleRejectQuestionRequest}
            canViewPlan={usePlanMode && !!activePlan}
            onViewPlan={() => setShowPlanView(true)}
            planLabel={activePlan?.fileName}
          />
        </div>

        {/* Plan Viewer - Split view */}
        <AnimatePresence>
          {showPlanView && activePlan && (
            <motion.div
              initial={{ width: 0, opacity: 0 }}
              animate={{ width: "50%", opacity: 1 }}
              exit={{ width: 0, opacity: 0 }}
              transition={{ duration: 0.3 }}
              className="overflow-hidden"
            >
              <PlanViewer plan={activePlan} onClose={() => setShowPlanView(false)} />
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      {/* Execution plan panel - only show in plan mode */}
      {usePlanMode && (
        <ExecutionPlanPanel
          operations={stagedOperations}
          onExecute={async () => {
            try {
              await executePlan();
              console.log("[ExecutionPlan] Plan executed successfully");

              // Send confirmation message to AI that plan was executed
              if (currentSessionId && stagedOperations.length > 0) {
                const confirmMessage = `The execution plan with ${stagedOperations.length} change(s) has been applied successfully. You can continue with the next steps.`;

                // Send as a user message so the AI knows to continue
                setTimeout(async () => {
                  try {
                    // Use the same agent (plan agent if in plan mode)
                    await sendMessageAndStartRun(
                      currentSessionId,
                      confirmMessage,
                      undefined,
                      usePlanMode ? "plan" : undefined,
                      usePlanMode ? "plan" : "immediate"
                    );
                  } catch (err) {
                    console.error("[ExecutionPlan] Failed to send confirmation:", err);
                  }
                }, 500); // Small delay to ensure UI updates first
              }
            } catch (err) {
              console.error("[ExecutionPlan] Failed to execute plan:", err);
              setError(`Failed to execute plan: ${err}`);
            }
          }}
          onRemove={async (id) => {
            try {
              await removeOperation(id);
            } catch (err) {
              console.error("[ExecutionPlan] Failed to remove operation:", err);
              setError(`Failed to remove operation: ${err}`);
            }
          }}
          onClear={async () => {
            try {
              await clearStaging();
            } catch (err) {
              console.error("[ExecutionPlan] Failed to clear staging:", err);
              setError(`Failed to clear staging: ${err}`);
            }
          }}
          isExecuting={isExecutingPlan}
        />
      )}

      {/* Activity drawer - Hidden for now as it's always empty */}
      {/* <ActivityDrawer activities={activities} isGenerating={isGenerating} /> */}

      {/* Ralph Panel */}
      {showRalphPanel && ralphStatusSnapshot && (
        <RalphPanel runId={ralphStatusSnapshot.run_id} onClose={() => setShowRalphPanel(false)} />
      )}

      {/* Logs Drawer */}
      {showLogsDrawer && (
        <LogsDrawer onClose={() => setShowLogsDrawer(false)} sessionId={currentSessionId} />
      )}
      {showPythonWizard && <PythonSetupWizard onClose={() => setShowPythonWizard(false)} />}
    </div>
  );
}

interface EmptyStateProps {
  needsConnection: boolean;
  isConnecting: boolean;
  onConnect: () => void;
  workspacePath: string | null;
  onSendMessage: (message: string) => void;
  hasConfiguredProvider: boolean;
  onOpenSettings?: () => void;
  onOpenPacks?: () => void;
  onOpenExtensions?: (tab?: "skills" | "plugins" | "mcp" | "modes") => void;
}

type SuggestionPrompt = {
  title: string;
  description: string;
  prompt: string;
};

type SuggestionAction = {
  title: string;
  description: string;
  action: "openPacks" | "openMcp" | "openModes";
};

type Suggestion = SuggestionPrompt | SuggestionAction;

// Suggestion prompts - mix of developer and general user tasks
const SUGGESTION_PROMPTS: SuggestionPrompt[] = [
  {
    title: "📁 Summarize this project",
    description: "Give me an overview of what this project does",
    prompt:
      "Give me a comprehensive overview of this project. What does it do, what are the main components, and how is it organized?",
  },
  {
    title: "🔍 Find and explain",
    description: "Help me understand a specific file or folder",
    prompt: "List the files in this project and help me understand what each one does.",
  },
  {
    title: "📝 Analyze a document",
    description: "Read and summarize any text file",
    prompt:
      "Find any text documents, markdown files, or READMEs in this project and summarize their contents.",
  },
  {
    title: "✨ Suggest improvements",
    description: "What could be better in this project?",
    prompt: "Analyze this project and suggest improvements. What could be done better?",
  },
  {
    title: "🐛 Find issues",
    description: "Look for potential bugs or problems",
    prompt: "Search this codebase for potential bugs, issues, or areas that might cause problems.",
  },
  {
    title: "📖 Create documentation",
    description: "Generate a README or docs",
    prompt:
      "Create comprehensive documentation for this project, including a README with setup instructions.",
  },
];

function EmptyState({
  needsConnection,
  isConnecting,
  onConnect,
  workspacePath,
  onSendMessage,
  hasConfiguredProvider,
  onOpenSettings,
  onOpenPacks,
  onOpenExtensions,
}: EmptyStateProps) {
  const [suggestions] = useState<Suggestion[]>(() => {
    const shuffled = [...SUGGESTION_PROMPTS].sort(() => Math.random() - 0.5);
    const pinned: SuggestionAction[] = [];
    if (onOpenPacks) {
      pinned.push({
        title: "Install starter packs",
        description: "Browse skill templates and starter packs to install",
        action: "openPacks",
      });
    }
    if (onOpenExtensions) {
      pinned.push({
        title: "Set up MCP",
        description: "Add tool servers and test connectivity",
        action: "openMcp",
      });
      pinned.push({
        title: "Create custom modes",
        description: "Define advanced mode profiles and restrictions",
        action: "openModes",
      });
    }
    return [...pinned, ...shuffled].slice(0, 4);
  });

  const handleSuggestionClick = (suggestion: Suggestion) => {
    if ("prompt" in suggestion) {
      onSendMessage(suggestion.prompt);
      return;
    }
    if (suggestion.action === "openPacks") {
      onOpenPacks?.();
      return;
    }
    if (suggestion.action === "openMcp") {
      onOpenExtensions?.("mcp");
      return;
    }
    if (suggestion.action === "openModes") {
      onOpenExtensions?.("modes");
    }
  };

  return (
    <motion.div
      className="flex min-h-full flex-col items-center justify-center p-8 pt-16"
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
    >
      <div className="max-w-lg w-full text-center">
        <div className="mx-auto mb-6 flex h-16 w-16 items-center justify-center rounded-2xl bg-gradient-to-br from-primary/20 to-secondary/20">
          <Sparkles className="h-8 w-8 text-primary" />
        </div>

        <h2 className="mb-3 text-2xl font-bold text-text">What can I help you with?</h2>

        {!hasConfiguredProvider && (
          <div className="mb-6 rounded-xl border border-yellow-500/30 bg-yellow-500/10 p-4 text-left">
            <div className="flex items-start gap-3">
              <AlertCircle className="mt-0.5 h-5 w-5 flex-shrink-0 text-yellow-500" />
              <div>
                <h3 className="font-semibold text-text">No Model Configured</h3>
                <p className="mt-1 text-sm text-text-muted">
                  You need to configure an LLM provider (OpenAI, Anthropic, etc.) to start chatting.
                </p>
                {onOpenSettings && (
                  <button
                    onClick={onOpenSettings}
                    className="mt-2 text-sm font-medium text-primary hover:underline"
                  >
                    Configure Settings →
                  </button>
                )}
              </div>
            </div>
          </div>
        )}

        <p className="mb-8 text-text-muted">
          I can read and write files, search your codebase, run commands, and help you accomplish
          tasks in your folder.
        </p>

        {needsConnection && workspacePath && (
          <button
            onClick={onConnect}
            disabled={isConnecting}
            className="mb-8 inline-flex items-center gap-2 rounded-lg bg-primary px-6 py-3 font-medium text-white transition-colors hover:bg-primary/90 disabled:opacity-50"
          >
            {isConnecting ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin" />
                Connecting...
              </>
            ) : (
              <>
                <Sparkles className="h-4 w-4" />
                Connect AI
              </>
            )}
          </button>
        )}

        <div className="grid grid-cols-2 gap-3 text-left">
          {suggestions.map((suggestion, index) => (
            <SuggestionCard
              key={index}
              title={suggestion.title}
              description={suggestion.description}
              icon={
                "action" in suggestion ? (
                  suggestion.action === "openPacks" ? (
                    <Sparkles className="h-4 w-4 text-primary" />
                  ) : (
                    <Link2 className="h-4 w-4 text-primary" />
                  )
                ) : undefined
              }
              onClick={() => handleSuggestionClick(suggestion)}
              disabled={needsConnection || isConnecting}
            />
          ))}
        </div>
      </div>
    </motion.div>
  );
}

interface SuggestionCardProps {
  title: string;
  description: string;
  icon?: ReactNode;
  onClick: () => void;
  disabled?: boolean;
}

function SuggestionCard({ title, description, icon, onClick, disabled }: SuggestionCardProps) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="rounded-lg border border-border bg-surface p-4 text-left transition-all hover:border-primary/50 hover:bg-surface-elevated hover:shadow-lg hover:shadow-primary/5 disabled:opacity-50 disabled:cursor-not-allowed"
    >
      <div className={cn("flex items-start gap-3", !icon && "gap-0")}>
        {icon && (
          <div className="mt-0.5 flex h-8 w-8 items-center justify-center rounded-lg bg-primary/10">
            {icon}
          </div>
        )}
        <div className={cn("min-w-0", icon ? "" : "w-full")}>
          <p className="font-medium text-text">{title}</p>
          <p className="text-sm text-text-muted line-clamp-2">{description}</p>
        </div>
      </div>
    </button>
  );
}
