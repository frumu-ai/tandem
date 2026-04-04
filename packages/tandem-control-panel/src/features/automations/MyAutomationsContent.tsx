import { useState } from "react";
import { AutomationCalendar } from "./AutomationCalendar";
import {
  DeleteAutomationDialog,
  LegacyAutomationEditDialog,
  WorkflowAutomationEditDialog,
} from "./MyAutomationsDialogs";
import { RunDebuggerDialog } from "./RunDebuggerDialog";
import { EmptyState } from "../../pages/ui";
import {
  WORKFLOW_SORT_MODES,
  formatAutomationCreatedAtLabel,
} from "../../../lib/automations/workflow-list.js";

export function MyAutomationsContent({ state, actions, helpers }: any) {
  const [runningSectionsOpen, setRunningSectionsOpen] = useState({
    active: false,
    issues: false,
    history: false,
  });
  const {
    rootRef,
    viewMode,
    calendarEvents,
    workflowAutomationCount,
    workflowAutomationSections,
    legacyAutomationRows,
    totalSavedAutomations,
    legacyAutomationCount,
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
    runStatusDerivedFromBlockedNodes,
    canContinueBlockedWorkflow,
    continueBlockedNodeId,
    canRecoverWorkflowRun,
    runDebuggerRetryNodeId,
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
    workflowContextRun,
    workflowBlackboard,
    editDraft,
    workflowEditDraft,
    deleteConfirm,
    overlapHistoryEntries,
    providerOptions,
    mcpServers,
  } = state;

  const {
    setCalendarRange,
    openCalendarAutomationEdit,
    onRunCalendarAutomation,
    updateCalendarAutomationFromEvent,
    onOpenAdvancedEdit,
    setWorkflowEditDraft,
    runNowV2Mutation,
    automationActionMutation,
    beginEdit,
    toggleWorkflowFavorite,
    setWorkflowSortMode,
    runNowMutation,
    isPausedAutomation,
    onSelectRunId,
    onOpenRunningView,
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
    onRefreshRunDebugger,
    setSelectedBoardTaskId,
    focusArtifactEntry,
    setSelectedSessionFilterId,
    onCopySessionLog,
    setSessionLogPinnedToBottom,
    setSelectedLogSource,
    setSelectedRunArtifactKey,
    onCopyFullDebugContext,
    workflowTaskContinueMutation,
    workflowTaskRetryMutation,
    workflowTaskRequeueMutation,
    workflowRepairMutation,
    workflowRecoverMutation,
    backlogTaskClaimMutation,
    backlogTaskRequeueMutation,
    runActionMutation,
    taskResetPreviewQuery,
  } = actions;

  const {
    statusColor,
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
    isActiveRunStatus,
    compactIdentifier,
    sessionLabel,
    formatTimestampLabel,
  } = helpers;

  const toggleRunningSection = (section: "active" | "issues" | "history") =>
    setRunningSectionsOpen((current) => ({
      ...current,
      [section]: !current[section],
    }));

  const workflowSortLabel =
    WORKFLOW_SORT_MODES.find((option) => option.value === workflowSortMode)?.label ||
    "Created: newest first";

  const renderWorkflowAutomationCard = (row: any) => {
    const automation = row?.automation || row;
    const id = String(
      row?.id || automation?.automation_id || automation?.automationId || ""
    ).trim();
    const status = String(row?.status || automation?.status || "draft").trim();
    const paused = !!row?.paused || status.toLowerCase() === "paused";
    const favorite = !!row?.isFavorite;
    const categoryLabel = String(row?.categoryLabel || "").trim();
    const createdAtMs = Number(row?.createdAtMs || 0) || 0;
    return (
      <div key={id} className="tcp-card flex flex-col gap-3 group">
        <div className="flex items-start justify-between gap-2">
          <div className="flex items-center gap-2.5 min-w-0">
            <span className="text-xl">🧩</span>
            <div className="min-w-0">
              <strong className="block truncate text-sm font-bold tracking-tight text-white mb-0.5">
                {String(automation?.name || id || "Workflow automation")}
              </strong>
              <div className="flex flex-wrap items-center gap-1.5">
                {categoryLabel ? (
                  <span className="tcp-badge-ok text-[10px] py-0 px-1.5">{categoryLabel}</span>
                ) : null}
                {String(automation?.description || "").trim() ? null : (
                  <span className="text-[10px] text-slate-500 uppercase tracking-[0.2em]">
                    No description
                  </span>
                )}
              </div>
            </div>
          </div>
          <div className="flex items-center gap-1.5 shrink-0">
            <button
              type="button"
              className={`tcp-icon-btn h-8 w-8 ${favorite ? "text-amber-300" : "text-slate-400"}`}
              onClick={() => toggleWorkflowFavorite(id)}
              disabled={!id || workflowPreferencesLoading}
              title={favorite ? "Remove from favorites" : "Add to favorites"}
              aria-label={favorite ? "Remove from favorites" : "Add to favorites"}
              aria-pressed={favorite}
            >
              <i data-lucide="star" className={`w-3.5 h-3.5 ${favorite ? "fill-current" : ""}`}></i>
            </button>
            <button
              className="tcp-icon-btn h-8 w-8 opacity-100 md:opacity-0 group-hover:opacity-100 transition-opacity"
              onClick={() => {
                if (isMissionBlueprintAutomation(automation)) {
                  onOpenAdvancedEdit(automation);
                  return;
                }
                setWorkflowEditDraft(workflowAutomationToEditDraft(automation));
              }}
              disabled={!id}
              title="Edit workflow automation"
              aria-label="Edit workflow automation"
            >
              <i data-lucide="pencil" className="w-3.5 h-3.5"></i>
            </button>
            <span
              className={`text-[10px] font-bold uppercase tracking-wider px-1.5 py-0.5 rounded ${statusColor(status)}`}
            >
              {status}
            </span>
          </div>
        </div>

        {String(automation?.description || "").trim() ? (
          <div className="tcp-subtle text-xs line-clamp-2 leading-relaxed">
            {String(automation.description)}
          </div>
        ) : (
          <div className="tcp-subtle text-xs italic opacity-40">No description provided</div>
        )}

        {String(automation?.metadata?.standup?.report_path_template || "").trim() ? (
          <div className="text-[10px] text-emerald-300/80 font-mono tracking-tight bg-emerald-500/10 p-1.5 rounded-md truncate">
            report: {String(automation?.metadata?.standup?.report_path_template || "")}
          </div>
        ) : null}

        <div className="tcp-subtle text-[11px] font-medium flex items-center gap-1.5">
          <i data-lucide="calendar" className="w-3 h-3"></i>
          {formatAutomationV2ScheduleLabel(automation?.schedule)}
        </div>

        {createdAtMs ? (
          <div className="tcp-subtle text-[11px] font-medium flex items-center gap-1.5">
            <i data-lucide="clock" className="w-3 h-3"></i>
            Created {formatAutomationCreatedAtLabel(automation)}
          </div>
        ) : null}

        <div className="mt-auto pt-3 flex flex-wrap gap-2 border-t border-white/5">
          <button
            className="tcp-btn-primary flex-1 h-8 px-2 text-[11px]"
            onClick={() => runNowV2Mutation.mutate({ id })}
            disabled={!id || runNowV2Mutation.isPending}
          >
            <i data-lucide="play" className="w-3 h-3"></i>
            {runNowV2Mutation.isPending ? "Starting..." : "Run"}
          </button>
          <button
            className="tcp-btn h-8 px-2 text-[11px]"
            onClick={() => runNowV2Mutation.mutate({ id, dryRun: true })}
            disabled={!id || runNowV2Mutation.isPending}
          >
            <i data-lucide="flask-conical" className="w-3 h-3"></i>
            Dry
          </button>
          <button
            className="tcp-btn h-8 px-2 text-[11px]"
            onClick={() =>
              automationActionMutation.mutate({
                action: paused ? "resume" : "pause",
                automationId: id,
                family: "v2",
              })
            }
            disabled={!id || automationActionMutation.isPending}
          >
            <i data-lucide={paused ? "play" : "pause"} className="w-3 h-3"></i>
            {paused ? "Resume" : "Pause"}
          </button>
          <button
            className="tcp-btn-danger h-8 w-8 px-0 flex items-center justify-center"
            onClick={() =>
              setDeleteConfirm({
                automationId: id,
                family: "v2",
                title: String(automation?.name || id || "workflow automation"),
              })
            }
            disabled={!id || automationActionMutation.isPending}
            title="Remove"
          >
            <i data-lucide="trash-2" className="w-3.5 h-3.5"></i>
          </button>
        </div>
      </div>
    );
  };

  const renderLegacyAutomationCard = (row: any) => {
    const automation = row?.automation || row;
    const id = String(
      row?.id || automation?.automation_id || automation?.id || automation?.routine_id || ""
    ).trim();
    const favorite = !!row?.isFavorite;
    const status = String(row?.status || automation?.status || "active").trim();
    const paused = isPausedAutomation(automation);
    const createdAtMs = Number(row?.createdAtMs || 0) || 0;
    return (
      <div key={id} className="tcp-list-item">
        <div className="mb-1 flex items-center justify-between gap-2">
          <div className="flex items-center gap-2 min-w-0">
            <span>⏰</span>
            <strong className="truncate">{String(automation?.name || id || "Automation")}</strong>
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              className={`tcp-icon-btn h-7 w-7 ${favorite ? "text-amber-300" : ""}`}
              onClick={() => toggleWorkflowFavorite(id)}
              disabled={!id || workflowPreferencesLoading}
              title={favorite ? "Remove from favorites" : "Add to favorites"}
              aria-label={favorite ? "Remove from favorites" : "Add to favorites"}
              aria-pressed={favorite}
            >
              <i data-lucide="star" className={`w-3.5 h-3.5 ${favorite ? "fill-current" : ""}`}></i>
            </button>
            <button className="tcp-btn h-7 px-2 text-xs" onClick={() => beginEdit(automation)}>
              <i data-lucide="pencil"></i>
            </button>
            <span className={statusColor(status)}>{status}</span>
          </div>
        </div>
        {createdAtMs ? (
          <div className="tcp-subtle text-xs mb-1 flex items-center gap-1.5">
            <i data-lucide="clock" className="w-3 h-3"></i>
            Created {formatAutomationCreatedAtLabel(automation)}
          </div>
        ) : null}
        <div className="tcp-subtle text-xs">{formatScheduleLabel(automation?.schedule)}</div>
        <div className="mt-2">
          <div className="flex flex-wrap gap-2">
            <button className="tcp-btn h-7 px-2 text-xs" onClick={() => runNowMutation.mutate(id)}>
              <i data-lucide="play"></i>
              Run now
            </button>
            <button
              className="tcp-btn h-7 px-2 text-xs"
              onClick={() =>
                automationActionMutation.mutate({
                  action: paused ? "resume" : "pause",
                  automationId: id,
                  family: "legacy",
                })
              }
              disabled={!id || automationActionMutation.isPending}
            >
              <i data-lucide={paused ? "play" : "pause"}></i>
              {paused ? "Resume" : "Pause"}
            </button>
            <button
              className="tcp-btn h-7 px-2 text-xs"
              onClick={() => {
                const latestForAutomation = runs.find((run: any) => {
                  const automationId = String(
                    run?.automation_id || run?.routine_id || run?.id || ""
                  ).trim();
                  return automationId === id;
                });
                const runId = String(
                  latestForAutomation?.run_id || latestForAutomation?.id || ""
                ).trim();
                if (runId) {
                  onSelectRunId(runId);
                  onOpenRunningView();
                } else {
                  toast("info", "No runs yet for this automation.");
                }
              }}
            >
              <i data-lucide="info"></i>
              Debug latest
            </button>
            <button
              className="tcp-btn-danger h-7 px-2 text-xs"
              onClick={() =>
                setDeleteConfirm({
                  automationId: id,
                  family: "legacy",
                  title: String(automation?.name || automation?.label || id || "automation"),
                })
              }
              disabled={!id || automationActionMutation.isPending}
            >
              <i data-lucide="trash-2"></i>
              Remove
            </button>
          </div>
        </div>
      </div>
    );
  };

  return (
    <div ref={rootRef} className="grid gap-4">
      {viewMode === "calendar" ? (
        <AutomationCalendar
          events={calendarEvents}
          onRangeChange={setCalendarRange}
          onOpenAutomation={openCalendarAutomationEdit}
          onRunAutomation={onRunCalendarAutomation}
          onEventDrop={updateCalendarAutomationFromEvent}
          statusColor={statusColor}
          runActionsDisabled={runNowMutation.isPending || runNowV2Mutation.isPending}
        />
      ) : null}

      {viewMode === "list" ? (
        <div className="space-y-5 mb-4">
          <div className="flex items-start justify-between gap-2">
            <div className="space-y-1">
              <p className="text-[11px] font-medium uppercase tracking-[0.24em] text-slate-500">
                Workflow Automations
              </p>
              <p className="tcp-subtle text-xs">
                Favorites stay pinned first, and the rest follow your saved sort preference.
              </p>
            </div>
            <span className="tcp-badge-ghost text-xs tracking-wide">
              {workflowAutomationCount} items
            </span>
          </div>

          <div className="tcp-card flex flex-col gap-3">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="space-y-1">
                <p className="text-[11px] font-medium uppercase tracking-[0.24em] text-slate-500">
                  Sort & Favorites
                </p>
                <p className="tcp-subtle text-xs">
                  Current sort: {workflowSortLabel}. Profile-backed preferences only affect this
                  list, not workflow execution.
                </p>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <label className="tcp-subtle text-xs uppercase tracking-[0.2em]">Sort</label>
                <select
                  className="tcp-input h-8 min-w-[190px] py-1 text-xs"
                  value={workflowSortMode}
                  onChange={(event) => setWorkflowSortMode(event.target.value)}
                  disabled={workflowPreferencesLoading}
                >
                  {WORKFLOW_SORT_MODES.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </div>
            </div>
          </div>

          {workflowAutomationSections.length > 0 ? (
            <div className="space-y-5">
              {workflowAutomationSections.map((section: any) => (
                <section key={section.key} className="space-y-3">
                  <div className="flex items-start justify-between gap-2">
                    <div className="space-y-1">
                      <p className="text-[11px] font-medium uppercase tracking-[0.24em] text-slate-500">
                        {section.label}
                      </p>
                      <p className="tcp-subtle text-xs">{section.description}</p>
                    </div>
                    <span className="tcp-badge-ghost text-xs tracking-wide">
                      {section.count} items
                    </span>
                  </div>
                  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
                    {section.rows.map((row: any) => renderWorkflowAutomationCard(row))}
                  </div>
                </section>
              ))}
            </div>
          ) : (
            <div className="tcp-list-item">
              <div className="font-medium">No workflow automations saved yet</div>
              <div className="tcp-subtle mt-1 text-xs">
                This section is separate from run history and only shows workflow automation
                definitions.
              </div>
            </div>
          )}
        </div>
      ) : null}

      {viewMode === "list" ? (
        <div className="grid gap-2 mb-4">
          <div className="flex items-center justify-between gap-2">
            <p className="text-xs text-slate-500 uppercase tracking-wide font-medium">
              Saved Automations
            </p>
            <span className="tcp-badge-info">{totalSavedAutomations} saved</span>
          </div>

          <div className="grid gap-2">
            <div className="flex items-start justify-between gap-2">
              <div className="space-y-1">
                <p className="text-[11px] font-medium uppercase tracking-[0.24em] text-slate-500">
                  Scheduled Automations
                </p>
                <p className="tcp-subtle text-xs">
                  Legacy routines remain here with their familiar run, pause, and debug actions.
                </p>
              </div>
              <span className="tcp-subtle text-xs">{legacyAutomationCount} items</span>
            </div>
            {legacyAutomationRows.length > 0 ? (
              <div className="grid gap-2">
                {legacyAutomationRows.map((row: any) => renderLegacyAutomationCard(row))}
              </div>
            ) : (
              <div className="tcp-list-item">
                <div className="font-medium">No scheduled automations saved yet</div>
                <div className="tcp-subtle mt-1 text-xs">
                  This section shows automation definitions, not execution history.
                </div>
              </div>
            )}
          </div>
        </div>
      ) : null}

      {viewMode === "list" && packs.length > 0 ? (
        <div className="mt-12 pt-8 border-t border-white/5 opacity-60 hover:opacity-100 transition-opacity">
          <p className="text-[10px] text-slate-500 uppercase tracking-widest font-bold mb-3">
            System: Installed Packs
          </p>
          <div className="grid gap-2">
            {packs.map((pack: any, i: number) => (
              <div key={String(pack?.id || pack?.name || i)} className="tcp-list-item py-2">
                <div className="flex items-center justify-between gap-2">
                  <div className="flex items-center gap-2">
                    <span className="text-sm opacity-70">📦</span>
                    <strong className="text-xs">{String(pack?.name || pack?.id || "Pack")}</strong>
                  </div>
                  <span className="text-[10px] text-slate-500">
                    {String(pack?.version || "1.0.0")}
                  </span>
                </div>
              </div>
            ))}
          </div>
        </div>
      ) : null}

      {viewMode === "running" ? (
        <div className="grid gap-2">
          <button
            type="button"
            className="tcp-list-item text-left"
            onClick={() => toggleRunningSection("active")}
            aria-expanded={runningSectionsOpen.active}
          >
            <div className="flex items-start justify-between gap-3">
              <div className="grid gap-1">
                <div className="flex items-center gap-2">
                  <i
                    data-lucide={runningSectionsOpen.active ? "chevron-down" : "chevron-right"}
                  ></i>
                  <p className="text-xs font-medium uppercase tracking-wide text-slate-500">
                    Active Running Tasks
                  </p>
                </div>
                <div className="flex flex-wrap gap-2">
                  <span className="tcp-badge-warn">{workflowQueueCounts.active} active</span>
                  <span className="tcp-badge-info">
                    {workflowQueueCounts.queuedCapacity} queued for capacity
                  </span>
                  <span className="tcp-badge-info">
                    {workflowQueueCounts.queuedWorkspaceLock} queued for workspace lock
                  </span>
                  {workflowQueueCounts.queuedOther > 0 ? (
                    <span className="tcp-badge-info">
                      {workflowQueueCounts.queuedOther} other queued
                    </span>
                  ) : null}
                </div>
              </div>
              <span className="tcp-subtle text-xs">
                {runningSectionsOpen.active ? "Collapse" : "Expand"}
              </span>
            </div>
          </button>
          {runningSectionsOpen.active ? (
            activeRuns.length > 0 ? (
              activeRuns.slice(0, 14).map((run: any, index: number) => {
                const runId = String(run?.run_id || run?.id || index).trim();
                const activeRunStatus = workflowStatusDisplay(run);
                const startedAt =
                  run?.started_at_ms || run?.startedAtMs || run?.created_at_ms || run?.createdAtMs;
                const runStatusDetail = workflowStatusSubtleDetail(run);
                return (
                  <div key={runId || index} className="tcp-list-item">
                    <div className="flex items-center justify-between gap-2">
                      <div className="grid gap-0.5">
                        <span className="font-medium text-sm">{runDisplayTitle(run)}</span>
                        <span className="tcp-subtle text-xs">
                          {runId || "unknown run"} · running for {runTimeLabel(run)}
                        </span>
                        {formatRunDateTime(startedAt) ? (
                          <span className="tcp-subtle text-xs">
                            Started: {formatRunDateTime(startedAt)}
                          </span>
                        ) : null}
                        {runObjectiveText(run) ? (
                          <span className="text-xs text-slate-400">
                            {shortText(runObjectiveText(run), 160)}
                          </span>
                        ) : null}
                        {runStatusDetail ? (
                          <span className="tcp-subtle text-xs">{runStatusDetail}</span>
                        ) : null}
                      </div>
                      <span className={statusColor(activeRunStatus)}>
                        {activeRunStatus || "unknown"}
                      </span>
                    </div>
                    <div className="mt-2 flex flex-wrap gap-2">
                      <button
                        className="tcp-btn h-7 px-2 text-xs"
                        onClick={() => onSelectRunId(runId)}
                      >
                        <i data-lucide="bug"></i>
                        Inspect
                      </button>
                      <button
                        className="tcp-btn h-7 px-2 text-xs"
                        onClick={() =>
                          runActionMutation.mutate({
                            action: "pause",
                            runId,
                            family: runId.startsWith("automation-v2-run-") ? "v2" : "legacy",
                          })
                        }
                        disabled={!runId || runActionMutation.isPending}
                      >
                        <i data-lucide="pause"></i>
                        Pause
                      </button>
                      <button
                        className="tcp-btn h-7 px-2 text-xs"
                        onClick={() =>
                          runActionMutation.mutate({
                            action: "resume",
                            runId,
                            family: runId.startsWith("automation-v2-run-") ? "v2" : "legacy",
                          })
                        }
                        disabled={!runId || runActionMutation.isPending}
                      >
                        <i data-lucide="play"></i>
                        Resume
                      </button>
                      {runId.startsWith("automation-v2-run-") ? (
                        <button
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={() =>
                            runActionMutation.mutate({
                              action: "cancel",
                              runId,
                              family: "v2",
                              reason: "cancelled from active runs panel",
                            })
                          }
                          disabled={!runId || runActionMutation.isPending}
                        >
                          <i data-lucide="square"></i>
                          Cancel
                        </button>
                      ) : null}
                    </div>
                  </div>
                );
              })
            ) : (
              <div className="tcp-list-item">
                <div className="font-medium">Active Running Tasks</div>
                <div className="tcp-subtle mt-1 text-xs">
                  No active runs right now. Start a run to inspect live task execution.
                </div>
              </div>
            )
          ) : null}
        </div>
      ) : null}

      {viewMode === "running" && failedRuns.length > 0 ? (
        <div className="grid gap-2">
          <button
            type="button"
            className="tcp-list-item text-left"
            onClick={() => toggleRunningSection("issues")}
            aria-expanded={runningSectionsOpen.issues}
          >
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-2">
                <i data-lucide={runningSectionsOpen.issues ? "chevron-down" : "chevron-right"}></i>
                <p className="text-xs font-medium uppercase tracking-wide text-slate-500">
                  Recently Blocked Or Failed Runs
                </p>
              </div>
              <div className="flex items-center gap-2">
                <span className="tcp-badge-err">{failedRuns.length} issues</span>
                <span className="tcp-subtle text-xs">
                  {runningSectionsOpen.issues ? "Collapse" : "Expand"}
                </span>
              </div>
            </div>
          </button>
          {runningSectionsOpen.issues
            ? failedRuns.slice(0, 10).map((run: any, index: number) => {
                const runId = String(run?.run_id || run?.id || index).trim();
                const failedRunStatus = workflowStatusDisplay(run);
                const runStatusDetail = workflowStatusSubtleDetail(run);
                return (
                  <div key={`failed-${runId || index}`} className="tcp-list-item">
                    <div className="flex items-center justify-between gap-2">
                      <div className="grid gap-0.5">
                        <span className="font-medium text-sm">{runDisplayTitle(run)}</span>
                        <span className="tcp-subtle text-xs">{runId || "unknown run"}</span>
                        {formatRunDateTime(
                          run?.finished_at_ms ||
                            run?.finishedAtMs ||
                            run?.updated_at_ms ||
                            run?.updatedAtMs
                        ) ? (
                          <span className="tcp-subtle text-xs">
                            Finished:{" "}
                            {formatRunDateTime(
                              run?.finished_at_ms ||
                                run?.finishedAtMs ||
                                run?.updated_at_ms ||
                                run?.updatedAtMs
                            )}
                          </span>
                        ) : null}
                        {runObjectiveText(run) ? (
                          <span className="text-xs text-slate-400">
                            {shortText(runObjectiveText(run), 160)}
                          </span>
                        ) : null}
                        {runStatusDetail ? (
                          <span className="tcp-subtle text-xs">{runStatusDetail}</span>
                        ) : null}
                      </div>
                      <div className="flex items-center gap-2">
                        <span className={statusColor(failedRunStatus)}>
                          {failedRunStatus || "failed"}
                        </span>
                        <button
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={() => onSelectRunId(runId)}
                        >
                          <i data-lucide="bug"></i>
                          Inspect
                        </button>
                      </div>
                    </div>
                  </div>
                );
              })
            : null}
        </div>
      ) : null}

      {runs.length > 0 && viewMode === "running" ? (
        <div className="grid gap-2">
          <button
            type="button"
            className="tcp-list-item text-left"
            onClick={() => toggleRunningSection("history")}
            aria-expanded={runningSectionsOpen.history}
          >
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-2">
                <i data-lucide={runningSectionsOpen.history ? "chevron-down" : "chevron-right"}></i>
                <p className="text-xs text-slate-500 uppercase tracking-wide font-medium">
                  {viewMode === "running" ? "Run Log Explorer" : "Recent Runs"}
                </p>
              </div>
              <span className="tcp-subtle text-xs">
                {runs.length} runs · {runningSectionsOpen.history ? "Collapse" : "Expand"}
              </span>
            </div>
          </button>
          {runningSectionsOpen.history
            ? runs.slice(0, 12).map((run: any, index: number) => (
                <div key={String(run?.run_id || run?.id || index)} className="tcp-list-item">
                  <div className="flex items-center justify-between gap-2">
                    <span className="font-medium text-sm">{runDisplayTitle(run)}</span>
                    <span className={statusColor(workflowStatusDisplay(run))}>
                      {workflowStatusDisplay(run) || "unknown"}
                    </span>
                  </div>
                  <div className="mt-1 flex items-center justify-between gap-2">
                    <div className="grid gap-0.5">
                      <span className="tcp-subtle text-xs">
                        {String(run?.run_id || run?.id || "")}
                      </span>
                      {formatRunDateTime(
                        run?.started_at_ms ||
                          run?.startedAtMs ||
                          run?.created_at_ms ||
                          run?.createdAtMs
                      ) ? (
                        <span className="tcp-subtle text-xs">
                          Started:{" "}
                          {formatRunDateTime(
                            run?.started_at_ms ||
                              run?.startedAtMs ||
                              run?.created_at_ms ||
                              run?.createdAtMs
                          )}
                        </span>
                      ) : null}
                      {run?.finished_at_ms || run?.finishedAtMs ? (
                        <span className="tcp-subtle text-xs">
                          Finished: {formatRunDateTime(run?.finished_at_ms || run?.finishedAtMs)}
                        </span>
                      ) : null}
                      {runObjectiveText(run) ? (
                        <span className="text-xs text-slate-400">
                          {shortText(runObjectiveText(run), 160)}
                        </span>
                      ) : null}
                      {workflowStatusSubtleDetail(run) ? (
                        <span className="tcp-subtle text-xs">
                          {workflowStatusSubtleDetail(run)}
                        </span>
                      ) : null}
                    </div>
                    <button
                      className="tcp-btn h-7 px-2 text-xs"
                      onClick={() => {
                        onSelectRunId(String(run?.run_id || run?.id || "").trim());
                        onOpenRunningView();
                      }}
                    >
                      <i data-lucide="info"></i>
                      {viewMode === "running" ? "Logs" : "Details"}
                    </button>
                  </div>
                </div>
              ))
            : null}
        </div>
      ) : null}

      {!runs.length && viewMode === "running" ? (
        <EmptyState text="Run one automation, then use Logs to inspect full execution events." />
      ) : null}
      {!totalSavedAutomations && !packs.length && !runs.length && viewMode === "list" ? (
        <EmptyState text="No automations yet. Create your first one with the wizard!" />
      ) : null}
      <RunDebuggerDialog
        state={{
          selectedRunId,
          selectedRun,
          isWorkflowRun,
          runStatus,
          runStatusDerivedFromBlockedNodes,
          canContinueBlockedWorkflow,
          continueBlockedNodeId,
          canRecoverWorkflowRun,
          runDebuggerRetryNodeId,
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
          workflowContextRun,
          workflowBlackboard,
        }}
        actions={{
          onSelectRunId,
          onRefresh: onRefreshRunDebugger,
          onClose: () => {
            setSelectedBoardTaskId("");
            onSelectRunId("");
          },
          onSelectBoardTaskId: setSelectedBoardTaskId,
          focusArtifactEntry,
          onSessionFilterChange: setSelectedSessionFilterId,
          onCopySessionLog,
          onJumpToLatest: () => {
            setSessionLogPinnedToBottom(true);
            const el = sessionLogRef.current;
            if (el) el.scrollTop = el.scrollHeight;
          },
          onPinnedStateChange: setSessionLogPinnedToBottom,
          onSelectLogSource: setSelectedLogSource,
          onFocusNode: (nodeId: string) => setSelectedBoardTaskId(`node-${nodeId}`),
          onToggleArtifact: (key: string) =>
            setSelectedRunArtifactKey((current: string) => (current === key ? "" : key)),
          onCopyFullDebugContext,
          onNavigateFeed: () => navigate("feed"),
          workflowTaskContinueMutation,
          workflowTaskRetryMutation,
          workflowTaskRequeueMutation,
          workflowRepairMutation,
          workflowRecoverMutation,
          backlogTaskClaimMutation,
          backlogTaskRequeueMutation,
          runActionMutation,
          taskResetPreviewQuery,
        }}
        helpers={{
          runTimeLabel,
          workflowCompletedNodeCount,
          workflowBlockedNodeCount,
          workflowActiveSessionCount,
          statusColor,
          isActiveRunStatus,
          compactIdentifier,
          sessionLabel,
          formatTimestampLabel,
        }}
      />
      <LegacyAutomationEditDialog
        editDraft={editDraft}
        setEditDraft={setEditDraft}
        updateAutomationMutation={updateAutomationMutation}
      />
      <WorkflowAutomationEditDialog
        workflowEditDraft={workflowEditDraft}
        setWorkflowEditDraft={setWorkflowEditDraft}
        validateWorkspaceRootInput={validateWorkspaceRootInput}
        validateModelInput={validateModelInput}
        validatePlannerModelInput={validatePlannerModelInput}
        automationWizardConfig={automationWizardConfig}
        providerOptions={providerOptions}
        mcpServers={mcpServers}
        overlapHistoryEntries={overlapHistoryEntries}
        runNowV2Mutation={runNowV2Mutation}
        updateWorkflowAutomationMutation={updateWorkflowAutomationMutation}
      />
      <DeleteAutomationDialog
        deleteConfirm={deleteConfirm}
        setDeleteConfirm={setDeleteConfirm}
        automationActionMutation={automationActionMutation}
      />
    </div>
  );
}
