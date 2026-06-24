import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
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
  workflowContextHistoryEntries,
  workflowDerivedRunStatus,
  workflowEventBlockers,
  workflowPersistedHistoryEntries,
  workflowNodeOutput,
  workflowNodeToolTelemetry,
  workflowProjectionFromRunSnapshot,
  workflowRecentNodeEventSummaries,
  workflowRunWasStalePaused,
  workflowSessionIds,
  workflowTaskInspectionDetails,
  workflowTelemetryDisplayEntries,
  workflowTelemetrySeedEvents,
  workflowNeedsRepairNodeIds,
  workflowTotalNodeCount,
} from "../orchestration/workflowStability";
import { MyAutomationsContent } from "./MyAutomationsContent";
import { updateWorkflowAutomationDraft } from "./workflowAutomationSave";
import { useBufferedAppender } from "./useBufferedAppender";
import { useSelectedRunLifecycle } from "./useSelectedRunLifecycle";
import { useAutomationRunMutations } from "./useAutomationRunMutations";
import { useAutomationRunStreams } from "./useAutomationRunStreams";
import { useCalendarAutomationEditing } from "./useCalendarAutomationEditing";
import { useOverlapHistoryEntries } from "./useOverlapHistoryEntries";
import { useRunSummaryRows } from "./useRunSummaryRows";
import { useSessionLogEntries } from "./useSessionLogEntries";
import { useRenderAutomationIcons } from "./useRenderAutomationIcons";
import { buildPlannerProviderOptions } from "../planner/plannerShared";
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
    workflowAutomationToEditDraft,
    isMissionBlueprintAutomation,
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
      api("/api/engine/automations/v2?view=summary").catch((error: any) => ({
        automations: [],
        error: error?.message || String(error),
      })) ?? Promise.resolve({ automations: [] }),
    refetchInterval: 20000,
  });
  const automationsV2ListError =
    typeof (automationsV2Query.data as any)?.error === "string"
      ? (automationsV2Query.data as any).error
      : automationsV2Query.error
        ? String((automationsV2Query.error as any)?.message || automationsV2Query.error)
        : "";
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
  const overlapHistoryEntries = useOverlapHistoryEntries(automationsV2, toArray);
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

  const {
    runNowMutation,
    runNowV2Mutation,
    runActionMutation,
    workflowRepairMutation,
    workflowRecoverMutation,
    workflowTaskRetryMutation,
    workflowTaskContinueMutation,
    workflowTaskRequeueMutation,
    workflowTaskDispositionMutation,
    backlogTaskClaimMutation,
    backlogTaskRequeueMutation,
  } = useAutomationRunMutations({
    client,
    toast,
    queryClient,
    selectedRunId,
    selectedBoardTaskId,
    onSelectRunId,
    onOpenRunningView,
  });
  const updateAutomationMutation = useMutation({
    mutationFn: (draft: any) =>
      updateWorkflowAutomationDraft({
        draft,
        client,
        automationsV2,
        helperFns,
      }),
    onSuccess: async () => {
      toast("ok", "Automation updated.");
      setEditDraft(null);
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const updateWorkflowAutomationMutation = useMutation({
    mutationFn: (draft: any) =>
      updateWorkflowAutomationDraft({
        draft,
        client,
        automationsV2,
        helperFns,
      }),
    onSuccess: async () => {
      toast("ok", "Workflow automation updated.");
      setWorkflowEditDraft(null);
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const openWorkflowAutomationEdit = async (automation: any) => {
    const automationId = String(
      automation?.automation_id || automation?.automationId || automation?.id || ""
    ).trim();
    let fullAutomation = automation;
    if (automationId && client?.automationsV2?.get) {
      try {
        const response = await client.automationsV2.get(automationId);
        if (response?.automation && typeof response.automation === "object") {
          fullAutomation = response.automation;
        }
      } catch {
        toast("err", "Could not load full workflow definition; showing cached summary.");
      }
    }
    setWorkflowEditDraft(workflowAutomationToEditDraft(fullAutomation));
  };

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
  const runSummaryRows = useRunSummaryRows({
    isWorkflowRun,
    runArtifacts,
    runStatus,
    runStatusDerivedNote,
    selectedRun,
    workflowContextEvents,
    workflowContextPatches,
    workflowProjection,
  });
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

  useAutomationRunStreams({
    selectedRunId,
    selectedSessionId,
    selectedContextRunId,
    isWorkflowRun,
    runInspectorActive,
    timestampOrNull,
    appendRunEvent,
    appendSessionEvent,
    queryClient,
  });
  useRenderAutomationIcons(rootRef, [
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
  const {
    beginEdit,
    isPausedAutomation,
    openCalendarAutomationEdit,
    updateCalendarAutomationFromEvent,
  } = useCalendarAutomationEditing({
    toast,
    scheduleToEditor,
    setEditDraft,
    isMissionBlueprintAutomation,
    onOpenAdvancedEdit,
    getAutomationCalendarFamily,
    workflowAutomationToEditDraft,
    setWorkflowEditDraft,
    rewriteCronForDroppedStart,
    updateAutomationMutation,
    updateWorkflowAutomationMutation,
  });
  const legacyAutomationCount = automations.length;
  const workflowAutomationCount = automationsV2.length;
  const workflowAutomationVisibleCount = workflowAutomationRows.length;
  const totalSavedAutomations = legacyAutomationCount + workflowAutomationCount;
  const blockers = useMemo(
    () => buildRunBlockers(selectedRun, sessionEvents, runEvents),
    [buildRunBlockers, runEvents, selectedRun, sessionEvents]
  );
  const sessionLogEntries = useSessionLogEntries({
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
    sessionLogRef,
    sessionLogPinnedToBottom,
  });

  return (
    <MyAutomationsContent
      state={{
        rootRef,
        viewMode,
        defaultRunningSectionsOpen,
        calendarEvents,
        workflowAutomationCount,
        workflowAutomationVisibleCount,
        automationsV2ListError,
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
        runsLoading: runsQuery.isLoading || workflowRunsQuery.isLoading,
        runsRefreshing: runsQuery.isFetching || workflowRunsQuery.isFetching,
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
        openWorkflowAutomationEdit,
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
