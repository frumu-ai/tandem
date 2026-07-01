import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Badge, EmptyState, LoadingState, PanelCard, Toolbar } from "../../ui/index.tsx";
import {
  buildEnterpriseScopeExplorerModel,
  selectEnterpriseScope,
  titleCase,
} from "../../../lib/enterprise/scope-explorer.js";
import { formatRunTimestamp } from "../../../lib/runs/stateful-runs.js";
import type { AppPageProps } from "../../pages/pageTypes";
import type {
  EnterpriseConnectorInstance,
  EnterpriseOrganizationUnit,
  EnterpriseOrganizationUnitAccessGrant,
  EnterpriseOrganizationUnitMembership,
  EnterpriseScopedGrant,
  EnterpriseSourceBinding,
  EnterpriseSourceObjectLifecycle,
} from "./queries";

type EnterpriseScopeExplorerProps = {
  api: AppPageProps["api"];
  navigate: AppPageProps["navigate"];
  orgUnits: EnterpriseOrganizationUnit[];
  memberships: EnterpriseOrganizationUnitMembership[];
  accessGrants: EnterpriseOrganizationUnitAccessGrant[];
  effectiveGrants: EnterpriseScopedGrant[];
  connectors: EnterpriseConnectorInstance[];
  sourceBindings: EnterpriseSourceBinding[];
  sourceObjects: EnterpriseSourceObjectLifecycle[];
  loading: boolean;
  error: unknown;
};

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error || "");
}

function statusTone(status: string): "ok" | "warn" | "err" | "info" | "ghost" {
  const key = String(status || "").toLowerCase();
  if (["active", "allow", "visible", "running", "completed", "ok"].includes(key)) return "ok";
  if (["blocked", "deny", "failed", "revoked", "quarantined", "cancelled"].includes(key)) return "err";
  if (["paused", "pending", "draft", "visible after review"].includes(key)) return "warn";
  return key ? "info" : "ghost";
}

function replaceRunSelectionHash(runId: string) {
  if (typeof window === "undefined" || !runId) return;
  const hash = `#/runs?run=${encodeURIComponent(runId)}`;
  window.history.replaceState(null, "", `${window.location.pathname}${window.location.search}${hash}`);
}

function ScopeMetric({ label, value }: { label: string; value: number }) {
  return (
    <div className="min-h-[4rem] rounded-md border border-white/10 bg-white/[0.03] px-3 py-2">
      <div className="text-[11px] uppercase tracking-wide text-tcp-text-muted">{label}</div>
      <div className="mt-1 text-2xl font-semibold tabular-nums text-tcp-text-primary">{value}</div>
    </div>
  );
}

function ScopeList({
  scopes,
  selectedScopeId,
  onSelect,
}: {
  scopes: any[];
  selectedScopeId: string;
  onSelect: (scopeId: string) => void;
}) {
  if (!scopes.length) return <EmptyState title="No scopes" text="Enterprise scopes will appear here." />;
  return (
    <div className="min-h-0 space-y-2 overflow-auto pr-1">
      {scopes.map((scope) => (
        <button
          key={scope.id}
          type="button"
          className={`tcp-list-item w-full text-left ${selectedScopeId === scope.id ? "border-emerald-400/40 bg-emerald-400/10" : ""}`}
          onClick={() => onSelect(scope.id)}
        >
          <div className="flex min-w-0 items-center justify-between gap-3">
            <div className="min-w-0">
              <div className="truncate text-sm font-medium text-tcp-text-primary">{scope.label}</div>
              <div className="mt-1 truncate text-[11px] text-tcp-text-muted">
                {scope.kind === "org_unit" ? scope.orgUnitId : scope.resourceLabel}
              </div>
            </div>
            <Badge tone={scope.kind === "org_unit" ? "info" : "ok"}>{titleCase(scope.kind)}</Badge>
          </div>
        </button>
      ))}
    </div>
  );
}

function OrgTree({ nodes }: { nodes: any[] }) {
  if (!nodes.length) return <EmptyState title="No org tree" text="Organization units will appear here." />;
  return (
    <div className="space-y-2">
      {nodes.slice(0, 8).map((node) => (
        <div key={node.id} className="rounded-md border border-white/10 bg-black/20 px-3 py-2">
          <div className="flex items-center justify-between gap-3">
            <div className="min-w-0" style={{ paddingLeft: `${Math.min(node.depth, 4) * 0.75}rem` }}>
              <div className="truncate text-sm font-medium text-tcp-text-primary">{node.label}</div>
              <div className="mt-1 truncate text-[11px] text-tcp-text-muted">{node.unitId}</div>
            </div>
            <Badge tone={statusTone(node.state)}>{node.state}</Badge>
          </div>
        </div>
      ))}
    </div>
  );
}

function PolicyLayers({ layers }: { layers: any[] }) {
  return (
    <div className="space-y-3">
      {layers.map((layer) => (
        <div key={layer.order} className="rounded-md border border-white/10 bg-black/20 px-3 py-3">
          <div className="flex min-w-0 items-center justify-between gap-3">
            <div className="min-w-0">
              <div className="text-[11px] uppercase tracking-wide text-tcp-text-muted">
                {layer.order}. {layer.layer}
              </div>
              <div className="mt-1 truncate text-sm font-medium text-tcp-text-primary">{layer.source}</div>
            </div>
            <Badge tone={layer.conflicts.length ? "err" : "ok"}>{layer.conflicts.length ? "conflict" : "clear"}</Badge>
          </div>
          <div className="mt-2 text-xs text-tcp-text-secondary">{layer.decision}</div>
          {layer.overrides.length ? (
            <div className="mt-2 flex flex-wrap gap-1">
              {layer.overrides.slice(0, 4).map((override: string) => (
                <span key={override} className="rounded border border-white/10 px-2 py-0.5 text-[11px] text-tcp-text-muted">
                  {override}
                </span>
              ))}
            </div>
          ) : null}
          {layer.conflicts.length ? (
            <div className="mt-2 space-y-1">
              {layer.conflicts.slice(0, 3).map((conflict: string) => (
                <div key={conflict} className="text-[11px] text-rose-200">
                  {conflict}
                </div>
              ))}
            </div>
          ) : null}
        </div>
      ))}
    </div>
  );
}

function KnowledgeBoundary({ rows }: { rows: any[] }) {
  if (!rows.length) return <EmptyState title="No knowledge sources" text="Source bindings will appear here." />;
  return (
    <div className="min-h-0 overflow-auto rounded-lg border border-white/10">
      <table className="w-full min-w-[820px] table-fixed text-left text-xs">
        <thead className="sticky top-0 z-10 bg-black/80 text-[11px] uppercase text-tcp-text-muted backdrop-blur">
          <tr>
            <th className="w-[17rem] px-3 py-2 font-medium">Source</th>
            <th className="w-[9rem] px-3 py-2 font-medium">Visibility</th>
            <th className="w-[15rem] px-3 py-2 font-medium">Reason</th>
            <th className="w-[14rem] px-3 py-2 font-medium">Resource</th>
            <th className="w-[8rem] px-3 py-2 font-medium">Objects</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-white/8">
          {rows.map((row) => (
            <tr key={row.id || row.label} className="align-top hover:bg-white/[0.03]">
              <td className="px-3 py-3">
                <div className="truncate text-sm font-medium text-tcp-text-primary">{row.label}</div>
                <div className="mt-1 truncate text-[11px] text-tcp-text-muted">{row.connectorId || row.sourceType}</div>
              </td>
              <td className="px-3 py-3">
                <Badge tone={row.visibility === "visible" ? "ok" : "err"}>{row.visibility}</Badge>
              </td>
              <td className="px-3 py-3 text-tcp-text-secondary">{row.reason}</td>
              <td className="px-3 py-3 text-tcp-text-secondary">{row.resourceLabel || "run scope"}</td>
              <td className="px-3 py-3 tabular-nums text-tcp-text-secondary">{row.objectCount}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function RecentRuns({
  rows,
  onOpenRun,
}: {
  rows: any[];
  onOpenRun: (runId: string) => void;
}) {
  if (!rows.length) return <EmptyState title="No scoped runs" text="Recent matching stateful runs will appear here." />;
  return (
    <div className="space-y-2">
      {rows.map((row) => (
        <div key={row.id} className="rounded-md border border-white/10 bg-black/20 px-3 py-2">
          <div className="flex min-w-0 items-start justify-between gap-3">
            <div className="min-w-0">
              <div className="truncate text-sm font-medium text-tcp-text-primary">{row.title}</div>
              <div className="mt-1 flex min-w-0 flex-wrap gap-2 text-[11px] text-tcp-text-muted">
                <span className="font-mono">{row.id}</span>
                <span>{row.owner || "tenant owner"}</span>
                <span>{formatRunTimestamp(row.updatedAtMs)}</span>
              </div>
            </div>
            <button type="button" className="tcp-btn h-7 px-2 text-xs" onClick={() => onOpenRun(row.id)}>
              <i data-lucide="external-link"></i>
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}

function GrantsPanel({ grants }: { grants: any[] }) {
  if (!grants.length) return <EmptyState title="No scoped grants" text="Matching grants will appear here." />;
  return (
    <div className="space-y-2">
      {grants.slice(0, 8).map((grant) => (
        <div key={grant.grant_id || grant.grantId} className="rounded-md border border-white/10 bg-black/20 px-3 py-2">
          <div className="flex min-w-0 items-center justify-between gap-3">
            <div className="min-w-0">
              <div className="truncate text-sm font-medium text-tcp-text-primary">
                {grant.grant_id || grant.grantId}
              </div>
              <div className="mt-1 truncate text-[11px] text-tcp-text-muted">
                {(grant.permissions || []).join(", ") || (grant.data_classes || []).join(", ") || "scope grant"}
              </div>
            </div>
            <Badge tone={statusTone(grant.effect || grant.state)}>{grant.effect || grant.state || "grant"}</Badge>
          </div>
        </div>
      ))}
    </div>
  );
}

export function EnterpriseScopeExplorer({
  api,
  navigate,
  orgUnits,
  memberships,
  accessGrants,
  effectiveGrants,
  connectors,
  sourceBindings,
  sourceObjects,
  loading,
  error,
}: EnterpriseScopeExplorerProps) {
  const [selectedScopeId, setSelectedScopeId] = useState("");
  const runsQuery = useQuery({
    queryKey: ["enterprise", "stateful-scope-runs"],
    queryFn: () => api("/api/engine/stateful-runtime/runs?limit=160"),
    refetchInterval: 10000,
  });
  const model = useMemo(
    () =>
      buildEnterpriseScopeExplorerModel({
        orgUnits,
        memberships,
        accessGrants,
        effectiveGrants,
        sourceBindings,
        sourceObjects,
        runs: runsQuery.data?.runs || [],
      }),
    [
      accessGrants,
      effectiveGrants,
      memberships,
      orgUnits,
      runsQuery.data,
      sourceBindings,
      sourceObjects,
    ]
  );
  const activeScopeId = selectedScopeId || model.scopes[0]?.id || "";
  const detail = useMemo(() => selectEnterpriseScope(model, activeScopeId), [activeScopeId, model]);

  useEffect(() => {
    if (!selectedScopeId && model.scopes[0]?.id) setSelectedScopeId(model.scopes[0].id);
  }, [model.scopes, selectedScopeId]);

  const openRun = (runId: string) => {
    replaceRunSelectionHash(runId);
    navigate("runs");
  };

  return (
    <PanelCard
      title="Scope Explorer"
      subtitle="Org ownership, policy inheritance, knowledge boundaries, and stateful run evidence."
      actions={
        <Toolbar>
          <button
            className="tcp-btn h-8 px-3 text-xs"
            type="button"
            onClick={() => runsQuery.refetch()}
            disabled={runsQuery.isFetching}
          >
            <i data-lucide="refresh-cw"></i>
            Runs
          </button>
        </Toolbar>
      }
    >
      <div className="grid gap-4">
        <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4 xl:grid-cols-6">
          <ScopeMetric label="Scopes" value={model.summary.scopes} />
          <ScopeMetric label="Org Units" value={model.summary.orgUnits} />
          <ScopeMetric label="Grants" value={model.summary.grants} />
          <ScopeMetric label="Sources" value={model.summary.sourceBindings} />
          <ScopeMetric label="Runs" value={model.summary.recentRuns} />
          <ScopeMetric label="Connectors" value={connectors.length} />
        </div>

        {error || runsQuery.error ? (
          <div className="rounded-md border border-yellow-400/20 bg-yellow-400/10 px-3 py-2 text-xs text-yellow-100">
            {errorText(error || runsQuery.error)}
          </div>
        ) : null}

        {loading && !model.scopes.length ? (
          <LoadingState title="Loading enterprise scopes" />
        ) : (
          <div className="grid min-h-0 gap-4 xl:grid-cols-[minmax(18rem,0.8fr)_minmax(0,1.4fr)]">
            <div className="grid min-h-0 gap-4">
              <PanelCard title="Scopes" subtitle={detail.scope?.label || "No selection"} fullHeight>
                <ScopeList scopes={model.scopes} selectedScopeId={activeScopeId} onSelect={setSelectedScopeId} />
              </PanelCard>
              <PanelCard title="Org Tree" subtitle={`${model.orgTree.flat.length} units`}>
                <OrgTree nodes={model.orgTree.flat} />
              </PanelCard>
            </div>

            <div className="grid min-h-0 gap-4">
              <div className="grid gap-4 xl:grid-cols-2">
                <PanelCard title="Policy Inheritance" subtitle={detail.scope?.resourceLabel || "Selected scope"}>
                  <PolicyLayers layers={detail.policyLayers} />
                </PanelCard>
                <PanelCard title="Scoped Grants" subtitle={`${detail.grants.length} grants`}>
                  <GrantsPanel grants={detail.grants} />
                </PanelCard>
              </div>
              <PanelCard
                title="Knowledge Boundaries"
                subtitle={`${detail.visibleKnowledge.length} visible / ${detail.blockedKnowledge.length} blocked`}
              >
                <KnowledgeBoundary rows={detail.knowledge} />
              </PanelCard>
              <PanelCard title="Automation Ownership" subtitle={`${detail.runs.length} recent runs`}>
                <RecentRuns rows={detail.runs} onOpenRun={openRun} />
              </PanelCard>
            </div>
          </div>
        )}
      </div>
    </PanelCard>
  );
}
