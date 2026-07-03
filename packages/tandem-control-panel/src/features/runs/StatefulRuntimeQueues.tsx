import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge, EmptyState, LoadingState, PanelCard, Toolbar } from "../../ui/index.tsx";
import {
  buildApprovalWaitRows,
  buildRecoveryQueueRows,
  buildWebhookInboxRows,
  filterStatefulQueueRows,
  summarizeApprovalWaitRows,
  summarizeRecoveryQueueRows,
  summarizeWebhookInboxRows,
  titleCase,
} from "../../../lib/runs/stateful-queues.js";
import { formatRunTimestamp } from "../../../lib/runs/stateful-runs.js";
import type { AppPageProps } from "../../pages/pageTypes";
import { StatefulRunFilterBar } from "./StatefulRunFilters";

type RuntimeQueueProps = Pick<AppPageProps, "api" | "navigate" | "toast"> & {
  filters: any;
  onFiltersChange: (filters: any) => void;
  onOpenRun: (runId: string) => void;
};

function errorText(error: any) {
  return String(error?.message || error || "").trim();
}

function StatTiles({ items }: { items: Array<{ key: string; label: string; value: number }> }) {
  return (
    <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4 xl:grid-cols-6">
      {items.map((item) => (
        <div key={item.key} className="min-h-[4.25rem] rounded-md border border-white/10 bg-white/[0.03] px-3 py-2">
          <div className="text-[11px] uppercase tracking-wide text-tcp-text-muted">{item.label}</div>
          <div className="mt-1 text-2xl font-semibold tabular-nums text-tcp-text-primary">{item.value}</div>
        </div>
      ))}
    </div>
  );
}

function RunButton({ runId, onOpenRun }: { runId: string; onOpenRun: (runId: string) => void }) {
  return runId ? (
    <button type="button" className="tcp-btn h-7 px-2 text-xs" onClick={() => onOpenRun(runId)} title="Open run">
      <i data-lucide="external-link"></i>
    </button>
  ) : null;
}

function InlineList({ items, empty }: { items: string[]; empty: string }) {
  const visible = items.filter(Boolean).slice(0, 3);
  if (!visible.length) return <span className="text-tcp-text-muted">{empty}</span>;
  return (
    <div className="space-y-1">
      {visible.map((item) => (
        <div key={item} className="line-clamp-1">
          {item}
        </div>
      ))}
    </div>
  );
}

function QueryProblem({ message }: { message: string }) {
  return (
    <div className="rounded-md border border-yellow-400/20 bg-yellow-400/10 px-3 py-2 text-xs text-yellow-100">
      {message}
    </div>
  );
}

function useNow(intervalMs: number) {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const interval = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(interval);
  }, [intervalMs]);

  return now;
}

export function WebhookInboxView({ api, navigate, filters, onFiltersChange, onOpenRun }: RuntimeQueueProps) {
  const eventsQuery = useQuery({
    queryKey: ["stateful-runtime", "webhook-inbox"],
    queryFn: () => api("/api/engine/automations/v2/webhook-events?limit=160"),
    refetchInterval: 10000,
  });
  const allRows = useMemo(() => buildWebhookInboxRows(eventsQuery.data || {}), [eventsQuery.data]);
  const rows = useMemo(() => filterStatefulQueueRows(allRows, filters), [allRows, filters]);
  const summary = useMemo(() => summarizeWebhookInboxRows(rows), [rows]);
  const loading = eventsQuery.isLoading && !eventsQuery.data;

  return (
    <div className="grid h-full min-h-0 grid-rows-[auto_1fr] gap-4">
      <PanelCard
        title="Webhook Inbox"
        subtitle="Raw and sanitized event intake with verification, idempotency, and run correlation."
        actions={
          <Toolbar>
            <button
              type="button"
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() => eventsQuery.refetch()}
              disabled={eventsQuery.isFetching}
            >
              <i data-lucide="refresh-cw"></i>
              Refresh
            </button>
            <button type="button" className="tcp-btn h-8 px-3 text-xs" onClick={() => navigate("automations")}>
              <i data-lucide="bot"></i>
              Automations
            </button>
          </Toolbar>
        }
      >
        <StatTiles
          items={[
            { key: "total", label: "Events", value: summary.total },
            { key: "accepted", label: "Accepted", value: summary.accepted },
            { key: "duplicate", label: "Duplicates", value: summary.duplicate },
            { key: "rejected", label: "Rejected", value: summary.rejected },
            { key: "failed", label: "Failed", value: summary.failed },
            { key: "redacted", label: "Redacted", value: summary.redacted },
          ]}
        />
        <div className="mt-4">
          <StatefulRunFilterBar filters={filters} onFiltersChange={onFiltersChange} />
        </div>
      </PanelCard>

      <PanelCard title="Raw Event Inbox" subtitle={`${rows.length} of ${allRows.length} retained events`} fullHeight>
        {eventsQuery.error ? <QueryProblem message={errorText(eventsQuery.error)} /> : null}
        {loading ? (
          <LoadingState title="Loading webhook events" />
        ) : rows.length ? (
          <div className="mt-3 min-h-0 flex-1 overflow-auto rounded-lg border border-white/10">
            <table className="w-full min-w-[1320px] table-fixed text-left text-xs">
              <thead className="sticky top-0 z-10 bg-black/80 text-[11px] uppercase text-tcp-text-muted backdrop-blur">
                <tr>
                  <th className="w-[19rem] px-3 py-2 font-medium">Event</th>
                  <th className="w-[8rem] px-3 py-2 font-medium">Status</th>
                  <th className="w-[15rem] px-3 py-2 font-medium">Verification</th>
                  <th className="w-[17rem] px-3 py-2 font-medium">Idempotency</th>
                  <th className="w-[17rem] px-3 py-2 font-medium">Correlation</th>
                  <th className="w-[15rem] px-3 py-2 font-medium">Payload Policy</th>
                  <th className="w-[10rem] px-3 py-2 font-medium">Received</th>
                  <th className="w-[7rem] px-3 py-2 font-medium"></th>
                </tr>
              </thead>
              <tbody className="divide-y divide-white/8">
                {rows.map((row: any) => (
                  <tr key={row.id} className="align-top hover:bg-white/[0.03]">
                    <td className="px-3 py-3">
                      <div className="truncate text-sm font-medium text-tcp-text-primary">{row.provider}</div>
                      <div className="mt-1 flex min-w-0 flex-wrap gap-2 text-[11px] text-tcp-text-muted">
                        <span>{row.providerEventKind}</span>
                        <span className="font-mono">{row.id}</span>
                      </div>
                      <div className="mt-1 truncate text-[11px] text-tcp-text-muted">
                        trigger {row.triggerId || "n/a"} / delivery {row.deliveryId || "n/a"}
                      </div>
                    </td>
                    <td className="px-3 py-3">
                      <Badge tone={row.statusTone}>{row.statusLabel}</Badge>
                    </td>
                    <td className="px-3 py-3 text-tcp-text-secondary">{row.verificationLabel}</td>
                    <td className="px-3 py-3 text-tcp-text-secondary">{row.dedupeLabel}</td>
                    <td className="px-3 py-3">
                      <div className="line-clamp-2 text-tcp-text-secondary">{row.correlationLabel}</div>
                      {row.deadLettered ? <div className="mt-1 text-[11px] text-rose-200">Dead-letter routed</div> : null}
                    </td>
                    <td className="px-3 py-3 text-tcp-text-secondary">{row.payloadLabel}</td>
                    <td className="px-3 py-3 text-tcp-text-secondary">{formatRunTimestamp(row.receivedAtMs)}</td>
                    <td className="px-3 py-3">
                      <div className="flex gap-1">
                        <RunButton runId={row.runId} onOpenRun={onOpenRun} />
                        <button
                          type="button"
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={() => navigate("automations")}
                          title="Open trigger manager"
                        >
                          <i data-lucide="list-tree"></i>
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <EmptyState title="No webhook events" text="Retained webhook intake events will appear here." />
        )}
      </PanelCard>
    </div>
  );
}

export function ApprovalWaitsView({ api, navigate, filters, onFiltersChange, onOpenRun }: RuntimeQueueProps) {
  const now = useNow(10000);
  const approvalsQuery = useQuery({
    queryKey: ["stateful-runtime", "approval-waits"],
    queryFn: () => api("/api/engine/approvals/pending"),
    refetchInterval: 5000,
  });
  const allRows = useMemo(() => buildApprovalWaitRows(approvalsQuery.data || {}, { now }), [approvalsQuery.data, now]);
  const rows = useMemo(() => filterStatefulQueueRows(allRows, filters), [allRows, filters]);
  const summary = useMemo(() => summarizeApprovalWaitRows(rows), [rows]);
  const loading = approvalsQuery.isLoading && !approvalsQuery.data;

  return (
    <div className="grid h-full min-h-0 grid-rows-[auto_1fr] gap-4">
      <PanelCard
        title="Approval Waits"
        subtitle="Durable approval pauses with phase, transition, timeout, and decision context."
        actions={
          <Toolbar>
            <button
              type="button"
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() => approvalsQuery.refetch()}
              disabled={approvalsQuery.isFetching}
            >
              <i data-lucide="refresh-cw"></i>
              Refresh
            </button>
            <button type="button" className="tcp-btn h-8 px-3 text-xs" onClick={() => navigate("approvals")}>
              <i data-lucide="shield-check"></i>
              Decisions
            </button>
          </Toolbar>
        }
      >
        <StatTiles
          items={[
            { key: "total", label: "Approvals", value: summary.total },
            { key: "pending", label: "Pending", value: summary.pending },
            { key: "expired", label: "Expired", value: summary.expired },
            { key: "escalated", label: "Escalated", value: summary.escalated },
            { key: "decided", label: "Decided", value: summary.decided },
          ]}
        />
        <div className="mt-4">
          <StatefulRunFilterBar filters={filters} onFiltersChange={onFiltersChange} />
        </div>
      </PanelCard>

      <PanelCard title="Pending Approval Waits" subtitle={`${rows.length} of ${allRows.length} waits`} fullHeight>
        {approvalsQuery.error ? <QueryProblem message={errorText(approvalsQuery.error)} /> : null}
        {loading ? (
          <LoadingState title="Loading approvals" />
        ) : rows.length ? (
          <div className="mt-3 min-h-0 flex-1 overflow-auto rounded-lg border border-white/10">
            <table className="w-full min-w-[1240px] table-fixed text-left text-xs">
              <thead className="sticky top-0 z-10 bg-black/80 text-[11px] uppercase text-tcp-text-muted backdrop-blur">
                <tr>
                  <th className="w-[18rem] px-3 py-2 font-medium">Approval</th>
                  <th className="w-[8rem] px-3 py-2 font-medium">Status</th>
                  <th className="w-[16rem] px-3 py-2 font-medium">Phase</th>
                  <th className="w-[15rem] px-3 py-2 font-medium">Timeout</th>
                  <th className="w-[18rem] px-3 py-2 font-medium">Decision History</th>
                  <th className="w-[10rem] px-3 py-2 font-medium">Requested</th>
                  <th className="w-[7rem] px-3 py-2 font-medium"></th>
                </tr>
              </thead>
              <tbody className="divide-y divide-white/8">
                {rows.map((row: any) => (
                  <tr key={row.id} className="align-top hover:bg-white/[0.03]">
                    <td className="px-3 py-3">
                      <div className="truncate text-sm font-medium text-tcp-text-primary">{row.title}</div>
                      <div className="mt-1 flex min-w-0 flex-wrap gap-2 text-[11px] text-tcp-text-muted">
                        <span className="font-mono">{row.id}</span>
                        <span>{row.source}</span>
                      </div>
                      <div className="mt-1 truncate text-[11px] text-tcp-text-muted">{row.actionKind}</div>
                    </td>
                    <td className="px-3 py-3">
                      <Badge tone={row.statusTone}>{row.statusLabel}</Badge>
                    </td>
                    <td className="px-3 py-3">
                      <div className="truncate text-tcp-text-secondary">{row.phaseId || row.nodeId || "n/a"}</div>
                      <div className="mt-1 truncate text-[11px] text-tcp-text-muted">
                        transition {row.transitionId || "n/a"}
                      </div>
                    </td>
                    <td className="px-3 py-3">
                      <div className="text-tcp-text-secondary">{row.timeoutLabel || "no deadline"}</div>
                      {row.escalationLabel ? (
                        <div className="mt-1 text-[11px] text-yellow-100">{row.escalationLabel}</div>
                      ) : null}
                    </td>
                    <td className="px-3 py-3 text-tcp-text-secondary">
                      <InlineList
                        empty="No decisions recorded"
                        items={row.decisionHistory.map((decision: any) =>
                          compactDecision(decision.decision, decision.actor, decision.transition)
                        )}
                      />
                    </td>
                    <td className="px-3 py-3 text-tcp-text-secondary">{formatRunTimestamp(row.requestedAtMs)}</td>
                    <td className="px-3 py-3">
                      <div className="flex gap-1">
                        <RunButton runId={row.runId} onOpenRun={onOpenRun} />
                        <button
                          type="button"
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={() => navigate("approvals")}
                          title="Open approvals inbox"
                        >
                          <i data-lucide="shield-check"></i>
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <EmptyState title="No approval waits" text="Pending approval waits will appear here." />
        )}
      </PanelCard>
    </div>
  );
}

function compactDecision(decision: string, actor: string, transition: string) {
  return [decision, actor ? `by ${actor}` : "", transition ? `-> ${transition}` : ""].filter(Boolean).join(" ");
}

function actionChoice(option: string) {
  switch (option) {
    case "retry":
      return "retry_dead_letter";
    case "ignore":
      return "ignore_dead_letter";
    case "compensate":
      return "compensate";
    case "abandon":
      return "abandon_with_audit";
    default:
      return option;
  }
}

function recoveryActionLabel(option: string) {
  switch (option) {
    case "retry":
      return "Request Retry";
    case "compensate":
      return "Link Runbook";
    case "ignore":
      return "Ignore";
    case "abandon":
      return "Abandon";
    default:
      return titleCase(option);
  }
}

function recoveryActionTitle(option: string) {
  switch (option) {
    case "retry":
      return "Record an operator retry request";
    case "compensate":
      return "Link this item to an operator compensation runbook";
    case "ignore":
      return "Record that this item is ignored";
    case "abandon":
      return "Record an audited abandon decision";
    default:
      return `Record ${titleCase(option)} recovery choice`;
  }
}

export function RecoveryQueueView({ api, toast, filters, onFiltersChange, onOpenRun }: RuntimeQueueProps) {
  const queryClient = useQueryClient();
  const reliabilityQuery = useQuery({
    queryKey: ["stateful-runtime", "reliability-queue"],
    queryFn: () => api("/api/engine/stateful-runtime/reliability?limit=240&active_recovery_only=true"),
    refetchInterval: 10000,
  });
  const allRows = useMemo(() => buildRecoveryQueueRows(reliabilityQuery.data || {}), [reliabilityQuery.data]);
  const rows = useMemo(() => filterStatefulQueueRows(allRows, filters), [allRows, filters]);
  const summary = useMemo(() => summarizeRecoveryQueueRows(rows), [rows]);
  const loading = reliabilityQuery.isLoading && !reliabilityQuery.data;

  const actionMutation = useMutation({
    mutationFn: ({ row, option }: { row: any; option: string }) =>
      api(`/api/engine/stateful-runtime/runs/${encodeURIComponent(row.runId)}/resume-plan`, {
        method: "POST",
        body: JSON.stringify({
          choice: actionChoice(option),
          reason: "Operator recovery request recorded from control panel queue.",
          dead_letter_id: row.kind === "dead_letter" ? row.id : undefined,
          compensation_id: row.kind === "compensation" ? row.id : undefined,
          target_effect_id: row.kind === "tool_effect" ? row.id : undefined,
        }),
      }),
    onSuccess: async () => {
      toast("ok", "Recovery request recorded.");
      await queryClient.invalidateQueries({ queryKey: ["stateful-runtime", "reliability-queue"] });
    },
    onError: (error: any) => toast("err", errorText(error) || "Recovery action failed."),
  });

  return (
    <div className="grid h-full min-h-0 grid-rows-[auto_1fr] gap-4">
      <PanelCard
        title="Recovery Queue"
        subtitle="Retry, outbox, dead-letter, and compensation evidence across stateful runs."
        actions={
          <Toolbar>
            <button
              type="button"
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() => reliabilityQuery.refetch()}
              disabled={reliabilityQuery.isFetching}
            >
              <i data-lucide="refresh-cw"></i>
              Refresh
            </button>
          </Toolbar>
        }
      >
        <StatTiles
          items={[
            { key: "total", label: "Items", value: summary.total },
            { key: "retryable", label: "Needs Retry", value: summary.retryable },
            { key: "waiting", label: "Backoff", value: summary.waitingBackoff },
            { key: "dead", label: "Dead Letters", value: summary.deadLettered },
            { key: "blocked", label: "Manual", value: summary.manuallyBlocked },
          ]}
        />
        <div className="mt-4">
          <StatefulRunFilterBar filters={filters} onFiltersChange={onFiltersChange} />
        </div>
      </PanelCard>

      <PanelCard title="Reliability Queue" subtitle={`${rows.length} of ${allRows.length} reliability records`} fullHeight>
        {reliabilityQuery.error ? <QueryProblem message={errorText(reliabilityQuery.error)} /> : null}
        {loading ? (
          <LoadingState title="Loading reliability queue" />
        ) : rows.length ? (
          <div className="mt-3 min-h-0 flex-1 overflow-auto rounded-lg border border-white/10">
            <table className="w-full min-w-[1320px] table-fixed text-left text-xs">
              <thead className="sticky top-0 z-10 bg-black/80 text-[11px] uppercase text-tcp-text-muted backdrop-blur">
                <tr>
                  <th className="w-[17rem] px-3 py-2 font-medium">Queue Item</th>
                  <th className="w-[11rem] px-3 py-2 font-medium">Class</th>
                  <th className="w-[8rem] px-3 py-2 font-medium">Status</th>
                  <th className="w-[16rem] px-3 py-2 font-medium">Target</th>
                  <th className="w-[16rem] px-3 py-2 font-medium">Reason</th>
                  <th className="w-[15rem] px-3 py-2 font-medium">Scope</th>
                  <th className="w-[9rem] px-3 py-2 font-medium">Updated</th>
                  <th className="w-[13rem] px-3 py-2 font-medium">Actions</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-white/8">
                {rows.map((row: any) => (
                  <tr key={`${row.kind}:${row.id}`} className="align-top hover:bg-white/[0.03]">
                    <td className="px-3 py-3">
                      <div className="truncate text-sm font-medium text-tcp-text-primary">{row.operation}</div>
                      <div className="mt-1 flex min-w-0 flex-wrap gap-2 text-[11px] text-tcp-text-muted">
                        <span>{row.kindLabel}</span>
                        <span className="font-mono">{row.id}</span>
                      </div>
                      <div className="mt-1 truncate text-[11px] text-tcp-text-muted">attempts {row.attempts}</div>
                    </td>
                    <td className="px-3 py-3 text-tcp-text-secondary">{row.categoryLabel}</td>
                    <td className="px-3 py-3">
                      <Badge tone={row.statusTone}>{row.statusLabel}</Badge>
                    </td>
                    <td className="px-3 py-3">
                      <InlineList
                        empty="No target"
                        items={[
                          row.provider || row.tool,
                          row.target,
                          row.nodeId ? `node ${row.nodeId}` : "",
                          row.sourceLabel,
                        ]}
                      />
                    </td>
                    <td className="px-3 py-3 text-tcp-text-secondary">
                      <div className="line-clamp-3">{row.reason || "No reason recorded"}</div>
                    </td>
                    <td className="px-3 py-3 text-tcp-text-secondary">
                      <div className="line-clamp-2">{row.scopeLabel || "local"}</div>
                    </td>
                    <td className="px-3 py-3 text-tcp-text-secondary">{formatRunTimestamp(row.updatedAtMs)}</td>
                    <td className="px-3 py-3">
                      <div className="flex flex-wrap gap-1">
                        <RunButton runId={row.runId} onOpenRun={onOpenRun} />
                        {row.runId
                          ? row.recoveryOptions.slice(0, 3).map((option: string) => (
                              <button
                                key={option}
                                type="button"
                                className="tcp-btn h-7 px-2 text-xs"
                                disabled={actionMutation.isPending}
                                onClick={() => actionMutation.mutate({ row, option })}
                                title={recoveryActionTitle(option)}
                              >
                                {recoveryActionLabel(option)}
                              </button>
                            ))
                          : null}
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <EmptyState title="No recovery items" text="Retry and dead-letter queue items will appear here." />
        )}
      </PanelCard>
    </div>
  );
}
