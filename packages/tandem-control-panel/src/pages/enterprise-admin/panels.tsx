import { useState } from "react";
import { Badge, EmptyState, IdChip, KeyValueRow, LoadingState, PanelCard } from "../../ui/index.tsx";
import { CONNECTOR_STATES, DATA_CLASSES, Field, RESOURCE_KINDS, connectorStateTone, errorText, formatLifecycleTime } from "./shared.tsx";
import type {
  EnterpriseConnectorImpactResponse,
  EnterpriseConnectorInstance,
  EnterpriseGoogleDriveImportResponse,
  EnterpriseGoogleDrivePreflightResponse,
  EnterpriseIngestionJob,
  EnterpriseIngestionQuarantine,
  EnterpriseOrganizationUnit,
  EnterpriseOrganizationUnitAccessGrant,
  EnterpriseOrganizationUnitMembership,
  EnterpriseScopedGrant,
  EnterpriseSourceBinding,
  EnterpriseSourceObjectLifecycle,
} from "../../features/enterprise/queries";
import { Icon } from "../../ui/Icon";

export function OrgUnitsPanel({
  rows,
  loading,
  error,
  onCreateNew,
}: {
  rows: EnterpriseOrganizationUnit[];
  loading: boolean;
  error: unknown;
  onCreateNew?: () => void;
}) {
  return (
    <PanelCard
      title="Org units"
      subtitle="Company-defined domains"
      actions={
        <div className="flex items-center gap-2">
          <Badge tone={error ? "err" : rows.length ? "ok" : "ghost"}>{rows.length}</Badge>
          {onCreateNew ? (
            <button className="tcp-btn h-7 px-2 text-xs" type="button" onClick={onCreateNew}>
              <Icon name="plus" />
              New
            </button>
          ) : null}
        </div>
      }
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

export function OrgUnitMembershipsPanel({
  rows,
  loading,
  error,
  onSetState,
  busyMembershipId,
  onCreateNew,
}: {
  rows: EnterpriseOrganizationUnitMembership[];
  loading: boolean;
  error: unknown;
  onSetState: (membershipId: string, state: string) => void;
  busyMembershipId?: string | null;
  onCreateNew?: () => void;
}) {
  return (
    <PanelCard
      title="Org memberships"
      subtitle="Users mapped to company domains"
      actions={
        <div className="flex items-center gap-2">
          <Badge tone={error ? "err" : rows.length ? "ok" : "ghost"}>{rows.length}</Badge>
          {onCreateNew ? (
            <button className="tcp-btn h-7 px-2 text-xs" type="button" onClick={onCreateNew}>
              <Icon name="plus" />
              New
            </button>
          ) : null}
        </div>
      }
      fullHeight
    >
      {loading ? (
        <LoadingState title="Loading" text="Reading organization memberships" />
      ) : error ? (
        <EmptyState title="Unavailable" text={errorText(error, "Memberships could not load.")} />
      ) : rows.length ? (
        <div className="grid gap-2">
          {rows.map((membership) => {
            const busy = busyMembershipId === membership.membership_id;
            return (
              <div
                key={membership.membership_id}
                className="rounded-lg border border-white/8 bg-black/20 p-3"
              >
                <div className="flex flex-wrap items-start justify-between gap-2">
                  <div>
                    <div className="font-medium text-tcp-text-primary">{membership.member.id}</div>
                    <div className="tcp-subtle text-xs">
                      {membership.member.kind} / {membership.unit.id}
                    </div>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <Badge tone={membership.state === "disabled" ? "warn" : "ok"}>
                      {membership.state || "active"}
                    </Badge>
                    <Badge tone="info">{membership.source || "direct"}</Badge>
                  </div>
                </div>
                <div className="mt-3 grid gap-2 text-xs text-tcp-text-secondary md:grid-cols-2">
                  <div>Created: {formatLifecycleTime(membership.created_at_ms)}</div>
                  <div>Expires: {formatLifecycleTime(membership.expires_at_ms)}</div>
                </div>
                <div className="mt-3 flex flex-wrap gap-2">
                  {["active", "disabled"].map((state) => (
                    <button
                      key={state}
                      className="tcp-btn"
                      type="button"
                      disabled={busy || membership.state === state}
                      onClick={() => onSetState(membership.membership_id, state)}
                    >
                      {state}
                    </button>
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      ) : (
        <EmptyState title="No memberships" text="Assign hosted users to org units." />
      )}
    </PanelCard>
  );
}

export function OrgUnitAccessGrantsPanel({
  rows,
  effectiveRows,
  loading,
  error,
  effectiveMemberId,
  onEffectiveMemberId,
  onSetState,
  busyGrantId,
  onCreateNew,
}: {
  rows: EnterpriseOrganizationUnitAccessGrant[];
  effectiveRows: EnterpriseScopedGrant[];
  loading: boolean;
  error: unknown;
  effectiveMemberId: string;
  onEffectiveMemberId: (memberId: string) => void;
  onSetState: (grantId: string, state: string) => void;
  busyGrantId?: string | null;
  onCreateNew?: () => void;
}) {
  return (
    <PanelCard
      title="Unit access"
      subtitle="Projected resource grants"
      actions={
        <div className="flex items-center gap-2">
          <Badge tone={error ? "err" : rows.length ? "ok" : "ghost"}>{rows.length}</Badge>
          {onCreateNew ? (
            <button className="tcp-btn h-7 px-2 text-xs" type="button" onClick={onCreateNew}>
              <Icon name="plus" />
              New
            </button>
          ) : null}
        </div>
      }
      fullHeight
    >
      <div className="mb-3 grid gap-2">
        <Field label="Preview member">
          <input
            className="tcp-input"
            value={effectiveMemberId}
            onInput={(event) => onEffectiveMemberId(event.currentTarget.value)}
            placeholder="user@company.com"
          />
        </Field>
        {effectiveMemberId ? (
          <div className="rounded-lg border border-white/8 bg-black/20 p-3 text-xs text-tcp-text-secondary">
            <div className="mb-2 font-medium text-tcp-text-primary">
              Effective grants: {effectiveRows.length}
            </div>
            {effectiveRows.length ? (
              <div className="grid gap-1">
                {effectiveRows.map((grant) => (
                  <div key={grant.grant_id}>
                    {grant.resource.resource_kind}/{grant.resource.resource_id} ·{" "}
                    {(grant.permissions || []).join(", ") || "no permissions"}
                  </div>
                ))}
              </div>
            ) : (
              <div>No active projected grants.</div>
            )}
          </div>
        ) : null}
      </div>
      {loading ? (
        <LoadingState title="Loading" text="Reading organization access grants" />
      ) : error ? (
        <EmptyState title="Unavailable" text={errorText(error, "Access grants could not load.")} />
      ) : rows.length ? (
        <div className="grid gap-2">
          {rows.map((grant) => {
            const busy = busyGrantId === grant.grant_id;
            return (
              <div
                key={grant.grant_id}
                className="rounded-lg border border-white/8 bg-black/20 p-3"
              >
                <div className="flex flex-wrap items-start justify-between gap-2">
                  <div>
                    <div className="font-medium text-tcp-text-primary">{grant.grant_id}</div>
                    <div className="tcp-subtle text-xs">
                      {grant.unit.id} / {grant.resource.resource_kind}:{grant.resource.resource_id}
                    </div>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <Badge tone={grant.state === "disabled" ? "warn" : "ok"}>
                      {grant.state || "active"}
                    </Badge>
                    <Badge tone={grant.effect === "deny" ? "err" : "info"}>
                      {grant.effect || "allow"}
                    </Badge>
                  </div>
                </div>
                <div className="mt-2 flex flex-wrap gap-1">
                  {(grant.permissions || []).map((permission) => (
                    <Badge key={permission} tone="ghost">
                      {permission}
                    </Badge>
                  ))}
                  {(grant.data_classes || []).map((dataClass) => (
                    <Badge key={dataClass} tone="info">
                      {dataClass}
                    </Badge>
                  ))}
                </div>
                <div className="mt-3 flex flex-wrap gap-2">
                  {["active", "disabled"].map((state) => (
                    <button
                      key={state}
                      className="tcp-btn"
                      type="button"
                      disabled={busy || grant.state === state}
                      onClick={() => onSetState(grant.grant_id, state)}
                    >
                      {state}
                    </button>
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      ) : (
        <EmptyState title="No access grants" text="Grant an org unit access to a resource." />
      )}
    </PanelCard>
  );
}

export function ConnectorsPanel({
  rows,
  loading,
  error,
  onSetState,
  onSelectImpact,
  selectedConnectorId,
  busyConnectorId,
  onCreateNew,
  onCreateCredentialRef,
}: {
  rows: EnterpriseConnectorInstance[];
  loading: boolean;
  error: unknown;
  onSetState: (connectorId: string, state: string) => void;
  onSelectImpact: (connectorId: string) => void;
  selectedConnectorId?: string | null;
  busyConnectorId?: string | null;
  onCreateNew?: () => void;
  onCreateCredentialRef?: () => void;
}) {
  return (
    <PanelCard
      title="Connectors"
      subtitle="Tenant-scoped ingestion lifecycle"
      actions={
        <div className="flex items-center gap-2">
          <Badge tone={error ? "err" : rows.length ? "ok" : "ghost"}>{rows.length}</Badge>
          {onCreateCredentialRef ? (
            <button
              className="tcp-btn h-7 px-2 text-xs"
              type="button"
              onClick={onCreateCredentialRef}
            >
              <Icon name="key-round" />
              Credential ref
            </button>
          ) : null}
          {onCreateNew ? (
            <button className="tcp-btn h-7 px-2 text-xs" type="button" onClick={onCreateNew}>
              <Icon name="plus" />
              New
            </button>
          ) : null}
        </div>
      }
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
                  <button
                    className="tcp-btn"
                    type="button"
                    disabled={selectedConnectorId === connector.connector_id}
                    onClick={() => onSelectImpact(connector.connector_id)}
                  >
                    <Icon name="radar" />
                    Impact
                  </button>
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

export function ConnectorImpactPanel({
  connectorId,
  payload,
  loading,
  error,
}: {
  connectorId?: string | null;
  payload?: EnterpriseConnectorImpactResponse | null;
  loading: boolean;
  error: unknown;
}) {
  const bindings = payload?.affected_bindings || [];
  const objects = payload?.affected_source_objects || [];
  const jobs = payload?.affected_ingestion_jobs || [];
  const quarantines = payload?.affected_quarantines || [];
  return (
    <PanelCard
      title="Connector impact"
      subtitle={connectorId || "Select connector"}
      actions={
        <Badge tone={payload?.cache_invalidation_required ? "warn" : "ghost"}>
          {payload?.cache_invalidation_required ? "invalidate" : "clear"}
        </Badge>
      }
    >
      {!connectorId ? (
        <EmptyState title="No connector selected" text="Choose Impact on a connector row." />
      ) : loading ? (
        <LoadingState title="Loading" text="Computing affected enterprise scope" />
      ) : error ? (
        <EmptyState title="Unavailable" text={errorText(error, "Impact could not load.")} />
      ) : (
        <div className="grid gap-3">
          <div className="grid gap-3 md:grid-cols-4">
            <div className="rounded-lg border border-white/8 bg-black/20 p-3">
              <div className="tcp-subtle text-xs uppercase tracking-[0.14em]">Bindings</div>
              <div className="mt-1 text-lg font-semibold text-tcp-text-primary">
                {bindings.length}
              </div>
            </div>
            <div className="rounded-lg border border-white/8 bg-black/20 p-3">
              <div className="tcp-subtle text-xs uppercase tracking-[0.14em]">Objects</div>
              <div className="mt-1 text-lg font-semibold text-tcp-text-primary">
                {objects.length}
              </div>
            </div>
            <div className="rounded-lg border border-white/8 bg-black/20 p-3">
              <div className="tcp-subtle text-xs uppercase tracking-[0.14em]">Jobs</div>
              <div className="mt-1 text-lg font-semibold text-tcp-text-primary">{jobs.length}</div>
            </div>
            <div className="rounded-lg border border-white/8 bg-black/20 p-3">
              <div className="tcp-subtle text-xs uppercase tracking-[0.14em]">Quarantine</div>
              <div className="mt-1 text-lg font-semibold text-tcp-text-primary">
                {quarantines.length}
              </div>
            </div>
          </div>
          <div className="grid gap-2 text-xs text-tcp-text-secondary md:grid-cols-2">
            <div>Window start: {formatLifecycleTime(payload?.compromise_window_started_at_ms)}</div>
            <div>Window end: {formatLifecycleTime(payload?.compromise_window_finished_at_ms)}</div>
          </div>
          {payload?.recommended_actions?.length ? (
            <div className="flex flex-wrap gap-2">
              {payload.recommended_actions.map((action) => (
                <Badge key={action} tone="info">
                  {action}
                </Badge>
              ))}
            </div>
          ) : null}
          {bindings.length ? (
            <div className="grid gap-2">
              {bindings.map((binding) => (
                <div
                  key={binding.binding_id}
                  className="rounded-lg border border-white/8 bg-black/20 p-3"
                >
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div>
                      <div className="font-medium text-tcp-text-primary">
                        {binding.source_root_label || binding.binding_id}
                      </div>
                      <div className="tcp-subtle text-xs">
                        {binding.resource_ref.resource_kind} / {binding.resource_ref.resource_id}
                      </div>
                    </div>
                    <Badge tone={binding.state === "enabled" ? "ok" : "warn"}>
                      {binding.state || "enabled"}
                    </Badge>
                  </div>
                </div>
              ))}
            </div>
          ) : null}
        </div>
      )}
    </PanelCard>
  );
}

export function SourceBindingsPanel({
  rows,
  loading,
  error,
  onSetState,
  selectedBindingId,
  onSelectBinding,
  busyBindingId,
  onCreateNew,
}: {
  rows: EnterpriseSourceBinding[];
  loading: boolean;
  error: unknown;
  onSetState: (bindingId: string, state: string) => void;
  selectedBindingId?: string | null;
  onSelectBinding: (bindingId: string) => void;
  busyBindingId?: string | null;
  onCreateNew?: () => void;
}) {
  return (
    <PanelCard
      title="Source bindings"
      subtitle="External sources mapped to resource scopes"
      actions={
        <div className="flex items-center gap-2">
          <Badge tone={error ? "err" : rows.length ? "ok" : "ghost"}>{rows.length}</Badge>
          {onCreateNew ? (
            <button className="tcp-btn h-7 px-2 text-xs" type="button" onClick={onCreateNew}>
              <Icon name="plus" />
              New
            </button>
          ) : null}
        </div>
      }
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
                    <Icon name="list-tree" />
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


export function GoogleDriveOperationsPanel({
  binding,
  preflightPayload,
  importPayload,
  reindexPayload,
  preflightBusy,
  importBusy,
  reindexBusy,
  preflightError,
  importError,
  reindexError,
  onPreflight,
  onImport,
  onReindexBinding,
}: {
  binding?: EnterpriseSourceBinding | null;
  preflightPayload?: EnterpriseGoogleDrivePreflightResponse | null;
  importPayload?: EnterpriseGoogleDriveImportResponse | null;
  reindexPayload?: EnterpriseGoogleDriveImportResponse | null;
  preflightBusy: boolean;
  importBusy: boolean;
  reindexBusy: boolean;
  preflightError: unknown;
  importError: unknown;
  reindexError: unknown;
  onPreflight: () => void;
  onImport: (input: {
    tier: string;
    project_id?: string;
    session_id?: string;
    sync_deletes: boolean;
  }) => void;
  onReindexBinding: (input: {
    tier: string;
    project_id?: string;
    session_id?: string;
    sync_deletes: boolean;
  }) => void;
}) {
  const [tier, setTier] = useState("global");
  const [projectId, setProjectId] = useState("");
  const [sessionId, setSessionId] = useState("");
  const [syncDeletes, setSyncDeletes] = useState(false);
  const isGoogleDrive = binding?.source_type === "google_drive";
  const preflight = preflightPayload?.preflight;
  const latestPayload = reindexPayload || importPayload;
  const stats = latestPayload?.stats || {};
  const scopeMissing =
    (tier === "project" && !projectId.trim()) || (tier === "session" && !sessionId.trim());

  return (
    <PanelCard
      title="Google Drive import"
      subtitle={binding ? binding.source_root_label || binding.binding_id : "Select a binding"}
      actions={
        <Badge tone={isGoogleDrive ? "ok" : "ghost"}>
          {isGoogleDrive ? "drive" : "unavailable"}
        </Badge>
      }
    >
      {!binding ? (
        <EmptyState title="No binding selected" text="Choose a source binding." />
      ) : !isGoogleDrive ? (
        <EmptyState title="Not Google Drive" text="Select a Google Drive source binding." />
      ) : (
        <div className="grid gap-3">
          <div className="flex flex-wrap gap-2">
            <button
              className="tcp-btn"
              type="button"
              disabled={preflightBusy || importBusy || reindexBusy}
              onClick={onPreflight}
            >
              <Icon name="radar" />
              Preflight
            </button>
            <button
              className="tcp-btn tcp-btn-primary"
              type="button"
              disabled={preflightBusy || importBusy || reindexBusy || scopeMissing}
              onClick={() =>
                onImport({
                  tier,
                  project_id: projectId.trim() || undefined,
                  session_id: sessionId.trim() || undefined,
                  sync_deletes: syncDeletes,
                })
              }
            >
              <Icon name="download-cloud" />
              Import
            </button>
            <button
              className="tcp-btn"
              type="button"
              disabled={preflightBusy || importBusy || reindexBusy || scopeMissing}
              onClick={() =>
                onReindexBinding({
                  tier,
                  project_id: projectId.trim() || undefined,
                  session_id: sessionId.trim() || undefined,
                  sync_deletes: syncDeletes,
                })
              }
            >
              <Icon name="refresh-cw" />
              Reindex binding
            </button>
          </div>

          <div className="grid gap-3 md:grid-cols-3">
            <Field label="Tier">
              <select
                className="tcp-select"
                value={tier}
                onChange={(event) => setTier(event.currentTarget.value)}
              >
                <option value="global">global</option>
                <option value="project">project</option>
                <option value="session">session</option>
              </select>
            </Field>
            <Field label="Project ID">
              <input
                className="tcp-input"
                value={projectId}
                onInput={(event) => setProjectId(event.currentTarget.value)}
                disabled={tier !== "project"}
                placeholder={tier === "project" ? "finance-project" : ""}
              />
            </Field>
            <Field label="Session ID">
              <input
                className="tcp-input"
                value={sessionId}
                onInput={(event) => setSessionId(event.currentTarget.value)}
                disabled={tier !== "session"}
                placeholder={tier === "session" ? "session-id" : ""}
              />
            </Field>
          </div>

          <label className="flex items-center gap-2 text-sm text-tcp-text-secondary">
            <input
              type="checkbox"
              checked={syncDeletes}
              onChange={(event) => setSyncDeletes(event.currentTarget.checked)}
            />
            Sync deletes
          </label>

          {preflightError || importError || reindexError ? (
            <div className="rounded-lg border border-red-500/25 bg-red-500/10 px-3 py-2 text-sm text-red-100">
              {errorText(
                preflightError || importError || reindexError,
                "Google Drive action failed."
              )}
            </div>
          ) : null}

          {preflight ? (
            <div className="grid gap-3 rounded-lg border border-white/8 bg-black/20 p-3 md:grid-cols-3">
              <div>
                <div className="tcp-subtle text-xs uppercase tracking-[0.14em]">Files</div>
                <div className="mt-1 text-lg font-semibold text-tcp-text-primary">
                  {preflight.file_count}
                </div>
              </div>
              <div className="md:col-span-2">
                <div className="tcp-subtle text-xs uppercase tracking-[0.14em]">Folder</div>
                <div className="mt-1 break-all text-sm text-tcp-text-primary">
                  {preflight.folder_id}
                </div>
              </div>
            </div>
          ) : null}

          {latestPayload ? (
            <div className="grid gap-3 rounded-lg border border-white/8 bg-black/20 p-3 md:grid-cols-4">
              <div>
                <div className="tcp-subtle text-xs uppercase tracking-[0.14em]">Fetched</div>
                <div className="mt-1 text-lg font-semibold text-tcp-text-primary">
                  {latestPayload.drive_files_fetched || 0}
                </div>
              </div>
              <div>
                <div className="tcp-subtle text-xs uppercase tracking-[0.14em]">Indexed</div>
                <div className="mt-1 text-lg font-semibold text-tcp-text-primary">
                  {stats.indexed_files || 0}
                </div>
              </div>
              <div>
                <div className="tcp-subtle text-xs uppercase tracking-[0.14em]">Chunks</div>
                <div className="mt-1 text-lg font-semibold text-tcp-text-primary">
                  {stats.chunks_created || 0}
                </div>
              </div>
              <div>
                <div className="tcp-subtle text-xs uppercase tracking-[0.14em]">Job</div>
                <div className="mt-1">
                  <Badge
                    tone={
                      latestPayload.ingestion_job?.state === "completed"
                        ? "ok"
                        : latestPayload.ingestion_job?.state === "quarantined"
                          ? "warn"
                          : "ghost"
                    }
                  >
                    {latestPayload.ingestion_job?.state || "queued"}
                  </Badge>
                </div>
              </div>
              <div className="break-all text-xs text-tcp-text-secondary md:col-span-4">
                {latestPayload.ingestion_job?.job_id}
              </div>
            </div>
          ) : null}
        </div>
      )}
    </PanelCard>
  );
}

export function SourceObjectLifecyclePanel({
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
                      <IdChip value={object.source_object_id} className="mt-0.5" />
                    </div>
                    <div className="flex flex-wrap gap-2">
                      <Badge tone={object.state === "active" ? "ok" : "warn"}>{object.state}</Badge>
                      <Badge tone="info">{object.data_class}</Badge>
                    </div>
                  </div>
                  <div className="mt-3 grid gap-x-4 md:grid-cols-2">
                    <KeyValueRow
                      label="Resource"
                      value={`${object.resource_ref?.resource_kind || "—"} / ${object.resource_ref?.resource_id || "—"}`}
                    />
                    <KeyValueRow
                      label="Last seen"
                      value={formatLifecycleTime(object.last_seen_at_ms)}
                    />
                    <KeyValueRow
                      label="Indexed path"
                      value={<span className="break-all">{object.indexed_path}</span>}
                    />
                    <KeyValueRow label="Tier" value={object.tier} />
                  </div>
                  <div className="mt-3 flex flex-wrap gap-2">
                    <button
                      className="tcp-btn"
                      type="button"
                      disabled={isBusy}
                      onClick={() => onReindex(object.source_object_id)}
                    >
                      <Icon name="refresh-cw" />
                      Reindex
                    </button>
                    <button
                      className="tcp-btn"
                      type="button"
                      disabled={isBusy}
                      onClick={() => beginRescope(object)}
                    >
                      <Icon name="move-horizontal" />
                      Re-scope
                    </button>
                    <button
                      className="tcp-btn tcp-btn-danger"
                      type="button"
                      disabled={isBusy}
                      onClick={() => onDelete(object.source_object_id)}
                    >
                      <Icon name="trash-2" />
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
                  <Icon name="shield-check" />
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

export function IngestionJobsPanel({
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

export function IngestionQuarantinesPanel({
  binding,
  rows,
  loading,
  error,
  onReview,
  busyQuarantineId,
}: {
  binding?: EnterpriseSourceBinding | null;
  rows: EnterpriseIngestionQuarantine[];
  loading: boolean;
  error: unknown;
  onReview: (quarantineId: string, disposition: "release" | "delete" | "reindex") => void;
  busyQuarantineId?: string | null;
}) {
  return (
    <PanelCard
      title="Ingestion quarantine"
      subtitle={binding ? binding.source_root_label || binding.binding_id : "All bindings"}
      actions={<Badge tone={error ? "err" : rows.length ? "warn" : "ghost"}>{rows.length}</Badge>}
      fullHeight
    >
      {loading ? (
        <LoadingState title="Loading" text="Reading quarantine records" />
      ) : error ? (
        <EmptyState title="Unavailable" text={errorText(error, "Quarantine could not load.")} />
      ) : rows.length ? (
        <div className="grid gap-2">
          {rows.map((row) => {
            const reviewed = Boolean(row.disposition);
            const busy = busyQuarantineId === row.quarantine_id;
            return (
              <div
                key={row.quarantine_id}
                className="rounded-lg border border-white/8 bg-black/20 p-3"
              >
                <div className="flex flex-wrap items-start justify-between gap-2">
                  <div>
                    <div className="break-all font-medium text-tcp-text-primary">
                      {row.quarantine_id}
                    </div>
                    <div className="tcp-subtle text-xs">
                      {row.connector_id} / {row.binding_id}
                    </div>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <Badge tone={reviewed ? "ok" : "warn"}>{row.disposition || "pending"}</Badge>
                    <Badge tone="info">{row.source_object_ids?.length || 0} objects</Badge>
                  </div>
                </div>
                <div className="mt-3 grid gap-2 text-xs text-tcp-text-secondary md:grid-cols-2">
                  <div>Created: {formatLifecycleTime(row.created_at_ms)}</div>
                  <div>Reviewed: {formatLifecycleTime(row.reviewed_at_ms)}</div>
                  <div className="break-all md:col-span-2">Reason: {row.reason}</div>
                </div>
                <div className="mt-3 flex flex-wrap gap-2">
                  {(["release", "delete", "reindex"] as const).map((disposition) => (
                    <button
                      key={disposition}
                      className="tcp-btn"
                      type="button"
                      disabled={busy || row.disposition === disposition}
                      onClick={() => onReview(row.quarantine_id, disposition)}
                    >
                      {disposition}
                    </button>
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      ) : (
        <EmptyState title="No quarantine" text="Review-required imports will appear here." />
      )}
    </PanelCard>
  );
}

