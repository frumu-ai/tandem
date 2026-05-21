import { useMemo, useState, type ReactNode } from "react";
import {
  AnimatedPage,
  Badge,
  EmptyState,
  LoadingState,
  PageHeader,
  PanelCard,
  StaggerGroup,
  Toolbar,
} from "../ui/index.tsx";
import {
  useCreateEnterpriseConnector,
  useCreateEnterpriseConnectorCredentialRef,
  useCreateEnterpriseOrgUnit,
  useCreateEnterpriseSourceBinding,
  useDeleteEnterpriseSourceObject,
  useEnterpriseConnectors,
  useEnterpriseIngestionJobs,
  useEnterpriseOrgUnits,
  useEnterpriseSourceBindings,
  useEnterpriseSourceObjects,
  useReindexEnterpriseSourceObject,
  useRescopeEnterpriseSourceObject,
  useRotateEnterpriseConnectorCredentialRef,
  useUpdateEnterpriseConnector,
  useUpdateEnterpriseSourceBinding,
  type CreateEnterpriseConnectorCredentialRefInput,
  type CreateEnterpriseConnectorInput,
  type CreateEnterpriseOrganizationUnitInput,
  type CreateEnterpriseSourceBindingInput,
  type RotateEnterpriseConnectorCredentialRefInput,
  type EnterpriseConnectorInstance,
  type EnterpriseIngestionJob,
  type EnterpriseNoopBase,
  type EnterpriseOrganizationUnit,
  type EnterpriseSourceBinding,
  type EnterpriseSourceObjectLifecycle,
} from "../features/enterprise/queries";
import type { AppPageProps } from "./pageTypes";

const ORG_UNIT_KINDS = [
  "department",
  "team",
  "role_domain",
  "contractor_group",
  "executive_group",
  "clinical_group",
  "operational_group",
  "custom",
];

const RESOURCE_KINDS = [
  "document_collection",
  "data_store",
  "shared_drive",
  "repository",
  "directory",
  "project",
  "knowledge_space",
  "memory_space",
];

const DATA_CLASSES = [
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

const CONNECTOR_STATES = ["active", "paused", "revoked", "quarantined"];
const CREDENTIAL_CLASSES = ["read_only", "read_write", "admin"];

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

function tenantOrg(payload?: EnterpriseNoopBase | null) {
  return payload?.tenant_context?.org_id || "local";
}

function tenantWorkspace(payload?: EnterpriseNoopBase | null) {
  return payload?.tenant_context?.workspace_id || "local";
}

function errorText(error: unknown, fallback: string) {
  return error instanceof Error ? error.message : fallback;
}

function GovernanceStatusStrip({
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

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="grid gap-1 text-sm">
      <span className="tcp-subtle text-xs uppercase tracking-[0.12em]">{label}</span>
      {children}
    </label>
  );
}

function ConnectorForm({
  onCreate,
  busy,
}: {
  onCreate: (input: CreateEnterpriseConnectorInput) => Promise<void>;
  busy: boolean;
}) {
  const [connectorId, setConnectorId] = useState("");
  const [provider, setProvider] = useState("manual_upload");
  const [displayName, setDisplayName] = useState("");
  const [state, setState] = useState("active");

  return (
    <PanelCard title="Create connector" subtitle="Lifecycle gate">
      <form
        className="grid gap-3"
        onSubmit={async (event) => {
          event.preventDefault();
          await onCreate({
            connector_id: connectorId.trim(),
            provider: provider.trim(),
            display_name: displayName.trim() || undefined,
            state,
          });
          setConnectorId("");
          setDisplayName("");
        }}
      >
        <div className="grid gap-3 md:grid-cols-2">
          <Field label="Connector ID">
            <input
              className="tcp-input"
              value={connectorId}
              onInput={(event) => setConnectorId(event.currentTarget.value)}
              placeholder="google-drive-hr"
              required
            />
          </Field>
          <Field label="Provider">
            <input
              className="tcp-input"
              value={provider}
              onInput={(event) => setProvider(event.currentTarget.value)}
              placeholder="google_drive"
              required
            />
          </Field>
          <Field label="Display Name">
            <input
              className="tcp-input"
              value={displayName}
              onInput={(event) => setDisplayName(event.currentTarget.value)}
              placeholder="HR Google Drive"
            />
          </Field>
          <Field label="State">
            <select
              className="tcp-select"
              value={state}
              onChange={(event) => setState(event.currentTarget.value)}
            >
              {CONNECTOR_STATES.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </select>
          </Field>
        </div>
        <div className="flex justify-end">
          <button className="tcp-btn tcp-btn-primary" type="submit" disabled={busy}>
            <i data-lucide="plug"></i>
            {busy ? "Creating" : "Create connector"}
          </button>
        </div>
      </form>
    </PanelCard>
  );
}

function ConnectorCredentialRefForm({
  tenantPayload,
  connectors,
  onCreate,
  onRotate,
  busy,
}: {
  tenantPayload?: EnterpriseNoopBase | null;
  connectors: EnterpriseConnectorInstance[];
  onCreate: (input: CreateEnterpriseConnectorCredentialRefInput) => Promise<void>;
  onRotate: (input: RotateEnterpriseConnectorCredentialRefInput) => Promise<void>;
  busy: boolean;
}) {
  const orgId = tenantOrg(tenantPayload);
  const workspaceId = tenantWorkspace(tenantPayload);
  const [mode, setMode] = useState<"attach" | "rotate">("attach");
  const [connectorId, setConnectorId] = useState("");
  const [credentialId, setCredentialId] = useState("");
  const [credentialClass, setCredentialClass] = useState("read_only");
  const [secretProvider, setSecretProvider] = useState("google_kms");
  const [secretId, setSecretId] = useState("");
  const [secretName, setSecretName] = useState("");
  const [resourceKind, setResourceKind] = useState("document_collection");
  const [resourceId, setResourceId] = useState("");

  const selectedConnectorId = connectorId || connectors[0]?.connector_id || "";
  const resetAfterSubmit = () => {
    setCredentialId("");
    setSecretId("");
    setSecretName("");
    setResourceId("");
  };

  return (
    <PanelCard title="Credential reference" subtitle="Secret-ref only">
      <form
        className="grid gap-3"
        onSubmit={async (event) => {
          event.preventDefault();
          const secret_ref = {
            org_id: orgId,
            workspace_id: workspaceId,
            provider: secretProvider.trim(),
            secret_id: secretId.trim(),
            name: secretName.trim(),
          };
          if (mode === "rotate") {
            await onRotate({
              connector_id: selectedConnectorId,
              credential_id: credentialId.trim(),
              secret_ref,
            });
          } else {
            await onCreate({
              connector_id: selectedConnectorId,
              credential_id: credentialId.trim(),
              credential_class: credentialClass,
              secret_ref,
              source_bound_resource: resourceId.trim()
                ? {
                    organization_id: orgId,
                    workspace_id: workspaceId,
                    resource_kind: resourceKind,
                    resource_id: resourceId.trim(),
                  }
                : undefined,
            });
          }
          resetAfterSubmit();
        }}
      >
        <div className="grid gap-3 md:grid-cols-2">
          <Field label="Mode">
            <select
              className="tcp-select"
              value={mode}
              onChange={(event) => setMode(event.currentTarget.value as "attach" | "rotate")}
            >
              <option value="attach">attach</option>
              <option value="rotate">rotate</option>
            </select>
          </Field>
          <Field label="Connector">
            <select
              className="tcp-select"
              value={selectedConnectorId}
              onChange={(event) => setConnectorId(event.currentTarget.value)}
              required
            >
              {connectors.length ? (
                connectors.map((connector) => (
                  <option key={connector.connector_id} value={connector.connector_id}>
                    {connector.display_name || connector.connector_id}
                  </option>
                ))
              ) : (
                <option value="">create connector first</option>
              )}
            </select>
          </Field>
          <Field label="Credential ID">
            <input
              className="tcp-input"
              value={credentialId}
              onInput={(event) => setCredentialId(event.currentTarget.value)}
              placeholder="readonly"
              required
            />
          </Field>
          <Field label="Credential Class">
            <select
              className="tcp-select"
              value={credentialClass}
              onChange={(event) => setCredentialClass(event.currentTarget.value)}
              disabled={mode === "rotate"}
            >
              {CREDENTIAL_CLASSES.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Secret Provider">
            <input
              className="tcp-input"
              value={secretProvider}
              onInput={(event) => setSecretProvider(event.currentTarget.value)}
              placeholder="google_kms"
              required
            />
          </Field>
          <Field label="Secret ID">
            <input
              className="tcp-input"
              value={secretId}
              onInput={(event) => setSecretId(event.currentTarget.value)}
              placeholder="kms://finance/readonly-v2"
              required
            />
          </Field>
          <Field label="Secret Name">
            <input
              className="tcp-input"
              value={secretName}
              onInput={(event) => setSecretName(event.currentTarget.value)}
              placeholder="Finance Drive read-only secret"
              required
            />
          </Field>
          <Field label="Bound Resource">
            <input
              className="tcp-input"
              value={resourceId}
              onInput={(event) => setResourceId(event.currentTarget.value)}
              placeholder="optional resource id"
              disabled={mode === "rotate"}
            />
          </Field>
          <Field label="Resource Kind">
            <select
              className="tcp-select"
              value={resourceKind}
              onChange={(event) => setResourceKind(event.currentTarget.value)}
              disabled={mode === "rotate" || !resourceId.trim()}
            >
              {RESOURCE_KINDS.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </select>
          </Field>
        </div>
        <div className="flex justify-end">
          <button
            className="tcp-btn tcp-btn-primary"
            type="submit"
            disabled={busy || !selectedConnectorId}
          >
            <i data-lucide={mode === "rotate" ? "rotate-cw" : "key-round"}></i>
            {busy ? "Saving" : mode === "rotate" ? "Rotate ref" : "Attach ref"}
          </button>
        </div>
      </form>
    </PanelCard>
  );
}

function OrgUnitForm({
  onCreate,
  busy,
}: {
  onCreate: (input: CreateEnterpriseOrganizationUnitInput) => Promise<void>;
  busy: boolean;
}) {
  const [unitId, setUnitId] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [taxonomyId, setTaxonomyId] = useState("department");
  const [kind, setKind] = useState("department");
  const [parentUnitId, setParentUnitId] = useState("");
  const [labels, setLabels] = useState("");

  return (
    <PanelCard title="Create org unit" subtitle="Company taxonomy">
      <form
        className="grid gap-3"
        onSubmit={async (event) => {
          event.preventDefault();
          await onCreate({
            unit_id: unitId.trim(),
            display_name: displayName.trim(),
            taxonomy_id: taxonomyId.trim() || undefined,
            kind,
            parent_unit_id: parentUnitId.trim() || undefined,
            labels: labels
              .split(",")
              .map((label) => label.trim())
              .filter(Boolean),
          });
          setUnitId("");
          setDisplayName("");
          setParentUnitId("");
          setLabels("");
        }}
      >
        <div className="grid gap-3 md:grid-cols-2">
          <Field label="Unit ID">
            <input
              className="tcp-input"
              value={unitId}
              onInput={(event) => setUnitId(event.currentTarget.value)}
              placeholder="hr"
              required
            />
          </Field>
          <Field label="Display Name">
            <input
              className="tcp-input"
              value={displayName}
              onInput={(event) => setDisplayName(event.currentTarget.value)}
              placeholder="Human Resources"
              required
            />
          </Field>
          <Field label="Taxonomy">
            <input
              className="tcp-input"
              value={taxonomyId}
              onInput={(event) => setTaxonomyId(event.currentTarget.value)}
              placeholder="department"
            />
          </Field>
          <Field label="Kind">
            <select
              className="tcp-select"
              value={kind}
              onChange={(event) => setKind(event.currentTarget.value)}
            >
              {ORG_UNIT_KINDS.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Parent Unit">
            <input
              className="tcp-input"
              value={parentUnitId}
              onInput={(event) => setParentUnitId(event.currentTarget.value)}
              placeholder="optional"
            />
          </Field>
          <Field label="Labels">
            <input
              className="tcp-input"
              value={labels}
              onInput={(event) => setLabels(event.currentTarget.value)}
              placeholder="people, benefits"
            />
          </Field>
        </div>
        <div className="flex justify-end">
          <button className="tcp-btn tcp-btn-primary" type="submit" disabled={busy}>
            <i data-lucide="plus"></i>
            {busy ? "Creating" : "Create unit"}
          </button>
        </div>
      </form>
    </PanelCard>
  );
}

function SourceBindingForm({
  tenantPayload,
  onCreate,
  busy,
}: {
  tenantPayload?: EnterpriseNoopBase | null;
  onCreate: (input: CreateEnterpriseSourceBindingInput) => Promise<void>;
  busy: boolean;
}) {
  const orgId = tenantOrg(tenantPayload);
  const workspaceId = tenantWorkspace(tenantPayload);
  const [bindingId, setBindingId] = useState("");
  const [connectorId, setConnectorId] = useState("manual_upload");
  const [sourceType, setSourceType] = useState("manual_upload");
  const [nativeSourceId, setNativeSourceId] = useState("");
  const [sourceRootLabel, setSourceRootLabel] = useState("");
  const [resourceKind, setResourceKind] = useState("document_collection");
  const [resourceId, setResourceId] = useState("");
  const [dataClass, setDataClass] = useState("internal");
  const [allowIndexing, setAllowIndexing] = useState(true);
  const [allowPromptContext, setAllowPromptContext] = useState(true);
  const [requireReview, setRequireReview] = useState(false);

  return (
    <PanelCard title="Create source binding" subtitle="External source to resource scope">
      <form
        className="grid gap-3"
        onSubmit={async (event) => {
          event.preventDefault();
          await onCreate({
            binding_id: bindingId.trim(),
            connector_id: connectorId.trim(),
            source_type: sourceType.trim(),
            native_source_id: nativeSourceId.trim(),
            source_root_label: sourceRootLabel.trim() || undefined,
            resource_ref: {
              organization_id: orgId,
              workspace_id: workspaceId,
              resource_kind: resourceKind,
              resource_id: resourceId.trim(),
            },
            data_class: dataClass,
            ingestion_policy: {
              allow_indexing: allowIndexing,
              allow_prompt_context: allowPromptContext,
              require_review: requireReview,
            },
          });
          setBindingId("");
          setNativeSourceId("");
          setSourceRootLabel("");
          setResourceId("");
        }}
      >
        <div className="grid gap-3 md:grid-cols-2">
          <Field label="Binding ID">
            <input
              className="tcp-input"
              value={bindingId}
              onInput={(event) => setBindingId(event.currentTarget.value)}
              placeholder="finance-drive"
              required
            />
          </Field>
          <Field label="Connector ID">
            <input
              className="tcp-input"
              value={connectorId}
              onInput={(event) => setConnectorId(event.currentTarget.value)}
              placeholder="google_drive"
              required
            />
          </Field>
          <Field label="Source Type">
            <input
              className="tcp-input"
              value={sourceType}
              onInput={(event) => setSourceType(event.currentTarget.value)}
              placeholder="google_drive"
              required
            />
          </Field>
          <Field label="Native Source ID">
            <input
              className="tcp-input"
              value={nativeSourceId}
              onInput={(event) => setNativeSourceId(event.currentTarget.value)}
              placeholder="drive-root-id"
              required
            />
          </Field>
          <Field label="Source Label">
            <input
              className="tcp-input"
              value={sourceRootLabel}
              onInput={(event) => setSourceRootLabel(event.currentTarget.value)}
              placeholder="Finance Drive"
            />
          </Field>
          <Field label="Data Class">
            <select
              className="tcp-select"
              value={dataClass}
              onChange={(event) => setDataClass(event.currentTarget.value)}
            >
              {DATA_CLASSES.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Resource Kind">
            <select
              className="tcp-select"
              value={resourceKind}
              onChange={(event) => setResourceKind(event.currentTarget.value)}
            >
              {RESOURCE_KINDS.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Resource ID">
            <input
              className="tcp-input"
              value={resourceId}
              onInput={(event) => setResourceId(event.currentTarget.value)}
              placeholder="finance-drive"
              required
            />
          </Field>
        </div>
        <div className="grid gap-2 rounded-lg border border-white/8 bg-black/20 p-3 md:grid-cols-3">
          <label className="flex items-center gap-2 text-sm text-tcp-text-secondary">
            <input
              type="checkbox"
              checked={allowIndexing}
              onChange={(event) => setAllowIndexing(event.currentTarget.checked)}
            />
            Indexing
          </label>
          <label className="flex items-center gap-2 text-sm text-tcp-text-secondary">
            <input
              type="checkbox"
              checked={allowPromptContext}
              onChange={(event) => setAllowPromptContext(event.currentTarget.checked)}
            />
            Prompt context
          </label>
          <label className="flex items-center gap-2 text-sm text-tcp-text-secondary">
            <input
              type="checkbox"
              checked={requireReview}
              onChange={(event) => setRequireReview(event.currentTarget.checked)}
            />
            Require review
          </label>
        </div>
        <div className="flex justify-end">
          <button className="tcp-btn tcp-btn-primary" type="submit" disabled={busy}>
            <i data-lucide="plus"></i>
            {busy ? "Creating" : "Create binding"}
          </button>
        </div>
      </form>
    </PanelCard>
  );
}

function OrgUnitsPanel({
  rows,
  loading,
  error,
}: {
  rows: EnterpriseOrganizationUnit[];
  loading: boolean;
  error: unknown;
}) {
  return (
    <PanelCard
      title="Org units"
      subtitle="Company-defined domains"
      actions={<Badge tone={error ? "err" : rows.length ? "ok" : "ghost"}>{rows.length}</Badge>}
      fullHeight
    >
      {loading ? (
        <LoadingState title="Loading" text="Reading organization units" />
      ) : error ? (
        <EmptyState title="Unavailable" text={errorText(error, "Org units could not load.")} />
      ) : rows.length ? (
        <div className="grid gap-2">
          {rows.map((unit) => (
            <div
              key={`${unit.taxonomy_id || "organization_unit"}:${unit.unit_id}`}
              className="rounded-lg border border-white/8 bg-black/20 p-3"
            >
              <div className="flex flex-wrap items-start justify-between gap-2">
                <div>
                  <div className="font-medium text-tcp-text-primary">{unit.display_name}</div>
                  <div className="tcp-subtle text-xs">
                    {unit.taxonomy_id || "organization_unit"} / {unit.unit_id}
                  </div>
                </div>
                <div className="flex flex-wrap gap-2">
                  <Badge tone={unit.state === "disabled" ? "warn" : "ok"}>
                    {unit.state || "active"}
                  </Badge>
                  <Badge tone="info">{unit.kind || "unspecified"}</Badge>
                </div>
              </div>
              {unit.labels?.length ? (
                <div className="mt-2 flex flex-wrap gap-1">
                  {unit.labels.map((label) => (
                    <Badge key={label} tone="ghost">
                      {label}
                    </Badge>
                  ))}
                </div>
              ) : null}
            </div>
          ))}
        </div>
      ) : (
        <EmptyState
          title="No org units"
          text="Create a company domain to start assigning access."
        />
      )}
    </PanelCard>
  );
}

function connectorStateTone(state?: string): "ok" | "warn" | "err" | "info" | "ghost" {
  if (state === "active") return "ok";
  if (state === "revoked" || state === "quarantined") return "err";
  if (state === "paused") return "warn";
  return "ghost";
}

function ConnectorsPanel({
  rows,
  loading,
  error,
  onSetState,
  busyConnectorId,
}: {
  rows: EnterpriseConnectorInstance[];
  loading: boolean;
  error: unknown;
  onSetState: (connectorId: string, state: string) => void;
  busyConnectorId?: string | null;
}) {
  return (
    <PanelCard
      title="Connectors"
      subtitle="Tenant-scoped ingestion lifecycle"
      actions={<Badge tone={error ? "err" : rows.length ? "ok" : "ghost"}>{rows.length}</Badge>}
      fullHeight
    >
      {loading ? (
        <LoadingState title="Loading" text="Reading connectors" />
      ) : error ? (
        <EmptyState title="Unavailable" text={errorText(error, "Connectors could not load.")} />
      ) : rows.length ? (
        <div className="grid gap-2">
          {rows.map((connector) => {
            const state = connector.state || "active";
            const isBusy = busyConnectorId === connector.connector_id;
            return (
              <div
                key={connector.connector_id}
                className="rounded-lg border border-white/8 bg-black/20 p-3"
              >
                <div className="flex flex-wrap items-start justify-between gap-2">
                  <div>
                    <div className="font-medium text-tcp-text-primary">
                      {connector.display_name || connector.connector_id}
                    </div>
                    <div className="tcp-subtle text-xs">
                      {connector.provider} / {connector.connector_id}
                    </div>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <Badge tone={connectorStateTone(state)}>{state}</Badge>
                    <Badge tone="info">
                      {connector.credential_refs?.length
                        ? `${connector.credential_refs.length} secret refs`
                        : "no secret refs"}
                    </Badge>
                  </div>
                </div>
                <div className="mt-3 grid gap-2 text-xs text-tcp-text-secondary md:grid-cols-2">
                  <div>Created: {formatLifecycleTime(connector.created_at_ms)}</div>
                  <div>Updated: {formatLifecycleTime(connector.updated_at_ms)}</div>
                </div>
                {connector.credential_refs?.length ? (
                  <div className="mt-3 grid gap-2">
                    {connector.credential_refs.map((credential) => (
                      <div
                        key={credential.credential_id}
                        className="rounded-md border border-white/8 bg-black/20 px-3 py-2 text-xs text-tcp-text-secondary"
                      >
                        <div className="flex flex-wrap items-center justify-between gap-2">
                          <span className="font-medium text-tcp-text-primary">
                            {credential.credential_id}
                          </span>
                          <Badge tone={credential.credential_class === "read_only" ? "ok" : "warn"}>
                            {credential.credential_class || "read_only"}
                          </Badge>
                        </div>
                        <div className="mt-1 break-all">
                          {credential.secret_ref.provider} / {credential.secret_ref.secret_id}
                        </div>
                        <div className="mt-1">
                          Rotated: {formatLifecycleTime(credential.rotated_at_ms)}
                        </div>
                      </div>
                    ))}
                  </div>
                ) : null}
                <div className="mt-3 flex flex-wrap gap-2">
                  {CONNECTOR_STATES.map((nextState) => (
                    <button
                      key={nextState}
                      className="tcp-btn"
                      type="button"
                      disabled={isBusy || state === nextState}
                      onClick={() => onSetState(connector.connector_id, nextState)}
                    >
                      {nextState}
                    </button>
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      ) : (
        <EmptyState
          title="No connectors"
          text="Create a connector lifecycle record before binding source data."
        />
      )}
    </PanelCard>
  );
}

function SourceBindingsPanel({
  rows,
  loading,
  error,
  onSetState,
  selectedBindingId,
  onSelectBinding,
  busyBindingId,
}: {
  rows: EnterpriseSourceBinding[];
  loading: boolean;
  error: unknown;
  onSetState: (bindingId: string, state: string) => void;
  selectedBindingId?: string | null;
  onSelectBinding: (bindingId: string) => void;
  busyBindingId?: string | null;
}) {
  return (
    <PanelCard
      title="Source bindings"
      subtitle="External sources mapped to resource scopes"
      actions={<Badge tone={error ? "err" : rows.length ? "ok" : "ghost"}>{rows.length}</Badge>}
      fullHeight
    >
      {loading ? (
        <LoadingState title="Loading" text="Reading source bindings" />
      ) : error ? (
        <EmptyState
          title="Unavailable"
          text={errorText(error, "Source bindings could not load.")}
        />
      ) : rows.length ? (
        <div className="grid gap-2">
          {rows.map((binding) => {
            const policy = binding.ingestion_policy || {};
            const isBusy = busyBindingId === binding.binding_id;
            return (
              <div
                key={binding.binding_id}
                className="rounded-lg border border-white/8 bg-black/20 p-3"
              >
                <div className="flex flex-wrap items-start justify-between gap-2">
                  <div>
                    <div className="font-medium text-tcp-text-primary">
                      {binding.source_root_label || binding.binding_id}
                    </div>
                    <div className="tcp-subtle text-xs">
                      {binding.connector_id} · {binding.source_type} · {binding.native_source_id}
                    </div>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <Badge tone={binding.state === "enabled" ? "ok" : "warn"}>
                      {binding.state || "enabled"}
                    </Badge>
                    <Badge tone="info">{binding.data_class}</Badge>
                  </div>
                </div>
                <div className="mt-3 grid gap-2 text-xs text-tcp-text-secondary md:grid-cols-2">
                  <div>
                    Resource: {binding.resource_ref?.resource_kind} /{" "}
                    {binding.resource_ref?.resource_id}
                  </div>
                  <div>
                    Index: {policy.allow_indexing === false ? "off" : "on"} · Prompt:{" "}
                    {policy.allow_prompt_context === false ? "off" : "on"} · Review:{" "}
                    {policy.require_review ? "required" : "not required"}
                  </div>
                </div>
                <div className="mt-3 flex flex-wrap gap-2">
                  {["enabled", "disabled", "quarantined"].map((state) => (
                    <button
                      key={state}
                      className="tcp-btn"
                      type="button"
                      disabled={isBusy || binding.state === state}
                      onClick={() => onSetState(binding.binding_id, state)}
                    >
                      {state}
                    </button>
                  ))}
                  <button
                    className="tcp-btn"
                    type="button"
                    onClick={() => onSelectBinding(binding.binding_id)}
                    disabled={selectedBindingId === binding.binding_id}
                  >
                    <i data-lucide="list-tree"></i>
                    Objects
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      ) : (
        <EmptyState
          title="No source bindings"
          text="Bind a source root before connector output can become searchable."
        />
      )}
    </PanelCard>
  );
}

function formatLifecycleTime(value?: number | null) {
  if (!value) return "never";
  return new Date(value).toLocaleString();
}

function SourceObjectLifecyclePanel({
  binding,
  rows,
  loading,
  error,
  onReindex,
  onDelete,
  onRescope,
  busyObjectId,
}: {
  binding?: EnterpriseSourceBinding | null;
  rows: EnterpriseSourceObjectLifecycle[];
  loading: boolean;
  error: unknown;
  onReindex: (sourceObjectId: string) => void;
  onDelete: (sourceObjectId: string) => void;
  onRescope: (
    sourceObjectId: string,
    resourceKind: string,
    resourceId: string,
    dataClass: string
  ) => void;
  busyObjectId?: string | null;
}) {
  const [rescopeTarget, setRescopeTarget] = useState<string | null>(null);
  const selectedObject = rows.find((row) => row.source_object_id === rescopeTarget) || rows[0];
  const [resourceKind, setResourceKind] = useState("document_collection");
  const [resourceId, setResourceId] = useState("");
  const [dataClass, setDataClass] = useState("internal");

  const beginRescope = (object: EnterpriseSourceObjectLifecycle) => {
    setRescopeTarget(object.source_object_id);
    setResourceKind(object.resource_ref?.resource_kind || "document_collection");
    setResourceId(object.resource_ref?.resource_id || "");
    setDataClass(object.data_class || "internal");
  };

  return (
    <PanelCard
      title="Source objects"
      subtitle={binding ? binding.source_root_label || binding.binding_id : "Select a binding"}
      actions={<Badge tone={error ? "err" : rows.length ? "ok" : "ghost"}>{rows.length}</Badge>}
      fullHeight
    >
      {!binding ? (
        <EmptyState
          title="No binding selected"
          text="Choose a source binding to inspect objects."
        />
      ) : loading ? (
        <LoadingState title="Loading" text="Reading source-object lifecycle records" />
      ) : error ? (
        <EmptyState title="Unavailable" text={errorText(error, "Source objects could not load.")} />
      ) : rows.length ? (
        <div className="grid gap-3">
          <div className="grid gap-2">
            {rows.map((object) => {
              const isBusy = busyObjectId === object.source_object_id;
              return (
                <div
                  key={object.source_object_id}
                  className="rounded-lg border border-white/8 bg-black/20 p-3"
                >
                  <div className="flex flex-wrap items-start justify-between gap-2">
                    <div>
                      <div className="break-all font-medium text-tcp-text-primary">
                        {object.native_object_id || object.indexed_path}
                      </div>
                      <div className="tcp-subtle break-all text-xs">{object.source_object_id}</div>
                    </div>
                    <div className="flex flex-wrap gap-2">
                      <Badge tone={object.state === "active" ? "ok" : "warn"}>{object.state}</Badge>
                      <Badge tone="info">{object.data_class}</Badge>
                    </div>
                  </div>
                  <div className="mt-3 grid gap-2 text-xs text-tcp-text-secondary md:grid-cols-2">
                    <div>
                      Resource: {object.resource_ref?.resource_kind} /{" "}
                      {object.resource_ref?.resource_id}
                    </div>
                    <div>Last seen: {formatLifecycleTime(object.last_seen_at_ms)}</div>
                    <div className="break-all">Indexed path: {object.indexed_path}</div>
                    <div>Tier: {object.tier}</div>
                  </div>
                  <div className="mt-3 flex flex-wrap gap-2">
                    <button
                      className="tcp-btn"
                      type="button"
                      disabled={isBusy}
                      onClick={() => onReindex(object.source_object_id)}
                    >
                      <i data-lucide="refresh-cw"></i>
                      Reindex
                    </button>
                    <button
                      className="tcp-btn"
                      type="button"
                      disabled={isBusy}
                      onClick={() => beginRescope(object)}
                    >
                      <i data-lucide="move-horizontal"></i>
                      Re-scope
                    </button>
                    <button
                      className="tcp-btn tcp-btn-danger"
                      type="button"
                      disabled={isBusy}
                      onClick={() => onDelete(object.source_object_id)}
                    >
                      <i data-lucide="trash-2"></i>
                      Delete
                    </button>
                  </div>
                </div>
              );
            })}
          </div>

          {selectedObject ? (
            <form
              className="grid gap-3 rounded-lg border border-white/8 bg-black/20 p-3"
              onSubmit={(event) => {
                event.preventDefault();
                onRescope(
                  selectedObject.source_object_id,
                  resourceKind,
                  resourceId.trim(),
                  dataClass
                );
              }}
            >
              <div className="font-medium text-tcp-text-primary">Re-scope selected object</div>
              <div className="tcp-subtle break-all text-xs">{selectedObject.source_object_id}</div>
              <div className="grid gap-3 md:grid-cols-3">
                <Field label="Resource Kind">
                  <select
                    className="tcp-select"
                    value={resourceKind}
                    onChange={(event) => setResourceKind(event.currentTarget.value)}
                  >
                    {RESOURCE_KINDS.map((option) => (
                      <option key={option} value={option}>
                        {option}
                      </option>
                    ))}
                  </select>
                </Field>
                <Field label="Resource ID">
                  <input
                    className="tcp-input"
                    value={resourceId}
                    onInput={(event) => setResourceId(event.currentTarget.value)}
                    required
                  />
                </Field>
                <Field label="Data Class">
                  <select
                    className="tcp-select"
                    value={dataClass}
                    onChange={(event) => setDataClass(event.currentTarget.value)}
                  >
                    {DATA_CLASSES.map((option) => (
                      <option key={option} value={option}>
                        {option}
                      </option>
                    ))}
                  </select>
                </Field>
              </div>
              <div className="flex justify-end">
                <button
                  className="tcp-btn tcp-btn-primary"
                  type="submit"
                  disabled={busyObjectId === selectedObject.source_object_id}
                >
                  <i data-lucide="shield-check"></i>
                  Apply scope
                </button>
              </div>
            </form>
          ) : null}
        </div>
      ) : (
        <EmptyState title="No source objects" text="Source-bound imports will appear here." />
      )}
    </PanelCard>
  );
}

function IngestionJobsPanel({
  binding,
  rows,
  loading,
  error,
}: {
  binding?: EnterpriseSourceBinding | null;
  rows: EnterpriseIngestionJob[];
  loading: boolean;
  error: unknown;
}) {
  return (
    <PanelCard
      title="Ingestion jobs"
      subtitle={binding ? binding.source_root_label || binding.binding_id : "All bindings"}
      actions={<Badge tone={error ? "err" : rows.length ? "ok" : "ghost"}>{rows.length}</Badge>}
      fullHeight
    >
      {loading ? (
        <LoadingState title="Loading" text="Reading ingestion audit records" />
      ) : error ? (
        <EmptyState title="Unavailable" text={errorText(error, "Ingestion jobs could not load.")} />
      ) : rows.length ? (
        <div className="grid gap-2">
          {rows.map((job) => (
            <div key={job.job_id} className="rounded-lg border border-white/8 bg-black/20 p-3">
              <div className="flex flex-wrap items-start justify-between gap-2">
                <div>
                  <div className="break-all font-medium text-tcp-text-primary">{job.job_id}</div>
                  <div className="tcp-subtle text-xs">
                    {job.connector_id} / {job.binding_id}
                  </div>
                </div>
                <div className="flex flex-wrap gap-2">
                  <Badge
                    tone={
                      job.state === "completed"
                        ? "ok"
                        : job.state === "failed" || job.state === "quarantined"
                          ? "err"
                          : "warn"
                    }
                  >
                    {job.state || "queued"}
                  </Badge>
                  <Badge tone="info">{job.source_object_ids?.length || 0} objects</Badge>
                </div>
              </div>
              <div className="mt-3 grid gap-2 text-xs text-tcp-text-secondary md:grid-cols-2">
                <div>Started: {formatLifecycleTime(job.started_at_ms)}</div>
                <div>Finished: {formatLifecycleTime(job.finished_at_ms)}</div>
                {job.quarantine_id ? (
                  <div className="break-all md:col-span-2">Quarantine: {job.quarantine_id}</div>
                ) : null}
              </div>
            </div>
          ))}
        </div>
      ) : (
        <EmptyState title="No jobs" text="Source-bound imports will create audit records here." />
      )}
    </PanelCard>
  );
}

export function EnterpriseAdminPage({ navigate, toast }: AppPageProps) {
  const orgUnits = useEnterpriseOrgUnits();
  const connectors = useEnterpriseConnectors();
  const sourceBindings = useEnterpriseSourceBindings();
  const [selectedBindingId, setSelectedBindingId] = useState<string | null>(null);
  const createOrgUnit = useCreateEnterpriseOrgUnit();
  const createConnector = useCreateEnterpriseConnector();
  const createConnectorCredentialRef = useCreateEnterpriseConnectorCredentialRef();
  const createSourceBinding = useCreateEnterpriseSourceBinding();
  const updateConnector = useUpdateEnterpriseConnector();
  const rotateConnectorCredentialRef = useRotateEnterpriseConnectorCredentialRef();
  const updateSourceBinding = useUpdateEnterpriseSourceBinding();
  const sourceObjects = useEnterpriseSourceObjects(selectedBindingId);
  const ingestionJobs = useEnterpriseIngestionJobs(selectedBindingId);
  const reindexSourceObject = useReindexEnterpriseSourceObject();
  const deleteSourceObject = useDeleteEnterpriseSourceObject();
  const rescopeSourceObject = useRescopeEnterpriseSourceObject();
  const orgRows = useMemo(() => orgUnits.data?.org_units || [], [orgUnits.data]);
  const connectorRows = useMemo(() => connectors.data?.connectors || [], [connectors.data]);
  const bindingRows = useMemo(
    () => sourceBindings.data?.source_bindings || [],
    [sourceBindings.data]
  );
  const objectRows = useMemo(() => sourceObjects.data?.source_objects || [], [sourceObjects.data]);
  const ingestionJobRows = useMemo(
    () => ingestionJobs.data?.ingestion_jobs || [],
    [ingestionJobs.data]
  );
  const selectedBinding =
    bindingRows.find((binding) => binding.binding_id === selectedBindingId) || null;
  const busyObjectId =
    reindexSourceObject.isPending || deleteSourceObject.isPending || rescopeSourceObject.isPending
      ? reindexSourceObject.variables?.source_object_id ||
        deleteSourceObject.variables?.source_object_id ||
        rescopeSourceObject.variables?.source_object_id ||
        null
      : null;
  const payload = orgUnits.data || connectors.data || sourceBindings.data;
  const headerBadges = (
    <>
      <Badge tone={noopStatus(payload) ? "warn" : "ok"}>{payload?.status || "checking"}</Badge>
      <Badge tone="info">{compactTenant(payload)}</Badge>
    </>
  );
  const refreshEnterpriseState = () => {
    orgUnits.refetch();
    connectors.refetch();
    sourceBindings.refetch();
    if (selectedBindingId) {
      sourceObjects.refetch();
    }
    ingestionJobs.refetch();
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
          connectorsPayload={connectors.data}
          sourceBindingsPayload={sourceBindings.data}
        />

        <div className="grid gap-4 xl:grid-cols-3">
          <OrgUnitForm
            busy={createOrgUnit.isPending}
            onCreate={async (input) => {
              try {
                await createOrgUnit.mutateAsync(input);
                toast("ok", "Organization unit created.");
              } catch (error) {
                toast("err", errorText(error, "Organization unit could not be created."));
              }
            }}
          />
          <ConnectorForm
            busy={createConnector.isPending}
            onCreate={async (input) => {
              try {
                await createConnector.mutateAsync(input);
                toast("ok", "Connector created.");
              } catch (error) {
                toast("err", errorText(error, "Connector could not be created."));
              }
            }}
          />
          <ConnectorCredentialRefForm
            tenantPayload={payload}
            connectors={connectorRows}
            busy={createConnectorCredentialRef.isPending || rotateConnectorCredentialRef.isPending}
            onCreate={async (input) => {
              try {
                await createConnectorCredentialRef.mutateAsync(input);
                toast("ok", "Credential reference attached.");
              } catch (error) {
                toast("err", errorText(error, "Credential reference could not be attached."));
              }
            }}
            onRotate={async (input) => {
              try {
                await rotateConnectorCredentialRef.mutateAsync(input);
                toast("ok", "Credential reference rotated.");
              } catch (error) {
                toast("err", errorText(error, "Credential reference could not be rotated."));
              }
            }}
          />
          <SourceBindingForm
            tenantPayload={payload}
            busy={createSourceBinding.isPending}
            onCreate={async (input) => {
              try {
                await createSourceBinding.mutateAsync(input);
                toast("ok", "Source binding created.");
              } catch (error) {
                toast("err", errorText(error, "Source binding could not be created."));
              }
            }}
          />
        </div>

        <div className="grid gap-4 xl:grid-cols-3">
          <OrgUnitsPanel rows={orgRows} loading={orgUnits.isLoading} error={orgUnits.error} />
          <ConnectorsPanel
            rows={connectorRows}
            loading={connectors.isLoading}
            error={connectors.error}
            busyConnectorId={
              updateConnector.isPending ? updateConnector.variables?.connector_id || null : null
            }
            onSetState={(connectorId, state) => {
              updateConnector
                .mutateAsync({ connector_id: connectorId, state })
                .then(() => toast("ok", `Connector ${state}.`))
                .catch((error) =>
                  toast("err", errorText(error, "Connector could not be updated."))
                );
            }}
          />
          <SourceBindingsPanel
            rows={bindingRows}
            loading={sourceBindings.isLoading}
            error={sourceBindings.error}
            selectedBindingId={selectedBindingId}
            onSelectBinding={setSelectedBindingId}
            busyBindingId={
              updateSourceBinding.isPending
                ? updateSourceBinding.variables?.binding_id || null
                : null
            }
            onSetState={(bindingId, state) => {
              updateSourceBinding
                .mutateAsync({ binding_id: bindingId, state })
                .then(() => toast("ok", `Source binding ${state}.`))
                .catch((error) =>
                  toast("err", errorText(error, "Source binding could not be updated."))
                );
            }}
          />
        </div>

        <div className="grid gap-4 xl:grid-cols-2">
          <SourceObjectLifecyclePanel
            binding={selectedBinding}
            rows={objectRows}
            loading={sourceObjects.isLoading}
            error={sourceObjects.error}
            busyObjectId={busyObjectId}
            onReindex={(sourceObjectId) => {
              if (!selectedBindingId) return;
              reindexSourceObject
                .mutateAsync({
                  binding_id: selectedBindingId,
                  source_object_id: sourceObjectId,
                })
                .then(() => toast("ok", "Source object reindex requested."))
                .catch((error) =>
                  toast("err", errorText(error, "Source object could not be reindexed."))
                );
            }}
            onDelete={(sourceObjectId) => {
              if (!selectedBindingId) return;
              deleteSourceObject
                .mutateAsync({
                  binding_id: selectedBindingId,
                  source_object_id: sourceObjectId,
                })
                .then(() => toast("ok", "Source object deleted."))
                .catch((error) =>
                  toast("err", errorText(error, "Source object could not be deleted."))
                );
            }}
            onRescope={(sourceObjectId, resourceKind, resourceId, dataClass) => {
              if (!selectedBindingId || !selectedBinding || !resourceId) return;
              rescopeSourceObject
                .mutateAsync({
                  binding_id: selectedBindingId,
                  source_object_id: sourceObjectId,
                  resource_ref: {
                    ...selectedBinding.resource_ref,
                    resource_kind: resourceKind,
                    resource_id: resourceId,
                  },
                  data_class: dataClass,
                })
                .then(() => toast("ok", "Source object scope updated."))
                .catch((error) =>
                  toast("err", errorText(error, "Source object scope could not be updated."))
                );
            }}
          />
          <IngestionJobsPanel
            binding={selectedBinding}
            rows={ingestionJobRows}
            loading={ingestionJobs.isLoading}
            error={ingestionJobs.error}
          />
        </div>
      </StaggerGroup>
    </AnimatedPage>
  );
}
