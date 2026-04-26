import { Badge, DetailDrawer, PanelCard } from "../ui/index.tsx";
import { EmptyState } from "./ui";
import { ProviderModelSelector } from "../components/ProviderModelSelector";
import { ChatInterfacePanel } from "../components/ChatInterfacePanel";
import { renderMarkdownSafe } from "../lib/markdown";
import { PlannerDiagnosticsPanel } from "../features/planner/PlannerDiagnosticsPanel";
import { PlannerSessionRail } from "../features/planner/PlannerSessionRail";
import { ConfirmDialog, PromptDialog } from "../components/ControlPanelDialogs";

export function TaskPlanningPanelView(props: any) {
  const { rootRef, session, planner, status, provider, actions, dialogs } = props as any;
  const {
    plannerSessions,
    selectedSessionId,
    sessionsOpen,
    plannerSessionSummaries,
    activePlannerSessionSummary,
    plannerTitle,
  } = session as any;
  const {
    goal,
    workspaceRoot,
    notes,
    plannerInput,
    plannerProvider,
    plannerModel,
    planPreview,
    planningConversation,
    planningChangeSummary,
    plannerError,
    plannerDiagnostics,
    publishing,
    resetting,
    clarification,
    saveStatus,
    publishStatus,
    lastSavedAtMs,
    publishedAtMs,
    publishedTasks,
    plannerChatMessages,
    plannerInputPlaceholder,
    plannerStatusTitle,
    plannerStatusDetail,
    exportMarkdown,
    plannerFallbackReason,
    hasPlannerResponse,
    displayTasks,
  } = planner as any;
  const {
    isGitHubProject,
    taskSourceType,
    engineHealthy,
    plannerCanUseLlm,
    hasExplicitPlannerOverride,
    hasBasePlannerModel,
    plannerSelectionMatchesWorkspaceDefault,
    resolvedWorkspaceRoot,
    canPublishToGitHub,
    plannerTimedOut,
    clarificationNeeded,
    planIsFallbackOnly,
    isPlanning,
  } = status as any;
  const {
    providerOptions,
    providerStatus,
    selectedProjectSlug,
    selectedProject,
    githubProjectBoardSnapshot,
    connectedMcpServers,
  } = provider as any;
  const {
    activatePlannerSession,
    createNewPlannerSession,
    renamePlannerSession,
    duplicatePlannerSession,
    deletePlannerSession,
    setSessionsOpen,
    previewMutation,
    reviseMutation,
    resetMutation,
    patchActivePlannerSession,
    setPlannerInput,
    setGoal,
    setWorkspaceRoot,
    setNotes,
    setPlannerProvider,
    setPlannerModel,
    setPlannerError,
    setPlannerDiagnostics,
    setClarification,
    setSaveStatus,
    publishTasks,
    plannerOperatorPreferences,
    plannerSessionTitle,
  } = actions as any;
  const {
    renameSessionDialog,
    deleteSessionDialog,
    confirmRenamePlannerSession,
    confirmDeletePlannerSession,
    setRenameSessionDialog,
    setDeleteSessionDialog,
  } = dialogs as any;

  return (
    <div ref={rootRef} className="grid gap-4 xl:grid-cols-[320px_minmax(0,1fr)]">
      <DetailDrawer
        open={sessionsOpen}
        title="Planner sessions"
        onClose={() => setSessionsOpen(false)}
      >
        <PlannerSessionRail
          sessions={plannerSessionSummaries}
          selectedSessionId={selectedSessionId}
          onSelectSession={activatePlannerSession}
          onCreateSession={() => void createNewPlannerSession()}
          onRenameSession={(sessionId) => void renamePlannerSession(sessionId)}
          onDuplicateSession={(sessionId) => void duplicatePlannerSession(sessionId)}
          onDeleteSession={(sessionId) => void deletePlannerSession(sessionId)}
        />
      </DetailDrawer>

      <div className="hidden min-h-0 xl:block xl:self-start xl:sticky xl:top-4">
        <PlannerSessionRail
          sessions={plannerSessionSummaries}
          selectedSessionId={selectedSessionId}
          onSelectSession={activatePlannerSession}
          onCreateSession={() => void createNewPlannerSession()}
          onRenameSession={(sessionId) => void renamePlannerSession(sessionId)}
          onDuplicateSession={(sessionId) => void duplicatePlannerSession(sessionId)}
          onDeleteSession={(sessionId) => void deletePlannerSession(sessionId)}
        />
      </div>

      <div className="grid gap-4 min-w-0">
        <PanelCard
          title={plannerTitle}
          subtitle="Use the built-in scrum-master planner to turn a goal into reviewable implementation tasks."
          actions={
            <button
              type="button"
              className="tcp-btn xl:hidden"
              onClick={() => setSessionsOpen(true)}
            >
              <i data-lucide="history"></i>
              Sessions
            </button>
          }
        >
          <div className="grid gap-4">
            <div className="rounded-2xl border border-emerald-500/20 bg-emerald-500/5 p-4">
              <div className="flex flex-wrap items-center gap-2">
                <Badge tone="ok">scrum-master</Badge>
                <Badge tone={isGitHubProject ? "info" : "warn"}>
                  {taskSourceType || "unknown task source"}
                </Badge>
                <Badge tone={engineHealthy ? "ok" : "warn"}>
                  {engineHealthy ? "Engine ready" : "Engine unavailable"}
                </Badge>
              </div>
              <p className="tcp-subtle mt-3 text-sm">
                Describe the work once, let the planner break it into repo-aware tasks, then revise
                it with comments before publishing. GitHub Project approval creates issues and moves
                them into the board. Local kanban approval saves a durable export bundle.
              </p>
            </div>

            <div className="grid gap-3 xl:grid-cols-2">
              <div className="rounded-2xl border border-white/10 bg-black/20 p-4 xl:col-span-2">
                <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
                  <div>
                    <div className="text-xs uppercase tracking-wide text-slate-500">
                      Planner model
                    </div>
                    <div className="tcp-subtle text-xs">
                      Leave this on workspace default to use the base provider and model, or pick an
                      explicit planner override for richer revisions.
                    </div>
                  </div>
                  <Badge tone={plannerCanUseLlm ? "ok" : "warn"}>
                    {hasExplicitPlannerOverride
                      ? "Planner override active"
                      : hasBasePlannerModel
                        ? "Workspace default active"
                        : "No planner model configured"}
                  </Badge>
                </div>
                <ProviderModelSelector
                  providerLabel="Planner provider"
                  modelLabel="Planner model"
                  draft={{ provider: plannerProvider, model: plannerModel }}
                  providers={providerOptions}
                  onChange={({ provider, model }) => {
                    setPlannerProvider(provider);
                    setPlannerModel(model);
                  }}
                  inheritLabel="Workspace default"
                  disabled={isPlanning}
                />
                <div className="mt-3 flex flex-wrap gap-2">
                  <button
                    type="button"
                    className="tcp-btn"
                    onClick={() => {
                      setPlannerProvider("");
                      setPlannerModel("");
                    }}
                    disabled={isPlanning || (!plannerProvider && !plannerModel)}
                  >
                    Use workspace default
                  </button>
                  <button
                    type="button"
                    className="tcp-btn"
                    onClick={() => {
                      setPlannerProvider(providerStatus.defaultProvider || "");
                      setPlannerModel(providerStatus.defaultModel || "");
                    }}
                    disabled={
                      isPlanning || !hasBasePlannerModel || plannerSelectionMatchesWorkspaceDefault
                    }
                  >
                    Restore workspace model
                  </button>
                  <button
                    type="button"
                    className="tcp-btn"
                    onClick={() => {
                      setPlannerError("");
                      setPlannerDiagnostics(null);
                      setSaveStatus("");
                    }}
                    disabled={isPlanning || (!plannerError && !plannerDiagnostics)}
                  >
                    Clear diagnostics
                  </button>
                </div>
                <div className="mt-2 text-xs text-slate-500">
                  Base model:{" "}
                  {hasBasePlannerModel
                    ? `${String(providerStatus.defaultProvider || "")} / ${String(providerStatus.defaultModel || "")}`
                    : "not configured"}
                </div>
              </div>
              <label className="grid gap-2 xl:col-span-2">
                <span className="text-xs uppercase tracking-wide text-slate-500">
                  Workspace root
                </span>
                <input
                  className="tcp-input text-sm"
                  value={workspaceRoot}
                  onInput={(event) => setWorkspaceRoot((event.target as HTMLInputElement).value)}
                  placeholder="/absolute/path/to/the/repo/checkout"
                  disabled={isPlanning}
                />
                {resolvedWorkspaceRoot ? (
                  <span className="tcp-subtle text-xs">
                    Resolved from the selected project: {resolvedWorkspaceRoot}
                  </span>
                ) : null}
                <span className="tcp-subtle text-xs">
                  The planner uses this checkout to inspect files and glob directories for the
                  selected project.
                </span>
              </label>
              <label className="grid gap-2 xl:col-span-2">
                <span className="text-xs uppercase tracking-wide text-slate-500">Planner chat</span>
              </label>
              <div className="xl:col-span-2 rounded-2xl border border-white/10 bg-black/20 p-4">
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="text-xs uppercase tracking-wide text-slate-500">
                      Active session
                    </div>
                    <div className="truncate text-lg font-semibold text-slate-100">
                      {activePlannerSessionSummary?.title || "Untitled plan"}
                    </div>
                    <div className="mt-1 flex flex-wrap gap-2 text-xs text-slate-500">
                      <span>
                        {activePlannerSessionSummary?.projectSlug ||
                          selectedProjectSlug ||
                          "unbound"}
                      </span>
                      <span>·</span>
                      <span>
                        {activePlannerSessionSummary?.updatedAtLabel ||
                          (lastSavedAtMs ? new Date(lastSavedAtMs).toLocaleString() : "unknown")}
                      </span>
                      <span>·</span>
                      <span>{activePlannerSessionSummary?.revisionCount || 0} rev</span>
                    </div>
                  </div>
                  <div className="flex flex-wrap items-center gap-2">
                    <Badge tone={activePlannerSessionSummary?.statusTone || "ghost"}>
                      {activePlannerSessionSummary?.statusLabel || "Draft"}
                    </Badge>
                    <button
                      type="button"
                      className="tcp-btn h-8 px-2 text-xs"
                      onClick={() =>
                        selectedSessionId ? void renamePlannerSession(selectedSessionId) : undefined
                      }
                      disabled={!selectedSessionId || isPlanning}
                    >
                      <i data-lucide="pencil"></i>
                      Rename
                    </button>
                    <button
                      type="button"
                      className="tcp-btn h-8 px-2 text-xs"
                      onClick={() =>
                        selectedSessionId
                          ? void duplicatePlannerSession(selectedSessionId)
                          : undefined
                      }
                      disabled={!selectedSessionId || isPlanning}
                    >
                      <i data-lucide="copy"></i>
                      Duplicate
                    </button>
                    <button
                      type="button"
                      className="tcp-btn h-8 px-2 text-xs"
                      onClick={() =>
                        selectedSessionId ? void deletePlannerSession(selectedSessionId) : undefined
                      }
                      disabled={!selectedSessionId || isPlanning}
                    >
                      <i data-lucide="trash-2"></i>
                      Delete
                    </button>
                    <button
                      type="button"
                      className="tcp-btn h-8 px-2 text-xs xl:hidden"
                      onClick={() => setSessionsOpen(true)}
                    >
                      <i data-lucide="history"></i>
                      Sessions
                    </button>
                  </div>
                </div>
              </div>
              {!hasPlannerResponse ? (
                <div className="xl:col-span-2 rounded-2xl border border-dashed border-white/10 bg-black/10 p-4">
                  <div className="text-sm font-medium text-slate-100">
                    Start a plan with a short prompt
                  </div>
                  <div className="mt-1 text-xs text-slate-500">
                    Use one of the prompts below, then start a new plan when you are ready to create
                    a draft thread.
                  </div>
                  <div className="mt-3 flex flex-wrap gap-2">
                    {[
                      "Plan the implementation for this bug fix.",
                      "Break this feature into backend, frontend, and verification tasks.",
                      "Review this idea and suggest an implementation plan.",
                      "Refine the plan to reduce risk and unknowns.",
                    ].map((prompt) => (
                      <button
                        key={prompt}
                        type="button"
                        className="tcp-btn text-sm"
                        onClick={() => {
                          setPlannerInput(prompt);
                        }}
                        disabled={isPlanning}
                      >
                        {prompt}
                      </button>
                    ))}
                  </div>
                  <div className="mt-3">
                    <button
                      type="button"
                      className="tcp-btn-primary"
                      onClick={createNewPlannerSession}
                      disabled={isPlanning}
                    >
                      <i data-lucide="plus"></i>
                      Start new plan
                    </button>
                  </div>
                </div>
              ) : null}
            </div>

            <ChatInterfacePanel
              messages={plannerChatMessages}
              emptyText={
                selectedSessionId
                  ? "No plan yet. Use a starter prompt or describe the work."
                  : "No planner session selected. Start a new plan to create a draft thread."
              }
              inputValue={plannerInput}
              inputPlaceholder={plannerInputPlaceholder}
              sendLabel={planPreview ? "Send to planner" : "Generate plan"}
              onInputChange={setPlannerInput}
              onSend={() =>
                void (planPreview ? reviseMutation(plannerInput) : previewMutation(plannerInput))
              }
              sendDisabled={isPlanning || !selectedSessionId || !String(plannerInput || "").trim()}
              inputDisabled={isPlanning || !selectedSessionId}
              statusTitle={plannerStatusTitle}
              statusDetail={isPlanning ? plannerStatusDetail : ""}
              questionTitle="Planner question"
              questionText={clarification.status === "waiting" ? clarification.question : ""}
              quickReplies={clarification.status === "waiting" ? clarification.options : []}
              onQuickReply={(option) => void reviseMutation(option.label)}
              questionHint="Reply in the planner chat box below or choose a suggested answer."
              autoFocusKey={selectedSessionId}
            />

            <div className="flex flex-wrap gap-2">
              {planPreview ? (
                <button
                  type="button"
                  className="tcp-btn"
                  onClick={() => void previewMutation(plannerInput || goal)}
                  disabled={isPlanning}
                >
                  <i data-lucide="refresh-cw"></i>
                  Regenerate plan
                </button>
              ) : null}
              <button
                type="button"
                className="tcp-btn"
                onClick={() => void resetMutation()}
                disabled={resetting || isPlanning}
              >
                <i data-lucide="rotate-ccw"></i>
                {resetting ? "Resetting…" : "Reset plan"}
              </button>
              <button
                type="button"
                className="tcp-btn"
                onClick={() => {
                  setPlannerInput("");
                  setNotes("");
                  setPlannerError("");
                  setPlannerDiagnostics(null);
                  setClarification({ status: "none" });
                  setSaveStatus("");
                }}
                disabled={
                  isPlanning || (!plannerInput && !notes && !plannerError && !plannerDiagnostics)
                }
              >
                <i data-lucide="eraser"></i>
                Clear composer and warnings
              </button>
              <button
                type="button"
                className="tcp-btn"
                onClick={() => {
                  void patchActivePlannerSession({
                    title: plannerSessionTitle({
                      goal,
                      plan: planPreview,
                      fallbackTime: lastSavedAtMs || Date.now(),
                    }),
                    workspace_root: workspaceRoot,
                    goal,
                    notes,
                    planner_provider: plannerProvider,
                    planner_model: plannerModel,
                    plan_source: "coding_task_planning",
                    allowed_mcp_servers: connectedMcpServers,
                    operator_preferences: plannerOperatorPreferences({
                      plannerProvider,
                      plannerModel,
                      defaultProvider: providerStatus.defaultProvider,
                      defaultModel: providerStatus.defaultModel,
                      selectedProjectSlug,
                      taskSourceType,
                      selectedProject,
                      goal,
                      notes,
                    }),
                    current_plan_id: planPreview?.plan_id,
                    draft: planPreview
                      ? {
                          initial_plan: planPreview,
                          current_plan: planPreview,
                          plan_revision: Number(planPreview.plan_revision || 1) || 1,
                          conversation: planningConversation || {
                            conversation_id: `wfchat-${selectedSessionId}`,
                            plan_id: planPreview.plan_id,
                            created_at_ms: Date.now(),
                            updated_at_ms: Date.now(),
                            messages: [],
                          },
                          planner_diagnostics: plannerDiagnostics,
                          last_success_materialization: null,
                        }
                      : undefined,
                    published_at_ms: publishedAtMs || undefined,
                    published_tasks: publishedTasks.length ? publishedTasks : undefined,
                  });
                  setSaveStatus("Planner session synced.");
                }}
                disabled={isPlanning}
              >
                <i data-lucide="save"></i>
                Sync session
              </button>
              <button
                type="button"
                className="tcp-btn"
                onClick={async () => {
                  try {
                    await navigator.clipboard.writeText(exportMarkdown);
                  } catch {
                    // ignore clipboard failures in the view layer
                  }
                }}
                disabled={isPlanning}
              >
                <i data-lucide="copy"></i>
                Copy markdown
              </button>
            </div>

            {plannerError && !clarificationNeeded ? (
              <div
                className={`rounded-2xl p-3 text-sm ${
                  plannerTimedOut
                    ? "border border-amber-500/40 bg-amber-950/30 text-amber-100"
                    : clarificationNeeded
                      ? "border border-amber-500/40 bg-amber-950/30 text-amber-100"
                      : "border border-red-500/40 bg-red-950/30 text-red-200"
                }`}
              >
                {plannerTimedOut ? (
                  <div className="mb-1 text-xs uppercase tracking-wide text-amber-300">
                    Planner timed out
                  </div>
                ) : clarificationNeeded ? (
                  <div className="mb-1 text-xs uppercase tracking-wide text-amber-300">
                    Planner question
                  </div>
                ) : null}
                {plannerError}
              </div>
            ) : null}

            {plannerDiagnostics || planningChangeSummary.length ? (
              <PlannerDiagnosticsPanel
                plannerDiagnostics={{
                  ...plannerDiagnostics,
                  summary:
                    plannerDiagnostics?.summary ||
                    plannerDiagnostics?.detail ||
                    (plannerFallbackReason === "no_planner_model"
                      ? "The planner fell back because no usable planner model reached the backend for this generated plan."
                      : plannerFallbackReason === "clarification_needed"
                        ? "The planner needs one more answer before it can generate a richer repo-aware plan."
                        : ""),
                }}
                teachingLibrary={null}
                planningChangeSummary={planningChangeSummary}
              />
            ) : null}

            {saveStatus || publishStatus ? (
              <div className="rounded-2xl border border-white/10 bg-black/20 p-3 text-sm text-slate-300">
                {saveStatus ? <div>{saveStatus}</div> : null}
                {publishStatus ? <div className="mt-1">{publishStatus}</div> : null}
                {lastSavedAtMs ? (
                  <div className="mt-1 text-xs text-slate-500">
                    Last saved {new Date(lastSavedAtMs).toLocaleString()}
                  </div>
                ) : null}
              </div>
            ) : null}
          </div>
        </PanelCard>

        {hasPlannerResponse ? (
          <div className="grid gap-4">
            <PanelCard
              title="Plan details"
              subtitle="Planner metadata and markdown stay visible here"
            >
              {planPreview ? (
                <div className="grid gap-3">
                  <div className="grid gap-2 text-sm text-slate-300">
                    <div>
                      <span className="tcp-subtle">Title:</span>{" "}
                      {String(planPreview?.title || "") || "Untitled plan"}
                    </div>
                    <div>
                      <span className="tcp-subtle">Confidence:</span>{" "}
                      {String(planPreview?.confidence || "") || "unknown"}
                    </div>
                    <div>
                      <span className="tcp-subtle">Plan source:</span>{" "}
                      {String(planPreview?.plan_source || planPreview?.planSource || "") ||
                        "coding_task_planning"}
                    </div>
                  </div>
                  {typeof planPreview?.description === "string" &&
                  planPreview.description.trim() ? (
                    <div className="rounded-2xl border border-white/10 bg-black/20 p-3">
                      <div className="text-xs uppercase tracking-wide text-slate-500">
                        Planner markdown
                      </div>
                      <div
                        className="tcp-markdown tcp-markdown-ai mt-2 text-sm"
                        dangerouslySetInnerHTML={{
                          __html: renderMarkdownSafe(String(planPreview.description || "")),
                        }}
                      />
                    </div>
                  ) : null}
                  <div className="grid gap-2">
                    <button
                      type="button"
                      className="tcp-btn-primary"
                      disabled={
                        publishing ||
                        isPlanning ||
                        !goal.trim() ||
                        !workspaceRoot.trim() ||
                        clarificationNeeded ||
                        plannerTimedOut ||
                        planIsFallbackOnly
                      }
                      onClick={() => void publishTasks()}
                    >
                      <i
                        data-lucide={
                          isGitHubProject && canPublishToGitHub ? "badge-check" : "arrow-up-circle"
                        }
                      ></i>
                      {publishing
                        ? "Publishing…"
                        : isGitHubProject && canPublishToGitHub
                          ? "Approve and publish to GitHub Project"
                          : "Save local task bundle"}
                    </button>
                    <div className="text-xs text-slate-500">
                      {clarificationNeeded
                        ? "Answer the planner's question before approving or publishing tasks."
                        : plannerTimedOut
                          ? "Retry the planner revision or switch models before approving tasks."
                          : planIsFallbackOnly
                            ? "Wait for a real task breakdown before approving or publishing tasks."
                            : isGitHubProject && canPublishToGitHub
                              ? "This will create GitHub issues, add each issue to the selected project board, and move it into Ready when the board metadata is available."
                              : "Local kanban mode saves the plan locally so you can apply it to the board file or keep it as a durable draft."}
                    </div>
                  </div>
                </div>
              ) : (
                <EmptyState text="No plan has been generated yet." />
              )}
            </PanelCard>

            <PanelCard
              title="Planned tasks"
              subtitle="Review the generated backlog before publishing"
            >
              {displayTasks.length ? (
                <div className="grid gap-3">
                  {displayTasks.map((task, index) => (
                    <div
                      key={`${task.id}-${index}`}
                      className="rounded-2xl border border-white/10 bg-black/20 p-4"
                    >
                      <div className="flex flex-wrap items-start justify-between gap-2">
                        <div className="min-w-0">
                          <div className="text-sm font-semibold text-slate-100">{task.title}</div>
                          <div className="mt-1 text-xs text-slate-500">
                            {task.kind || "task"}
                            {task.dependsOn.length
                              ? ` · depends on ${task.dependsOn.join(", ")}`
                              : ""}
                          </div>
                        </div>
                        <Badge tone="info">Step {index + 1}</Badge>
                      </div>
                      <div className="mt-3 grid gap-2 text-sm text-slate-300">
                        <div>
                          <span className="tcp-subtle">Summary:</span> {task.objective}
                        </div>
                        {task.outputContract ? (
                          <div>
                            <span className="tcp-subtle">Expected result:</span>{" "}
                            {task.outputContract}
                          </div>
                        ) : null}
                        {task.inputRefs.length ? (
                          <div className="text-xs text-slate-500">
                            <span className="tcp-subtle">Inputs:</span>{" "}
                            {task.inputRefs
                              .map((row) => `${row.alias} <- ${row.fromStepId}`)
                              .join(", ")}
                          </div>
                        ) : null}
                      </div>
                    </div>
                  ))}
                </div>
              ) : planPreview ? (
                <EmptyState
                  text={
                    clarificationNeeded
                      ? "Answer the planner's question to generate a real task breakdown."
                      : plannerTimedOut
                        ? "The last revision timed out, so the planner kept the current fallback draft."
                        : "The planner returned a plan, but no usable step list was available."
                  }
                />
              ) : (
                <EmptyState text="Generate a plan to see task drafts here." />
              )}
            </PanelCard>
          </div>
        ) : null}

        <PromptDialog
          open={!!renameSessionDialog}
          title="Rename planner session"
          message={
            <span>
              Choose a new name for{" "}
              <strong>
                {renameSessionDialog?.sessionId
                  ? plannerSessions.find((row: any) => row.id === renameSessionDialog.sessionId)
                      ?.title || "this session"
                  : "this session"}
              </strong>
              .
            </span>
          }
          label="Session name"
          value={renameSessionDialog?.value || ""}
          placeholder="Untitled plan"
          confirmLabel="Rename"
          confirmIcon="square-pen"
          confirmDisabled={!String(renameSessionDialog?.value || "").trim()}
          onCancel={() => setRenameSessionDialog(null)}
          onChange={(value) =>
            setRenameSessionDialog((current: any) => (current ? { ...current, value } : current))
          }
          onConfirm={() => void confirmRenamePlannerSession()}
        />

        <ConfirmDialog
          open={!!deleteSessionDialog}
          title="Delete session"
          message={
            <span>
              This will permanently remove{" "}
              <strong>{deleteSessionDialog?.title || "this session"}</strong> and all its messages.
            </span>
          }
          confirmLabel="Delete session"
          confirmIcon="trash-2"
          confirmTone="danger"
          onCancel={() => setDeleteSessionDialog(null)}
          onConfirm={() => void confirmDeletePlannerSession()}
        />
      </div>
    </div>
  );
}
