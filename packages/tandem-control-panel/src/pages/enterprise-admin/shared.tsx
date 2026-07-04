import type { ReactNode } from "react";
import { Badge, PanelCard } from "../../ui/index.tsx";
import type { EnterpriseNoopBase } from "../../features/enterprise/queries";

export const ORG_UNIT_KINDS = [
  "department",
  "team",
  "role_domain",
  "contractor_group",
  "executive_group",
  "clinical_group",
  "operational_group",
  "custom",
];

export const RESOURCE_KINDS = [
  "document_collection",
  "data_store",
  "shared_drive",
  "repository",
  "directory",
  "project",
  "knowledge_space",
  "memory_space",
];

export const DATA_CLASSES = [
  "internal",
  "confidential",
  "restricted",
  "executive",
  "regulated",
  "customer_data",
  "source_code",
  "financial_record",
  "public",
];

export const ACCESS_PERMISSIONS = ["view", "read", "edit", "execute", "delegate", "admin"];
export const CONNECTOR_STATES = ["active", "paused", "revoked", "quarantined"];
export const CREDENTIAL_CLASSES = ["read_only", "read_write", "admin"];
export const MEMBER_KINDS = ["human_user", "group", "department", "agent_worker", "service_account"];
export const MEMBERSHIP_SOURCES = [
  "direct",
  "hosted_control_plane",
  "scim",
  "google_workspace",
  "okta",
  "manual_import",
];

export function compactTenant(payload?: EnterpriseNoopBase | null) {
  const tenant = payload?.tenant_context;
  if (!tenant) return "tenant unavailable";
  const org = tenant.org_id || "local";
  const workspace = tenant.workspace_id || "local";
  const deployment = tenant.deployment_id ? ` · ${tenant.deployment_id}` : "";
  return `${org} / ${workspace}${deployment}`;
}

export function actorLabel(payload?: EnterpriseNoopBase | null) {
  const principal = payload?.request_principal;
  return principal?.actor_id || principal?.source || "local operator";
}

export function noopStatus(payload?: EnterpriseNoopBase | null) {
  if (!payload) return null;
  return payload.status === "noop" || payload.bridge_state === "absent";
}

export function tenantOrg(payload?: EnterpriseNoopBase | null) {
  return payload?.tenant_context?.org_id || "local";
}

export function tenantWorkspace(payload?: EnterpriseNoopBase | null) {
  return payload?.tenant_context?.workspace_id || "local";
}

export function errorText(error: unknown, fallback: string) {
  return error instanceof Error ? error.message : fallback;
}

export function GovernanceStatusStrip({
  orgUnitsPayload,
  connectorsPayload,
  sourceBindingsPayload,
}: {
  orgUnitsPayload?: EnterpriseNoopBase | null;
  connectorsPayload?: EnterpriseNoopBase | null;
  sourceBindingsPayload?: EnterpriseNoopBase | null;
}) {
  const payload = orgUnitsPayload || connectorsPayload || sourceBindingsPayload;
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
        <div className="mt-3 rounded-lg border border-emerald-500/20 bg-emerald-500/10 px-3 py-2 text-sm text-emerald-100">
          {payload.message}
        </div>
      ) : null}
    </PanelCard>
  );
}

export function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="grid gap-1 text-sm">
      <span className="tcp-subtle text-xs uppercase tracking-[0.12em]">{label}</span>
      {children}
    </label>
  );
}

export function connectorStateTone(state?: string): "ok" | "warn" | "err" | "info" | "ghost" {
  if (state === "active") return "ok";
  if (state === "revoked" || state === "quarantined") return "err";
  if (state === "paused") return "warn";
  return "ghost";
}

export function formatLifecycleTime(value?: number | null) {
  if (!value) return "never";
  return new Date(value).toLocaleString();
}
