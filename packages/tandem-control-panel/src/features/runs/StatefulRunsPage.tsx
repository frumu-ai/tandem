import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Badge, LoadingState, PanelCard, Toolbar } from "../../ui/index.tsx";
import { EmptyState } from "../../pages/ui";
import {
  buildRunObservabilityDetail,
  buildStatefulRunRows,
  filterStatefulRunRows,
  formatRunTimestamp,
  summarizeStatefulRuns,
} from "../../../lib/runs/stateful-runs.js";
import type { AppPageProps } from "../../pages/pageTypes";
import { StatefulRunFilterBar } from "./StatefulRunFilters";
import { Icon } from "../../ui/Icon";

type RunsProps = Pick<AppPageProps, "api" | "client" | "navigate">;
type RunListRequest = Pick<AppPageProps, "api" | "client">;
type StatefulRunsPageProps = RunsProps & {
  filters: any;
  onFiltersChange: (filters: any) => void;
};

type RunListPayload = {
  statefulRuns: any[];
  workflowRuns: any[];
  legacyRuns: any[];
  contextRuns: any[];
  errors: string[];
};

type RunRowsResult = {
  runs: any[];
  error?: string;
};

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

function errorText(error: any) {
  return String(error?.message || error || "").trim();
}

async function runListPayload({ api, client }: RunListRequest): Promise<RunListPayload> {
  const canonicalRuns: RunRowsResult = await api("/api/engine/stateful-runtime/runs?limit=120").catch((error: any) => ({
    runs: [],
    error: errorText(error),
  }));
  if (!canonicalRuns?.error) {
    const contextRuns: RunRowsResult = await api("/api/engine/context/runs?limit=120").catch((error: any) => ({
      runs: [],
      error: errorText(error),
    }));
    return {
      statefulRuns: toArray(canonicalRuns, "runs"),
      workflowRuns: [],
      legacyRuns: [],
      contextRuns: toArray(contextRuns, "runs"),
      errors: [contextRuns?.error].filter(Boolean),
    };
  }

  const [workflowRuns, legacyRuns, contextRuns]: RunRowsResult[] = await Promise.all([
    api("/api/engine/automations/v2/runs?limit=120").catch((error: any) => ({
      runs: [],
      error: errorText(error),
    })),
    client?.automations?.listRuns?.({ limit: 120 }).catch((error: any) => ({
      runs: [],
      error: errorText(error),
    })) ?? Promise.resolve({ runs: [] } as RunRowsResult),
    api("/api/engine/context/runs?limit=120").catch((error: any) => ({
      runs: [],
      error: errorText(error),
    })),
  ]);

  return {
    statefulRuns: [],
    workflowRuns: toArray(workflowRuns, "runs"),
    legacyRuns: toArray(legacyRuns, "runs"),
    contextRuns: toArray(contextRuns, "runs"),
    errors: [canonicalRuns?.error, workflowRuns?.error, legacyRuns?.error, contextRuns?.error].filter(Boolean),
  };
}

function metricItems(summary: any) {
  return [
    { key: "total", label: "Total", value: summary.total },
    { key: "active", label: "Active", value: summary.active },
    { key: "waiting", label: "Waiting", value: summary.waiting },
    { key: "queued", label: "Queued", value: summary.queued },
    { key: "failed", label: "Failed", value: summary.failed },
    { key: "tenants", label: "Tenants", value: summary.tenants },
    { key: "orgUnits", label: "Org Units", value: summary.orgUnits },
    { key: "knowledgeSources", label: "Knowledge", value: summary.knowledgeSources },
  ];
}

function badgeTone(statusGroup: string): "ok" | "warn" | "err" | "info" | "ghost" {
  const normalized = String(statusGroup || "").toLowerCase();
  if (["active", "available", "scoped", "held", "tracked"].includes(normalized)) return "info";
  if (["waiting", "queued", "changed", "fallback", "constrained", "claimed", "rehydrated"].includes(normalized)) {
    return "warn";
  }
  if (["failed", "error"].includes(normalized)) return "err";
  if (["completed", "unchanged", "terminal"].includes(normalized)) return "ok";
  return "ghost";
}

function selectedRunIdFromHash() {
  if (typeof window === "undefined") return "";
  const [, query = ""] = String(window.location.hash || "").split("?");
  return new URLSearchParams(query).get("run") || "";
}

function replaceRunSelectionHash(runId: string) {
  if (typeof window === "undefined" || !runId) return;
  const hash = `#/runs?run=${encodeURIComponent(runId)}`;
  window.history.replaceState(null, "", `${window.location.pathname}${window.location.search}${hash}`);
}

async function runObservabilityPayload(api: RunsProps["api"], runId: string) {
  const query = "event_limit=80&snapshot_limit=25&reliability_limit=80&audit_limit=25";
  return api(`/api/engine/stateful-runtime/runs/${encodeURIComponent(runId)}/observability?${query}`);
}

function compactRows(rows: any[], limit = 5) {
  return Array.isArray(rows) ? rows.slice(0, limit) : [];
}

function recordTime(row: any) {
  return row?.occurredAtMs || row?.updatedAtMs || row?.createdAtMs || 0;
}

function DetailRecords({ title, rows, emptyText }: { title: string; rows: any[]; emptyText: string }) {
  const visibleRows = compactRows(rows);
  return (
    <section className="border-t border-white/10 pt-3">
      <div className="mb-2 flex items-center justify-between gap-3">
        <h3 className="text-xs font-semibold uppercase tracking-wide text-tcp-text-muted">{title}</h3>
        <span className="tcp-text-caption tabular-nums text-tcp-text-muted">{rows?.length || 0}</span>
      </div>
      {visibleRows.length ? (
        <div className="space-y-2">
          {visibleRows.map((row: any, index: number) => (
            <div key={`${row.id || row.label || title}:${index}`} className="border-l border-white/10 pl-3">
              <div className="flex min-w-0 items-center justify-between gap-2">
                <span className="truncate text-xs font-medium text-tcp-text-secondary">{row.label || row.id}</span>
                {row.status ? <Badge tone={badgeTone(row.statusGroup || row.status)}>{row.status}</Badge> : null}
              </div>
              {row.detail ? <div className="mt-1 line-clamp-2 tcp-text-caption text-tcp-text-muted">{row.detail}</div> : null}
              {row.changes?.length ? (
                <div className="mt-2 space-y-1">
                  {row.changes.slice(0, 4).map((change: any) => (
                    <div key={change.key || change.label} className="grid gap-0.5 tcp-text-micro text-tcp-text-muted">
                      <span>{change.label}</span>
                      <span className="break-all font-mono text-tcp-text-secondary">
                        {change.from} -&gt; {change.to}
                      </span>
                    </div>
                  ))}
                </div>
              ) : null}
              {row.seq || recordTime(row) ? (
                <div className="mt-1 flex min-w-0 gap-2 tcp-text-micro text-tcp-text-muted">
                  {row.seq ? <span className="font-mono">seq {row.seq}</span> : null}
                  {recordTime(row) ? <span>{formatRunTimestamp(recordTime(row))}</span> : null}
                </div>
              ) : null}
            </div>
          ))}
        </div>
      ) : (
        <div className="text-xs text-tcp-text-muted">{emptyText}</div>
      )}
    </section>
  );
}

function RunObservabilityPanel({
  selectedRow,
  detail,
  loading,
  error,
  onOpen,
}: {
  selectedRow: any;
  detail: any;
  loading: boolean;
  error: string;
  onOpen: () => void;
}) {
  if (!selectedRow) {
    return (
      <PanelCard title="Run Detail" subtitle="Select a run to inspect durable state." fullHeight>
        <EmptyState title="No run selected" text="Run details will appear here." />
      </PanelCard>
    );
  }

  if (selectedRow.source === "context") {
    return (
      <PanelCard title="Run Detail" subtitle={selectedRow.title} fullHeight>
        <div className="space-y-3 text-sm text-tcp-text-secondary">
          <p>Context run details are available from the Orchestrator surface.</p>
          <button type="button" className="tcp-btn h-8 px-3 text-xs" onClick={onOpen}>
            <Icon name="external-link" />
            Open
          </button>
        </div>
      </PanelCard>
    );
  }

  return (
    <PanelCard
      title="Run Detail"
      subtitle={selectedRow.title}
      fullHeight
      actions={
        <Toolbar>
          <button type="button" className="tcp-btn h-8 px-3 text-xs" onClick={onOpen}>
            <Icon name="external-link" />
            Open
          </button>
        </Toolbar>
      }
    >
      {loading ? (
        <LoadingState title="Loading run detail" />
      ) : error ? (
        <EmptyState title="Detail unavailable" text={error} />
      ) : (
        <div className="min-h-0 space-y-4 overflow-auto pr-1">
          <div className="grid gap-3 sm:grid-cols-2">
            <div>
              <div className="tcp-text-caption uppercase tracking-wide text-tcp-text-muted">Status</div>
              <div className="mt-1 flex items-center gap-2">
                <Badge tone={badgeTone(selectedRow.statusGroup)}>{detail.statusLabel || selectedRow.statusLabel}</Badge>
                {detail.isBlocked ? <Badge tone="warn">blocked</Badge> : null}
              </div>
            </div>
            <div>
              <div className="tcp-text-caption uppercase tracking-wide text-tcp-text-muted">Runtime Phase</div>
              <div className="mt-1 truncate text-sm text-tcp-text-secondary">
                {detail.runtimePhase || detail.phase || selectedRow.phase}
              </div>
              {detail.phase && detail.phase !== detail.runtimePhase ? (
                <div className="mt-1 truncate tcp-text-caption text-tcp-text-muted">{detail.phase}</div>
              ) : null}
            </div>
            <div>
              <div className="tcp-text-caption uppercase tracking-wide text-tcp-text-muted">Current Wait</div>
              <div className="mt-1 truncate text-sm text-tcp-text-secondary">
                {detail.currentWait?.label || selectedRow.currentWait || "n/a"}
              </div>
              {detail.currentWait?.detail ? (
                <div className="mt-1 truncate tcp-text-caption text-tcp-text-muted">{detail.currentWait.detail}</div>
              ) : null}
            </div>
            <div>
              <div className="tcp-text-caption uppercase tracking-wide text-tcp-text-muted">Workflow Version</div>
              <div className="mt-1 truncate font-mono tcp-text-caption text-tcp-text-secondary">
                {detail.workflowDefinitionVersion || "n/a"}
              </div>
            </div>
          </div>

          <section className="border-t border-white/10 pt-3">
            <h3 className="text-xs font-semibold uppercase tracking-wide text-tcp-text-muted">Replay Boundary</h3>
            <div className="mt-2 grid grid-cols-3 gap-2 text-xs">
              <div>
                <div className="text-tcp-text-muted">Events</div>
                <div className="mt-1 font-mono text-tcp-text-secondary">{detail.replay?.eventCount || 0}</div>
              </div>
              <div>
                <div className="text-tcp-text-muted">Seq</div>
                <div className="mt-1 font-mono text-tcp-text-secondary">
                  {detail.replay?.firstSeq && detail.replay?.lastSeq
                    ? `${detail.replay.firstSeq}-${detail.replay.lastSeq}`
                    : "n/a"}
                </div>
              </div>
              <div>
                <div className="text-tcp-text-muted">Snapshots</div>
                <div className="mt-1 font-mono text-tcp-text-secondary">{detail.counts?.snapshots || 0}</div>
              </div>
            </div>
            {detail.replay?.unsafeReasons?.length ? (
              <div className="mt-2 space-y-1">
                {compactRows(detail.replay.unsafeReasons, 3).map((reason: string) => (
                  <div key={reason} className="tcp-text-caption text-yellow-100">
                    {reason}
                  </div>
                ))}
              </div>
            ) : (
              <div className="mt-2 text-xs text-tcp-text-muted">No replay blockers reported by the aggregate view.</div>
            )}
          </section>

          {detail.crashRecoverySnapshotDiff ? (
            <DetailRecords
              title="Crash Recovery"
              rows={[detail.crashRecoverySnapshotDiff]}
              emptyText="No crash recovery checkpoint diff."
            />
          ) : null}
          <DetailRecords title="Operator Actions" rows={detail.allowedActions || []} emptyText="No operator actions." />
          <DetailRecords title="Blocking Reasons" rows={detail.blockingReasons || []} emptyText="No blocking reasons." />
          <DetailRecords
            title="Legal Next Transitions"
            rows={detail.allowedNextPhases || []}
            emptyText="No legal transition data."
          />
          <DetailRecords
            title="Locks & Constraints"
            rows={detail.lockConstraints || []}
            emptyText="No locks or workspace constraints."
          />
          <DetailRecords title="Phase History" rows={detail.phaseHistory || []} emptyText="No phase history." />
          <DetailRecords title="Events" rows={detail.events || []} emptyText="No event tail." />
          <DetailRecords title="Snapshots" rows={detail.snapshots || []} emptyText="No snapshots." />
          <DetailRecords title="Snapshot Diffs" rows={detail.snapshotDiffs || []} emptyText="Need at least two snapshots." />
          <DetailRecords title="Policy Decisions" rows={detail.policyDecisions || []} emptyText="No policy decisions." />
          <DetailRecords title="Tool Effects" rows={detail.toolEffects || []} emptyText="No tool effects." />
          <DetailRecords title="Outbox" rows={detail.outbox || []} emptyText="No outbox rows." />
          <DetailRecords title="Dead Letters" rows={detail.deadLetters || []} emptyText="No dead letters." />
          <DetailRecords title="Compensations" rows={detail.compensations || []} emptyText="No compensations." />
          <DetailRecords title="Protected Audit" rows={detail.protectedAuditEvents || []} emptyText="No protected audit rows." />
        </div>
      )}
    </PanelCard>
  );
}

export function StatefulRunsPage({
  api,
  client,
  navigate,
  filters,
  onFiltersChange,
}: StatefulRunsPageProps) {
  const [selectedRunKey, setSelectedRunKey] = useState("");
  const [selectedRunIdHint, setSelectedRunIdHint] = useState(selectedRunIdFromHash);
  const runsQuery = useQuery({
    queryKey: ["stateful-runs", "list"],
    queryFn: () => runListPayload({ api, client }),
    refetchInterval: 10000,
  });

  const rows = useMemo(
    () =>
      buildStatefulRunRows({
        statefulRuns: runsQuery.data?.statefulRuns ?? [],
        workflowRuns: runsQuery.data?.workflowRuns ?? [],
        legacyRuns: runsQuery.data?.legacyRuns ?? [],
        contextRuns: runsQuery.data?.contextRuns ?? [],
      }),
    [runsQuery.data]
  );
  const filteredRows = useMemo(() => filterStatefulRunRows(rows, filters), [rows, filters]);
  const summary = useMemo(() => summarizeStatefulRuns(rows), [rows]);
  const selectedRow = useMemo(
    () =>
      filteredRows.find((row: any) => `${row.source}:${row.id}` === selectedRunKey) ||
      filteredRows.find((row: any) => row.id === selectedRunIdHint || row.canonicalId === selectedRunIdHint) ||
      filteredRows[0] ||
      null,
    [filteredRows, selectedRunIdHint, selectedRunKey]
  );
  const selectedObservabilityRunId = selectedRow?.observabilityRunId || selectedRow?.id || "";
  const detailQuery = useQuery({
    queryKey: ["stateful-runs", "observability", selectedObservabilityRunId],
    queryFn: () =>
      runObservabilityPayload(api, selectedObservabilityRunId).catch((error: any) => ({
        error: errorText(error),
      })),
    enabled: Boolean(selectedObservabilityRunId && selectedRow?.source !== "context"),
    refetchInterval: 10000,
  });
  const detail = useMemo(
    () => buildRunObservabilityDetail(detailQuery.data && !detailQuery.data.error ? detailQuery.data : {}),
    [detailQuery.data]
  );
  const loading = runsQuery.isLoading && !runsQuery.data;
  const errors = runsQuery.data?.errors ?? [];
  const selectRow = (rowKey: string, row: any) => {
    const runId = row?.canonicalId || row?.id || "";
    setSelectedRunKey(rowKey);
    setSelectedRunIdHint(runId);
    replaceRunSelectionHash(runId);
  };

  return (
    <div className="grid h-full min-h-0 grid-rows-[auto_1fr] gap-4">
      <PanelCard
        title="Stateful Runs"
        subtitle="Workflow, automation, and context activity by tenant and workspace."
        actions={
          <Toolbar>
            <button
              type="button"
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() => runsQuery.refetch()}
              disabled={runsQuery.isFetching}
            >
              <Icon name="refresh-cw" />
              Refresh
            </button>
            <button
              type="button"
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() => navigate("automations")}
            >
              <Icon name="bot" />
              Automations
            </button>
          </Toolbar>
        }
      >
        <div className="grid gap-4">
          <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4 xl:grid-cols-8">
            {metricItems(summary).map((item) => (
              <div
                key={item.key}
                className="min-h-[4.5rem] rounded-md border border-white/10 bg-white/[0.03] px-3 py-2"
              >
                <div className="tcp-text-caption uppercase tracking-wide text-tcp-text-muted">{item.label}</div>
                <div className="mt-1 text-2xl font-semibold tabular-nums text-tcp-text-primary">
                  {item.value}
                </div>
              </div>
            ))}
          </div>

          <StatefulRunFilterBar filters={filters} onFiltersChange={onFiltersChange} />

          {errors.length ? (
            <div className="rounded-md border border-yellow-400/20 bg-yellow-400/10 px-3 py-2 text-xs text-yellow-100">
              {errors.join(" · ")}
            </div>
          ) : null}
        </div>
      </PanelCard>

      <div className="grid min-h-0 gap-4 xl:grid-cols-[minmax(0,1.35fr)_minmax(24rem,0.65fr)]">
        <PanelCard
          title="Run List"
          subtitle={`${filteredRows.length} of ${rows.length} runs`}
          fullHeight
        >
          {loading ? (
            <LoadingState title="Loading runs" />
          ) : filteredRows.length ? (
            <div className="min-h-0 flex-1 overflow-auto rounded-lg border border-white/10">
              <table className="w-full min-w-[1380px] table-fixed text-left text-xs">
                <thead className="sticky top-0 z-10 bg-black/80 tcp-text-caption uppercase text-tcp-text-muted backdrop-blur">
                  <tr>
                    <th className="w-[20rem] px-3 py-2 font-medium">Run</th>
                    <th className="w-[8rem] px-3 py-2 font-medium">Status</th>
                    <th className="w-[10rem] px-3 py-2 font-medium">Phase</th>
                    <th className="w-[9rem] px-3 py-2 font-medium">Trigger</th>
                    <th className="w-[13rem] px-3 py-2 font-medium">Tenant</th>
                    <th className="w-[17rem] px-3 py-2 font-medium">Scope</th>
                    <th className="w-[15rem] px-3 py-2 font-medium">Workspace</th>
                    <th className="w-[14rem] px-3 py-2 font-medium">Wait</th>
                    <th className="w-[12rem] px-3 py-2 font-medium">Retry</th>
                    <th className="w-[10rem] px-3 py-2 font-medium">Updated</th>
                    <th className="w-[9rem] px-3 py-2 font-medium"></th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-white/8">
                  {filteredRows.map((row: any) => {
                    const rowKey = `${row.source}:${row.id}`;
                    const selected = rowKey === `${selectedRow?.source}:${selectedRow?.id}`;
                    return (
                      <tr
                        key={rowKey}
                        className={`align-top hover:bg-white/[0.03] ${selected ? "bg-white/[0.045]" : ""}`}
                        onClick={() => selectRow(rowKey, row)}
                      >
                        <td className="px-3 py-3">
                          <div className="min-w-0">
                            <div className="truncate text-sm font-medium text-tcp-text-primary">{row.title}</div>
                            <div className="mt-1 flex min-w-0 flex-wrap gap-2 tcp-text-caption text-tcp-text-muted">
                              <span className="font-mono">{row.id}</span>
                              <span>{row.sourceLabel}</span>
                            </div>
                          </div>
                        </td>
                        <td className="px-3 py-3">
                          <Badge tone={badgeTone(row.statusGroup)}>{row.statusLabel}</Badge>
                        </td>
                        <td className="px-3 py-3 text-tcp-text-secondary">{row.phase}</td>
                        <td className="px-3 py-3 text-tcp-text-secondary">{row.triggerSource}</td>
                        <td className="px-3 py-3">
                          <div className="truncate text-tcp-text-secondary">{row.tenantOrg}</div>
                          <div className="truncate tcp-text-caption text-tcp-text-muted">{row.tenantWorkspace}</div>
                        </td>
                        <td className="px-3 py-3">
                          <div className="min-h-[4.25rem] rounded-md border border-white/10 bg-white/[0.025] px-2.5 py-2">
                            <div className="truncate text-tcp-text-secondary">
                              {row.orgUnitName || row.orgUnitId || "Tenant scoped"}
                            </div>
                            <div className="mt-1 truncate font-mono tcp-text-caption text-tcp-text-muted">
                              {row.resourceLabel || row.resourceId || "n/a"}
                            </div>
                            <div className="mt-1 flex min-w-0 flex-wrap gap-1">
                              {row.policyVersion ? (
                                <span className="rounded border border-white/10 px-1.5 py-0.5 tcp-text-micro text-tcp-text-muted">
                                  {row.policyVersion}
                                </span>
                              ) : null}
                              {row.dataClasses?.slice(0, 2).map((dataClass: string) => (
                                <span
                                  key={dataClass}
                                  className="rounded border border-white/10 px-1.5 py-0.5 tcp-text-micro text-tcp-text-muted"
                                >
                                  {dataClass}
                                </span>
                              ))}
                              {row.knowledgeSourceCount ? (
                                <span className="rounded border border-white/10 px-1.5 py-0.5 tcp-text-micro text-tcp-text-muted">
                                  {row.knowledgeSourceCount} src
                                </span>
                              ) : null}
                            </div>
                          </div>
                        </td>
                        <td className="px-3 py-3">
                          <div className="truncate font-mono tcp-text-caption text-tcp-text-secondary">
                            {row.workspace || "n/a"}
                          </div>
                        </td>
                        <td className="px-3 py-3">
                          <div className="line-clamp-2 text-tcp-text-secondary">{row.currentWait || "n/a"}</div>
                          {row.waitDetail ? (
                            <div className="mt-1 truncate tcp-text-caption text-tcp-text-muted">{row.waitDetail}</div>
                          ) : null}
                        </td>
                        <td className="px-3 py-3">
                          <div className="truncate text-tcp-text-secondary">{row.retryState}</div>
                          {row.retryDetail ? (
                            <div className="mt-1 truncate tcp-text-caption text-tcp-text-muted">{row.retryDetail}</div>
                          ) : null}
                        </td>
                        <td className="px-3 py-3 text-tcp-text-secondary">
                          {formatRunTimestamp(row.updatedAtMs)}
                        </td>
                        <td className="px-3 py-3">
                          <div className="flex gap-1">
                            <button
                              type="button"
                              aria-label="Inspect run detail"
                              className="tcp-btn h-7 px-2 text-xs"
                              onClick={(event) => {
                                event.stopPropagation();
                                selectRow(rowKey, row);
                              }}
                              title="Inspect run detail"
                            >
                              <Icon name="search" />
                            </button>
                            <button
                              type="button"
                              aria-label={`Open ${row.sourceLabel.toLowerCase()} view`}
                              className="tcp-btn h-7 px-2 text-xs"
                              onClick={(event) => {
                                event.stopPropagation();
                                navigate(row.route);
                              }}
                              title={`Open ${row.sourceLabel.toLowerCase()} view`}
                            >
                              <Icon name="external-link" />
                            </button>
                          </div>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          ) : (
            <EmptyState
              title={rows.length ? "No matching runs" : "No runs yet"}
              text={rows.length ? "Try another filter." : "Run activity will appear here."}
            />
          )}
        </PanelCard>

        <RunObservabilityPanel
          selectedRow={selectedRow}
          detail={detail}
          loading={detailQuery.isLoading && !detailQuery.data && selectedRow?.source !== "context"}
          error={detailQuery.data?.error || ""}
          onOpen={() => selectedRow && navigate(selectedRow.route)}
        />
      </div>
    </div>
  );
}
