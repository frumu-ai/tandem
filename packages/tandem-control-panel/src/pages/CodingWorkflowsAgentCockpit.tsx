import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge, PanelCard } from "../ui/index.tsx";
import { TandemLogoAnimation } from "../ui/TandemLogoAnimation";
import { EmptyState } from "./ui";
import { formatStatus, runStatus, runTitle, runUpdatedAt, toArray } from "./CodingWorkflowsHelpers";
import { subscribeSse } from "../services/sse.js";
import { api } from "../lib/api.ts";

type ThreadEntry = {
  id: string;
  kind: "operator" | "agent" | "system" | "linear" | "github";
  title: string;
  body: string;
  atMs: number;
  meta?: string;
};

function safeText(value: any, fallback = "unknown") {
  const text = String(value ?? "").trim();
  return text || fallback;
}

function formatTime(value: any) {
  const timestamp = Number(value || 0);
  if (!timestamp) return "not recorded";
  return new Date(timestamp).toLocaleString();
}

function timestampMs(value: any) {
  if (value === undefined || value === null || value === "") return 0;
  if (typeof value === "number" && Number.isFinite(value)) {
    return value > 0 && value < 10_000_000_000 ? value * 1000 : value;
  }
  const text = String(value).trim();
  if (!text) return 0;
  const numeric = Number(text);
  if (Number.isFinite(numeric)) {
    return numeric > 0 && numeric < 10_000_000_000 ? numeric * 1000 : numeric;
  }
  const parsed = Date.parse(text);
  return Number.isFinite(parsed) ? parsed : 0;
}

function sourceKind(eventType: string): ThreadEntry["kind"] {
  const key = eventType.toLowerCase();
  if (key.includes("linear")) return "linear";
  if (key.includes("github") || key.includes("pull_request")) return "github";
  if (key.includes("coder") || key.includes("agent") || key.includes("repair")) return "agent";
  return "system";
}

function entryTone(kind: ThreadEntry["kind"]): "ok" | "warn" | "err" | "info" | "ghost" {
  if (kind === "linear") return "ok";
  if (kind === "github") return "info";
  if (kind === "operator") return "warn";
  if (kind === "agent") return "ghost";
  return "ghost";
}

function compactJson(value: any) {
  if (value === undefined || value === null || value === "") return "";
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function taskSourceLabel(source: any) {
  const type = String(source?.type || "").trim();
  if (type === "linear") return "Linear issue";
  if (type === "github_project") return "GitHub Project item";
  if (type === "kanban_board") return "Kanban card";
  if (type === "manual") return "Manual task";
  return type ? formatStatus(type) : "Task source";
}

function feedbackMessageEntry(message: any): ThreadEntry {
  const state = String(message?.delivery_state || "pending").trim();
  return {
    id: String(message?.id || `${message?.run_id || "run"}:${message?.seq || Date.now()}`),
    kind: "operator",
    title: "Operator feedback",
    body: String(message?.body || "").trim(),
    atMs: timestampMs(message?.created_at_ms || message?.timestamp_ms || 0),
    meta: [
      `seq ${message?.seq || "?"}`,
      state ? `delivery ${formatStatus(state)}` : "",
      message?.actor ? `actor ${message.actor}` : "",
    ]
      .filter(Boolean)
      .join(" · "),
  };
}

function buildThreadEntries({
  runId,
  run,
  events,
  summary,
  blackboard,
  feedbackMessages,
}: {
  runId: string;
  run: any;
  events: any[];
  summary: string;
  blackboard: any;
  feedbackMessages: any[];
}) {
  const rows: ThreadEntry[] = [];
  const createdAt = Number(run?.created_at_ms || run?.snapshot?.created_at_ms || 0);
  rows.push({
    id: `${runId}:run`,
    kind: "system",
    title: "ACA run selected",
    body: `${runTitle(run)} is ${formatStatus(runStatus(run))}.`,
    atMs: createdAt || runUpdatedAt(run),
    meta: runId,
  });
  const task = blackboard?.task || run?.blackboard?.task || run?.snapshot?.blackboard?.task || {};
  const source = task?.source || {};
  if (source?.identifier || source?.issue_url || source?.issueUrl || source?.project_item_id) {
    rows.push({
      id: `${runId}:source`,
      kind: String(source?.type || "") === "linear" ? "linear" : "github",
      title: taskSourceLabel(source),
      body: [
        source?.identifier ? `Identifier: ${source.identifier}` : "",
        source?.status ? `Status: ${source.status}` : "",
        source?.issue_url || source?.issueUrl ? `URL: ${source.issue_url || source.issueUrl}` : "",
      ]
        .filter(Boolean)
        .join("\n"),
      atMs: createdAt || runUpdatedAt(run),
      meta: safeText(source?.type, "source"),
    });
  }
  if (summary) {
    rows.push({
      id: `${runId}:summary`,
      kind: "agent",
      title: "Agent summary",
      body: summary,
      atMs: runUpdatedAt(run),
      meta: "handoff",
    });
  }
  events.forEach((event, index) => {
    const eventType = String(event?.type || event?.event_type || event?.event || "event").trim();
    const payload = event?.payload ?? event?.properties ?? event?.metadata ?? event;
    rows.push({
      id: `${runId}:event:${index}:${eventType}`,
      kind: sourceKind(eventType),
      title: formatStatus(eventType),
      body: compactJson(payload).slice(0, 1400),
      atMs: timestampMs(
        event?.timestamp_ms || event?.created_at_ms || event?.at_ms || event?.timestamp || 0
      ),
      meta: eventType,
    });
  });
  return [...rows, ...feedbackMessages.map(feedbackMessageEntry)].sort((a, b) => {
    const left = Number(a.atMs || 0);
    const right = Number(b.atMs || 0);
    if (left !== right) return left - right;
    return a.id.localeCompare(b.id);
  });
}

export function CodingWorkflowsAgentCockpit({
  selectedRunId,
  selectedRun,
  selectedProject,
  runDetailQuery,
  coderRuns,
  reconcileCoderRun,
  cancelCoderRun,
  lastRunEvent,
}: {
  selectedRunId: string;
  selectedRun: any;
  selectedProject: any;
  runDetailQuery: any;
  coderRuns: any[];
  reconcileCoderRun: (runId: string) => void;
  cancelCoderRun: (runId: string) => void;
  lastRunEvent: string;
}) {
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState("");
  const [feedbackMessages, setFeedbackMessages] = useState<any[]>([]);
  const [feedbackLoading, setFeedbackLoading] = useState(false);
  const [feedbackError, setFeedbackError] = useState("");
  const [sendingFeedback, setSendingFeedback] = useState(false);
  const [approvalBusyId, setApprovalBusyId] = useState("");
  const [approvalHistoryOpen, setApprovalHistoryOpen] = useState(false);
  const [taskResetting, setTaskResetting] = useState(false);
  const [actionNotice, setActionNotice] = useState("");
  const runId = String(selectedRunId || "").trim();

  useEffect(() => {
    setDraft("");
    setFeedbackMessages([]);
    setFeedbackError("");
    if (!runId) return;
    let cancelled = false;
    setFeedbackLoading(true);
    api(`/api/aca/runs/${encodeURIComponent(runId)}/feedback`)
      .then((payload: any) => {
        if (!cancelled) setFeedbackMessages(toArray(payload, "messages"));
      })
      .catch((error: any) => {
        if (!cancelled) setFeedbackError(error instanceof Error ? error.message : String(error));
      })
      .finally(() => {
        if (!cancelled) setFeedbackLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [api, runId]);

  useEffect(() => {
    if (!runId) return;
    const url = `/api/aca/runs/${encodeURIComponent(runId)}/feedback/events`;
    const unsubscribe = subscribeSse(url, (event: MessageEvent) => {
      let envelope: any = null;
      try {
        envelope = JSON.parse(String(event?.data || "{}"));
      } catch {
        envelope = null;
      }
      if (!envelope || envelope.event_type === "hello" || envelope.event_type === "ping") return;
      const message = envelope?.payload?.message;
      if (!message?.id) return;
      setFeedbackMessages((current) => {
        const next = current.filter((entry: any) => String(entry?.id || "") !== String(message.id));
        next.push(message);
        return next.sort((a: any, b: any) => Number(a?.seq || 0) - Number(b?.seq || 0));
      });
    });
    return () => unsubscribe();
  }, [runId]);

  const detail = runDetailQuery.data || {};
  const pendingApprovalsQuery = useQuery({
    queryKey: ["aca", "run-approvals", runId, "pending"],
    queryFn: async () => {
      if (!runId) return { approvals: [] };
      return api(`/api/aca/runs/${encodeURIComponent(runId)}/approvals?status=pending`).catch(() => ({ approvals: [] }));
    },
    enabled: Boolean(runId),
    refetchInterval: 5000,
  });
  const failedApprovalsQuery = useQuery({
    queryKey: ["aca", "run-approvals", runId, "failed"],
    queryFn: async () => {
      if (!runId) return { approvals: [] };
      return api(`/api/aca/runs/${encodeURIComponent(runId)}/approvals?status=failed`).catch(() => ({ approvals: [] }));
    },
    enabled: Boolean(runId),
    refetchInterval: 5000,
  });
  const approvalHistoryQuery = useQuery({
    queryKey: ["aca", "run-approvals", runId, "history"],
    queryFn: async () => {
      if (!runId) return { approvals: [] };
      return api(`/api/aca/runs/${encodeURIComponent(runId)}/approvals?limit=50`).catch(() => ({ approvals: [] }));
    },
    enabled: Boolean(runId && approvalHistoryOpen),
  });
  const blackboard = detail.blackboard || selectedRun?.blackboard || selectedRun?.snapshot?.blackboard || {};
  const task = blackboard?.task || selectedRun?.blackboard?.task || selectedRun?.snapshot?.blackboard?.task || {};
  const source = task?.source || {};
  const repo = task?.repo || blackboard?.repo || selectedRun?.repo || {};
  const pullRequest = blackboard?.pull_request_lifecycle || detail.pull_request_lifecycle || {};
  const merge = blackboard?.pull_request_merge || detail.pull_request_merge || {};
  const coderRun = coderRuns.find((row: any) => {
    const acaRunId = String(row?.run_id || row?.aca_run_id || "").trim();
    const tandemRunId = String(row?.coder_run_id || row?.tandem_run_id || "").trim();
    return acaRunId === runId || tandemRunId === runId;
  });
  const events = useMemo(() => toArray(detail, "events"), [detail]);
  const pendingApprovals = useMemo(
    () => toArray(pendingApprovalsQuery.data || {}, "approvals"),
    [pendingApprovalsQuery.data]
  );
  const failedApprovals = useMemo(
    () => toArray(failedApprovalsQuery.data || {}, "approvals"),
    [failedApprovalsQuery.data]
  );
  const approvalHistory = useMemo(
    () =>
      toArray(approvalHistoryQuery.data || {}, "approvals").filter(
        (approval: any) => !["pending", "failed"].includes(String(approval?.status || ""))
      ),
    [approvalHistoryQuery.data]
  );
  const approvalsLoading = pendingApprovalsQuery.isLoading || failedApprovalsQuery.isLoading;
  const summary = String(detail.summary || "").trim();
  const threadEntries = useMemo(
    () =>
      runId && selectedRun
        ? buildThreadEntries({
            runId,
            run: selectedRun,
            events,
            summary,
            blackboard,
            feedbackMessages,
          })
        : [],
    [blackboard, events, feedbackMessages, runId, selectedRun, summary]
  );
  const sourceType = String(source?.type || selectedProject?.taskSource?.type || "").trim();
  const prUrl = String(pullRequest?.url || blackboard?.pull_request || "").trim();

  async function addFeedback() {
    const text = draft.trim();
    if (!text || !runId || sendingFeedback) return;
    setSendingFeedback(true);
    setFeedbackError("");
    try {
      const payload = await api(`/api/aca/runs/${encodeURIComponent(runId)}/feedback`, {
        method: "POST",
        body: JSON.stringify({
          body: text,
          actor: "operator",
          kind: "operator_feedback",
          task_id: task?.id || task?.identifier || source?.identifier || "",
          thread_id: runId,
          source_refs: {
            source_type: source?.type || "",
            source_identifier: source?.identifier || source?.issue_id || "",
            source_url: source?.issue_url || source?.issueUrl || "",
            pull_request_url: prUrl || "",
          },
        }),
      });
      const message = payload?.message;
      if (message?.id) {
        setFeedbackMessages((current) => {
          const next = current.filter((entry: any) => String(entry?.id || "") !== String(message.id));
          next.push(message);
          return next.sort((a: any, b: any) => Number(a?.seq || 0) - Number(b?.seq || 0));
        });
      }
      setDraft("");
    } catch (error: any) {
      setFeedbackError(error instanceof Error ? error.message : String(error));
    } finally {
      setSendingFeedback(false);
    }
  }

  async function decideApproval(approvalId: string, decision: "approve" | "reject") {
    if (!approvalId || approvalBusyId) return;
    setApprovalBusyId(approvalId);
    setFeedbackError("");
    try {
      await api(`/api/aca/approvals/${encodeURIComponent(approvalId)}/${decision}`, {
        method: "POST",
        body: JSON.stringify({ actor: "operator" }),
      });
      if (decision === "approve") {
        await api(`/api/aca/runs/${encodeURIComponent(runId)}/resume-approved-actions`, {
          method: "POST",
        }).catch(() => ({}));
      }
      await pendingApprovalsQuery.refetch();
      await failedApprovalsQuery.refetch();
      if (approvalHistoryOpen) await approvalHistoryQuery.refetch();
      await runDetailQuery.refetch?.();
    } catch (error: any) {
      setFeedbackError(error instanceof Error ? error.message : String(error));
    } finally {
      setApprovalBusyId("");
    }
  }

  async function approvePendingApprovals() {
    if (!runId || approvalBusyId || !pendingApprovals.length) return;
    setApprovalBusyId("batch-approve");
    setFeedbackError("");
    try {
      await api(`/api/aca/runs/${encodeURIComponent(runId)}/approvals/approve-pending`, {
        method: "POST",
        body: JSON.stringify({ actor: "operator" }),
      });
      await api(`/api/aca/runs/${encodeURIComponent(runId)}/resume-approved-actions`, {
        method: "POST",
      }).catch(() => ({}));
      await pendingApprovalsQuery.refetch();
      await failedApprovalsQuery.refetch();
      if (approvalHistoryOpen) await approvalHistoryQuery.refetch();
      await runDetailQuery.refetch?.();
    } catch (error: any) {
      setFeedbackError(error instanceof Error ? error.message : String(error));
    } finally {
      setApprovalBusyId("");
    }
  }

  async function retryFailedApprovals() {
    if (!runId || approvalBusyId || !failedApprovals.length) return;
    setApprovalBusyId("retry-failed");
    setFeedbackError("");
    try {
      await api(`/api/aca/runs/${encodeURIComponent(runId)}/approvals/retry-failed`, {
        method: "POST",
        body: JSON.stringify({ actor: "operator" }),
      });
      await api(`/api/aca/runs/${encodeURIComponent(runId)}/resume-approved-actions`, {
        method: "POST",
      }).catch(() => ({}));
      await pendingApprovalsQuery.refetch();
      await failedApprovalsQuery.refetch();
      if (approvalHistoryOpen) await approvalHistoryQuery.refetch();
      await runDetailQuery.refetch?.();
    } catch (error: any) {
      setFeedbackError(error instanceof Error ? error.message : String(error));
    } finally {
      setApprovalBusyId("");
    }
  }

  async function resetBlockedTaskToBacklog() {
    const projectSlug = String(selectedProject?.slug || selectedProject?.id || "").trim();
    const itemRef = String(
      source?.identifier || source?.issue_id || source?.issueId || task?.task_id || task?.id || ""
    ).trim();
    if (!projectSlug || !itemRef || taskResetting) return;
    setTaskResetting(true);
    setActionNotice("");
    setFeedbackError("");
    try {
      await api(`/api/aca/projects/${encodeURIComponent(projectSlug)}/tasks/${encodeURIComponent(itemRef)}/state`, {
        method: "POST",
        body: JSON.stringify({ state: "backlog" }),
      });
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-project-board", projectSlug] }),
        queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-project-tasks", projectSlug] }),
        queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-runs"] }),
        runDetailQuery.refetch?.(),
      ]);
      setActionNotice(`Moved ${itemRef} back to Backlog.`);
    } catch (error: any) {
      setFeedbackError(error instanceof Error ? error.message : String(error));
    } finally {
      setTaskResetting(false);
    }
  }

  if (!runId || !selectedRun) {
    const hydratingRun = Boolean(runId || lastRunEvent || runDetailQuery.isLoading || runDetailQuery.isFetching);
    if (hydratingRun) {
      return (
        <PanelCard
          title="Agent cockpit"
          subtitle="Connecting to the active ACA run"
          actions={lastRunEvent ? <Badge tone="ghost">Live {formatStatus(lastRunEvent)}</Badge> : null}
        >
          <div className="flex min-h-[180px] flex-col items-center justify-center gap-4 border border-white/10 bg-black/20 p-6 text-center">
            <TandemLogoAnimation
              className="h-20 w-20"
              mode="compact"
              title="Loading ACA run"
            />
            <div>
              <div className="text-sm font-semibold text-slate-100">
                {runId ? "Loading run cockpit" : "Waiting for the run to appear"}
              </div>
              <div className="tcp-subtle mt-2 max-w-xl text-sm">
                {runId
                  ? `Tandem is hydrating ${runId} with status, thread, approvals, and logs.`
                  : "Tandem received the run event and is waiting for ACA to publish the run record."}
              </div>
            </div>
          </div>
        </PanelCard>
      );
    }
    return (
      <PanelCard title="Agent cockpit" subtitle="Select an ACA run to inspect its operational thread.">
        <EmptyState text="No run selected. Start or select a Coder run from Intake or Overview." />
      </PanelCard>
    );
  }

  const prState = String(pullRequest?.lifecycle_state || pullRequest?.state || "").trim();
  const runState = runStatus(selectedRun);
  const live = !["completed", "done", "failed", "cancelled", "canceled", "blocked"].includes(runState);
  const projectSlug = String(selectedProject?.slug || selectedProject?.id || "").trim();
  const taskRef = String(
    source?.identifier || source?.issue_id || source?.issueId || task?.task_id || task?.id || ""
  ).trim();
  const canResetBlockedTask =
    sourceType === "linear" &&
    Boolean(projectSlug && taskRef) &&
    ["blocked", "failed", "cancelled", "canceled"].includes(runState);
  const blocker =
    detail?.status?.blocker ||
    detail?.blocker ||
    selectedRun?.blocker ||
    selectedRun?.snapshot?.blocker ||
    {};
  const blockerActive = Boolean(blocker?.active || blocker?.kind || blocker?.message);
  const actionUnavailable = "Backend action route not connected yet";
  const visibleApprovalCount =
    pendingApprovals.length + failedApprovals.length + (approvalHistoryOpen ? approvalHistory.length : 0);
  const approvalsPanel = (
    <PanelCard
      title="External approvals"
      subtitle="Approval-gated MCP actions for this run"
      actions={
        <div className="flex flex-wrap gap-2">
          <Badge tone={pendingApprovals.length ? "warn" : "ghost"}>{pendingApprovals.length} pending</Badge>
          {failedApprovals.length ? <Badge tone="err">{failedApprovals.length} failed</Badge> : null}
        </div>
      }
    >
      {approvalsLoading ? (
        <div className="tcp-subtle text-sm">Checking approvals...</div>
      ) : (
        <div className="grid gap-3">
          {pendingApprovals.length || failedApprovals.length ? (
            <div className="flex flex-wrap gap-2">
              {pendingApprovals.length ? (
                <button
                  type="button"
                  className="tcp-btn-primary h-8 px-3 text-xs"
                  disabled={Boolean(approvalBusyId)}
                  onClick={approvePendingApprovals}
                >
                  Approve pending ({pendingApprovals.length})
                </button>
              ) : null}
              {failedApprovals.length ? (
                <button
                  type="button"
                  className="tcp-btn h-8 px-3 text-xs"
                  disabled={Boolean(approvalBusyId)}
                  onClick={retryFailedApprovals}
                >
                  Retry failed ({failedApprovals.length})
                </button>
              ) : null}
            </div>
          ) : null}

          {pendingApprovals.length ? (
            <div className="grid max-h-72 gap-3 overflow-auto pr-1 md:grid-cols-2">
              {pendingApprovals.map((approval: any) => {
                const approvalId = String(approval?.approval_id || "");
                const target = approval?.target || {};
                const payload = approval?.payload || {};
                return (
                  <article key={approvalId} className="rounded-lg border border-amber-400/30 bg-amber-950/15 p-3 text-xs">
                    <div className="flex items-start justify-between gap-2">
                      <div className="min-w-0">
                        <div className="font-semibold text-slate-100">{formatStatus(String(approval?.action_type || "action"))}</div>
                        <div className="tcp-subtle mt-1 truncate">
                          {safeText(target.base_repo || target.identifier)}
                          {target.pr_number ? `#${target.pr_number}` : ""}
                        </div>
                      </div>
                      <Badge tone="warn">Pending</Badge>
                    </div>
                    {payload.body ? (
                      <pre className="mt-2 max-h-20 overflow-auto whitespace-pre-wrap break-words text-[11px] leading-5 text-slate-300">
                        {String(payload.body).slice(0, 700)}
                      </pre>
                    ) : null}
                    <div className="mt-3 flex gap-2">
                      <button
                        type="button"
                        className="tcp-btn-primary h-8 px-3 text-xs"
                        disabled={Boolean(approvalBusyId)}
                        onClick={() => decideApproval(approvalId, "approve")}
                      >
                        Approve
                      </button>
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs"
                        disabled={Boolean(approvalBusyId)}
                        onClick={() => decideApproval(approvalId, "reject")}
                      >
                        Reject
                      </button>
                    </div>
                  </article>
                );
              })}
            </div>
          ) : (
            <div className="rounded-lg border border-white/10 bg-black/20 p-3 text-sm text-slate-300">
              No pending approvals.
            </div>
          )}

          {failedApprovals.length ? (
            <div className="grid max-h-48 gap-2 overflow-auto pr-1 md:grid-cols-2">
              {failedApprovals.map((approval: any) => {
                const approvalId = String(approval?.approval_id || "");
                const target = approval?.target || {};
                return (
                  <div key={approvalId} className="rounded-lg border border-red-400/25 bg-red-950/15 p-3 text-xs">
                    <div className="flex items-start justify-between gap-2">
                      <div className="min-w-0">
                        <div className="truncate font-semibold text-slate-200">
                          {formatStatus(String(approval?.action_type || "action"))}
                        </div>
                        <div className="tcp-subtle mt-1 truncate">
                          {safeText(target.base_repo || target.identifier)}
                          {target.pr_number ? `#${target.pr_number}` : ""}
                        </div>
                      </div>
                      <Badge tone="err">Failed</Badge>
                    </div>
                    {approval?.error ? (
                      <div className="mt-2 line-clamp-2 break-words text-red-200">{String(approval.error)}</div>
                    ) : null}
                  </div>
                );
              })}
            </div>
          ) : null}

          <details
            className="rounded-lg border border-white/10 bg-black/20 p-3 text-xs"
            open={approvalHistoryOpen}
            onToggle={(event) => setApprovalHistoryOpen((event.currentTarget as HTMLDetailsElement).open)}
          >
            <summary className="cursor-pointer select-none font-semibold text-slate-200">
              History{approvalHistoryOpen ? ` (${approvalHistory.length})` : ""}
            </summary>
            <div className="mt-3">
              {approvalHistoryQuery.isLoading ? (
                <div className="tcp-subtle">Loading approval history...</div>
              ) : approvalHistory.length ? (
                <div className="grid max-h-48 gap-2 overflow-auto pr-1 md:grid-cols-2">
                  {approvalHistory.map((approval: any) => {
                    const approvalId = String(approval?.approval_id || "");
                    const target = approval?.target || {};
                    const status = String(approval?.status || "unknown");
                    return (
                      <div key={approvalId} className="border border-white/10 bg-black/20 p-2">
                        <div className="flex items-start justify-between gap-2">
                          <div className="min-w-0">
                            <div className="truncate font-semibold text-slate-200">
                              {formatStatus(String(approval?.action_type || "action"))}
                            </div>
                            <div className="tcp-subtle mt-1 truncate">
                              {safeText(target.base_repo || target.identifier)}
                              {target.pr_number ? `#${target.pr_number}` : ""}
                            </div>
                          </div>
                          <Badge tone={status === "executed" ? "ok" : status === "failed" ? "err" : "ghost"}>
                            {formatStatus(status)}
                          </Badge>
                        </div>
                      </div>
                    );
                  })}
                </div>
              ) : (
                <div className="tcp-subtle">No historical approvals loaded for this run.</div>
              )}
            </div>
          </details>

          {!visibleApprovalCount && !approvalHistoryOpen ? (
            <div className="tcp-subtle text-xs">Executed and rejected approvals stay in History.</div>
          ) : null}
        </div>
      )}
    </PanelCard>
  );

  return (
    <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_360px]">
      <div className="grid gap-4 min-w-0">
        <PanelCard
          title="Agent cockpit"
          subtitle="Operational thread for the selected ACA task and run"
          actions={
            <div className="flex flex-wrap gap-2">
              <Badge tone={live ? "info" : "ghost"}>{formatStatus(runState)}</Badge>
              {lastRunEvent ? <Badge tone="ghost">Live {formatStatus(lastRunEvent)}</Badge> : null}
            </div>
          }
        >
          <div className="grid gap-3 md:grid-cols-3">
            <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
              <div className="tcp-kpi-label text-xs">Linear / source</div>
              <div className="mt-2 text-sm font-semibold">
                {safeText(source?.identifier || source?.issue_id || source?.project_item_id, "No issue linked")}
              </div>
              <div className="tcp-subtle mt-1 text-xs">{taskSourceLabel(source)}</div>
              {source?.issue_url || source?.issueUrl ? (
                <a
                  className="mt-2 block truncate text-xs text-sky-200 hover:text-sky-100"
                  href={String(source.issue_url || source.issueUrl)}
                  target="_blank"
                  rel="noreferrer"
                >
                  {String(source.issue_url || source.issueUrl)}
                </a>
              ) : null}
            </div>
            <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
              <div className="tcp-kpi-label text-xs">ACA run</div>
              <div className="mt-2 truncate text-sm font-semibold">{runId}</div>
              <div className="tcp-subtle mt-1 text-xs">
                {formatStatus(runState)} · {formatTime(runUpdatedAt(selectedRun))}
              </div>
              <div className="tcp-subtle mt-1 truncate text-xs">
                {safeText(coderRun?.coder_run_id || coderRun?.tandem_run_id, "No Tandem coder id")}
              </div>
            </div>
            <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
              <div className="tcp-kpi-label text-xs">GitHub PR</div>
              <div className="mt-2 text-sm font-semibold">{prState ? formatStatus(prState) : "No PR state"}</div>
              {prUrl ? (
                <a
                  className="mt-1 block truncate text-xs text-sky-200 hover:text-sky-100"
                  href={prUrl}
                  target="_blank"
                  rel="noreferrer"
                >
                  {prUrl}
                </a>
              ) : (
                <div className="tcp-subtle mt-1 text-xs">PR link appears after ACA opens one.</div>
              )}
              {merge?.status ? (
                <div className="tcp-subtle mt-1 text-xs">Merge {formatStatus(String(merge.status))}</div>
              ) : null}
            </div>
          </div>
        </PanelCard>

        {blockerActive ? (
          <PanelCard
            title="Run blocker"
            subtitle="Actionable recovery details from ACA"
            actions={<Badge tone="err">{formatStatus(String(blocker?.kind || "blocked"))}</Badge>}
          >
            <div className="grid gap-3 text-sm">
              <div className="rounded-lg border border-red-400/25 bg-red-950/20 p-3 text-red-100">
                {safeText(blocker?.message, "Run is blocked.")}
              </div>
              {blocker?.detail ? (
                <pre className="max-h-32 overflow-auto whitespace-pre-wrap break-words rounded-lg border border-white/10 bg-black/20 p-3 text-xs leading-5 text-slate-200">
                  {String(blocker.detail)}
                </pre>
              ) : null}
              {blocker?.recovery_action ? (
                <div className="rounded-lg border border-amber-400/25 bg-amber-950/20 p-3 text-xs text-amber-100">
                  {String(blocker.recovery_action)}
                </div>
              ) : null}
            </div>
          </PanelCard>
        ) : null}

        <PanelCard
          title="Thread"
          subtitle="System, agent, Linear, GitHub, and operator messages"
          actions={<Badge tone={threadEntries.length ? "info" : "ghost"}>{threadEntries.length} entries</Badge>}
        >
          {runDetailQuery.isLoading ? (
            <div className="tcp-subtle text-sm">Loading run thread...</div>
          ) : runDetailQuery.isError ? (
            <div className="rounded-2xl border border-red-500/20 bg-red-500/10 p-4 text-sm text-red-200">
              {runDetailQuery.error instanceof Error
                ? runDetailQuery.error.message
                : "Could not load run detail."}
            </div>
          ) : threadEntries.length ? (
            <div className="grid gap-3">
              {threadEntries.map((entry) => (
                <article key={entry.id} className="rounded-2xl border border-white/10 bg-black/20 p-4">
                  <div className="flex flex-wrap items-start justify-between gap-3">
                    <div className="min-w-0">
                      <div className="text-sm font-semibold">{entry.title}</div>
                      <div className="tcp-subtle mt-1 text-xs">
                        {formatTime(entry.atMs)}
                        {entry.meta ? ` · ${entry.meta}` : ""}
                      </div>
                    </div>
                    <Badge tone={entryTone(entry.kind)}>{formatStatus(entry.kind)}</Badge>
                  </div>
                  {entry.body ? (
                    <pre className="mt-3 max-h-56 overflow-auto whitespace-pre-wrap break-words text-xs leading-6 text-slate-200">
                      {entry.body}
                    </pre>
                  ) : null}
                </article>
              ))}
            </div>
          ) : (
            <EmptyState text="No run events are available yet." />
          )}
        </PanelCard>

        {approvalsPanel}
      </div>

      <div className="grid gap-4 content-start">
        <PanelCard title="Operator feedback" subtitle="Send feedback to the active run thread">
          <div className="grid gap-3">
            <textarea
              className="tcp-input min-h-[112px] resize-y"
              value={draft}
              onChange={(event) => setDraft((event.target as HTMLTextAreaElement).value)}
              placeholder="Leave task/run feedback for the active agent handoff."
            />
            <button
              type="button"
              className="tcp-btn-primary"
              onClick={addFeedback}
              disabled={!draft.trim() || sendingFeedback}
            >
              <i data-lucide="send"></i>
              {sendingFeedback ? "Sending..." : "Send feedback"}
            </button>
            {feedbackLoading ? <div className="tcp-subtle text-xs">Loading feedback...</div> : null}
            {feedbackError ? <div className="text-xs text-red-200">{feedbackError}</div> : null}
          </div>
        </PanelCard>

        <PanelCard title="Actions" subtitle="Controls for the selected task/run">
          <div className="grid gap-2">
            <button type="button" className="tcp-btn" onClick={() => reconcileCoderRun(runId)}>
              <i data-lucide="refresh-cw"></i>
              Refresh state
            </button>
            <button
              type="button"
              className="tcp-btn"
              onClick={resetBlockedTaskToBacklog}
              disabled={!canResetBlockedTask || taskResetting}
              title={
                canResetBlockedTask
                  ? "Move this Linear task back to Backlog so ACA can run it again."
                  : "Available for blocked or failed Linear ACA runs."
              }
            >
              <i data-lucide="rotate-ccw"></i>
              {taskResetting ? "Resetting..." : "Reset task to Backlog"}
            </button>
            <button
              type="button"
              className="tcp-btn"
              onClick={() => cancelCoderRun(runId)}
              disabled={!live}
            >
              <i data-lucide="pause"></i>
              Pause / cancel run
            </button>
            {[
              ["play", "Resume", actionUnavailable],
              ["wrench", "Request repair", actionUnavailable],
              ["badge-check", "Approve merge", actionUnavailable],
              ["x-circle", "Block task", actionUnavailable],
            ].map(([icon, label, reason]) => (
              <button key={label} type="button" className="tcp-btn" disabled title={reason}>
                <i data-lucide={icon}></i>
                {label}
              </button>
            ))}
            {actionNotice ? <div className="rounded-lg border border-emerald-400/20 bg-emerald-950/20 p-2 text-xs text-emerald-100">{actionNotice}</div> : null}
          </div>
        </PanelCard>

        <PanelCard title="Context" subtitle="Bound task and repository">
          <div className="grid gap-3 text-xs">
            <div className="rounded-2xl border border-white/10 bg-black/20 p-3">
              <div className="tcp-kpi-label">Task</div>
              <div className="mt-1 font-semibold text-slate-100">
                {safeText(task?.title || selectedRun?.task_title || runTitle(selectedRun), "Untitled")}
              </div>
              <div className="tcp-subtle mt-1">{safeText(sourceType, "unknown source")}</div>
            </div>
            <div className="rounded-2xl border border-white/10 bg-black/20 p-3">
              <div className="tcp-kpi-label">Repository</div>
              <div className="mt-1 break-words font-semibold text-slate-100">
                {safeText(repo?.slug || selectedProject?.slug, "No repo binding")}
              </div>
              <div className="tcp-subtle mt-1 break-words">{safeText(repo?.path, "No path recorded")}</div>
            </div>
          </div>
        </PanelCard>
      </div>
    </div>
  );
}
