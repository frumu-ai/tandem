import { useState } from "react";
import { describeScheduleValue } from "../scheduleBuilder";
import { renderMarkdownSafe } from "../../../lib/markdown";

function normalizeAllowedTools(raw: string[]) {
  const seen = new Set<string>();
  const values: string[] = [];
  for (const row of raw) {
    const value = String(row || "").trim();
    if (!value || seen.has(value)) continue;
    seen.add(value);
    values.push(value);
  }
  return values;
}

function parseCustomToolText(raw: string) {
  return normalizeAllowedTools(
    String(raw || "")
      .split(/[\n,]/g)
      .map((value) => String(value || "").trim())
      .filter(Boolean)
  );
}

type ExecutionMode = "single" | "team" | "swarm";

type Step4ReviewProps = {
  wizard: {
    goal: string;
    scheduleKind: "manual" | "cron" | "interval";
    cron: string;
    intervalSeconds: string;
    mode: ExecutionMode;
    maxAgents: string;
    modelProvider: string;
    modelId: string;
    plannerModelProvider: string;
    plannerModelId: string;
    workspaceRoot: string;
    selectedMcpServers: string[];
    toolAccessMode: "all" | "custom";
    customToolsText: string;
    exportPackDraft: boolean;
  };
  onToggleExportPackDraft: () => void;
  onSubmit: () => void;
  overlapAnalysis: any;
  overlapDecision: string;
  onSelectOverlapDecision: (decision: string) => void;
  isPending: boolean;
  planPreview: any;
  isPreviewing: boolean;
  planningConversation: any;
  planningChangeSummary: string[];
  onSendPlanningMessage: (message: string) => void;
  isSendingPlanningMessage: boolean;
  onResetPlanningChat: () => void;
  isResettingPlanningChat: boolean;
  plannerError: string;
  plannerDiagnostics: any;
  generatedSkill: any;
  installStatus: string;
  executionModes: Array<{ id: ExecutionMode; label: string; icon: string }>;
};

export function Step4Review({
  wizard,
  onToggleExportPackDraft,
  onSubmit,
  overlapAnalysis,
  overlapDecision,
  onSelectOverlapDecision,
  isPending,
  planPreview,
  isPreviewing,
  planningConversation,
  planningChangeSummary,
  onSendPlanningMessage,
  isSendingPlanningMessage,
  onResetPlanningChat,
  isResettingPlanningChat,
  plannerError,
  plannerDiagnostics,
  generatedSkill,
  installStatus,
  executionModes,
}: Step4ReviewProps) {
  const [planningNote, setPlanningNote] = useState("");
  const [goalExpanded, setGoalExpanded] = useState(false);
  const [expandedStepIds, setExpandedStepIds] = useState<Record<string, boolean>>({});

  const wizardSchedule = describeScheduleValue({
    scheduleKind: wizard.scheduleKind,
    cronExpression: wizard.cron,
    intervalSeconds: wizard.intervalSeconds,
  });
  const planOperatorPreferences =
    planPreview && typeof planPreview === "object"
      ? planPreview.operator_preferences || planPreview.operatorPreferences || {}
      : {};
  const effectiveMode = String(
    (planOperatorPreferences as any)?.execution_mode || wizard.mode || "team"
  ).trim() as ExecutionMode;
  const modeInfo = executionModes.find((m) => m.id === effectiveMode);
  const effectiveMaxParallel = Number(
    (planOperatorPreferences as any)?.max_parallel_agents ??
      (planOperatorPreferences as any)?.maxParallelAgents ??
      (effectiveMode === "swarm" ? wizard.maxAgents : 1)
  );
  const hasPlanPreview = !!planPreview;
  const effectiveModelProvider = String(
    hasPlanPreview
      ? (planOperatorPreferences as any)?.model_provider ||
          (planOperatorPreferences as any)?.modelProvider ||
          ""
      : wizard.modelProvider || ""
  ).trim();
  const effectiveModelId = String(
    hasPlanPreview
      ? (planOperatorPreferences as any)?.model_id ||
          (planOperatorPreferences as any)?.modelId ||
          ""
      : wizard.modelId || ""
  ).trim();
  const effectivePlannerRoleModel =
    (planOperatorPreferences as any)?.role_models?.planner ||
    (planOperatorPreferences as any)?.roleModels?.planner;
  const effectivePlannerModelProvider = String(
    hasPlanPreview
      ? effectivePlannerRoleModel?.provider_id || effectivePlannerRoleModel?.providerId || ""
      : wizard.plannerModelProvider || ""
  ).trim();
  const effectivePlannerModelId = String(
    hasPlanPreview
      ? effectivePlannerRoleModel?.model_id || effectivePlannerRoleModel?.modelId || ""
      : wizard.plannerModelId || ""
  ).trim();
  const effectiveWorkspaceRoot = String(
    planPreview?.workspace_root || planPreview?.workspaceRoot || wizard.workspaceRoot || ""
  ).trim();
  const effectiveMcpServers = Array.isArray(
    planPreview?.allowed_mcp_servers || planPreview?.allowedMcpServers
  )
    ? ((planPreview?.allowed_mcp_servers || planPreview?.allowedMcpServers) as string[])
    : wizard.selectedMcpServers;
  const effectiveToolAccessMode = String(
    hasPlanPreview
      ? (planOperatorPreferences as any)?.tool_access_mode ||
          (planOperatorPreferences as any)?.toolAccessMode ||
          "all"
      : wizard.toolAccessMode || "all"
  ).trim();
  const effectiveCustomTools = hasPlanPreview
    ? normalizeAllowedTools(
        (
          (planOperatorPreferences as any)?.tool_allowlist ||
          (planOperatorPreferences as any)?.toolAllowlist ||
          []
        ).map((v: any) => String(v || "").trim())
      )
    : parseCustomToolText(wizard.customToolsText);
  const effectiveSchedule = planPreview?.schedule
    ? describeScheduleValue({
        scheduleKind:
          planPreview.schedule.type === "cron"
            ? "cron"
            : planPreview.schedule.type === "interval"
              ? "interval"
              : "manual",
        cronExpression:
          planPreview.schedule.cron_expression || planPreview.schedule.cronExpression || "",
        intervalSeconds: String(
          planPreview.schedule.interval_seconds || planPreview.schedule.intervalSeconds || "3600"
        ),
      })
    : wizardSchedule;
  const effectivePlanTitle = String(planPreview?.title || "").trim();
  const plannerFallbackReason = String(
    plannerDiagnostics?.fallback_reason || plannerDiagnostics?.fallbackReason || ""
  ).trim();
  const plannerFallbackDetail = String(plannerDiagnostics?.detail || "").trim();
  const overlapMatchLayer = String(
    overlapAnalysis?.match_layer || overlapAnalysis?.matchLayer || ""
  )
    .trim()
    .toLowerCase();
  const overlapRequiresConfirmation = Boolean(
    overlapAnalysis?.requires_user_confirmation || overlapAnalysis?.requiresUserConfirmation
  );
  const overlapScore = Number(
    overlapAnalysis?.similarity_score ?? overlapAnalysis?.similarityScore ?? NaN
  );
  const toggleStepExpanded = (stepId: string) =>
    setExpandedStepIds((current) => ({ ...current, [stepId]: !current[stepId] }));

  return (
    <div className="grid gap-4">
      <p className="text-sm text-slate-400">Review your automation before deploying.</p>
      <div className="grid gap-3 rounded-xl border border-slate-700/60 bg-slate-900/40 p-4">
        {effectivePlanTitle ? (
          <div className="grid gap-1">
            <span className="text-xs uppercase tracking-wide text-slate-500">Plan Title</span>
            <span className="text-sm font-semibold text-slate-100">{effectivePlanTitle}</span>
          </div>
        ) : null}
        <div className="grid grid-cols-2 gap-3">
          <div className="grid gap-1">
            <span className="text-xs uppercase tracking-wide text-slate-500">Schedule</span>
            <span className="text-sm font-medium text-slate-200">{effectiveSchedule}</span>
          </div>
          <div className="grid gap-1">
            <span className="text-xs uppercase tracking-wide text-slate-500">Execution Mode</span>
            <span className="text-sm font-medium text-slate-200">
              {modeInfo?.icon} {modeInfo?.label || effectiveMode}
              {Number.isFinite(effectiveMaxParallel) && effectiveMaxParallel > 1
                ? ` · ${effectiveMaxParallel} agents`
                : ""}
            </span>
          </div>
        </div>
        {hasPlanPreview || effectiveModelProvider || effectiveModelId ? (
          <div className="grid gap-1">
            <span className="text-xs uppercase tracking-wide text-slate-500">Model Override</span>
            <span className="text-sm font-medium text-slate-200">
              {effectiveModelProvider || effectiveModelId
                ? `${effectiveModelProvider || "default provider"} / ${effectiveModelId || "default model"}`
                : "Workspace default"}
            </span>
          </div>
        ) : null}
        {hasPlanPreview || effectivePlannerModelProvider || effectivePlannerModelId ? (
          <div className="grid gap-1">
            <span className="text-xs uppercase tracking-wide text-slate-500">Planner Model</span>
            <span className="text-sm font-medium text-slate-200">
              {effectivePlannerModelProvider || effectivePlannerModelId
                ? `${effectivePlannerModelProvider || "default provider"} / ${effectivePlannerModelId || "default model"}`
                : "Using model override"}
            </span>
          </div>
        ) : null}
        <div className="grid gap-1">
          <span className="text-xs uppercase tracking-wide text-slate-500">Workspace Root</span>
          <code className="rounded bg-slate-800/60 px-2 py-1 text-xs text-slate-300">
            {effectiveWorkspaceRoot || "engine workspace root"}
          </code>
        </div>
        {hasPlanPreview || effectiveMcpServers.length ? (
          <div className="grid gap-1">
            <span className="text-xs uppercase tracking-wide text-slate-500">MCP Servers</span>
            {effectiveMcpServers.length ? (
              <div className="flex flex-wrap gap-1">
                {effectiveMcpServers.map((name: string) => (
                  <span key={name} className="tcp-badge-info">
                    {name}
                  </span>
                ))}
              </div>
            ) : (
              <span className="text-sm font-medium text-slate-400">None</span>
            )}
          </div>
        ) : null}
        <div className="grid gap-1">
          <span className="text-xs uppercase tracking-wide text-slate-500">Tool Access</span>
          {effectiveToolAccessMode === "custom" ? (
            effectiveCustomTools.length ? (
              <div className="flex flex-wrap gap-1">
                {effectiveCustomTools.map((tool: string) => (
                  <span key={tool} className="tcp-badge-info">
                    {tool}
                  </span>
                ))}
              </div>
            ) : (
              <span className="text-sm font-medium text-slate-400">Custom allowlist</span>
            )
          ) : (
            <span className="text-sm font-medium text-slate-200">All tools</span>
          )}
        </div>
        {wizard.scheduleKind === "cron" && wizard.cron ? (
          <div className="grid gap-1">
            <span className="text-xs uppercase tracking-wide text-slate-500">Cron</span>
            <code className="rounded bg-slate-800/60 px-2 py-1 text-xs text-slate-300">
              {wizard.cron}
            </code>
          </div>
        ) : null}
        <div className="grid gap-1">
          <span className="text-xs uppercase tracking-wide text-slate-500">Workflow Plan</span>
          {isPreviewing ? (
            <span className="text-sm text-slate-300">Planning workflow…</span>
          ) : planPreview ? (
            <div className="grid gap-1 text-sm text-slate-300">
              <span>
                Confidence: <strong>{String(planPreview?.confidence || "unknown")}</strong>
              </span>
              <span>
                Execution target:{" "}
                <strong>{String(planPreview?.execution_target || "automation_v2")}</strong>
              </span>
              {effectivePlanTitle ? (
                <span>
                  Title: <strong>{effectivePlanTitle}</strong>
                </span>
              ) : null}
              <span>
                Steps:{" "}
                <strong>{Array.isArray(planPreview?.steps) ? planPreview.steps.length : 0}</strong>
              </span>
              {Array.isArray(planPreview?.steps) && planPreview.steps.length ? (
                <div className="mt-1 grid gap-1">
                  {planPreview.steps.map((step: any, index: number) => {
                    const stepId = String(step?.step_id || step?.stepId || `step-${index + 1}`);
                    const expanded = !!expandedStepIds[stepId];
                    return (
                      <div
                        key={`${stepId}-${index}`}
                        className="rounded-lg border border-slate-800 bg-slate-950/40"
                      >
                        <button
                          className="flex w-full items-center justify-between gap-3 px-3 py-2 text-left"
                          onClick={() => toggleStepExpanded(stepId)}
                        >
                          <div className="min-w-0">
                            <div className="text-xs font-medium text-slate-200">
                              {stepId}
                              {step?.kind ? (
                                <span className="ml-2 text-[11px] uppercase tracking-wide text-slate-500">
                                  {String(step.kind)}
                                </span>
                              ) : null}
                            </div>
                          </div>
                          <span className="tcp-subtle shrink-0 text-xs">
                            {expanded ? "Hide" : "Details"}
                          </span>
                        </button>
                        {expanded &&
                        typeof step?.objective === "string" &&
                        step.objective.trim() ? (
                          <div className="border-t border-slate-800 px-3 py-3">
                            <div
                              className="text-sm text-slate-300"
                              dangerouslySetInnerHTML={{
                                __html: renderMarkdownSafe(step.objective || ""),
                              }}
                            />
                          </div>
                        ) : null}
                      </div>
                    );
                  })}
                </div>
              ) : null}
            </div>
          ) : (
            <span className="text-sm text-slate-400">
              Workflow preview has not been generated yet.
            </span>
          )}
        </div>
      </div>
      {plannerError ? (
        <div className="rounded-xl border border-red-500/40 bg-red-950/30 p-3 text-sm text-red-200">
          {plannerError}
        </div>
      ) : null}
      {overlapAnalysis?.matched_plan_id || overlapAnalysis?.matchedPlanId ? (
        <div className="rounded-xl border border-indigo-500/30 bg-indigo-950/20 p-3 text-sm text-indigo-100">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="font-medium text-indigo-200">Overlap review</div>
            <span className={overlapRequiresConfirmation ? "tcp-badge-warning" : "tcp-badge-info"}>
              {overlapRequiresConfirmation ? "confirmation required" : "decision ready"}
            </span>
          </div>
          <div className="mt-2 grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
            <div>
              prior plan:{" "}
              <strong>
                {String(overlapAnalysis?.matched_plan_id || overlapAnalysis?.matchedPlanId)}
              </strong>
            </div>
            <div>
              revision:{" "}
              <strong>
                {String(
                  overlapAnalysis?.matched_plan_revision ||
                    overlapAnalysis?.matchedPlanRevision ||
                    "n/a"
                )}
              </strong>
            </div>
            <div>
              match layer: <strong>{overlapMatchLayer || "n/a"}</strong>
            </div>
            <div>
              recommended decision:{" "}
              <strong>{String(overlapAnalysis?.decision || "new").toLowerCase()}</strong>
            </div>
          </div>
          {Number.isFinite(overlapScore) ? (
            <div className="mt-2 text-xs text-indigo-200/80">
              Similarity score: {(overlapScore * 100).toFixed(0)}%
            </div>
          ) : null}
          {overlapRequiresConfirmation ? (
            <div className="mt-3 grid gap-2">
              <div className="text-xs uppercase tracking-wide text-indigo-200/80">
                Choose how to handle this overlap
              </div>
              <div className="flex flex-wrap gap-2">
                {["reuse", "merge", "fork", "new"].map((decision) => (
                  <button
                    key={decision}
                    type="button"
                    className={
                      overlapDecision === decision
                        ? "tcp-btn-primary h-8 px-3 text-xs"
                        : "tcp-btn h-8 px-3 text-xs"
                    }
                    onClick={() => onSelectOverlapDecision(decision)}
                  >
                    {decision}
                  </button>
                ))}
              </div>
              {!overlapDecision ? (
                <div className="text-xs text-amber-200">
                  Select a decision before creating the automation.
                </div>
              ) : null}
            </div>
          ) : null}
        </div>
      ) : null}
      {planningChangeSummary.length ? (
        <div className="rounded-xl border border-emerald-500/30 bg-emerald-950/20 p-3">
          <div className="text-xs uppercase tracking-wide text-emerald-300">
            Latest Plan Changes
          </div>
          <div className="mt-2 flex flex-wrap gap-2">
            {planningChangeSummary.map((item: string, index: number) => (
              <span key={`${item}-${index}`} className="tcp-badge-ok">
                {item}
              </span>
            ))}
          </div>
        </div>
      ) : null}
      {planPreview ? (
        <div className="grid gap-3 rounded-xl border border-slate-700/60 bg-slate-900/40 p-4">
          <div className="flex items-center justify-between gap-2">
            <span className="text-xs uppercase tracking-wide text-slate-500">Planning Chat</span>
            <button
              className="tcp-btn h-7 px-2 text-xs"
              disabled={isResettingPlanningChat || !planPreview?.plan_id}
              onClick={onResetPlanningChat}
            >
              {isResettingPlanningChat ? "Resetting…" : "Reset Plan"}
            </button>
          </div>
          <div className="max-h-56 overflow-auto rounded-lg border border-slate-800 bg-slate-950/50 p-3">
            {Array.isArray(planningConversation?.messages) &&
            planningConversation.messages.length ? (
              <div className="grid gap-3">
                {planningConversation.messages.map((message: any, index: number) => (
                  <div key={`${message?.created_at_ms || index}-${index}`} className="grid gap-1">
                    <span className="text-[11px] uppercase tracking-wide text-slate-500">
                      {String(message?.role || "assistant")}
                    </span>
                    <div className="text-sm text-slate-200">
                      {String(message?.text || "").trim()}
                    </div>
                  </div>
                ))}
              </div>
            ) : (
              <div className="text-sm text-slate-400">
                Add planning notes here to revise the workflow before creating it.
              </div>
            )}
          </div>
          <textarea
            className="tcp-input min-h-[84px] text-sm"
            value={planningNote}
            onInput={(e) => setPlanningNote((e.target as HTMLTextAreaElement).value)}
            placeholder='Example: "Make this weekly, run it from /srv/acme/app, and remove notifications."'
          />
          <div className="flex justify-end">
            <button
              className="tcp-btn-primary"
              disabled={isSendingPlanningMessage || !planningNote.trim() || !planPreview?.plan_id}
              onClick={() => {
                const note = planningNote.trim();
                if (!note) return;
                onSendPlanningMessage(note);
                setPlanningNote("");
              }}
            >
              {isSendingPlanningMessage ? "Updating plan…" : "Update Plan"}
            </button>
          </div>
        </div>
      ) : null}
      <div className="rounded-xl border border-slate-700/40 bg-slate-800/20 p-3 text-xs text-slate-400">
        <label className="flex items-start gap-3 rounded-lg border border-slate-700/50 bg-slate-900/30 p-3 text-sm text-slate-300">
          <input
            type="checkbox"
            className="mt-0.5"
            checked={wizard.exportPackDraft}
            onChange={onToggleExportPackDraft}
          />
          <span className="grid gap-1">
            <span className="font-medium text-slate-200">Also export a reusable pack draft</span>
            <span className="text-xs text-slate-400">
              After creating the automation, Tandem will also create a Pack Builder draft so this
              workflow can be saved and reused later.
            </span>
          </span>
        </label>
      </div>
      {generatedSkill || installStatus ? (
        <div className="rounded-xl border border-slate-700/40 bg-slate-800/20 p-3 text-xs text-slate-400">
          <div className="text-xs uppercase tracking-wide text-slate-500">
            Reusable Skill Export
          </div>
          <div className="mt-1 grid gap-1">
            {generatedSkill ? (
              <>
                <span>
                  Draft status:{" "}
                  <strong className="text-slate-300">
                    {String(generatedSkill?.status || "generated")}
                  </strong>
                </span>
                <span className="text-amber-200">
                  This draft is prompt-based and may be stale if you changed the workflow plan in
                  planning chat.
                </span>
              </>
            ) : null}
            {installStatus ? <span>{installStatus}</span> : null}
          </div>
        </div>
      ) : null}
      <div className="rounded-xl border border-slate-700/40 bg-slate-800/20 p-3 text-xs text-slate-400">
        💡 Tandem will save this automation and schedule a{" "}
        <strong className="text-slate-300">{modeInfo?.label || effectiveMode}</strong> that runs{" "}
        <strong className="text-slate-300">{effectiveSchedule}</strong>. You can pause, edit or
        delete it anytime.
      </div>
      <button
        className="tcp-btn-primary"
        disabled={
          isPending ||
          isPreviewing ||
          !wizard.goal.trim() ||
          !planPreview ||
          (overlapRequiresConfirmation && !overlapDecision)
        }
        onClick={onSubmit}
      >
        {isPending ? "Creating automation…" : "🚀 Create Automation"}
      </button>
    </div>
  );
}
