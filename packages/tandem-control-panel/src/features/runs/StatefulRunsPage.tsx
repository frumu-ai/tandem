import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Badge, LoadingState, PanelCard, Toolbar } from "../../ui/index.tsx";
import { EmptyState } from "../../pages/ui";
import {
  DEFAULT_STATEFUL_RUN_FILTERS,
  RUN_SOURCE_FILTERS,
  RUN_STATUS_FILTERS,
  buildStatefulRunRows,
  filterStatefulRunRows,
  formatRunTimestamp,
  normalizeStatefulRunFilters,
  summarizeStatefulRuns,
} from "../../../lib/runs/stateful-runs.js";
import type { AppPageProps } from "../../pages/pageTypes";

type RunsProps = Pick<AppPageProps, "api" | "client" | "navigate">;
type RunListRequest = Pick<AppPageProps, "api" | "client">;

type RunListPayload = {
  statefulRuns: any[];
  workflowRuns: any[];
  legacyRuns: any[];
  contextRuns: any[];
  errors: string[];
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
  const canonicalRuns = await api("/api/engine/stateful-runtime/runs?limit=120").catch((error: any) => ({
    runs: [],
    error: errorText(error),
  }));
  if (!canonicalRuns?.error) {
    const contextRuns = await api("/api/engine/context/runs?limit=120").catch((error: any) => ({
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

  const [workflowRuns, legacyRuns, contextRuns] = await Promise.all([
    api("/api/engine/automations/v2/runs?limit=120").catch((error: any) => ({
      runs: [],
      error: errorText(error),
    })),
    client?.automations?.listRuns?.({ limit: 120 }).catch((error: any) => ({
      runs: [],
      error: errorText(error),
    })) ?? Promise.resolve({ runs: [] }),
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
  if (statusGroup === "active") return "info";
  if (statusGroup === "waiting" || statusGroup === "queued") return "warn";
  if (statusGroup === "failed") return "err";
  if (statusGroup === "completed") return "ok";
  return "ghost";
}

function setFilter(filters: any, key: string, value: string) {
  return normalizeStatefulRunFilters({ ...filters, [key]: value });
}

export function StatefulRunsPage({ api, client, navigate }: RunsProps) {
  const [filters, setFilters] = useState(DEFAULT_STATEFUL_RUN_FILTERS);
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
  const hasFilters = JSON.stringify(normalizeStatefulRunFilters(filters)) !== JSON.stringify(DEFAULT_STATEFUL_RUN_FILTERS);
  const loading = runsQuery.isLoading && !runsQuery.data;
  const errors = runsQuery.data?.errors ?? [];

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
              <i data-lucide="refresh-cw"></i>
              Refresh
            </button>
            <button
              type="button"
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() => navigate("automations")}
            >
              <i data-lucide="bot"></i>
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
                <div className="text-[11px] uppercase tracking-wide text-tcp-text-muted">{item.label}</div>
                <div className="mt-1 text-2xl font-semibold tabular-nums text-tcp-text-primary">
                  {item.value}
                </div>
              </div>
            ))}
          </div>

          <div className="grid gap-2 md:grid-cols-2 xl:grid-cols-4 2xl:grid-cols-[1.4fr_repeat(5,minmax(0,1fr))_auto]">
            <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
              <span>Search</span>
              <input
                className="tcp-input h-9 text-sm"
                value={filters.query}
                onChange={(event) => setFilters((current) => setFilter(current, "query", event.currentTarget.value))}
                placeholder="Run, workflow, trigger"
              />
            </label>
            <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
              <span>Status</span>
              <select
                className="tcp-input h-9 text-sm"
                value={filters.status}
                onChange={(event) => setFilters((current) => setFilter(current, "status", event.currentTarget.value))}
              >
                {RUN_STATUS_FILTERS.map((option: any) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>
            <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
              <span>Source</span>
              <select
                className="tcp-input h-9 text-sm"
                value={filters.source}
                onChange={(event) => setFilters((current) => setFilter(current, "source", event.currentTarget.value))}
              >
                {RUN_SOURCE_FILTERS.map((option: any) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>
            <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
              <span>Tenant</span>
              <input
                className="tcp-input h-9 text-sm"
                value={filters.tenant}
                onChange={(event) => setFilters((current) => setFilter(current, "tenant", event.currentTarget.value))}
                placeholder="Org or workspace"
              />
            </label>
            <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
              <span>Workspace</span>
              <input
                className="tcp-input h-9 text-sm"
                value={filters.workspace}
                onChange={(event) =>
                  setFilters((current) => setFilter(current, "workspace", event.currentTarget.value))
                }
                placeholder="Path"
              />
            </label>
            <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
              <span>Org Unit</span>
              <input
                className="tcp-input h-9 text-sm"
                value={filters.orgUnit}
                onChange={(event) => setFilters((current) => setFilter(current, "orgUnit", event.currentTarget.value))}
                placeholder="Unit or owner"
              />
            </label>
            <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
              <span>Resource</span>
              <input
                className="tcp-input h-9 text-sm"
                value={filters.resource}
                onChange={(event) => setFilters((current) => setFilter(current, "resource", event.currentTarget.value))}
                placeholder="Kind or ID"
              />
            </label>
            <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
              <span>Policy</span>
              <input
                className="tcp-input h-9 text-sm"
                value={filters.policy}
                onChange={(event) => setFilters((current) => setFilter(current, "policy", event.currentTarget.value))}
                placeholder="Version"
              />
            </label>
            <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
              <span>Data</span>
              <input
                className="tcp-input h-9 text-sm"
                value={filters.dataClass}
                onChange={(event) => setFilters((current) => setFilter(current, "dataClass", event.currentTarget.value))}
                placeholder="Class"
              />
            </label>
            <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
              <span>Knowledge</span>
              <input
                className="tcp-input h-9 text-sm"
                value={filters.knowledge}
                onChange={(event) => setFilters((current) => setFilter(current, "knowledge", event.currentTarget.value))}
                placeholder="Source"
              />
            </label>
            <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
              <span>Phase</span>
              <input
                className="tcp-input h-9 text-sm"
                value={filters.wait}
                onChange={(event) => setFilters((current) => setFilter(current, "wait", event.currentTarget.value))}
                placeholder="Wait or retry"
              />
            </label>
            <div className="flex items-end">
              <button
                type="button"
                className="tcp-btn h-9 w-full px-3 text-xs"
                onClick={() => setFilters(DEFAULT_STATEFUL_RUN_FILTERS)}
                disabled={!hasFilters}
              >
                <i data-lucide="x"></i>
                Clear
              </button>
            </div>
          </div>

          {errors.length ? (
            <div className="rounded-md border border-yellow-400/20 bg-yellow-400/10 px-3 py-2 text-xs text-yellow-100">
              {errors.join(" · ")}
            </div>
          ) : null}
        </div>
      </PanelCard>

      <PanelCard
        title="Run List"
        subtitle={`${filteredRows.length} of ${rows.length} runs`}
        fullHeight
      >
        {loading ? (
          <LoadingState title="Loading runs" />
        ) : filteredRows.length ? (
          <div className="min-h-0 flex-1 overflow-auto rounded-lg border border-white/10">
            <table className="w-full min-w-[1340px] table-fixed text-left text-xs">
              <thead className="sticky top-0 z-10 bg-black/80 text-[11px] uppercase text-tcp-text-muted backdrop-blur">
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
                  <th className="w-[6rem] px-3 py-2 font-medium"></th>
                </tr>
              </thead>
              <tbody className="divide-y divide-white/8">
                {filteredRows.map((row: any) => (
                  <tr key={`${row.source}:${row.id}`} className="align-top hover:bg-white/[0.03]">
                    <td className="px-3 py-3">
                      <div className="min-w-0">
                        <div className="truncate text-sm font-medium text-tcp-text-primary">{row.title}</div>
                        <div className="mt-1 flex min-w-0 flex-wrap gap-2 text-[11px] text-tcp-text-muted">
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
                      <div className="truncate text-[11px] text-tcp-text-muted">{row.tenantWorkspace}</div>
                    </td>
                    <td className="px-3 py-3">
                      <div className="min-h-[4.25rem] rounded-md border border-white/10 bg-white/[0.025] px-2.5 py-2">
                        <div className="truncate text-tcp-text-secondary">
                          {row.orgUnitName || row.orgUnitId || "Tenant scoped"}
                        </div>
                        <div className="mt-1 truncate font-mono text-[11px] text-tcp-text-muted">
                          {row.resourceLabel || row.resourceId || "n/a"}
                        </div>
                        <div className="mt-1 flex min-w-0 flex-wrap gap-1">
                          {row.policyVersion ? (
                            <span className="rounded border border-white/10 px-1.5 py-0.5 text-[10px] text-tcp-text-muted">
                              {row.policyVersion}
                            </span>
                          ) : null}
                          {row.dataClasses?.slice(0, 2).map((dataClass: string) => (
                            <span
                              key={dataClass}
                              className="rounded border border-white/10 px-1.5 py-0.5 text-[10px] text-tcp-text-muted"
                            >
                              {dataClass}
                            </span>
                          ))}
                          {row.knowledgeSourceCount ? (
                            <span className="rounded border border-white/10 px-1.5 py-0.5 text-[10px] text-tcp-text-muted">
                              {row.knowledgeSourceCount} src
                            </span>
                          ) : null}
                        </div>
                      </div>
                    </td>
                    <td className="px-3 py-3">
                      <div className="truncate font-mono text-[11px] text-tcp-text-secondary">
                        {row.workspace || "n/a"}
                      </div>
                    </td>
                    <td className="px-3 py-3">
                      <div className="line-clamp-2 text-tcp-text-secondary">{row.currentWait || "n/a"}</div>
                      {row.waitDetail ? (
                        <div className="mt-1 truncate text-[11px] text-tcp-text-muted">{row.waitDetail}</div>
                      ) : null}
                    </td>
                    <td className="px-3 py-3">
                      <div className="truncate text-tcp-text-secondary">{row.retryState}</div>
                      {row.retryDetail ? (
                        <div className="mt-1 truncate text-[11px] text-tcp-text-muted">{row.retryDetail}</div>
                      ) : null}
                    </td>
                    <td className="px-3 py-3 text-tcp-text-secondary">
                      {formatRunTimestamp(row.updatedAtMs)}
                    </td>
                    <td className="px-3 py-3">
                      <button
                        type="button"
                        className="tcp-btn h-7 px-2 text-xs"
                        onClick={() => navigate(row.route)}
                        title={`Open ${row.sourceLabel.toLowerCase()} view`}
                      >
                        <i data-lucide="external-link"></i>
                        Open
                      </button>
                    </td>
                  </tr>
                ))}
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
    </div>
  );
}
