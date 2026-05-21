import { useMemo } from "react";
import {
  Badge,
  EmptyState,
  LoadingState,
  PageHeader,
  PanelCard,
  StaggerGroup,
  Toolbar,
  AnimatedPage,
} from "../ui/index.tsx";
import {
  useEnterpriseOrgUnits,
  useEnterpriseSourceBindings,
  type EnterpriseNoopBase,
} from "../features/enterprise/queries";
import type { AppPageProps } from "./pageTypes";

function compactTenant(payload?: EnterpriseNoopBase | null) {
  const tenant = payload?.tenant_context;
  if (!tenant) return "tenant unavailable";
  const org = tenant.org_id || "local";
  const workspace = tenant.workspace_id || "local";
  const deployment = tenant.deployment_id ? ` · ${tenant.deployment_id}` : "";
  return `${org} / ${workspace}${deployment}`;
}

function actorLabel(payload?: EnterpriseNoopBase | null) {
  const principal = payload?.request_principal;
  return principal?.actor_id || principal?.source || "local operator";
}

function noopStatus(payload?: EnterpriseNoopBase | null) {
  if (!payload) return null;
  return payload.status === "noop" || payload.bridge_state === "absent";
}

function GovernanceStatusStrip({
  orgUnitsPayload,
  sourceBindingsPayload,
}: {
  orgUnitsPayload?: EnterpriseNoopBase | null;
  sourceBindingsPayload?: EnterpriseNoopBase | null;
}) {
  const payload = orgUnitsPayload || sourceBindingsPayload;
  const isNoop = noopStatus(payload);
  return (
    <PanelCard>
      <div className="grid gap-3 md:grid-cols-3">
        <div className="rounded-lg border border-white/8 bg-black/20 p-3">
          <div className="tcp-subtle text-xs uppercase tracking-[0.14em]">Tenant</div>
          <div className="mt-1 text-sm font-medium text-tcp-text-primary">
            {compactTenant(payload)}
          </div>
        </div>
        <div className="rounded-lg border border-white/8 bg-black/20 p-3">
          <div className="tcp-subtle text-xs uppercase tracking-[0.14em]">Principal</div>
          <div className="mt-1 text-sm font-medium text-tcp-text-primary">
            {actorLabel(payload)}
          </div>
        </div>
        <div className="rounded-lg border border-white/8 bg-black/20 p-3">
          <div className="tcp-subtle text-xs uppercase tracking-[0.14em]">Bridge</div>
          <div className="mt-1 flex flex-wrap items-center gap-2">
            <Badge tone={isNoop ? "warn" : "ok"}>{payload?.bridge_state || "checking"}</Badge>
            <span className="tcp-subtle text-xs">{payload?.status || "loading"}</span>
          </div>
        </div>
      </div>
      {payload?.message ? (
        <div className="mt-3 rounded-lg border border-amber-500/20 bg-amber-500/10 px-3 py-2 text-sm text-amber-100">
          {payload.message}
        </div>
      ) : null}
    </PanelCard>
  );
}

function ObjectListPanel({
  title,
  subtitle,
  count,
  rows,
  loading,
  error,
  emptyTitle,
  emptyText,
}: {
  title: string;
  subtitle: string;
  count: number;
  rows: any[];
  loading: boolean;
  error: unknown;
  emptyTitle: string;
  emptyText: string;
}) {
  return (
    <PanelCard
      title={title}
      subtitle={subtitle}
      actions={<Badge tone={error ? "err" : count ? "ok" : "ghost"}>{count}</Badge>}
      fullHeight
    >
      {loading ? (
        <LoadingState title="Loading" text="Reading enterprise admin state" />
      ) : error ? (
        <EmptyState
          title="Unavailable"
          text={error instanceof Error ? error.message : "Enterprise admin state could not load."}
        />
      ) : rows.length ? (
        <div className="grid gap-2">
          {rows.map((row, index) => (
            <pre
              key={String(row?.id || row?.unit_id || row?.binding_id || index)}
              className="max-h-44 overflow-auto rounded-lg border border-white/8 bg-black/25 p-3 text-xs text-tcp-text-secondary"
            >
              {JSON.stringify(row, null, 2)}
            </pre>
          ))}
        </div>
      ) : (
        <EmptyState title={emptyTitle} text={emptyText} />
      )}
    </PanelCard>
  );
}

function GovernancePlanPanel() {
  const rows = [
    ["Org units", "Admin-defined domains such as department/hr or clinical_role/doctors."],
    ["Source bindings", "External source roots mapped to ResourceRef and DataClass."],
    ["Credentials", "Secret references only; read-only by default."],
    ["Ingestion", "Paused, revoked, disabled, or quarantined sources cannot index."],
  ];
  return (
    <PanelCard title="Governance lanes" subtitle="Phase A shell">
      <div className="grid gap-2">
        {rows.map(([label, detail]) => (
          <div
            key={label}
            className="grid gap-1 rounded-lg border border-white/8 bg-black/20 p-3 sm:grid-cols-[10rem_minmax(0,1fr)] sm:items-center"
          >
            <div className="text-sm font-medium text-tcp-text-primary">{label}</div>
            <div className="tcp-subtle text-sm">{detail}</div>
          </div>
        ))}
      </div>
    </PanelCard>
  );
}

export function EnterpriseAdminPage({ navigate }: AppPageProps) {
  const orgUnits = useEnterpriseOrgUnits();
  const sourceBindings = useEnterpriseSourceBindings();
  const orgRows = useMemo(() => orgUnits.data?.org_units || [], [orgUnits.data]);
  const bindingRows = useMemo(
    () => sourceBindings.data?.source_bindings || [],
    [sourceBindings.data]
  );
  const headerBadges = (
    <>
      <Badge tone={noopStatus(orgUnits.data || sourceBindings.data) ? "warn" : "ok"}>
        {orgUnits.data?.status || sourceBindings.data?.status || "checking"}
      </Badge>
      <Badge tone="info">{compactTenant(orgUnits.data || sourceBindings.data)}</Badge>
    </>
  );
  const refreshEnterpriseState = () => {
    orgUnits.refetch();
    sourceBindings.refetch();
  };

  return (
    <AnimatedPage className="grid gap-4">
      <PageHeader
        eyebrow="Enterprise"
        title="Admin governance"
        subtitle="Org-unit taxonomy and source-binding controls for hosted enterprise data access."
        badges={headerBadges}
        actions={
          <Toolbar>
            <button className="tcp-btn" type="button" onClick={refreshEnterpriseState}>
              <i data-lucide="refresh-cw"></i>
              Refresh
            </button>
            <button className="tcp-btn" type="button" onClick={() => navigate("settings")}>
              <i data-lucide="settings"></i>
              Settings
            </button>
          </Toolbar>
        }
      />

      <StaggerGroup className="grid gap-4">
        <GovernanceStatusStrip
          orgUnitsPayload={orgUnits.data}
          sourceBindingsPayload={sourceBindings.data}
        />

        <div className="grid gap-4 xl:grid-cols-2">
          <ObjectListPanel
            title="Org units"
            subtitle="Company-defined domains"
            count={Number(orgUnits.data?.count ?? orgRows.length)}
            rows={orgRows}
            loading={orgUnits.isLoading}
            error={orgUnits.error}
            emptyTitle="No org units"
            emptyText="Enterprise admin storage is not configured."
          />
          <ObjectListPanel
            title="Source bindings"
            subtitle="External sources mapped to resource scopes"
            count={Number(sourceBindings.data?.count ?? bindingRows.length)}
            rows={bindingRows}
            loading={sourceBindings.isLoading}
            error={sourceBindings.error}
            emptyTitle="No source bindings"
            emptyText="Enterprise admin storage is not configured."
          />
        </div>

        <GovernancePlanPanel />
      </StaggerGroup>
    </AnimatedPage>
  );
}
