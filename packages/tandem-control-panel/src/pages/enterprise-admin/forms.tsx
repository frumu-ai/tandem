import { useState } from "react";
import { PanelCard } from "../../ui/index.tsx";
import {
  Field,
  ACCESS_PERMISSIONS,
  CONNECTOR_STATES,
  CREDENTIAL_CLASSES,
  DATA_CLASSES,
  MEMBER_KINDS,
  MEMBERSHIP_SOURCES,
  ORG_UNIT_KINDS,
  RESOURCE_KINDS,
  tenantOrg,
  tenantWorkspace,
} from "./shared.tsx";
import type {
  CreateEnterpriseConnectorCredentialRefInput,
  CreateEnterpriseConnectorInput,
  CreateEnterpriseOrganizationUnitAccessGrantInput,
  CreateEnterpriseOrganizationUnitInput,
  CreateEnterpriseOrganizationUnitMembershipInput,
  CreateEnterpriseSourceBindingInput,
  EnterpriseConnectorInstance,
  EnterpriseNoopBase,
  EnterpriseOrganizationUnit,
  RotateEnterpriseConnectorCredentialRefInput,
} from "../../features/enterprise/queries";

export function ConnectorForm({
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

export function ConnectorCredentialRefForm({
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

export function OrgUnitForm({
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

export function OrgUnitMembershipForm({
  orgUnits,
  onCreate,
  busy,
}: {
  orgUnits: EnterpriseOrganizationUnit[];
  onCreate: (input: CreateEnterpriseOrganizationUnitMembershipInput) => Promise<void>;
  busy: boolean;
}) {
  const [unitKey, setUnitKey] = useState("");
  const [memberKind, setMemberKind] = useState("human_user");
  const [memberId, setMemberId] = useState("");
  const [source, setSource] = useState("hosted_control_plane");
  const [expiresAt, setExpiresAt] = useState("");
  const selectedUnitKey =
    unitKey ||
    (orgUnits[0] ? `${orgUnits[0].taxonomy_id || "organization_unit"}/${orgUnits[0].unit_id}` : "");
  const [taxonomyId, unitId] = selectedUnitKey.split("/");

  return (
    <PanelCard title="Assign member" subtitle="Hosted org-unit membership">
      <form
        className="grid gap-3"
        onSubmit={async (event) => {
          event.preventDefault();
          await onCreate({
            taxonomy_id: taxonomyId,
            unit_id: unitId,
            member_kind: memberKind,
            member_id: memberId.trim(),
            source,
            expires_at_ms: expiresAt ? new Date(expiresAt).getTime() : undefined,
          });
          setMemberId("");
          setExpiresAt("");
        }}
      >
        <div className="grid gap-3 md:grid-cols-2">
          <Field label="Org Unit">
            <select
              className="tcp-select"
              value={selectedUnitKey}
              onChange={(event) => setUnitKey(event.currentTarget.value)}
              required
            >
              {orgUnits.length ? (
                orgUnits.map((unit) => {
                  const key = `${unit.taxonomy_id || "organization_unit"}/${unit.unit_id}`;
                  return (
                    <option key={key} value={key}>
                      {unit.display_name} ({key})
                    </option>
                  );
                })
              ) : (
                <option value="">create org unit first</option>
              )}
            </select>
          </Field>
          <Field label="Member Kind">
            <select
              className="tcp-select"
              value={memberKind}
              onChange={(event) => setMemberKind(event.currentTarget.value)}
            >
              {MEMBER_KINDS.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Member ID">
            <input
              className="tcp-input"
              value={memberId}
              onInput={(event) => setMemberId(event.currentTarget.value)}
              placeholder="user@company.com"
              required
            />
          </Field>
          <Field label="Source">
            <select
              className="tcp-select"
              value={source}
              onChange={(event) => setSource(event.currentTarget.value)}
            >
              {MEMBERSHIP_SOURCES.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Expires">
            <input
              className="tcp-input"
              type="datetime-local"
              value={expiresAt}
              onInput={(event) => setExpiresAt(event.currentTarget.value)}
            />
          </Field>
        </div>
        <div className="flex justify-end">
          <button className="tcp-btn tcp-btn-primary" type="submit" disabled={busy || !unitId}>
            <i data-lucide="user-plus"></i>
            {busy ? "Assigning" : "Assign member"}
          </button>
        </div>
      </form>
    </PanelCard>
  );
}

export function OrgUnitAccessGrantForm({
  orgUnits,
  onCreate,
  busy,
}: {
  orgUnits: EnterpriseOrganizationUnit[];
  onCreate: (input: CreateEnterpriseOrganizationUnitAccessGrantInput) => Promise<void>;
  busy: boolean;
}) {
  const [unitKey, setUnitKey] = useState("");
  const [grantId, setGrantId] = useState("");
  const [resourceKind, setResourceKind] = useState("data_store");
  const [resourceId, setResourceId] = useState("");
  const [effect, setEffect] = useState("allow");
  const [permissions, setPermissions] = useState(["view", "read"]);
  const [dataClasses, setDataClasses] = useState(["internal"]);
  const [expiresAt, setExpiresAt] = useState("");
  const selectedUnitKey =
    unitKey ||
    (orgUnits[0] ? `${orgUnits[0].taxonomy_id || "organization_unit"}/${orgUnits[0].unit_id}` : "");
  const [taxonomyId, unitId] = selectedUnitKey.split("/");

  const toggle = (value: string, rows: string[], setRows: (rows: string[]) => void) => {
    setRows(rows.includes(value) ? rows.filter((row) => row !== value) : [...rows, value]);
  };

  return (
    <PanelCard title="Grant unit access" subtitle="Org unit to resource grants">
      <form
        className="grid gap-3"
        onSubmit={async (event) => {
          event.preventDefault();
          await onCreate({
            grant_id: grantId.trim() || undefined,
            taxonomy_id: taxonomyId,
            unit_id: unitId,
            resource_kind: resourceKind,
            resource_id: resourceId.trim(),
            effect,
            permissions,
            data_classes: dataClasses,
            expires_at_ms: expiresAt ? new Date(expiresAt).getTime() : undefined,
          });
          setGrantId("");
          setResourceId("");
          setExpiresAt("");
        }}
      >
        <div className="grid gap-3 md:grid-cols-2">
          <Field label="Org Unit">
            <select
              className="tcp-select"
              value={selectedUnitKey}
              onChange={(event) => setUnitKey(event.currentTarget.value)}
              required
            >
              {orgUnits.length ? (
                orgUnits.map((unit) => {
                  const key = `${unit.taxonomy_id || "organization_unit"}/${unit.unit_id}`;
                  return (
                    <option key={key} value={key}>
                      {unit.display_name} ({key})
                    </option>
                  );
                })
              ) : (
                <option value="">create org unit first</option>
              )}
            </select>
          </Field>
          <Field label="Grant ID">
            <input
              className="tcp-input"
              value={grantId}
              onInput={(event) => setGrantId(event.currentTarget.value)}
              placeholder="grant-doctors-patient-cases"
            />
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
              placeholder="patient-cases"
              required
            />
          </Field>
          <Field label="Effect">
            <select
              className="tcp-select"
              value={effect}
              onChange={(event) => setEffect(event.currentTarget.value)}
            >
              <option value="allow">allow</option>
              <option value="deny">deny</option>
            </select>
          </Field>
          <Field label="Expires">
            <input
              className="tcp-input"
              type="datetime-local"
              value={expiresAt}
              onInput={(event) => setExpiresAt(event.currentTarget.value)}
            />
          </Field>
        </div>
        <div className="grid gap-3 md:grid-cols-2">
          <Field label="Permissions">
            <div className="flex flex-wrap gap-2">
              {ACCESS_PERMISSIONS.map((option) => (
                <label
                  key={option}
                  className="flex items-center gap-1 text-xs text-tcp-text-secondary"
                >
                  <input
                    type="checkbox"
                    checked={permissions.includes(option)}
                    onChange={() => toggle(option, permissions, setPermissions)}
                  />
                  {option}
                </label>
              ))}
            </div>
          </Field>
          <Field label="Data Classes">
            <div className="flex flex-wrap gap-2">
              {DATA_CLASSES.map((option) => (
                <label
                  key={option}
                  className="flex items-center gap-1 text-xs text-tcp-text-secondary"
                >
                  <input
                    type="checkbox"
                    checked={dataClasses.includes(option)}
                    onChange={() => toggle(option, dataClasses, setDataClasses)}
                  />
                  {option}
                </label>
              ))}
            </div>
          </Field>
        </div>
        <div className="flex justify-end">
          <button
            className="tcp-btn tcp-btn-primary"
            type="submit"
            disabled={busy || !unitId || !permissions.length || !dataClasses.length}
          >
            <i data-lucide="shield-plus"></i>
            {busy ? "Granting" : "Create grant"}
          </button>
        </div>
      </form>
    </PanelCard>
  );
}

export function SourceBindingForm({
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
