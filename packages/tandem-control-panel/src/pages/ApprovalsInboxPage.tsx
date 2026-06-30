import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api } from "../lib/api";
import { renderMarkdownSafe } from "../lib/markdown";
import { TandemLogoAnimation } from "../ui/TandemLogoAnimation";
import { AnimatedPage, Badge, LoadingState, PanelCard, StatusPulse } from "../ui/index.tsx";
import { EmptyState, PageCard } from "./ui";
import type { AppPageProps } from "./pageTypes";

/**
 * ApprovalsInboxPage
 *
 * One operator-shaped page that aggregates every pending approval across
 * automation_v2 mission runs (and, in future, coder + workflow runs). Backed
 * by `GET /api/engine/approvals/pending` — the cross-subsystem aggregator
 * added in W1.5.
 *
 * Goals:
 * - Polled every 5s (SSE upgrade is a later improvement; polling is fine for v1).
 * - Approve / Reject / Rework against the authoritative subsystem handler
 *   (`POST /automations/v2/runs/{run_id}/gate`) — never re-implement decision
 *   logic here.
 * - Race-aware: a 409 from the gate endpoint surfaces the winner's identity
 *   (W2.6), so the inbox renders "already decided by …" instead of an error
 *   when another surface beat us to it.
 */

type DecisionKind = "approve" | "rework" | "cancel";

type ApprovalRequest = {
  request_id: string;
  approval_wait?: {
    approval_request_id?: string;
    transition_id?: string | null;
  } | null;
  source: string;
  tenant: { org_id: string; workspace_id: string; user_id?: string };
  run_id: string;
  node_id?: string;
  workflow_name?: string;
  action_kind?: string;
  action_preview_markdown?: string;
  surface_payload?: Record<string, any> | null;
  requested_at_ms: number;
  expires_at_ms?: number | null;
  decisions?: Array<DecisionKind | string>;
  rework_targets?: string[];
  instructions?: string;
};

type PendingResponse = {
  approvals?: ApprovalRequest[];
  count?: number;
};

function externalApprovalPreview(row: any): string {
  const target = row?.target || {};
  const payload = row?.payload || {};
  const targetLabel = `${String(target.base_repo || target.identifier || "unknown")}${
    target.pr_number ? `#${target.pr_number}` : ""
  }`;
  const lines = [
    `**${String(row?.action_type || "external_action")}**`,
    "",
    `Target: \`${targetLabel}\``,
    `Risk: \`${String(row?.risk_level || "unknown")}\``,
  ];
  const body = String(payload.body || "").trim();
  if (body) lines.push("", "Payload:", "```text", body.slice(0, 2000), "```");
  return lines.join("\n");
}

function normalizeExternalApproval(row: any): ApprovalRequest {
  const approvalId = String(row?.approval_id || row?.id || "");
  const runId = String(row?.run_id || "");
  return {
    request_id: approvalId,
    source: "aca_external_action",
    tenant: { org_id: "local", workspace_id: "aca" },
    run_id: runId,
    workflow_name: `ACA ${String(row?.adapter || "external action")}`,
    action_kind: String(row?.action_type || "external_action"),
    action_preview_markdown: externalApprovalPreview(row),
    surface_payload: {
      approve_endpoint: `/api/aca/approvals/${encodeURIComponent(approvalId)}/approve`,
      reject_endpoint: `/api/aca/approvals/${encodeURIComponent(approvalId)}/reject`,
      resume_endpoint: `/api/aca/runs/${encodeURIComponent(runId)}/resume-approved-actions`,
      raw: row,
    },
    requested_at_ms: Number(row?.created_at_ms || Date.now()),
    decisions: ["approve", "cancel"],
    instructions:
      "Review and approve this ACA external action. Approval executes it through the connected MCP server and verifies it afterwards.",
  };
}

function formatRelativeTime(ms: number): string {
  const seconds = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  return `${days}d ago`;
}

function formatDeadline(ms?: number | null): string | null {
  if (!ms) return null;
  const deltaSeconds = Math.round((ms - Date.now()) / 1000);
  if (deltaSeconds <= 0) return "expired";
  if (deltaSeconds < 60) return `expires in ${deltaSeconds}s`;
  const minutes = Math.round(deltaSeconds / 60);
  if (minutes < 60) return `expires in ${minutes}m`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `expires in ${hours}h`;
  const days = Math.round(hours / 24);
  return `expires in ${days}d`;
}

function decideEndpointFor(request: ApprovalRequest): string | null {
  if (request.source === "aca_external_action") {
    const payload = request.surface_payload as any;
    const endpoint = payload?.approve_endpoint || payload?.reject_endpoint;
    return typeof endpoint === "string" ? endpoint : null;
  }
  const explicit = (request.surface_payload as any)?.decide_endpoint;
  if (typeof explicit === "string" && explicit.startsWith("/")) {
    return explicit.startsWith("/api/engine") ? explicit : `/api/engine${explicit}`;
  }
  if (request.source === "automation_v2" && request.run_id) {
    return `/api/engine/automations/v2/runs/${request.run_id}/gate`;
  }
  return null;
}

async function postDecision(
  request: ApprovalRequest,
  decision: DecisionKind,
  reason?: string
): Promise<{ ok: boolean; alreadyDecidedBy?: string; conflictBody?: any }> {
  let endpoint = decideEndpointFor(request);
  if (request.source === "aca_external_action") {
    const payload = request.surface_payload as any;
    endpoint = decision === "approve" ? payload?.approve_endpoint : payload?.reject_endpoint;
  }
  if (!endpoint) {
    throw new Error(`No decide endpoint known for source=${request.source}`);
  }
  try {
    const approvalRequestId =
      request.approval_wait?.approval_request_id ??
      (request.surface_payload as any)?.approval_request_id ??
      request.request_id;
    const transitionId =
      request.approval_wait?.transition_id ??
      (request.surface_payload as any)?.transition_id;
    await api(endpoint, {
      method: "POST",
      body: JSON.stringify({
        decision,
        reason: reason || undefined,
        approval_request_id: approvalRequestId,
        transition_id: transitionId || undefined,
      }),
    });
    const resumeEndpoint = (request.surface_payload as any)?.resume_endpoint;
    if (decision === "approve" && typeof resumeEndpoint === "string" && resumeEndpoint.startsWith("/")) {
      await api(resumeEndpoint, { method: "POST" }).catch(() => ({}));
    }
    return { ok: true };
  } catch (error: any) {
    // Surface the W2.6 race body when a 409 lands.
    if (error?.status === 409) {
      // The api() helper has already extracted error.message from the body;
      // we cannot read winningDecision here without changes to api(). For v1
      // we fall back to a friendly message and let the caller toast it.
      return {
        ok: false,
        alreadyDecidedBy: "another operator",
        conflictBody: error?.message,
      };
    }
    throw error;
  }
}

function decisionStyle(decision: DecisionKind): string {
  switch (decision) {
    case "approve":
      return "tcp-btn h-8 px-3 text-xs bg-emerald-600 text-white hover:bg-emerald-500";
    case "cancel":
      return "tcp-btn h-8 px-3 text-xs bg-rose-600 text-white hover:bg-rose-500";
    case "rework":
      return "tcp-btn h-8 px-3 text-xs";
  }
}

function decisionLabel(decision: DecisionKind): string {
  switch (decision) {
    case "approve":
      return "Approve";
    case "rework":
      return "Rework";
    case "cancel":
      return "Reject";
  }
}

function decisionPastTenseLabel(decision: DecisionKind): string {
  switch (decision) {
    case "approve":
      return "Approved";
    case "rework":
      return "Sent back for rework";
    case "cancel":
      return "Rejected";
  }
}

export function ApprovalsInboxPage({ toast }: AppPageProps) {
  const queryClient = useQueryClient();
  const [reasonByRequest, setReasonByRequest] = useState<Record<string, string>>({});
  const [showReasonForm, setShowReasonForm] = useState<string | null>(null);

  const pendingQuery = useQuery<PendingResponse>({
    queryKey: ["approvals", "pending"],
    queryFn: async () => {
      const res = await api("/api/engine/approvals/pending");
      return (res || {}) as PendingResponse;
    },
    refetchInterval: 5000,
    staleTime: 0,
  });

  const acaPendingQuery = useQuery<PendingResponse>({
    queryKey: ["approvals", "aca-external-actions", "pending"],
    queryFn: async () => {
      const res = await api("/api/aca/approvals/pending").catch(() => ({ approvals: [] }));
      const approvals = Array.isArray((res as any)?.approvals)
        ? (res as any).approvals.map(normalizeExternalApproval)
        : [];
      return { approvals, count: approvals.length };
    },
    refetchInterval: 5000,
    staleTime: 0,
  });

  const decideMutation = useMutation({
    mutationFn: async (vars: {
      request: ApprovalRequest;
      decision: DecisionKind;
      reason?: string;
    }) => {
      return postDecision(vars.request, vars.decision, vars.reason);
    },
    onSuccess: async (result, vars) => {
      if (result.ok) {
        toast(
          "ok",
          `${decisionPastTenseLabel(vars.decision)} gate ${
            vars.request.workflow_name || vars.request.run_id
          }`
        );
      } else {
        toast("warn", `Already decided by ${result.alreadyDecidedBy || "another operator"}.`);
      }
      await queryClient.invalidateQueries({ queryKey: ["approvals", "pending"] });
      await queryClient.invalidateQueries({ queryKey: ["approvals", "aca-external-actions", "pending"] });
      setShowReasonForm(null);
    },
    onError: (error: any) => {
      toast("err", error?.message || String(error));
    },
  });

  const approvals = pendingQuery.data?.approvals ?? [];
  const acaApprovals = acaPendingQuery.data?.approvals ?? [];
  const allApprovals = [...acaApprovals, ...approvals].sort(
    (left, right) => Number(right.requested_at_ms || 0) - Number(left.requested_at_ms || 0)
  );
  const count = allApprovals.length;
  const loadingApprovals =
    pendingQuery.isLoading ||
    acaPendingQuery.isLoading ||
    ((pendingQuery.isFetching || acaPendingQuery.isFetching) && count === 0);

  return (
    <AnimatedPage className="grid h-full min-h-0 gap-4">
      <PageCard
        title="Approvals Inbox"
        subtitle="Pending approvals across every workflow that has paused on a human gate. Decisions go through the authoritative subsystem handler."
        actions={
          <div className="flex items-center gap-3 text-xs">
            <StatusPulse
              tone={pendingQuery.isFetching ? "live" : count > 0 ? "warn" : "info"}
              text={pendingQuery.isFetching ? "checking approvals" : "approvals"}
            />
            <Badge tone={count > 0 ? "warn" : "ok"}>{count} pending</Badge>
            <button
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() => pendingQuery.refetch()}
              disabled={pendingQuery.isFetching}
            >
              {pendingQuery.isFetching ? (
                <TandemLogoAnimation
                  mode="compact"
                  className="h-4 w-4"
                  title="Refreshing approvals"
                />
              ) : null}
              {pendingQuery.isFetching ? "Refreshing" : "Refresh"}
            </button>
          </div>
        }
        fullHeight
      >
        {loadingApprovals ? (
          <LoadingState
            title="Checking for pending approvals…"
            text="Polling /api/engine/approvals/pending"
          />
        ) : allApprovals.length === 0 ? (
          <EmptyState
            title="No approvals waiting"
            text="Workflows with auto-injected approval gates pause here when they reach an external action."
          />
        ) : (
          <div className="grid gap-4">
            {allApprovals.map((request) => (
              <ApprovalRequestCard
                key={request.request_id}
                request={request}
                showReasonForm={showReasonForm}
                setShowReasonForm={setShowReasonForm}
                reason={reasonByRequest[request.request_id] || ""}
                setReason={(reason) =>
                  setReasonByRequest((current) => ({
                    ...current,
                    [request.request_id]: reason,
                  }))
                }
                decideMutation={decideMutation}
              />
            ))}
          </div>
        )}
      </PageCard>
    </AnimatedPage>
  );
}

function ApprovalRequestCard({
  request,
  showReasonForm,
  setShowReasonForm,
  reason,
  setReason,
  decideMutation,
}: {
  request: ApprovalRequest;
  showReasonForm: string | null;
  setShowReasonForm: (requestId: string | null) => void;
  reason: string;
  setReason: (reason: string) => void;
  decideMutation: ReturnType<
    typeof useMutation<
      { ok: boolean; alreadyDecidedBy?: string },
      any,
      {
        request: ApprovalRequest;
        decision: DecisionKind;
        reason?: string;
      }
    >
  >;
}) {
  const previewMarkdown = dedupeMarkdown(request.action_preview_markdown, request.instructions);
  const decisions = approvalDecisionsForRequest(request);
  const deadline = formatDeadline(request.expires_at_ms);

  return (
    <PanelCard
      title={request.workflow_name || request.run_id}
      subtitle={
        request.action_kind
          ? `${sourceLabel(request.source)} · ${request.action_kind}`
          : sourceLabel(request.source)
      }
      actions={
        <span className="text-xs text-tcp-text-tertiary">
          {deadline
            ? `${formatRelativeTime(request.requested_at_ms)} - ${deadline}`
            : formatRelativeTime(request.requested_at_ms)}
        </span>
      }
    >
      <div className="grid gap-3 text-sm">
        {request.instructions ? (
          <div className="text-tcp-text-secondary">{request.instructions}</div>
        ) : null}
        {previewMarkdown ? (
          <div
            className="prose prose-invert max-w-none text-sm"
            dangerouslySetInnerHTML={{
              __html: renderMarkdownSafe(previewMarkdown),
            }}
          />
        ) : (
          <div className="rounded-md border border-amber-500/30 bg-amber-500/10 p-3 text-xs text-amber-100">
            No action preview was provided for this approval. Review the run artifacts before
            approving.
          </div>
        )}
        <div className="flex flex-wrap gap-2 text-xs text-tcp-text-tertiary">
          <span>
            <strong>run:</strong> <code>{request.run_id}</code>
          </span>
          {request.node_id ? (
            <span>
              <strong>node:</strong> <code>{request.node_id}</code>
            </span>
          ) : null}
          <span>
            <strong>tenant:</strong>{" "}
            <code>
              {request.tenant.org_id}/{request.tenant.workspace_id}
            </code>
          </span>
        </div>

        {showReasonForm === request.request_id ? (
          <div className="grid gap-2 rounded-md border border-tcp-border-subtle p-3">
            <label className="text-xs font-medium">
              Rework feedback (sent to the agent so it can revise)
            </label>
            <textarea
              className="tcp-input h-20 w-full rounded text-xs"
              placeholder="What should change before this can be approved?"
              value={reason}
              onChange={(event) => setReason(event.target.value)}
            />
            <div className="flex justify-end gap-2">
              <button
                className="tcp-btn h-8 px-3 text-xs"
                onClick={() => setShowReasonForm(null)}
                disabled={decideMutation.isPending}
              >
                Dismiss
              </button>
              <button
                className={decisionStyle("rework")}
                onClick={() =>
                  decideMutation.mutate({
                    request,
                    decision: "rework",
                    reason,
                  })
                }
                disabled={decideMutation.isPending}
              >
                Send back for rework
              </button>
            </div>
          </div>
        ) : (
          <div className="flex flex-wrap items-center gap-2 pt-1">
            {decisions.map((decision) => {
              if (decision === "rework") {
                return (
                  <button
                    key={decision}
                    className={decisionStyle(decision)}
                    onClick={() => setShowReasonForm(request.request_id)}
                    disabled={decideMutation.isPending}
                  >
                    {decisionLabel(decision)}
                  </button>
                );
              }
              return (
                <button
                  key={decision}
                  className={decisionStyle(decision)}
                  onClick={() => decideMutation.mutate({ request, decision })}
                  disabled={decideMutation.isPending}
                >
                  {decisionLabel(decision)}
                </button>
              );
            })}
          </div>
        )}
      </div>
    </PanelCard>
  );
}

function approvalDecisionsForRequest(request: ApprovalRequest): DecisionKind[] {
  const seen = new Set<DecisionKind>();
  const decisions: DecisionKind[] = [];
  for (const raw of request.decisions || ["approve", "rework", "cancel"]) {
    const decision = normalizeDecision(raw);
    if (!["approve", "rework", "cancel"].includes(decision) || seen.has(decision)) continue;
    seen.add(decision);
    decisions.push(decision);
  }
  if (request.rework_targets?.length && !seen.has("rework")) {
    decisions.push("rework");
  }
  return decisions;
}

function normalizeDecision(raw: unknown): DecisionKind {
  const value = String(raw).toLowerCase();
  if (["reject", "deny"].includes(value)) return "cancel";
  if (["changes", "request_changes", "ask_changes"].includes(value)) return "rework";
  return value as DecisionKind;
}

function dedupeMarkdown(markdown?: string, instructions?: string): string | undefined {
  const preview = markdown?.trim();
  if (!preview) return undefined;
  const instructionText = instructions?.trim();
  if (instructionText && preview === instructionText) return undefined;
  return preview;
}

function sourceLabel(source: string): string {
  switch (source) {
    case "aca_external_action":
      return "ACA external action";
    case "automation_v2":
      return "automation v2";
    case "coder":
      return "coder";
    case "workflow":
      return "workflow";
    default:
      return source;
  }
}
