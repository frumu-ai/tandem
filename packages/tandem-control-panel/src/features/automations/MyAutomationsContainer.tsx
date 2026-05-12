import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { renderIcons } from "../../app/icons.js";
import { api } from "../../lib/api";
import {
  DEFAULT_WORKFLOW_LIBRARY_FILTERS,
  DEFAULT_WORKFLOW_SORT_MODE,
  classifyAutomationSource,
  filterWorkflowAutomations,
  getAutomationCreatedAtMs,
  getAutomationId,
  getAutomationName,
  normalizeFavoriteAutomationIds,
  normalizeWorkflowLibraryFilters,
  normalizeWorkflowSortMode,
  sortWorkflowAutomations,
  toggleFavoriteAutomationId,
  workflowLibraryFiltersEqual,
} from "../../../lib/automations/workflow-list.js";
import { formatJson } from "../../pages/ui";
import { projectOrchestrationRun } from "../orchestrator/blackboardProjection";
import {
  workflowActiveSessionCount,
  workflowArtifactValidation,
  workflowBlockedNodeIds,
  workflowCompletedNodeCount,
  workflowContextHistoryEntries,
  workflowDerivedRunStatus,
  workflowEventAt,
  workflowEventBlockers,
  workflowEventRunId,
  workflowEventSessionId,
  workflowEventType,
  workflowPendingNodeCount,
  workflowPersistedHistoryEntries,
  workflowNodeOutput,
  workflowNodeToolTelemetry,
  workflowProjectionFromRunSnapshot,
  workflowRecentNodeEventSummaries,
  workflowRunWasStalePaused,
  workflowSessionIds,
  workflowSessionLogEventEntries,
  workflowTaskInspectionDetails,
  workflowTelemetryDisplayEntries,
  workflowTelemetrySeedEvents,
  workflowBlockedNodeCount,
  workflowNeedsRepairNodeIds,
  workflowTotalNodeCount,
} from "../orchestration/workflowStability";
import { useEngineStream } from "../stream/useEngineStream";
import { MyAutomationsContent } from "./MyAutomationsContent";
import { useBufferedAppender } from "./useBufferedAppender";
import { useSelectedRunLifecycle } from "./useSelectedRunLifecycle";
import { buildPlannerProviderOptions } from "../planner/plannerShared";
import {
  executionProfileLabel,
  workflowEffectiveExecutionProfile,
  workflowRequestedExecutionProfile,
} from "./AutomationsRunHelpers";

export function MyAutomationsContainer({
  client,
  toast,
  navigate,
  viewMode,
  selectedRunId,
  onSelectRunId,
  onOpenRunningView,
  onOpenAdvancedEdit,
  onRecreateWorkflowAutomation,
  defaultRunningSectionsOpen,
  helperFns,
  automationWizardConfig,
}: any) {
  const {
    toArray,
    normalizeMcpServers,
    validateModelInput,
    validatePlannerModelInput,
    validateWorkspaceRootInput,
    workflowEditToOperatorPreferences,
    compileWorkflowModelPolicy,
    cloneJsonValue,
    compileWorkflowToolAllowlist,
    parseConnectorBindingsJson,
    workflowNodeModelPolicyWithOverride,
    deriveConnectorBindingResolutionFromPlanPackage,
    workflowAutomationToEditDraft,
    isMissionBlueprintAutomation,
    workflowEditToSchedule,
    buildCalendarOccurrences,
    normalizeTimestamp,
    workflowQueueReason,
    detectWorkflowActiveTaskId,
    detectWorkflowActiveTaskIds,
    workflowDescendantTaskIds,
    deriveRunDebugHints,
    explainRunFailure,
    buildRunBlockers,
    isStandupAutomation,
    getAutomationCalendarFamily,
    rewriteCronForDroppedStart,
    statusColor,
    formatScheduleLabel,
    formatAutomationV2ScheduleLabel,
    workflowStatusDisplay,
    workflowStatusSubtleDetail,
    runDisplayTitle,
    formatRunDateTime,
    runObjectiveText,
    shortText,
    runTimeLabel,
    compactIdentifier,
    sessionLabel,
    formatTimestampLabel,
    isActiveRunStatus,
    scheduleToEditor,
    uniqueStrings,
    collectPathStrings,
    timestampOrNull,
    sessionMessageId,
    sessionMessageCreatedAt,
    sessionMessageVariant,
    sessionMessageText,
    sessionMessageParts,
  } = helperFns;

  const queryClient = useQueryClient();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState<{
    automationId: string;
    family: "legacy" | "v2";
    title: string;
  } | null>(null);
  const [editDraft, setEditDraft] = useState<{
    automationId: string;
    name: string;
    objective: string;
    mode: "standalone" | "orchestrated";
    requiresApproval: boolean;
    scheduleKind: "cron" | "interval";
    cronExpression: string;
    intervalSeconds: string;
  } | null>(null);
  const [selectedLogSource, setSelectedLogSource] = useState<
    "all" | "automations" | "context" | "global"
  >("all");
  const [runEvents, setRunEvents] = useState<
    Array<{ id: string; source: "automations" | "context" | "global"; at: number; event: any }>
  >([]);
  const [selectedSessionId, setSelectedSessionId] = useState<string>("");
  const [selectedSessionFilterId, setSelectedSessionFilterId] = useState<string>("all");
  const [selectedBoardTaskId, setSelectedBoardTaskId] = useState<string>("");
  const [selectedRunArtifactKey, setSelectedRunArtifactKey] = useState<string>("");
  const [sessionEvents, setSessionEvents] = useState<Array<{ id: string; at: number; event: any }>>(
    []
  );
  const boardDetailRef = useRef<HTMLDivElement | null>(null);
  const artifactsSectionRef = useRef<HTMLDivElement | null>(null);
  const sessionLogRef = useRef<HTMLDivElement | null>(null);
  const [sessionLogPinnedToBottom, setSessionLogPinnedToBottom] = useState(false);
  const [workflowEditDraft, setWorkflowEditDraft] = useState<any | null>(null);
  const [calendarRange, setCalendarRange] = useState(() => {
    const now = new Date();
    const utcDay = now.getUTCDay();
    const start = new Date(
      Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate() - utcDay, 0, 0, 0, 0)
    );
    return {
      startMs: start.getTime(),
      endMs: start.getTime() + 7 * 24 * 60 * 60 * 1000,
    };
  });
  const isWorkflowRun = selectedRunId.startsWith("automation-v2-run-");
  const runInspectorActive = viewMode === "running" && !!selectedRunId;

  const automationsQuery = useQuery({
    queryKey: ["automations", "list"],
    queryFn: () =>
      client?.automations?.list?.().catch(() => ({ automations: [] })) ??
      Promise.resolve({ automations: [] }),
    refetchInterval: 20000,
  });
  const automationsV2Query = useQuery({
    queryKey: ["automations", "v2", "list"],
    queryFn: () =>
      client?.automationsV2?.list?.().catch(() => ({ automations: [] })) ??
      Promise.resolve({ automations: [] }),
    refetchInterval: 20000,
  });
  const automationsV2 = useMemo(() => {
    const rows = toArray(automationsV2Query.data, "automations");
    const byId = new Map<string, any>();
    for (const row of rows) {
      const id = String(row?.automation_id || row?.automationId || row?.id || "").trim();
      if (!id) continue;
      if (!byId.has(id)) byId.set(id, row);
    }
    return Array.from(byId.values());
  }, [automationsV2Query.data, toArray]);
  const overlapHistoryEntries = useMemo(() => {
    const rows: Array<Record<string, any>> = [];
    for (const automation of automationsV2) {
      const automationId = String(
        automation?.automation_id || automation?.automationId || automation?.id || ""
      ).trim();
      const automationName = String(automation?.name || "").trim();
      const planPackage = automation?.metadata?.plan_package || automation?.metadata?.planPackage;
      const overlapLog = toArray(planPackage?.overlap_policy, "overlap_log");
      const sourcePlanId = String(
        planPackage?.plan_id || planPackage?.planId || automationId || ""
      ).trim();
      const sourcePlanRevision = Number(
        planPackage?.plan_revision || planPackage?.planRevision || 0
      );
      const sourceLifecycleState = String(
        planPackage?.lifecycle_state || planPackage?.lifecycleState || automation?.status || ""
      ).trim();
      for (const entry of overlapLog) {
        rows.push({
          rowKey: [
            automationId || sourcePlanId || "automation",
            String(entry?.matched_plan_id || entry?.matchedPlanId || ""),
            String(entry?.matched_plan_revision || entry?.matchedPlanRevision || ""),
            String(entry?.decision || ""),
            String(entry?.decided_at || entry?.decidedAt || ""),
          ].join(":"),
          sourceLabel: automationName || automationId || sourcePlanId || "workflow plan",
          sourceAutomationId: automationId,
          sourcePlanId,
          sourcePlanRevision: Number.isFinite(sourcePlanRevision) ? sourcePlanRevision : 0,
          sourceLifecycleState,
          matchedPlanId: String(entry?.matched_plan_id || entry?.matchedPlanId || "").trim(),
          matchedPlanRevision: Number(
            entry?.matched_plan_revision || entry?.matchedPlanRevision || 0
          ),
          matchLayer: String(entry?.match_layer || entry?.matchLayer || "").trim(),
          similarityScore: entry?.similarity_score ?? entry?.similarityScore ?? null,
          decision: String(entry?.decision || "").trim(),
          decidedBy: String(entry?.decided_by || entry?.decidedBy || "").trim(),
          decidedAt: String(entry?.decided_at || entry?.decidedAt || "").trim(),
        });
      }
    }
    return rows.sort((left, right) => {
      const leftAt = Number(Date.parse(String(left.decidedAt || "")));
      const rightAt = Number(Date.parse(String(right.decidedAt || "")));
      if (Number.isFinite(leftAt) && Number.isFinite(rightAt) && leftAt !== rightAt) {
        return rightAt - leftAt;
      }
      return String(left.sourcePlanId || left.sourceAutomationId || left.rowKey).localeCompare(
        String(right.sourcePlanId || right.sourceAutomationId || right.rowKey)
      );
    });
  }, [automationsV2, toArray]);
  const providerCatalogQuery = useQuery({
    queryKey: ["providers", "catalog", "workflow-edit"],
    queryFn: () =>
      client?.providers?.catalog?.().catch(() => ({ all: [] })) ?? Promise.resolve({ all: [] }),
    refetchInterval: 30000,
  });
  const providersConfigQuery = useQuery({
    queryKey: ["providers", "config", "workflow-edit"],
    queryFn: () =>
      client?.providers?.config?.().catch(() => ({ providers: {} })) ??
      Promise.resolve({ providers: {} }),
    refetchInterval: 30000,
  });
  const mcpServersQuery = useQuery({
    queryKey: ["mcp", "servers", "workflow-edit"],
    queryFn: () =>
      client?.mcp?.list?.().catch(() => ({ servers: [] })) ?? Promise.resolve({ servers: [] }),
    refetchInterval: 15000,
  });
  const runsQuery = useQuery({
    queryKey: ["automations", "runs"],
    queryFn: () =>
      client?.automations?.listRuns?.({ limit: 20 }).catch(() => ({ runs: [] })) ??
      Promise.resolve({ runs: [] }),
    refetchInterval: 9000,
  });
  const workflowRunsQuery = useQuery({
    queryKey: ["automations", "v2", "runs", "all"],
    queryFn: () =>
      api("/api/engine/automations/v2/runs?limit=40").catch(() => ({ runs: [] as any[] })),
    refetchInterval: 9000,
  });
  const runDetailQuery = useQuery({
    queryKey: ["automations", "run", selectedRunId],
    enabled: runInspectorActive,
    queryFn: () =>
      (isWorkflowRun
        ? client?.automationsV2?.getRun?.(selectedRunId)
        : client?.automations?.getRun?.(selectedRunId)
      )?.catch(() => ({ run: null })) ?? Promise.resolve({ run: null }),
    refetchInterval: runInspectorActive ? 5000 : false,
  });
  const runArtifactsQuery = useQuery({
    queryKey: ["automations", "run", "artifacts", selectedRunId],
    enabled: runInspectorActive && !isWorkflowRun,
    queryFn: () =>
      client?.automations?.listArtifacts?.(selectedRunId).catch(() => ({ artifacts: [] })),
    refetchInterval: runInspectorActive ? 8000 : false,
  });
  const taskResetPreviewQuery = useQuery({
    queryKey: ["automations", "run", "task-reset-preview", selectedRunId, selectedBoardTaskId],
    enabled:
      runInspectorActive &&
      isWorkflowRun &&
      String(selectedBoardTaskId || "").startsWith("node-") &&
      !!String(selectedBoardTaskId || "").trim() &&
      !!client?.automationsV2?.previewTaskReset,
    queryFn: () =>
      client?.automationsV2
        ?.previewTaskReset(
          selectedRunId,
          String(selectedBoardTaskId || "")
            .replace(/^node-/, "")
            .trim()
        )
        .catch(() => ({ preview: null })) ?? Promise.resolve({ preview: null }),
    refetchInterval: false,
  });
  const availableSessionIds = useMemo(
    () => workflowSessionIds((runDetailQuery.data as any)?.run),
    [runDetailQuery.data]
  );
  const sessionMessageQueries = useQueries({
    queries: availableSessionIds.map((sessionId) => ({
      queryKey: ["automations", "run", "session", selectedRunId, sessionId, "messages"],
      enabled: runInspectorActive && !!sessionId,
      queryFn: () => client?.sessions?.messages?.(sessionId).catch(() => []) ?? Promise.resolve([]),
      refetchInterval:
        runInspectorActive &&
        sessionId &&
        isActiveRunStatus((runDetailQuery.data as any)?.run?.status)
          ? 4000
          : false,
    })),
  });
  const selectedAutomationId = String(
    (runDetailQuery.data as any)?.run?.automation_id ||
      (runDetailQuery.data as any)?.run?.routine_id ||
      ""
  ).trim();
  const selectedContextRunId = String(
    (runInspectorActive ? (runDetailQuery.data as any)?.contextRunID : "") ||
      (runInspectorActive && isWorkflowRun && selectedRunId ? `automation-v2-${selectedRunId}` : "")
  ).trim();
  const runHistoryQuery = useQuery({
    queryKey: ["automations", "history", selectedAutomationId],
    enabled: runInspectorActive && !!selectedAutomationId && !isWorkflowRun,
    queryFn: () =>
      client?.automations?.history?.(selectedAutomationId, 80).catch(() => ({ events: [] })),
    refetchInterval: runInspectorActive ? 10000 : false,
  });
  const persistedRunEventsQuery = useQuery({
    queryKey: ["automations", "run", "events", selectedRunId],
    enabled: runInspectorActive && !!client?.runEvents,
    queryFn: () => client.runEvents(selectedRunId, { tail: 400 }).catch(() => []),
    refetchInterval:
      runInspectorActive && isActiveRunStatus((runDetailQuery.data as any)?.run?.status)
        ? 5000
        : false,
  });
  // Polling intervals are intentionally slow safety nets — the live data path
  // is the SSE event stream, which calls queueContextInvalidation() to refresh
  // these queries on demand. The blackboard payload in particular can be 1+ MB,
  // so we no longer poll it at all and rely entirely on event-driven refetch.
  const contextRunPollMs =
    selectedContextRunId && isActiveRunStatus((runDetailQuery.data as any)?.run?.status)
      ? 30000
      : false;
  const workflowContextRunQuery = useQuery({
    queryKey: ["automations", "run", "context", selectedContextRunId],
    enabled: runInspectorActive && !!selectedContextRunId,
    queryFn: () =>
      api(`/api/engine/context/runs/${encodeURIComponent(selectedContextRunId)}`).catch(() => ({
        run: null,
      })),
    refetchInterval: contextRunPollMs,
  });
  const workflowContextBlackboardQuery = useQuery({
    queryKey: ["automations", "run", "context", selectedContextRunId, "blackboard"],
    enabled: runInspectorActive && !!selectedContextRunId,
    queryFn: () =>
      api(`/api/engine/context/runs/${encodeURIComponent(selectedContextRunId)}/blackboard`).catch(
        () => ({
          blackboard: null,
        })
      ),
    refetchInterval: false,
  });
  const workflowContextEventsQuery = useQuery({
    queryKey: ["automations", "run", "context", selectedContextRunId, "events"],
    enabled: runInspectorActive && !!selectedContextRunId,
    queryFn: () =>
      api(`/api/engine/context/runs/${encodeURIComponent(selectedContextRunId)}/events`).catch(
        () => ({ events: [] })
      ),
    refetchInterval: contextRunPollMs,
  });
  const workflowContextPatchesQuery = useQuery({
    queryKey: ["automations", "run", "context", selectedContextRunId, "patches"],
    enabled: runInspectorActive && !!selectedContextRunId,
    queryFn: () =>
      api(
        `/api/engine/context/runs/${encodeURIComponent(selectedContextRunId)}/blackboard/patches`
      ).catch(() => ({ patches: [] })),
    refetchInterval: contextRunPollMs,
  });
  const packsQuery = useQuery({
    queryKey: ["automations", "packs"],
    queryFn: () =>
      client?.packs?.list?.().catch(() => ({ packs: [] })) ?? Promise.resolve({ packs: [] }),
    refetchInterval: 30000,
  });

  const runNowMutation = useMutation({
    mutationFn: (id: string) => client?.automations?.runNow?.(id),
    onSuccess: async () => {
      toast("ok", "Automation triggered.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const runNowV2Mutation = useMutation({
    mutationFn: async ({
      id,
      executionProfile,
    }: {
      id: string;
      executionProfile?: "strict" | "guided" | "yolo";
    }) => {
      if (!client?.automationsV2?.runNow) {
        throw new Error("Workflow run now is not available in this client.");
      }
      return client.automationsV2.runNow(id, executionProfile ? { executionProfile } : undefined);
    },
    onSuccess: async (payload: any) => {
      const runId = String(payload?.run?.run_id || payload?.run?.runId || "").trim();
      toast("ok", "Workflow automation triggered.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const parseEngineError = (err: unknown): { code: string; message: string } => {
    const raw = err instanceof Error ? err.message : String(err ?? "");
    const jsonStart = raw.indexOf("{");
    if (jsonStart >= 0) {
      try {
        const body = JSON.parse(raw.slice(jsonStart));
        return {
          code: String(body?.code || "").trim(),
          message: String(body?.error || body?.message || "").trim(),
        };
      } catch {
        // fall through
      }
    }
    return { code: "", message: raw };
  };

  const friendlyEngineError = (err: unknown, fallback: string): string => {
    const { code, message } = parseEngineError(err);
    if (
      code === "AUTOMATION_V2_RUN_TASK_NOT_MUTABLE" ||
      code === "AUTOMATION_V2_RUN_NOT_RECOVERABLE"
    ) {
      return "Run is still active. Pause or cancel it first, then retry.";
    }
    return message || fallback;
  };

  const isRunNotMutableError = (err: unknown): boolean => {
    const { code } = parseEngineError(err);
    return (
      code === "AUTOMATION_V2_RUN_TASK_NOT_MUTABLE" || code === "AUTOMATION_V2_RUN_NOT_RECOVERABLE"
    );
  };

  const withAutoPauseRetry = async <T,>(runId: string, action: () => Promise<T>): Promise<T> => {
    try {
      return await action();
    } catch (err) {
      if (!isRunNotMutableError(err) || !client?.automationsV2?.pauseRun) {
        throw err;
      }
      try {
        await client.automationsV2.pauseRun(runId, "auto-pause for retry");
      } catch {
        // ignore — engine may already be in a state pause cannot apply to
      }
      await new Promise((resolve) => setTimeout(resolve, 400));
      return await action();
    }
  };

  const runActionMutation = useMutation({
    mutationFn: async ({
      action,
      runId,
      family,
      reason,
    }: {
      action: "pause" | "resume" | "cancel";
      runId: string;
      family: "legacy" | "v2";
      reason?: string;
    }) => {
      if (family === "v2") {
        if (action === "cancel") return client.automationsV2.cancelRun(runId, reason);
        if (action === "pause") return client.automationsV2.pauseRun(runId, reason);
        return client.automationsV2.resumeRun(runId, reason);
      }
      if (action === "cancel") {
        throw new Error("Cancel is only available for workflow runs in this client.");
      }
      if (action === "pause") return client.automations.pauseRun(runId, reason);
      return client.automations.resumeRun(runId, reason);
    },
    onSuccess: async (_payload, vars) => {
      if (vars.action === "cancel") {
        toast("ok", "Run cancelled.");
      } else {
        toast("ok", "Run action applied.");
      }
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const workflowRepairMutation = useMutation({
    mutationFn: async ({
      runId,
      nodeId,
      reason,
    }: {
      runId: string;
      nodeId: string;
      reason?: string;
    }) => {
      if (!client?.automationsV2?.repairRun) {
        throw new Error("Workflow repair is not available in this client.");
      }
      return withAutoPauseRetry(runId, () =>
        client.automationsV2.repairRun(runId, {
          node_id: nodeId,
          reason: reason ?? "",
        })
      );
    },
    onSuccess: async (payload: any) => {
      const runId = String(
        payload?.run?.run_id || payload?.run?.runId || selectedRunId || ""
      ).trim();
      toast("ok", "Workflow continued from blocked step.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", friendlyEngineError(error, "Workflow repair failed.")),
  });
  const workflowRecoverMutation = useMutation({
    mutationFn: async ({ runId, reason }: { runId: string; reason?: string }) => {
      if (!client?.automationsV2?.recoverRun) {
        throw new Error("Workflow retry is not available in this client.");
      }
      return withAutoPauseRetry(runId, () => client.automationsV2.recoverRun(runId, reason));
    },
    onSuccess: async (payload: any) => {
      const runId = String(
        payload?.run?.run_id || payload?.run?.runId || selectedRunId || ""
      ).trim();
      toast("ok", "Workflow run queued again.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", friendlyEngineError(error, "Workflow retry failed.")),
  });
  const workflowTaskRetryMutation = useMutation({
    mutationFn: async ({
      runId,
      nodeId,
      reason,
    }: {
      runId: string;
      nodeId: string;
      reason?: string;
    }) => {
      if (!client?.automationsV2?.retryTask) {
        throw new Error("Task retry is not available in this client.");
      }
      return withAutoPauseRetry(runId, () => client.automationsV2.retryTask(runId, nodeId, reason));
    },
    onSuccess: async (payload: any) => {
      const runId = String(
        payload?.run?.run_id || payload?.run?.runId || selectedRunId || ""
      ).trim();
      toast("ok", "Task retried and subtree requeued.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", friendlyEngineError(error, "Task retry failed.")),
  });
  const workflowTaskContinueMutation = useMutation({
    mutationFn: async ({
      runId,
      nodeId,
      reason,
    }: {
      runId: string;
      nodeId: string;
      reason?: string;
    }) => {
      if (!client?.automationsV2?.continueTask) {
        throw new Error("Task continue is not available in this client.");
      }
      return withAutoPauseRetry(runId, () =>
        client.automationsV2.continueTask(runId, nodeId, reason)
      );
    },
    onSuccess: async (payload: any) => {
      const runId = String(
        payload?.run?.run_id || payload?.run?.runId || selectedRunId || ""
      ).trim();
      toast("ok", "Blocked task continued with minimal reset.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", friendlyEngineError(error, "Task continue failed.")),
  });
  const workflowTaskRequeueMutation = useMutation({
    mutationFn: async ({
      runId,
      nodeId,
      reason,
    }: {
      runId: string;
      nodeId: string;
      reason?: string;
    }) => {
      if (!client?.automationsV2?.requeueTask) {
        throw new Error("Task requeue is not available in this client.");
      }
      return withAutoPauseRetry(runId, () =>
        client.automationsV2.requeueTask(runId, nodeId, reason)
      );
    },
    onSuccess: async (payload: any) => {
      const runId = String(
        payload?.run?.run_id || payload?.run?.runId || selectedRunId || ""
      ).trim();
      toast("ok", "Task requeued and subtree reset.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", friendlyEngineError(error, "Task requeue failed.")),
  });
  const workflowTaskDispositionMutation = useMutation({
    mutationFn: async ({
      runId,
      nodeId,
      disposition,
      reason,
    }: {
      runId: string;
      nodeId: string;
      disposition: "unmarked" | "accepted" | "rejected" | "re_ran_strict";
      reason?: string;
    }) => {
      if (!client?.automationsV2?.setTaskDisposition) {
        throw new Error("Task disposition is not available in this client.");
      }
      return client.automationsV2.setTaskDisposition(runId, nodeId, disposition, reason);
    },
    onSuccess: async (payload: any) => {
      const runId = String(
        payload?.run?.run_id || payload?.run?.runId || selectedRunId || ""
      ).trim();
      const disposition = String(payload?.disposition || "");
      const changed = payload?.changed === true;
      toast(
        "ok",
        changed
          ? `Marked artifact as ${disposition.replace(/_/g, " ")}.`
          : `Already marked ${disposition.replace(/_/g, " ")}.`
      );
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
      }
    },
    onError: (error) => toast("err", friendlyEngineError(error, "Disposition update failed.")),
  });
  const backlogTaskClaimMutation = useMutation({
    mutationFn: async ({
      runId,
      taskId,
      agentId,
      reason,
    }: {
      runId: string;
      taskId: string;
      agentId?: string;
      reason?: string;
    }) => {
      if (!client?.automationsV2?.claimBacklogTask) {
        throw new Error("Backlog task claim is not available in this client.");
      }
      return client.automationsV2.claimBacklogTask(runId, taskId, {
        agent_id: agentId,
        reason,
      });
    },
    onSuccess: async () => {
      toast("ok", "Backlog task claimed.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (selectedRunId) {
        onSelectRunId(selectedRunId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const backlogTaskRequeueMutation = useMutation({
    mutationFn: async ({
      runId,
      taskId,
      reason,
    }: {
      runId: string;
      taskId: string;
      reason?: string;
    }) => {
      if (!client?.automationsV2?.requeueBacklogTask) {
        throw new Error("Backlog task requeue is not available in this client.");
      }
      return client.automationsV2.requeueBacklogTask(runId, taskId, reason);
    },
    onSuccess: async () => {
      toast("ok", "Backlog task requeued.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (selectedRunId) {
        onSelectRunId(selectedRunId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  useEffect(() => {
    runActionMutation.reset();
    workflowRepairMutation.reset();
    workflowRecoverMutation.reset();
    workflowTaskRetryMutation.reset();
    workflowTaskContinueMutation.reset();
    workflowTaskRequeueMutation.reset();
    workflowTaskDispositionMutation.reset();
    backlogTaskClaimMutation.reset();
    backlogTaskRequeueMutation.reset();
  }, [
    backlogTaskClaimMutation,
    backlogTaskRequeueMutation,
    runActionMutation,
    selectedBoardTaskId,
    selectedRunId,
    workflowRecoverMutation,
    workflowRepairMutation,
    workflowTaskContinueMutation,
    workflowTaskDispositionMutation,
    workflowTaskRequeueMutation,
    workflowTaskRetryMutation,
  ]);

  const updateAutomationMutation = useMutation({
    mutationFn: async (draft: any) => {
      const name = String(draft.name || "").trim();
      const objective = String(draft.objective || "").trim();
      const cronExpression = String(draft.cronExpression || "").trim();
      const intervalSeconds = Number(draft.intervalSeconds);
      if (!name) throw new Error("Automation name is required.");
      if (!objective) throw new Error("Objective is required.");
      if (draft.scheduleKind === "cron" && !cronExpression) {
        throw new Error("Cron expression is required.");
      }
      if (
        draft.scheduleKind === "interval" &&
        (!Number.isFinite(intervalSeconds) || intervalSeconds <= 0)
      ) {
        throw new Error("Interval seconds must be greater than zero.");
      }
      return client.automations.update(draft.automationId, {
        name,
        mode: draft.mode,
        mission: { objective },
        policy: { approval: { requires_approval: !!draft.requiresApproval } },
        schedule:
          draft.scheduleKind === "cron"
            ? { cron: { expression: cronExpression } }
            : { interval_seconds: { seconds: Math.round(intervalSeconds) } },
      });
    },
    onSuccess: async () => {
      toast("ok", "Automation updated.");
      setEditDraft(null);
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const updateWorkflowAutomationMutation = useMutation({
    mutationFn: async (draft: any) => {
      const name = String(draft.name || "").trim();
      const description = String(draft.description || "").trim();
      const workspaceRoot = String(draft.workspaceRoot || "").trim();
      const modelError = validateModelInput(draft.modelProvider, draft.modelId);
      const plannerModelError = validatePlannerModelInput(
        draft.plannerModelProvider,
        draft.plannerModelId
      );
      const workspaceError = validateWorkspaceRootInput(workspaceRoot);
      if (!name) throw new Error("Automation name is required.");
      if (workspaceError) throw new Error(workspaceError);
      if (modelError) throw new Error(modelError);
      if (plannerModelError) throw new Error(plannerModelError);
      if (draft.scheduleKind === "cron" && !String(draft.cronExpression || "").trim()) {
        throw new Error("Cron expression is required.");
      }
      if (
        draft.scheduleKind === "interval" &&
        (!Number.isFinite(Number(draft.intervalSeconds)) || Number(draft.intervalSeconds) <= 0)
      ) {
        throw new Error("Interval seconds must be greater than zero.");
      }
      const operatorPreferences = workflowEditToOperatorPreferences(draft);
      const modelPolicy = compileWorkflowModelPolicy(operatorPreferences);
      const baseModelPolicy = modelPolicy
        ? (cloneJsonValue(modelPolicy) as Record<string, any>)
        : null;
      const selectedMcpServers = draft.selectedMcpServers
        .map((row: any) => String(row || "").trim())
        .filter(Boolean);
      const toolAllowlist = compileWorkflowToolAllowlist(
        selectedMcpServers,
        draft.toolAccessMode,
        draft.customToolsText,
        draft.selectedMcpTools,
        draft.mcpOtherAllowedTools
      );
      const connectorBindings = parseConnectorBindingsJson(draft.connectorBindingsJson);
      const sharedContextPackIds = uniqueStrings(
        String(draft.sharedContextPackIdsText || "")
          .split(/[\n,]/g)
          .map((value: string) => String(value || "").trim())
          .filter(Boolean)
      );
      const sharedContextBindings = sharedContextPackIds.map((packId: string) => ({
        pack_id: packId,
        required: true,
      }));
      const stepModelPolicies = new Map<string, Record<string, any> | null>();
      for (const node of draft.nodes) {
        const nodeAgentId = String(node.agentId || "").trim();
        if (!nodeAgentId) continue;
        const nodeModelProvider = String(node.modelProvider || "").trim();
        const nodeModelId = String(node.modelId || "").trim();
        const nodeModelError = validateModelInput(nodeModelProvider, nodeModelId);
        if (nodeModelError) {
          throw new Error(`${node.title || node.nodeId || nodeAgentId}: ${nodeModelError}`);
        }
        stepModelPolicies.set(
          nodeAgentId,
          workflowNodeModelPolicyWithOverride(baseModelPolicy, nodeModelProvider, nodeModelId)
        );
      }
      const nextScopeSnapshot = draft.scopeSnapshot ? cloneJsonValue(draft.scopeSnapshot) : null;
      if (nextScopeSnapshot && typeof nextScopeSnapshot === "object") {
        nextScopeSnapshot.connector_bindings = connectorBindings;
        nextScopeSnapshot.connector_binding_resolution =
          deriveConnectorBindingResolutionFromPlanPackage(nextScopeSnapshot, connectorBindings);
      }
      const existing = automationsV2.find(
        (row: any) =>
          String(row?.automation_id || row?.automationId || row?.id || "").trim() ===
          draft.automationId
      );
      const agents = Array.isArray(existing?.agents)
        ? existing.agents.map((agent: any) => {
            const agentId = String(agent?.agent_id || agent?.agentId || "").trim();
            const nextModelPolicy = stepModelPolicies.has(agentId)
              ? stepModelPolicies.get(agentId)
              : agent?.model_policy || agent?.modelPolicy || modelPolicy;
            return {
              ...agent,
              model_policy: nextModelPolicy ? cloneJsonValue(nextModelPolicy) : null,
              modelPolicy: undefined,
              tool_policy: {
                ...(agent?.tool_policy || {}),
                allowlist: toolAllowlist,
                denylist: Array.isArray(agent?.tool_policy?.denylist)
                  ? agent.tool_policy.denylist
                  : [],
              },
              mcp_policy: {
                ...(agent?.mcp_policy || {}),
                allowed_servers: selectedMcpServers,
                allowed_tools:
                  draft.toolAccessMode === "all"
                    ? null
                    : Array.isArray(draft.selectedMcpTools)
                      ? [...draft.mcpOtherAllowedTools, ...draft.selectedMcpTools]
                      : draft.mcpOtherAllowedTools,
              },
            };
          })
        : [];
      const flowNodes = Array.isArray(existing?.flow?.nodes)
        ? existing.flow.nodes.map((node: any, index: number) => {
            const nodeId = String(
              node?.node_id || node?.nodeId || node?.id || `node-${index}`
            ).trim();
            const draftNode = draft.nodes.find((row: any) => row.nodeId === nodeId);
            const nodeMcpAllowedTools =
              draftNode?.toolAccessMode === "custom"
                ? Array.isArray(draftNode.mcpAllowedTools)
                  ? [...(draftNode.mcpOtherAllowedTools || []), ...draftNode.mcpAllowedTools]
                  : draftNode.mcpOtherAllowedTools || []
                : [];
            return draftNode
              ? {
                  ...node,
                  objective: String(draftNode.objective || "").trim(),
                  tool_policy:
                    draftNode.toolAccessMode === "custom"
                      ? {
                          allowlist: [
                            ...(draftNode.toolAllowlist || []),
                            ...(draftNode.mcpAllowedTools === null
                              ? (draftNode.mcpAllowedServers || []).map(
                                  (server: string) =>
                                    `mcp.${
                                      String(server || "")
                                        .trim()
                                        .toLowerCase()
                                        .replace(/[^a-z0-9]+/g, "_")
                                        .replace(/^_+|_+$/g, "") || "mcp"
                                    }.*`
                                )
                              : nodeMcpAllowedTools),
                          ]
                            .map((entry: string) => String(entry || "").trim())
                            .filter(Boolean),
                          denylist: (draftNode.toolDenylist || [])
                            .map((entry: string) => String(entry || "").trim())
                            .filter(Boolean),
                        }
                      : undefined,
                  toolPolicy: undefined,
                  mcp_policy:
                    draftNode.toolAccessMode === "custom"
                      ? {
                          allowed_servers: (draftNode.mcpAllowedServers || [])
                            .map((entry: string) => String(entry || "").trim())
                            .filter(Boolean),
                          allowed_tools:
                            draftNode.mcpAllowedTools === null
                              ? null
                              : nodeMcpAllowedTools
                                  .map((entry: string) => String(entry || "").trim())
                                  .filter(Boolean),
                        }
                      : undefined,
                  mcpPolicy: undefined,
                }
              : node;
          })
        : [];
      const existingMetadata =
        existing?.metadata && typeof existing.metadata === "object" ? existing.metadata : {};
      const nextPlanPackage = nextScopeSnapshot
        ? {
            ...(cloneJsonValue(existingMetadata?.plan_package) || {}),
            ...nextScopeSnapshot,
          }
        : existingMetadata?.plan_package;
      const sharedContextProjectKey = String(
        nextScopeSnapshot?.project_key ||
          nextScopeSnapshot?.projectKey ||
          existingMetadata?.shared_context_project_key ||
          existingMetadata?.sharedContextProjectKey ||
          ""
      ).trim();
      if (nextScopeSnapshot && typeof nextScopeSnapshot === "object") {
        nextScopeSnapshot.shared_context_pack_ids = sharedContextPackIds;
        nextScopeSnapshot.shared_context_bindings = sharedContextBindings;
        if (sharedContextProjectKey) {
          nextScopeSnapshot.shared_context_project_key = sharedContextProjectKey;
        }
        nextScopeSnapshot.shared_context_workspace_root = workspaceRoot;
      }
      const nextPlanPackageBundle =
        nextScopeSnapshot && existingMetadata?.plan_package_bundle
          ? {
              ...cloneJsonValue(existingMetadata.plan_package_bundle),
              scope_snapshot: nextScopeSnapshot,
            }
          : existingMetadata?.plan_package_bundle;
      return client.automationsV2.update(draft.automationId, {
        name,
        description: description || null,
        schedule: workflowEditToSchedule(draft),
        workspace_root: workspaceRoot,
        execution: {
          ...(existing?.execution || {}),
          max_parallel_agents:
            draft.executionMode === "single"
              ? 1
              : Math.max(
                  2,
                  Math.min(16, Number.parseInt(String(draft.maxParallelAgents || "4"), 10) || 4)
                ),
          ...(draft.executionProfile === "strict" ||
          draft.executionProfile === "guided" ||
          draft.executionProfile === "yolo"
            ? { profile: draft.executionProfile }
            : { profile: null }),
        },
        flow: existing?.flow
          ? {
              ...existing.flow,
              nodes: flowNodes,
            }
          : existing?.flow,
        agents,
        ...(draft.handoffConfig != null ? { handoff_config: draft.handoffConfig } : {}),
        ...(Array.isArray(draft.watchConditions) && draft.watchConditions.length > 0
          ? { watch_conditions: draft.watchConditions }
          : {}),
        ...(draft.scopePolicy != null ? { scope_policy: draft.scopePolicy } : {}),
        metadata: {
          ...existingMetadata,
          workspace_root: workspaceRoot,
          operator_preferences: operatorPreferences,
          allowed_mcp_servers: selectedMcpServers,
          ...(nextPlanPackage ? { plan_package: nextPlanPackage } : {}),
          ...(nextPlanPackageBundle ? { plan_package: nextPlanPackageBundle } : {}),
          shared_context_pack_ids: sharedContextPackIds,
          shared_context_bindings: sharedContextBindings,
          ...(sharedContextProjectKey
            ? { shared_context_project_key: sharedContextProjectKey }
            : {}),
          shared_context_workspace_root: workspaceRoot,
        },
      });
    },
    onSuccess: async () => {
      toast("ok", "Workflow automation updated.");
      setWorkflowEditDraft(null);
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const automationActionMutation = useMutation({
    mutationFn: async ({
      action,
      automationId,
      family,
    }: {
      action: "pause" | "resume" | "delete";
      automationId: string;
      family: "legacy" | "v2";
    }) => {
      if (family === "v2") {
        if (action === "delete") return client.automationsV2.delete(automationId);
        if (action === "pause") return client.automationsV2.pause(automationId);
        return client.automationsV2.resume(automationId);
      }
      if (action === "delete") return client.automations.delete(automationId);
      return client.automations.update(automationId, {
        status: action === "pause" ? "paused" : "enabled",
      });
    },
    onSuccess: async (_payload, vars) => {
      if (vars.action === "delete") toast("ok", "Automation removed.");
      if (vars.action === "pause") toast("ok", "Automation paused.");
      if (vars.action === "resume") toast("ok", "Automation resumed.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const automations = useMemo(() => {
    const merged = [
      ...toArray(automationsQuery.data, "automations"),
      ...toArray(automationsQuery.data, "routines"),
    ];
    const byId = new Map<string, any>();
    for (const row of merged) {
      const id = String(row?.automation_id || row?.routine_id || row?.id || "").trim();
      if (!id) continue;
      if (!byId.has(id)) byId.set(id, row);
    }
    return Array.from(byId.values());
  }, [automationsQuery.data, toArray]);
  const workflowPreferencesQuery = useQuery({
    queryKey: ["control-panel", "preferences"],
    queryFn: () =>
      api("/api/control-panel/preferences", { method: "GET" }).catch(() => ({
        preferences: {
          favorite_automation_ids: [],
          workflow_library_filters: DEFAULT_WORKFLOW_LIBRARY_FILTERS,
          workflow_sort_mode: DEFAULT_WORKFLOW_SORT_MODE,
        },
      })),
    retry: false,
    staleTime: 60_000,
    refetchInterval: 60_000,
  });
  const workflowPreferences = (workflowPreferencesQuery.data as any)?.preferences || {};
  const workflowSortMode = normalizeWorkflowSortMode(
    workflowPreferences.workflow_sort_mode || DEFAULT_WORKFLOW_SORT_MODE
  );
  const workflowLibraryFilters = useMemo(
    () =>
      normalizeWorkflowLibraryFilters(
        workflowPreferences.workflow_library_filters ||
          workflowPreferences.workflowLibraryFilters ||
          {}
      ),
    [workflowPreferences.workflow_library_filters, workflowPreferences.workflowLibraryFilters]
  );
  const workflowLibraryFilteringActive = useMemo(
    () => !workflowLibraryFiltersEqual(workflowLibraryFilters, DEFAULT_WORKFLOW_LIBRARY_FILTERS),
    [workflowLibraryFilters]
  );
  const favoriteAutomationIds = useMemo(
    () => normalizeFavoriteAutomationIds(workflowPreferences.favorite_automation_ids || []),
    [workflowPreferences.favorite_automation_ids]
  );
  const favoriteAutomationIdSet = useMemo(
    () => new Set(favoriteAutomationIds),
    [favoriteAutomationIds]
  );
  const updateWorkflowPreferencesMutation = useMutation({
    mutationFn: async (patch: {
      favorite_automation_ids?: string[];
      workflow_library_filters?: any;
      workflow_sort_mode?: string;
    }) =>
      api("/api/control-panel/preferences", {
        method: "PATCH",
        body: JSON.stringify({ preferences: patch }),
      }),
    onMutate: async (patch) => {
      await queryClient.cancelQueries({ queryKey: ["control-panel", "preferences"] });
      const previous = queryClient.getQueryData(["control-panel", "preferences"]);
      queryClient.setQueryData(["control-panel", "preferences"], (current: any) => ({
        ...(current || {}),
        ok: true,
        preferences: {
          ...(current?.preferences || {}),
          ...patch,
        },
      }));
      return { previous };
    },
    onError: (_error, _patch, context) => {
      if (context?.previous !== undefined) {
        queryClient.setQueryData(["control-panel", "preferences"], context.previous);
      }
    },
    onSuccess: (payload) => {
      queryClient.setQueryData(["control-panel", "preferences"], payload);
    },
  });
  const setWorkflowSortMode = (nextSortMode: string) => {
    updateWorkflowPreferencesMutation.mutate({
      workflow_sort_mode: normalizeWorkflowSortMode(nextSortMode),
      favorite_automation_ids: favoriteAutomationIds,
      workflow_library_filters: workflowLibraryFilters,
    });
  };
  const setWorkflowLibraryFilters = (nextFilters: any) => {
    updateWorkflowPreferencesMutation.mutate({
      workflow_library_filters: normalizeWorkflowLibraryFilters(nextFilters),
      workflow_sort_mode: workflowSortMode,
      favorite_automation_ids: favoriteAutomationIds,
    });
  };
  const toggleWorkflowLibrarySourceFilter = (sourceKey: string) => {
    const key = String(sourceKey || "").trim();
    if (!key) return;
    setWorkflowLibraryFilters({
      ...workflowLibraryFilters,
      sources: {
        ...workflowLibraryFilters.sources,
        [key]: !workflowLibraryFilters.sources?.[key],
      },
    });
  };
  const toggleWorkflowLibraryStatusFilter = (statusKey: string) => {
    const key = String(statusKey || "").trim();
    if (!key) return;
    setWorkflowLibraryFilters({
      ...workflowLibraryFilters,
      statuses: {
        ...workflowLibraryFilters.statuses,
        [key]: !workflowLibraryFilters.statuses?.[key],
      },
    });
  };
  const resetWorkflowLibraryFilters = () => {
    setWorkflowLibraryFilters(DEFAULT_WORKFLOW_LIBRARY_FILTERS);
  };
  const toggleWorkflowFavorite = (automationId: string) => {
    const nextFavoriteIds = toggleFavoriteAutomationId(favoriteAutomationIds, automationId);
    updateWorkflowPreferencesMutation.mutate({
      favorite_automation_ids: nextFavoriteIds,
      workflow_sort_mode: workflowSortMode,
      workflow_library_filters: workflowLibraryFilters,
    });
  };
  const classifyWorkflowAutomation = useMemo(
    () => (automation: any) => {
      if (isStandupAutomation(automation)) {
        return { key: "standup", label: "Standup" };
      }
      if (isMissionBlueprintAutomation(automation)) {
        return { key: "mission_blueprint", label: "Mission Blueprint" };
      }
      if (automation?.schedule) {
        return { key: "scheduled", label: "Scheduled" };
      }
      if (
        String(automation?.mode || "")
          .trim()
          .toLowerCase() === "standalone"
      ) {
        return { key: "manual", label: "Manual" };
      }
      return { key: "other", label: "Other" };
    },
    [isMissionBlueprintAutomation, isStandupAutomation]
  );
  const workflowAutomationRows = useMemo(() => {
    const visibleAutomations = filterWorkflowAutomations(automationsV2, workflowLibraryFilters);
    return sortWorkflowAutomations(visibleAutomations, {
      sortMode: workflowSortMode,
      favoriteAutomationIds: favoriteAutomationIdSet,
    }).map((automation: any) => {
      const id = getAutomationId(automation);
      const category = classifyWorkflowAutomation(automation);
      const source = classifyAutomationSource(automation);
      return {
        automation,
        id,
        name: getAutomationName(automation),
        createdAtMs: getAutomationCreatedAtMs(automation),
        isFavorite: favoriteAutomationIdSet.has(id),
        status: String(automation?.status || "draft").trim(),
        paused:
          String(automation?.status || "draft")
            .trim()
            .toLowerCase() === "paused",
        categoryKey: category.key,
        categoryLabel: category.label,
        sourceKey: source.key,
        sourceLabel: source.label,
      };
    });
  }, [
    automationsV2,
    classifyWorkflowAutomation,
    favoriteAutomationIdSet,
    workflowLibraryFilters,
    workflowSortMode,
  ]);
  const workflowAutomationSections = useMemo(() => {
    const categoryOrder = [
      { key: "standup", label: "Standup" },
      { key: "mission_blueprint", label: "Mission Blueprint" },
      { key: "scheduled", label: "Scheduled" },
      { key: "manual", label: "Manual" },
      { key: "other", label: "Other" },
    ];
    const favorites = workflowAutomationRows.filter((row: any) => row.isFavorite);
    const sections: Array<{
      key: string;
      label: string;
      description: string;
      count: number;
      rows: Array<any>;
    }> = [];
    if (favorites.length > 0) {
      sections.push({
        key: "favorites",
        label: "Favorites",
        description: "Pinned here for this profile.",
        count: favorites.length,
        rows: favorites,
      });
    }
    const remaining = workflowAutomationRows.filter((row: any) => !row.isFavorite);
    for (const category of categoryOrder) {
      const rows = remaining.filter((row: any) => row.categoryKey === category.key);
      if (!rows.length) continue;
      sections.push({
        key: category.key,
        label: category.label,
        description:
          category.key === "standup"
            ? "Standup and daily workflow automations."
            : category.key === "mission_blueprint"
              ? "Blueprint-style workflow automations."
              : category.key === "scheduled"
                ? "Automations driven by schedules or recurring triggers."
                : category.key === "manual"
                  ? "Automations that are usually started by hand."
                  : "Workflow automations that do not fit the other groups yet.",
        count: rows.length,
        rows,
      });
    }
    return sections;
  }, [workflowAutomationRows]);
  const legacyAutomationRows = useMemo(() => {
    return sortWorkflowAutomations(automations, {
      sortMode: workflowSortMode,
      favoriteAutomationIds: favoriteAutomationIdSet,
    }).map((automation: any) => {
      const id = String(
        automation?.automation_id || automation?.id || automation?.routine_id || ""
      ).trim();
      return {
        automation,
        id,
        name: getAutomationName(automation),
        createdAtMs: getAutomationCreatedAtMs(automation),
        isFavorite: favoriteAutomationIdSet.has(id),
        status: String(automation?.status || "active").trim(),
      };
    });
  }, [automations, favoriteAutomationIdSet, workflowSortMode]);
  const workflowPreferencesLoading =
    workflowPreferencesQuery.isLoading || updateWorkflowPreferencesMutation.isPending;
  const calendarEvents = useMemo(() => {
    const legacyEvents = automations.flatMap((automation: any) =>
      buildCalendarOccurrences({
        automation,
        family: "legacy",
        rangeStartMs: calendarRange.startMs,
        rangeEndMs: calendarRange.endMs,
      })
    );
    const workflowEvents = automationsV2.flatMap((automation: any) =>
      buildCalendarOccurrences({
        automation,
        family: "v2",
        rangeStartMs: calendarRange.startMs,
        rangeEndMs: calendarRange.endMs,
      })
    );
    return [...legacyEvents, ...workflowEvents];
  }, [
    automations,
    automationsV2,
    buildCalendarOccurrences,
    calendarRange.endMs,
    calendarRange.startMs,
  ]);
  const legacyRuns = toArray(runsQuery.data, "runs");
  const providerOptions = useMemo<any[]>(() => {
    return buildPlannerProviderOptions({
      providerCatalog: providerCatalogQuery.data,
      providerConfig: providersConfigQuery.data,
      defaultProvider: "",
      defaultModel: "",
    });
  }, [providerCatalogQuery.data, providersConfigQuery.data]);
  const mcpServers = useMemo(
    () => normalizeMcpServers(mcpServersQuery.data),
    [mcpServersQuery.data, normalizeMcpServers]
  );
  const workflowRuns = toArray(workflowRunsQuery.data, "runs");
  const runs = useMemo(() => {
    const automationNamesById = new Map<string, string>();
    for (const automation of automations) {
      const automationId = String(
        automation?.automation_id || automation?.routine_id || automation?.id || ""
      ).trim();
      const automationName = String(automation?.name || automation?.title || "").trim();
      if (automationId && automationName && !automationNamesById.has(automationId)) {
        automationNamesById.set(automationId, automationName);
      }
    }
    for (const automation of automationsV2) {
      const automationId = String(
        automation?.automation_id || automation?.automationId || automation?.id || ""
      ).trim();
      const automationName = String(automation?.name || automation?.title || "").trim();
      if (automationId && automationName && !automationNamesById.has(automationId)) {
        automationNamesById.set(automationId, automationName);
      }
    }
    const all = [...legacyRuns, ...workflowRuns];
    const byId = new Map<string, any>();
    for (const run of all) {
      const runId = String(run?.run_id || run?.runId || run?.id || "").trim();
      if (!runId) continue;
      if (byId.has(runId)) continue;
      const automationId = String(run?.automation_id || run?.routine_id || "").trim();
      const automationName =
        String(run?.automation_name || run?.automationName || "").trim() ||
        automationNamesById.get(automationId) ||
        "";
      byId.set(
        runId,
        automationName
          ? {
              ...run,
              automation_name: automationName,
              automationName,
            }
          : run
      );
    }
    return Array.from(byId.values()).sort((a: any, b: any) => {
      const aAt = normalizeTimestamp(
        a?.started_at_ms || a?.startedAtMs || a?.created_at_ms || a?.createdAtMs || 0
      );
      const bAt = normalizeTimestamp(
        b?.started_at_ms || b?.startedAtMs || b?.created_at_ms || b?.createdAtMs || 0
      );
      return bAt - aAt;
    });
  }, [automations, automationsV2, legacyRuns, normalizeTimestamp, workflowRuns]);
  const packs = toArray(packsQuery.data, "packs");
  const activeRuns = runs.filter((run: any) => isActiveRunStatus(workflowDerivedRunStatus(run)));
  const workflowQueueCounts = useMemo(() => {
    let active = 0;
    let queuedCapacity = 0;
    let queuedWorkspaceLock = 0;
    let queuedOther = 0;
    workflowRuns.forEach((run: any) => {
      const status = workflowDerivedRunStatus(run);
      const reason = workflowQueueReason(run);
      if (status === "queued") {
        if (reason === "capacity") queuedCapacity += 1;
        else if (reason === "workspace_lock") queuedWorkspaceLock += 1;
        else queuedOther += 1;
        return;
      }
      if (isActiveRunStatus(status)) active += 1;
    });
    return { active, queuedCapacity, queuedWorkspaceLock, queuedOther };
  }, [isActiveRunStatus, workflowQueueReason, workflowRuns]);
  const failedRuns = runs.filter((run: any) => {
    const status = workflowDerivedRunStatus(run);
    return (
      status === "failed" ||
      status === "error" ||
      status === "blocked" ||
      status === "stalled" ||
      workflowRunWasStalePaused(run)
    );
  });
  const selectedRun = (runDetailQuery.data as any)?.run || null;
  const workflowBlackboard = (workflowContextBlackboardQuery.data as any)?.blackboard || null;
  const workflowContextEvents = Array.isArray((workflowContextEventsQuery.data as any)?.events)
    ? (workflowContextEventsQuery.data as any).events
    : [];
  const workflowContextPatches = Array.isArray((workflowContextPatchesQuery.data as any)?.patches)
    ? (workflowContextPatchesQuery.data as any).patches
    : [];
  const workflowProjection = useMemo(() => {
    if (!isWorkflowRun) return { tasks: [], currentTaskId: "", taskSource: "empty" as const };
    const activeTaskIds = detectWorkflowActiveTaskIds(selectedRun, [], sessionEvents);
    const activeTaskId = activeTaskIds[0] || "";
    const activeTaskIdSet = new Set(activeTaskIds);
    const contextProjection = projectOrchestrationRun({
      run: (workflowContextRunQuery.data as any)?.run || null,
      tasks: Array.isArray((workflowContextRunQuery.data as any)?.run?.steps)
        ? (workflowContextRunQuery.data as any)?.run.steps
        : [],
      blackboard: workflowBlackboard,
      events: workflowContextEvents,
    });
    if (contextProjection.tasks.length) {
      const normalizedTasks = activeTaskIdSet.size
        ? contextProjection.tasks.map((task: any) =>
            activeTaskIdSet.has(task.id) && ["pending", "runnable", "assigned"].includes(task.state)
              ? { ...task, state: "in_progress" as const }
              : task
          )
        : contextProjection.tasks;
      return {
        ...contextProjection,
        tasks: normalizedTasks,
        currentTaskId: contextProjection.currentTaskId || activeTaskId,
      };
    }
    const snapshotProjection = workflowProjectionFromRunSnapshot(selectedRun, activeTaskId);
    const normalizedTasks = activeTaskIdSet.size
      ? snapshotProjection.tasks.map((task: any) =>
          activeTaskIdSet.has(task.id) && ["pending", "runnable", "assigned"].includes(task.state)
            ? { ...task, state: "in_progress" as const }
            : task
        )
      : snapshotProjection.tasks;
    return {
      ...snapshotProjection,
      tasks: normalizedTasks,
      currentTaskId: snapshotProjection.currentTaskId || activeTaskId,
    };
  }, [
    detectWorkflowActiveTaskId,
    detectWorkflowActiveTaskIds,
    isWorkflowRun,
    selectedRun,
    sessionEvents,
    workflowBlackboard,
    workflowContextEvents,
    workflowContextRunQuery.data,
  ]);
  const selectedBoardTask = useMemo(
    () => workflowProjection.tasks.find((task: any) => task.id === selectedBoardTaskId) || null,
    [selectedBoardTaskId, workflowProjection.tasks]
  );
  const firstBlockedWorkflowTask = useMemo(
    () =>
      workflowProjection.tasks.find(
        (task: any) => String(task.state || "").toLowerCase() === "blocked"
      ) || null,
    [workflowProjection.tasks]
  );
  const selectedBoardTaskOutput = useMemo(() => {
    if (!selectedBoardTask) return null;
    const nodeId = String(selectedBoardTask.id || "").replace(/^node-/, "");
    return workflowNodeOutput(selectedRun, nodeId);
  }, [selectedBoardTask, selectedRun]);
  const selectedBoardTaskTelemetry = useMemo(
    () => workflowNodeToolTelemetry(selectedBoardTaskOutput),
    [selectedBoardTaskOutput]
  );
  const selectedBoardTaskArtifactValidation = useMemo(
    () => workflowArtifactValidation(selectedBoardTaskOutput),
    [selectedBoardTaskOutput]
  );
  const selectedBoardTaskInspection = useMemo(
    () => workflowTaskInspectionDetails(selectedBoardTask, selectedBoardTaskOutput) || {},
    [selectedBoardTask, selectedBoardTaskOutput]
  );
  const {
    validationBasis: selectedBoardTaskValidationBasis = null,
    qualityMode: selectedBoardTaskQualityMode = "",
    requestedQualityMode: selectedBoardTaskRequestedQualityMode = "",
    emergencyRollbackEnabled: selectedBoardTaskEmergencyRollbackEnabled = null,
    blockerCategory: selectedBoardTaskBlockerCategory = "",
    receiptLedger: selectedBoardTaskReceiptLedger = null,
    receiptTimeline: selectedBoardTaskReceiptTimeline = [],
    touchedFiles: selectedBoardTaskTouchedFiles = [],
    undeclaredFiles: selectedBoardTaskUndeclaredFiles = [],
    researchReadPaths: selectedBoardTaskResearchReadPaths = [],
    discoveredRelevantPaths: selectedBoardTaskDiscoveredRelevantPaths = [],
    reviewedPathsBackedByRead: selectedBoardTaskReviewedPathsBackedByRead = [],
    unreviewedRelevantPaths: selectedBoardTaskUnreviewedRelevantPaths = [],
    unmetResearchRequirements: selectedBoardTaskUnmetResearchRequirements = [],
    verificationOutcome: selectedBoardTaskVerificationOutcome = "",
    verificationPassed: selectedBoardTaskVerificationPassed = null,
    verificationResults: selectedBoardTaskVerificationResults = [],
    failureDetail: selectedBoardTaskFailureDetail = "",
    workflowClass: selectedBoardTaskWorkflowClass = "",
    phase: selectedBoardTaskPhase = "",
    failureKind: selectedBoardTaskFailureKind = "",
    warningCount: selectedBoardTaskWarningCount = 0,
    warningRequirements: selectedBoardTaskWarningRequirements = [],
    validationOutcome: selectedBoardTaskValidationOutcome = "",
    artifactCandidates: selectedBoardTaskArtifactCandidates = [],
  } = selectedBoardTaskInspection as any;
  const rawRunStatus = String(selectedRun?.status || "")
    .trim()
    .toLowerCase();
  const baseRunStatus = workflowDerivedRunStatus(selectedRun);
  const projectionTaskStates = workflowProjection.tasks.map((task: any) =>
    String(task?.state || "")
      .trim()
      .toLowerCase()
  );
  let runStatus = baseRunStatus;
  let runStatusDerivedNote = "";
  if (
    rawRunStatus !== baseRunStatus &&
    (rawRunStatus === "completed" || rawRunStatus === "done") &&
    workflowBlockedNodeCount(selectedRun) > 0
  ) {
    runStatusDerivedNote = "derived from blocked nodes";
  }
  if (
    isWorkflowRun &&
    (rawRunStatus === "completed" || rawRunStatus === "done") &&
    projectionTaskStates.some((state: string) => !["done", "validated"].includes(state))
  ) {
    if (projectionTaskStates.includes("failed")) {
      runStatus = "failed";
    } else if (projectionTaskStates.includes("blocked")) {
      runStatus = "blocked";
    } else if (
      projectionTaskStates.some((state: string) =>
        ["created", "pending", "runnable", "assigned", "in_progress"].includes(state)
      )
    ) {
      runStatus = "running";
    }
    runStatusDerivedNote = "derived from projected task board";
  }
  const runRepairGuidanceEntries = useMemo(() => {
    const direct = selectedRun?.nodeRepairGuidance;
    const directEntries =
      direct && typeof direct === "object" && !Array.isArray(direct)
        ? Object.entries(direct)
            .map(([nodeId, guidance]: [string, any]) => ({
              nodeId: String(nodeId || "").trim(),
              guidance: guidance || {},
            }))
            .filter((entry) => entry.nodeId)
        : [];
    if (directEntries.length) return directEntries;
    const outputs =
      selectedRun?.checkpoint?.node_outputs || selectedRun?.checkpoint?.nodeOutputs || {};
    return Object.entries(outputs)
      .map(([nodeId, output]: [string, any]) => {
        const artifactValidation = output?.artifact_validation || output?.artifactValidation || {};
        const validatorSummary = output?.validator_summary || output?.validatorSummary || {};
        const actions = Array.isArray(
          artifactValidation?.required_next_tool_actions ||
            artifactValidation?.requiredNextToolActions
        )
          ? artifactValidation.required_next_tool_actions ||
            artifactValidation.requiredNextToolActions
          : [];
        const unmet = Array.isArray(
          validatorSummary?.unmet_requirements || validatorSummary?.unmetRequirements
        )
          ? validatorSummary.unmet_requirements || validatorSummary.unmetRequirements
          : [];
        const reason = String(
          validatorSummary?.reason || output?.blocked_reason || output?.blockedReason || ""
        ).trim();
        const blockingClassification = String(
          artifactValidation?.blocking_classification ||
            artifactValidation?.blockingClassification ||
            ""
        ).trim();
        if (!actions.length && !unmet.length && !reason && !blockingClassification) return null;
        return {
          nodeId: String(nodeId || "").trim(),
          guidance: {
            status: output?.status || "",
            failureKind: output?.failure_kind || output?.failureKind || "",
            reason,
            unmetRequirements: unmet,
            blockingClassification,
            requiredNextToolActions: actions,
            repairAttempt:
              artifactValidation?.repair_attempt ?? artifactValidation?.repairAttempt ?? null,
            repairAttemptsRemaining:
              artifactValidation?.repair_attempts_remaining ??
              artifactValidation?.repairAttemptsRemaining ??
              null,
          },
        };
      })
      .filter(Boolean) as Array<{ nodeId: string; guidance: any }>;
  }, [selectedRun]);
  useEffect(() => {
    setSelectedRunArtifactKey("");
  }, [selectedRunId, selectedBoardTaskId]);
  const runArtifacts = isWorkflowRun
    ? Array.isArray(workflowBlackboard?.artifacts)
      ? workflowBlackboard.artifacts
      : []
    : toArray(runArtifactsQuery.data, "artifacts");
  const runArtifactEntries = useMemo(
    () =>
      runArtifacts.map((artifact: any, index: number) => {
        const key = String(artifact?.id || artifact?.artifact_id || `artifact-${index + 1}`).trim();
        const name = String(
          artifact?.name ||
            artifact?.label ||
            artifact?.kind ||
            artifact?.type ||
            artifact?.path ||
            key
        ).trim();
        const kind = String(artifact?.kind || artifact?.type || artifact?.path || "").trim();
        const paths = uniqueStrings(collectPathStrings(artifact));
        return { key, name: name || key, kind, artifact, paths };
      }),
    [collectPathStrings, runArtifacts, uniqueStrings]
  );
  const selectedBoardTaskRelatedPaths = useMemo(() => {
    if (!selectedBoardTask) return [];
    return uniqueStrings([
      ...collectPathStrings(selectedBoardTaskOutput),
      ...collectPathStrings(selectedBoardTaskArtifactValidation),
      String((selectedBoardTask as any).output_path || "").trim(),
    ]);
  }, [
    collectPathStrings,
    selectedBoardTask,
    selectedBoardTaskArtifactValidation,
    selectedBoardTaskOutput,
    uniqueStrings,
  ]);
  const selectedBoardTaskRelatedArtifacts = useMemo(() => {
    if (!selectedBoardTaskRelatedPaths.length) return [];
    return runArtifactEntries.filter((entry: any) =>
      entry.paths.some((path: any) => selectedBoardTaskRelatedPaths.includes(path))
    );
  }, [runArtifactEntries, selectedBoardTaskRelatedPaths]);
  const selectedBoardTaskNodeId = useMemo(
    () =>
      String(selectedBoardTask?.id || "").startsWith("node-")
        ? String(selectedBoardTask?.id || "")
            .replace(/^node-/, "")
            .trim()
        : "",
    [selectedBoardTask]
  );
  const selectedBoardTaskIsWorkflowNode = useMemo(
    () => String(selectedBoardTask?.id || "").startsWith("node-"),
    [selectedBoardTask]
  );
  const selectedBoardTaskIsProjectedBacklogItem = useMemo(
    () => String((selectedBoardTask as any)?.task_type || "").trim() === "automation_backlog_item",
    [selectedBoardTask]
  );
  const selectedBoardTaskStateNormalized = useMemo(
    () =>
      String(selectedBoardTask?.state || "")
        .trim()
        .toLowerCase(),
    [selectedBoardTask]
  );
  const serverBlockedNodeIds = useMemo(() => workflowBlockedNodeIds(selectedRun), [selectedRun]);
  const serverNeedsRepairNodeIds = useMemo(
    () => workflowNeedsRepairNodeIds(selectedRun),
    [selectedRun]
  );
  const selectedRunStatusNormalized = String(runStatus || "")
    .trim()
    .toLowerCase();
  const workflowRunCanMutateTasks =
    isWorkflowRun &&
    !!selectedRunId &&
    !!selectedRunStatusNormalized &&
    !["running", "queued", "pausing"].includes(selectedRunStatusNormalized);
  const selectedBoardTaskAppearsBlocked = selectedBoardTaskStateNormalized === "blocked";
  const selectedBoardTaskAppearsRetryable =
    selectedBoardTaskAppearsBlocked || selectedBoardTaskStateNormalized === "failed";
  const pendingRunAction = runActionMutation.isPending
    ? String(runActionMutation.variables?.action || "").trim()
    : "";
  const pendingRunActionMessage = pendingRunAction
    ? `Waiting for ${pendingRunAction} request to finish.`
    : "";
  const selectedBoardTaskBlockedOnServer =
    !!selectedBoardTaskNodeId && serverBlockedNodeIds.includes(selectedBoardTaskNodeId);
  const selectedBoardTaskNeedsRepairOnServer =
    !!selectedBoardTaskNodeId && serverNeedsRepairNodeIds.includes(selectedBoardTaskNodeId);
  const continueBlockedTask = selectedBoardTaskBlockedOnServer
    ? selectedBoardTask
    : workflowProjection.tasks.find((task: any) =>
        serverBlockedNodeIds.includes(
          String(task?.id || "")
            .replace(/^node-/, "")
            .trim()
        )
      ) || firstBlockedWorkflowTask;
  const continueBlockedNodeId = selectedBoardTaskBlockedOnServer
    ? selectedBoardTaskNodeId
    : String(continueBlockedTask?.id || "")
        .replace(/^node-/, "")
        .trim();
  const selectedBoardTaskNeedsWorkflowAction =
    String(selectedBoardTask?.id || "").startsWith("node-") &&
    (selectedBoardTaskBlockedOnServer ||
      selectedBoardTaskNeedsRepairOnServer ||
      selectedBoardTaskStateNormalized === "failed");
  // Retry only makes sense when something is actually broken — a failed run,
  // a blocked node, or a task that needs intervention. A cleanly paused run
  // with healthy tasks should only offer Resume / Cancel, not Retry.
  const canRecoverWorkflowRun =
    workflowRunCanMutateTasks &&
    (selectedRunStatusNormalized === "failed" ||
      serverBlockedNodeIds.length > 0 ||
      selectedBoardTaskNeedsWorkflowAction);
  const canContinueBlockedWorkflow =
    workflowRunCanMutateTasks && serverBlockedNodeIds.length > 0 && !!continueBlockedNodeId;
  const selectedBoardTaskLeaseExpiresAtMs = useMemo(
    () => Number((selectedBoardTask as any)?.lease_expires_at_ms || 0) || 0,
    [selectedBoardTask]
  );
  const selectedBoardTaskIsStale = useMemo(
    () =>
      Boolean((selectedBoardTask as any)?.is_stale) ||
      (selectedBoardTaskStateNormalized === "in_progress" &&
        selectedBoardTaskLeaseExpiresAtMs > 0 &&
        selectedBoardTaskLeaseExpiresAtMs <= Date.now()),
    [selectedBoardTask, selectedBoardTaskLeaseExpiresAtMs, selectedBoardTaskStateNormalized]
  );
  const selectedBoardTaskLifecycleEvents = useMemo(
    () => workflowRecentNodeEventSummaries(selectedRun, selectedBoardTaskNodeId, 8),
    [selectedBoardTaskNodeId, selectedRun]
  );
  const selectedBoardTaskResetTaskIds = useMemo(
    () => workflowDescendantTaskIds(workflowProjection.tasks, selectedBoardTask?.id || ""),
    [selectedBoardTask, workflowDescendantTaskIds, workflowProjection.tasks]
  );
  const selectedBoardTaskResetTasks = useMemo(
    () =>
      selectedBoardTaskResetTaskIds
        .map(
          (taskId: any) => workflowProjection.tasks.find((task: any) => task.id === taskId) || null
        )
        .filter(Boolean) as any[],
    [selectedBoardTaskResetTaskIds, workflowProjection.tasks]
  );
  const selectedBoardTaskResetNodeIds = useMemo(() => {
    const preview = (taskResetPreviewQuery.data as any)?.preview;
    const previewNodes = Array.isArray(preview?.reset_nodes)
      ? preview.reset_nodes.map((value: any) => String(value || "").trim()).filter(Boolean)
      : [];
    if (previewNodes.length) return previewNodes;
    return selectedBoardTaskResetTaskIds
      .map((taskId: any) => taskId.replace(/^node-/, "").trim())
      .filter(Boolean);
  }, [selectedBoardTaskResetTaskIds, taskResetPreviewQuery.data]);
  const selectedBoardTaskResetOutputPaths = useMemo(() => {
    const preview = (taskResetPreviewQuery.data as any)?.preview;
    const previewOutputs = Array.isArray(preview?.cleared_outputs)
      ? preview.cleared_outputs.map((value: any) => String(value || "").trim()).filter(Boolean)
      : [];
    if (previewOutputs.length) return uniqueStrings(previewOutputs);
    return uniqueStrings(
      selectedBoardTaskResetTasks.map((task: any) =>
        String((task as any)?.output_path || "").trim()
      )
    );
  }, [selectedBoardTaskResetTasks, taskResetPreviewQuery.data, uniqueStrings]);
  const focusArtifactEntry = (path: string) => {
    const targetPath = String(path || "").trim();
    const match = runArtifactEntries.find((entry: any) => entry.paths.includes(targetPath));
    setSelectedRunArtifactKey(match?.key || "");
    if (artifactsSectionRef.current) {
      artifactsSectionRef.current.scrollIntoView({ block: "nearest", behavior: "smooth" });
    }
  };
  const canTaskRetry =
    workflowRunCanMutateTasks &&
    selectedBoardTaskIsWorkflowNode &&
    !!selectedBoardTaskNodeId &&
    (selectedBoardTaskBlockedOnServer ||
      selectedBoardTaskNeedsRepairOnServer ||
      selectedBoardTaskStateNormalized === "failed");
  const runDebuggerRetryNodeId =
    selectedBoardTaskStateNormalized === "failed"
      ? selectedBoardTaskNodeId
      : selectedBoardTaskBlockedOnServer || selectedBoardTaskNeedsRepairOnServer
        ? selectedBoardTaskNodeId
        : "";
  const canTaskContinue =
    workflowRunCanMutateTasks &&
    selectedBoardTaskIsWorkflowNode &&
    !!selectedBoardTaskNodeId &&
    selectedBoardTaskBlockedOnServer;
  const selectedBoardTaskMutationLockedMessage = !selectedRunStatusNormalized
    ? "Loading run status..."
    : !workflowRunCanMutateTasks && selectedBoardTaskIsWorkflowNode
      ? "This workflow is still running. Wait until it is paused, blocked, failed, completed, or cancelled before mutating tasks."
      : "";
  const selectedBoardTaskServerActionMismatchMessage =
    selectedBoardTaskIsWorkflowNode &&
    selectedBoardTaskNodeId &&
    ((selectedBoardTaskAppearsBlocked && !selectedBoardTaskBlockedOnServer) ||
      (selectedBoardTaskAppearsRetryable &&
        selectedBoardTaskStateNormalized !== "failed" &&
        !selectedBoardTaskBlockedOnServer &&
        !selectedBoardTaskNeedsRepairOnServer))
      ? "This node is not currently blocked on the server."
      : "";
  const selectedBoardTaskServerActionMessage =
    pendingRunActionMessage ||
    selectedBoardTaskMutationLockedMessage ||
    selectedBoardTaskServerActionMismatchMessage;
  const canTaskRequeue =
    workflowRunCanMutateTasks &&
    selectedBoardTaskIsWorkflowNode &&
    !!selectedBoardTaskNodeId &&
    !["in_progress", "done", "blocked", "failed"].includes(selectedBoardTaskStateNormalized);
  const canBacklogTaskClaim =
    isWorkflowRun &&
    !!selectedRunId &&
    selectedBoardTaskIsProjectedBacklogItem &&
    !selectedBoardTaskIsWorkflowNode &&
    ["pending", "runnable"].includes(selectedBoardTaskStateNormalized);
  const canBacklogTaskRequeue =
    isWorkflowRun &&
    !!selectedRunId &&
    selectedBoardTaskIsProjectedBacklogItem &&
    !selectedBoardTaskIsWorkflowNode &&
    (["blocked", "failed"].includes(selectedBoardTaskStateNormalized) || selectedBoardTaskIsStale);
  const selectedBoardTaskImpactSummary = useMemo(() => {
    const preview = (taskResetPreviewQuery.data as any)?.preview;
    const rootTitle = String(selectedBoardTask?.title || selectedBoardTaskNodeId || "task").trim();
    const subtreeCount = selectedBoardTaskResetNodeIds.length;
    const descendantCount = Math.max(0, subtreeCount - (selectedBoardTaskNodeId ? 1 : 0));
    const outputCount = selectedBoardTaskResetOutputPaths.length;
    return {
      rootTitle,
      subtreeCount,
      descendantCount,
      outputCount,
      previewBacked: Boolean((taskResetPreviewQuery.data as any)?.preview),
      preservesUpstreamOutputs:
        typeof preview?.preserves_upstream_outputs === "boolean"
          ? preview.preserves_upstream_outputs
          : true,
    };
  }, [
    selectedBoardTask,
    selectedBoardTaskNodeId,
    selectedBoardTaskResetNodeIds.length,
    selectedBoardTaskResetOutputPaths.length,
    taskResetPreviewQuery.data,
  ]);
  const runHints = deriveRunDebugHints(selectedRun, runArtifacts);
  const runHistoryEvents = isWorkflowRun
    ? (() => {
        const contextHistory = workflowContextHistoryEntries(
          workflowContextEvents,
          workflowContextPatches
        );
        if (contextHistory.length) return contextHistory;
        return workflowPersistedHistoryEntries(
          Array.isArray(persistedRunEventsQuery.data) ? persistedRunEventsQuery.data : [],
          selectedRunId
        );
      })()
    : Array.isArray((runHistoryQuery.data as any)?.events)
      ? (runHistoryQuery.data as any).events
      : Array.isArray((runHistoryQuery.data as any)?.history)
        ? (runHistoryQuery.data as any).history
        : [];
  const telemetrySeedEvents = useMemo(() => {
    return workflowTelemetrySeedEvents(
      Array.isArray(persistedRunEventsQuery.data) ? persistedRunEventsQuery.data : [],
      workflowContextEvents,
      isWorkflowRun,
      selectedRunId
    );
  }, [isWorkflowRun, persistedRunEventsQuery.data, selectedRunId, workflowContextEvents]);
  const telemetryEvents = useMemo(() => {
    const all = [...telemetrySeedEvents, ...runEvents];
    const seen = new Set<string>();
    return all
      .filter((item) => {
        if (!item?.id || seen.has(item.id)) return false;
        seen.add(item.id);
        return true;
      })
      .sort((a, b) => Number(a.at || 0) - Number(b.at || 0));
  }, [telemetrySeedEvents, runEvents]);
  const filteredRunEvents = telemetryEvents.filter((item) =>
    selectedLogSource === "all" ? true : item.source === selectedLogSource
  );
  const filteredRunEventEntries = useMemo(
    () => workflowTelemetryDisplayEntries(filteredRunEvents),
    [filteredRunEvents]
  );
  const sessionMessages = useMemo(
    () =>
      sessionMessageQueries.flatMap((query, index) => {
        const sessionId = availableSessionIds[index] || "";
        const messages = Array.isArray(query.data) ? query.data : [];
        return messages.map((message: any) => ({
          sessionId,
          message,
        }));
      }),
    [availableSessionIds, sessionMessageQueries]
  );
  const runSummaryRows = useMemo(() => {
    const rows: Array<{ label: string; value: string }> = [];
    rows.push({ label: "status", value: runStatus || "unknown" });
    if (runStatusDerivedNote) {
      rows.push({ label: "status note", value: runStatusDerivedNote });
    }
    const effectiveProfile = workflowEffectiveExecutionProfile(selectedRun);
    const requestedProfile = workflowRequestedExecutionProfile(selectedRun);
    const profileValue =
      requestedProfile && requestedProfile !== effectiveProfile
        ? `${executionProfileLabel(effectiveProfile)} (requested ${executionProfileLabel(requestedProfile)})`
        : executionProfileLabel(effectiveProfile);
    rows.push({ label: "execution profile", value: profileValue });
    rows.push({ label: "artifacts", value: String(runArtifacts.length) });
    if (isWorkflowRun) {
      rows.push({ label: "tasks", value: String(workflowProjection.tasks.length) });
      rows.push({ label: "context events", value: String(workflowContextEvents.length) });
      rows.push({ label: "blackboard patches", value: String(workflowContextPatches.length) });
      rows.push({
        label: "completed nodes",
        value: String(workflowCompletedNodeCount(selectedRun)),
      });
      rows.push({ label: "pending nodes", value: String(workflowPendingNodeCount(selectedRun)) });
      rows.push({ label: "blocked nodes", value: String(workflowBlockedNodeCount(selectedRun)) });
    }
    if (String(selectedRun?.detail || "").trim()) {
      rows.push({ label: "detail", value: String(selectedRun.detail).trim() });
    }
    if (selectedRun?.requires_approval !== undefined) {
      rows.push({
        label: "requires approval",
        value: String(Boolean(selectedRun?.requires_approval)),
      });
    }
    if (String(selectedRun?.approval_reason || "").trim()) {
      rows.push({ label: "approval reason", value: String(selectedRun.approval_reason).trim() });
    }
    if (String(selectedRun?.denial_reason || "").trim()) {
      rows.push({ label: "denial reason", value: String(selectedRun.denial_reason).trim() });
    }
    if (String(selectedRun?.paused_reason || "").trim()) {
      rows.push({ label: "paused reason", value: String(selectedRun.paused_reason).trim() });
    }
    return rows;
  }, [
    isWorkflowRun,
    runArtifacts.length,
    runStatus,
    runStatusDerivedNote,
    selectedRun,
    workflowContextEvents.length,
    workflowContextPatches.length,
    workflowProjection.tasks.length,
  ]);
  const failureReason = useMemo(
    () => explainRunFailure(selectedRun),
    [explainRunFailure, selectedRun]
  );

  useSelectedRunLifecycle({
    enabled: runInspectorActive,
    availableSessionIds,
    queryClient,
    selectedRunId,
    selectedContextRunId,
    onSelectRunId,
    setSelectedSessionId,
    setSelectedSessionFilterId,
    setRunEvents,
    setSelectedLogSource,
    setSelectedBoardTaskId,
    setSessionEvents,
    setSessionLogPinnedToBottom,
  });

  const prevAutoSelectRunId = useRef("");
  useEffect(() => {
    if (!selectedRunId || !workflowProjection.tasks.length) return;
    if (prevAutoSelectRunId.current === selectedRunId) return;
    prevAutoSelectRunId.current = selectedRunId;
    setSelectedBoardTaskId(
      workflowProjection.currentTaskId ||
        workflowProjection.tasks.find((task: any) =>
          ["in_progress", "blocked", "assigned", "runnable", "pending"].includes(
            String(task.state || "").toLowerCase()
          )
        )?.id ||
        workflowProjection.tasks[0]?.id ||
        ""
    );
  }, [selectedRunId, workflowProjection.currentTaskId, workflowProjection.tasks]);

  const appendRunEvent = useBufferedAppender(setRunEvents, {
    cap: 300,
    getId: (row) => row.id,
  });
  const appendSessionEvent = useBufferedAppender(setSessionEvents, {
    cap: 500,
    getId: (row) => row.id,
  });

  // Coalesced SSE-driven invalidation for context queries — replaces aggressive
  // 5s polling. We mark these queries dirty when blackboard/context events
  // arrive and let React Query refetch on its next idle tick.
  const contextInvalidationRafRef = useRef<number | null>(null);
  const pendingContextInvalidations = useRef<{
    run: boolean;
    blackboard: boolean;
    events: boolean;
    patches: boolean;
  }>({ run: false, blackboard: false, events: false, patches: false });
  const flushContextInvalidationsRef = useRef<() => void>(() => {});
  flushContextInvalidationsRef.current = () => {
    contextInvalidationRafRef.current = null;
    const flags = pendingContextInvalidations.current;
    pendingContextInvalidations.current = {
      run: false,
      blackboard: false,
      events: false,
      patches: false,
    };
    if (!selectedContextRunId) return;
    const tasks: Array<Promise<unknown>> = [];
    if (flags.run) {
      tasks.push(
        queryClient.invalidateQueries({
          queryKey: ["automations", "run", "context", selectedContextRunId],
        })
      );
    }
    if (flags.blackboard) {
      tasks.push(
        queryClient.invalidateQueries({
          queryKey: ["automations", "run", "context", selectedContextRunId, "blackboard"],
        })
      );
    }
    if (flags.events) {
      tasks.push(
        queryClient.invalidateQueries({
          queryKey: ["automations", "run", "context", selectedContextRunId, "events"],
        })
      );
    }
    if (flags.patches) {
      tasks.push(
        queryClient.invalidateQueries({
          queryKey: ["automations", "run", "context", selectedContextRunId, "patches"],
        })
      );
    }
    if (tasks.length) void Promise.all(tasks);
  };
  const queueContextInvalidation = useCallback(
    (kinds: Partial<{ run: boolean; blackboard: boolean; events: boolean; patches: boolean }>) => {
      const pending = pendingContextInvalidations.current;
      if (kinds.run) pending.run = true;
      if (kinds.blackboard) pending.blackboard = true;
      if (kinds.events) pending.events = true;
      if (kinds.patches) pending.patches = true;
      if (contextInvalidationRafRef.current == null) {
        contextInvalidationRafRef.current = requestAnimationFrame(() =>
          flushContextInvalidationsRef.current()
        );
      }
    },
    []
  );
  useEffect(
    () => () => {
      if (contextInvalidationRafRef.current != null) {
        cancelAnimationFrame(contextInvalidationRafRef.current);
        contextInvalidationRafRef.current = null;
      }
    },
    []
  );

  useEngineStream(
    selectedRunId
      ? isWorkflowRun
        ? `/api/engine/automations/v2/events?run_id=${encodeURIComponent(selectedRunId)}`
        : `/api/engine/automations/events?run_id=${encodeURIComponent(selectedRunId)}`
      : "",
    (msg) => {
      try {
        const payload = JSON.parse(String(msg?.data || "{}"));
        if (!payload || payload.status === "ready") return;
        const runId = workflowEventRunId(payload);
        if (!runId || runId !== selectedRunId) return;
        const type = workflowEventType(payload);
        const at = workflowEventAt(payload);
        const id = `automations:${runId}:${type}:${at}:${Math.random().toString(16).slice(2, 8)}`;
        appendRunEvent({ id, source: "automations", at, event: payload });
        if (type) {
          const kind = String(type).toLowerCase();
          if (kind.includes("artifact") || kind.includes("blackboard") || kind.includes("patch")) {
            queueContextInvalidation({ blackboard: true, patches: true });
          }
          if (kind.includes("task") || kind.includes("node") || kind.endsWith(".updated")) {
            queueContextInvalidation({ run: true });
          }
        }
      } catch {
        return;
      }
    },
    { enabled: runInspectorActive }
  );
  useEngineStream(
    selectedContextRunId
      ? `/api/engine/context/runs/${encodeURIComponent(selectedContextRunId)}/events/stream?tail=50`
      : "",
    (msg) => {
      try {
        const payload = JSON.parse(String(msg?.data || "{}"));
        if (!payload || payload.status === "ready") return;
        const id = `context:${String(payload?.seq || "")}:${String(payload?.event_type || "")}`;
        const at =
          timestampOrNull(
            payload?.created_at_ms || payload?.timestamp_ms || payload?.timestampMs
          ) || Date.now();
        appendRunEvent({ id, source: "context", at, event: payload });
        const kind = String(payload?.event_type || "").toLowerCase();
        // Context-stream events drive blackboard/patch invalidation. Always
        // refresh the events query (it's small) and conditionally refresh the
        // heavier blackboard payload only when something blackboard-shaped fires.
        const fields: Partial<{
          run: boolean;
          blackboard: boolean;
          events: boolean;
          patches: boolean;
        }> = { events: true };
        if (
          kind.includes("blackboard") ||
          kind.includes("patch") ||
          kind.includes("artifact") ||
          kind.includes("task")
        ) {
          fields.blackboard = true;
          fields.patches = true;
        }
        if (kind.includes("run") || kind.includes("status") || kind.includes("phase")) {
          fields.run = true;
        }
        queueContextInvalidation(fields);
      } catch {
        return;
      }
    },
    { enabled: runInspectorActive && !!selectedContextRunId }
  );
  useEngineStream(
    selectedRunId && selectedSessionId
      ? `/api/engine/event?sessionID=${encodeURIComponent(selectedSessionId)}&runID=${encodeURIComponent(selectedRunId)}`
      : "",
    (msg) => {
      try {
        const payload = JSON.parse(String(msg?.data || "{}"));
        if (!payload) return;
        const type = workflowEventType(payload);
        const at = workflowEventAt(payload);
        const id = [
          type || "event",
          String(payload?.properties?.sessionID || payload?.sessionID || selectedSessionId || ""),
          String(payload?.properties?.runID || payload?.runID || selectedRunId || ""),
          String(payload?.properties?.messageID || payload?.messageID || ""),
          String(
            payload?.properties?.part?.id || payload?.properties?.seq || payload?.timestamp_ms || at
          ),
        ].join(":");
        appendSessionEvent({ id, at, event: payload });
      } catch {
        return;
      }
    },
    { enabled: runInspectorActive && !!selectedSessionId }
  );
  useEngineStream(
    selectedRunId ? "/api/global/event" : "",
    (msg) => {
      try {
        const payload = JSON.parse(String(msg?.data || "{}"));
        const runId = workflowEventRunId(payload);
        if (!runId || runId !== selectedRunId) return;
        const type = workflowEventType(payload);
        if (!type || type === "server.connected" || type === "engine.lifecycle.ready") return;
        const at = workflowEventAt(payload);
        const id = `global:${runId}:${type}:${at}:${Math.random().toString(16).slice(2, 8)}`;
        appendRunEvent({ id, source: "global", at, event: payload });
      } catch {
        return;
      }
    },
    { enabled: runInspectorActive }
  );
  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    renderIcons(root);
  }, [
    activeRuns.length,
    automations.length,
    automationsV2.length,
    failedRuns.length,
    packs.length,
    runActionMutation.isPending,
    runEvents.length,
    runNowMutation.isPending,
    runNowV2Mutation.isPending,
    runs.length,
    sessionEvents.length,
    workflowAutomationSections.length,
    legacyAutomationRows.length,
    workflowSortMode,
    workflowPreferencesLoading,
    updateAutomationMutation.isPending,
    workflowRuns.length,
    !!editDraft,
    !!selectedBoardTask,
    !!selectedRunId,
    !!selectedSessionId,
  ]);
  const beginEdit = (automation: any) => {
    const automationId = String(
      automation?.automation_id || automation?.id || automation?.routine_id || ""
    ).trim();
    if (!automationId) {
      toast("err", "Cannot edit automation without an id.");
      return;
    }
    const scheduleEditor = scheduleToEditor(automation?.schedule);
    setEditDraft({
      automationId,
      name: String(automation?.name || automationId || "").trim(),
      objective: String(
        automation?.mission?.objective || automation?.mission_snapshot?.objective || ""
      ).trim(),
      mode:
        String(automation?.mode || "").toLowerCase() === "standalone"
          ? "standalone"
          : "orchestrated",
      requiresApproval:
        automation?.requires_approval === true ||
        automation?.policy?.approval?.requires_approval === true,
      scheduleKind: scheduleEditor.scheduleKind === "cron" ? "cron" : "interval",
      cronExpression: scheduleEditor.cronExpression,
      intervalSeconds: String(scheduleEditor.intervalSeconds),
    });
  };
  const isPausedAutomation = (automation: any) => {
    const status = String(automation?.status || "")
      .trim()
      .toLowerCase();
    return status === "paused" || status === "disabled";
  };
  const openCalendarAutomationEdit = (automation: any) => {
    if (!automation) return;
    if (isMissionBlueprintAutomation(automation)) {
      onOpenAdvancedEdit(automation);
      return;
    }
    const family = getAutomationCalendarFamily(automation);
    if (family === "legacy") {
      beginEdit(automation);
      return;
    }
    const draft = workflowAutomationToEditDraft(automation);
    if (!draft) {
      toast("err", "Cannot open this workflow automation for editing.");
      return;
    }
    setWorkflowEditDraft(draft);
  };
  const updateCalendarAutomationFromEvent = async (info: any) => {
    const event = info?.event;
    const automation = event?.extendedProps?.automation;
    const family =
      String(event?.extendedProps?.family || "legacy").trim() === "v2" ? "v2" : "legacy";
    const cronExpression = String(event?.extendedProps?.cronExpression || "").trim();
    const start = event?.start ? new Date(event.start) : null;
    const nextCron = start ? rewriteCronForDroppedStart(cronExpression, start) : null;
    if (!automation || !start || !nextCron) {
      info?.revert?.();
      toast("info", "That schedule cannot be moved from the calendar yet.");
      return;
    }
    try {
      if (family === "legacy") {
        const automationId = String(
          automation?.automation_id || automation?.id || automation?.routine_id || ""
        ).trim();
        const scheduleEditor = scheduleToEditor(automation?.schedule);
        await updateAutomationMutation.mutateAsync({
          automationId,
          name: String(automation?.name || automationId || "").trim(),
          objective: String(
            automation?.mission?.objective || automation?.mission_snapshot?.objective || ""
          ).trim(),
          mode:
            String(automation?.mode || "").toLowerCase() === "standalone"
              ? "standalone"
              : "orchestrated",
          requiresApproval:
            automation?.requires_approval === true ||
            automation?.policy?.approval?.requires_approval === true,
          scheduleKind: "cron",
          cronExpression: nextCron,
          intervalSeconds: String(scheduleEditor.intervalSeconds || 3600),
        });
        return;
      }
      const draft = workflowAutomationToEditDraft(automation);
      if (!draft) {
        throw new Error("Workflow automation draft could not be created.");
      }
      await updateWorkflowAutomationMutation.mutateAsync({
        ...draft,
        scheduleKind: "cron",
        cronExpression: nextCron,
        intervalSeconds: draft.intervalSeconds || "3600",
      });
    } catch (error) {
      info?.revert?.();
      toast("err", error instanceof Error ? error.message : String(error));
    }
  };
  const legacyAutomationCount = automations.length;
  const workflowAutomationCount = automationsV2.length;
  const workflowAutomationVisibleCount = workflowAutomationRows.length;
  const totalSavedAutomations = legacyAutomationCount + workflowAutomationCount;
  const blockers = useMemo(
    () => buildRunBlockers(selectedRun, sessionEvents, runEvents),
    [buildRunBlockers, runEvents, selectedRun, sessionEvents]
  );
  const sessionLogEntries = useMemo(() => {
    const messageEntries = sessionMessages.map(({ sessionId, message }: any, index: number) => ({
      id: `message:${sessionId}:${sessionMessageId(message, index)}`,
      kind: "message" as const,
      sessionId,
      at: sessionMessageCreatedAt(message),
      variant: sessionMessageVariant(message),
      label: String(message?.info?.role || "session").trim() || "session",
      body: sessionMessageText(message),
      raw: message,
      parts: sessionMessageParts(message),
      sessionLabel: sessionLabel(sessionId),
    }));
    const liveEntries = workflowSessionLogEventEntries(sessionEvents, selectedSessionId).map(
      (entry) => ({
        ...entry,
        sessionLabel: sessionLabel(workflowEventSessionId(entry.raw, selectedSessionId)),
      })
    );
    const rows = [...messageEntries, ...liveEntries].sort((a, b) => a.at - b.at);
    if (selectedSessionFilterId === "all") return rows;
    return rows.filter((entry) => entry.sessionId === selectedSessionFilterId);
  }, [
    selectedSessionFilterId,
    selectedSessionId,
    sessionMessageCreatedAt,
    sessionMessageId,
    sessionMessageParts,
    sessionMessageText,
    sessionMessageVariant,
    sessionMessages,
    sessionEvents,
    sessionLabel,
  ]);
  useEffect(() => {
    const el = sessionLogRef.current;
    if (!el || !sessionLogPinnedToBottom) return;
    el.scrollTop = el.scrollHeight;
  }, [sessionLogEntries, sessionLogPinnedToBottom]);

  return (
    <MyAutomationsContent
      state={{
        rootRef,
        viewMode,
        defaultRunningSectionsOpen,
        calendarEvents,
        workflowAutomationCount,
        workflowAutomationVisibleCount,
        automationsV2,
        workflowAutomationSections,
        legacyAutomationRows,
        totalSavedAutomations,
        legacyAutomationCount,
        automations,
        workflowLibraryFilters,
        workflowLibraryFilteringActive,
        workflowSortMode,
        workflowPreferencesLoading,
        packs,
        activeRuns,
        workflowQueueCounts,
        failedRuns,
        runs,
        selectedRunId,
        selectedRun,
        isWorkflowRun,
        runStatus,
        runStatusDerivedNote,
        canContinueBlockedWorkflow,
        continueBlockedNodeId,
        canRecoverWorkflowRun,
        runDebuggerRetryNodeId,
        serverBlockedNodeIds,
        serverNeedsRepairNodeIds,
        selectedContextRunId,
        runSummaryRows,
        workflowProjection,
        runArtifacts,
        selectedBoardTaskId,
        selectedBoardTask,
        boardDetailRef,
        selectedBoardTaskOutput,
        selectedBoardTaskValidationOutcome,
        selectedBoardTaskWarningCount,
        selectedBoardTaskTelemetry,
        selectedBoardTaskArtifactValidation,
        selectedBoardTaskIsWorkflowNode,
        selectedBoardTaskIsProjectedBacklogItem,
        selectedBoardTaskWorkflowClass,
        selectedBoardTaskPhase,
        selectedBoardTaskFailureKind,
        selectedBoardTaskQualityMode,
        selectedBoardTaskEmergencyRollbackEnabled,
        selectedBoardTaskBlockerCategory,
        selectedBoardTaskValidationBasis,
        selectedBoardTaskReceiptLedger,
        selectedBoardTaskArtifactCandidates,
        selectedBoardTaskWarningRequirements,
        selectedBoardTaskReceiptTimeline,
        selectedBoardTaskLifecycleEvents,
        selectedBoardTaskResearchReadPaths,
        selectedBoardTaskDiscoveredRelevantPaths,
        selectedBoardTaskUnmetResearchRequirements,
        selectedBoardTaskReviewedPathsBackedByRead,
        selectedBoardTaskUnreviewedRelevantPaths,
        selectedBoardTaskVerificationOutcome,
        selectedBoardTaskVerificationPassed,
        selectedBoardTaskVerificationResults,
        selectedBoardTaskFailureDetail,
        selectedBoardTaskRelatedPaths,
        selectedBoardTaskRelatedArtifacts,
        selectedBoardTaskNodeId,
        selectedBoardTaskStateNormalized,
        selectedBoardTaskImpactSummary,
        selectedBoardTaskResetOutputPaths,
        canTaskContinue,
        canTaskRetry,
        selectedBoardTaskServerActionMessage,
        canTaskRequeue,
        canBacklogTaskClaim,
        canBacklogTaskRequeue,
        selectedBoardTaskTouchedFiles,
        selectedBoardTaskUndeclaredFiles,
        selectedBoardTaskRequestedQualityMode,
        selectedSessionId,
        selectedSessionFilterId,
        availableSessionIds,
        sessionLogEntries,
        sessionLogRef,
        selectedLogSource,
        telemetryEvents,
        filteredRunEventEntries,
        blockers,
        runHints,
        runRepairGuidanceEntries,
        artifactsSectionRef,
        runArtifactEntries,
        selectedRunArtifactKey,
        runHistoryEvents,
        workflowContextRun: (workflowContextRunQuery.data as any)?.run || null,
        workflowBlackboard,
        editDraft,
        workflowEditDraft,
        deleteConfirm,
        overlapHistoryEntries,
        providerOptions,
        mcpServers,
        client,
      }}
      actions={{
        setCalendarRange,
        openCalendarAutomationEdit,
        onRunCalendarAutomation: (automation: any, family: "legacy" | "v2") => {
          const automationId = String(
            automation?.automation_id || automation?.automationId || automation?.id || ""
          ).trim();
          if (!automationId) return;
          if (family === "v2") {
            runNowV2Mutation.mutate({ id: automationId });
            return;
          }
          runNowMutation.mutate(automationId);
        },
        updateCalendarAutomationFromEvent,
        onOpenAdvancedEdit,
        setWorkflowEditDraft,
        runNowV2Mutation,
        automationActionMutation,
        beginEdit,
        runNowMutation,
        isPausedAutomation,
        onSelectRunId,
        onOpenRunningView,
        onRecreateWorkflowAutomation,
        toast,
        setDeleteConfirm,
        navigate,
        setEditDraft,
        updateAutomationMutation,
        validateWorkspaceRootInput,
        validateModelInput,
        validatePlannerModelInput,
        automationWizardConfig,
        updateWorkflowAutomationMutation,
        onRefreshRunDebugger: () => {
          runActionMutation.reset();
          workflowRepairMutation.reset();
          workflowRecoverMutation.reset();
          workflowTaskRetryMutation.reset();
          workflowTaskContinueMutation.reset();
          workflowTaskRequeueMutation.reset();
          workflowTaskDispositionMutation.reset();
          backlogTaskClaimMutation.reset();
          backlogTaskRequeueMutation.reset();
          void Promise.all([
            queryClient.invalidateQueries({
              queryKey: ["automations", "run", selectedRunId],
            }),
            queryClient.invalidateQueries({
              queryKey: ["automations", "run", "artifacts", selectedRunId],
            }),
            selectedContextRunId
              ? queryClient.invalidateQueries({
                  queryKey: ["automations", "run", "context", selectedContextRunId],
                })
              : Promise.resolve(),
            selectedContextRunId
              ? queryClient.invalidateQueries({
                  queryKey: ["automations", "run", "context", selectedContextRunId, "blackboard"],
                })
              : Promise.resolve(),
            selectedContextRunId
              ? queryClient.invalidateQueries({
                  queryKey: ["automations", "run", "context", selectedContextRunId, "events"],
                })
              : Promise.resolve(),
            selectedContextRunId
              ? queryClient.invalidateQueries({
                  queryKey: ["automations", "run", "context", selectedContextRunId, "patches"],
                })
              : Promise.resolve(),
            selectedRunId
              ? queryClient.invalidateQueries({
                  queryKey: ["automations", "run", "session", selectedRunId],
                })
              : Promise.resolve(),
          ]);
        },
        setSelectedBoardTaskId,
        focusArtifactEntry,
        setSelectedSessionFilterId,
        onCopySessionLog: async () => {
          try {
            await navigator.clipboard.writeText(
              sessionLogEntries
                .map((entry: any) => {
                  const ts = new Date(entry.at).toLocaleTimeString();
                  const sessionTag = entry.sessionId ? ` · ${entry.sessionLabel}` : "";
                  return `[${ts}] ${entry.label}${sessionTag}\n${entry.body || formatJson(entry.raw)}`;
                })
                .join("\n\n")
            );
            toast("ok", "Copied session log.");
          } catch (error) {
            toast("err", error instanceof Error ? error.message : "Copy failed.");
          }
        },
        setSessionLogPinnedToBottom,
        setSelectedLogSource,
        setSelectedRunArtifactKey,
        onCopyFullDebugContext: async () => {
          try {
            await navigator.clipboard.writeText(
              [
                "=== RUN ===",
                formatJson(selectedRun),
                "=== ARTIFACTS ===",
                formatJson(runArtifacts),
                "=== TELEMETRY ===",
                formatJson(filteredRunEvents.map((row) => row.event)),
                "=== CONTEXT RUN ===",
                formatJson((workflowContextRunQuery.data as any)?.run || null),
                "=== BLACKBOARD ===",
                formatJson(workflowBlackboard),
                "=== HISTORY ===",
                formatJson(runHistoryEvents),
                "=== SESSION MESSAGES ===",
                formatJson(sessionMessages),
              ].join("\n\n")
            );
            toast("ok", "Copied full debug context.");
          } catch (error) {
            toast("err", error instanceof Error ? error.message : "Copy failed.");
          }
        },
        workflowTaskContinueMutation,
        workflowTaskRetryMutation,
        workflowTaskRequeueMutation,
        workflowTaskDispositionMutation,
        workflowRepairMutation,
        workflowRecoverMutation,
        backlogTaskClaimMutation,
        backlogTaskRequeueMutation,
        runActionMutation,
        taskResetPreviewQuery,
        toggleWorkflowFavorite,
        toggleWorkflowLibrarySourceFilter,
        toggleWorkflowLibraryStatusFilter,
        resetWorkflowLibraryFilters,
        setWorkflowSortMode,
      }}
      helpers={{
        statusColor,
        isStandupAutomation,
        isMissionBlueprintAutomation,
        workflowAutomationToEditDraft,
        formatAutomationV2ScheduleLabel,
        formatScheduleLabel,
        workflowStatusDisplay,
        workflowStatusSubtleDetail,
        runDisplayTitle,
        formatRunDateTime,
        runObjectiveText,
        shortText,
        runTimeLabel,
        workflowCompletedNodeCount,
        workflowBlockedNodeCount,
        workflowActiveSessionCount,
        workflowTotalNodeCount,
        isActiveRunStatus,
        compactIdentifier,
        sessionLabel,
        formatTimestampLabel,
      }}
    />
  );
}
