import { useMemo, useState } from "react";
import { AnimatedPage, Badge, DetailDrawer, PageHeader, StaggerGroup, Toolbar } from "../ui/index.tsx";
import { EnterpriseScopeExplorer } from "../features/enterprise/EnterpriseScopeExplorer";
import {
  useCreateEnterpriseConnector,
  useCreateEnterpriseConnectorCredentialRef,
  useCreateEnterpriseOrgUnit,
  useCreateEnterpriseOrgUnitAccessGrant,
  useCreateEnterpriseOrgUnitMembership,
  useCreateEnterpriseSourceBinding,
  useDeleteEnterpriseSourceObject,
  useEnterpriseConnectorImpact,
  useEnterpriseConnectors,
  useEnterpriseIngestionJobs,
  useEnterpriseIngestionQuarantines,
  useEnterpriseOrgUnitAccessGrants,
  useEnterpriseOrgUnitEffectiveGrants,
  useEnterpriseOrgUnitMemberships,
  useEnterpriseOrgUnits,
  useEnterpriseSourceBindings,
  useEnterpriseSourceObjects,
  useImportEnterpriseGoogleDriveBinding,
  usePreflightEnterpriseGoogleDriveBinding,
  useReindexEnterpriseGoogleDriveBinding,
  useReindexEnterpriseSourceObject,
  useReviewEnterpriseIngestionQuarantine,
  useRescopeEnterpriseSourceObject,
  useRotateEnterpriseConnectorCredentialRef,
  useUpdateEnterpriseConnector,
  useUpdateEnterpriseOrgUnitAccessGrant,
  useUpdateEnterpriseOrgUnitMembership,
  useUpdateEnterpriseSourceBinding,
} from "../features/enterprise/queries";
import type { AppPageProps } from "./pageTypes";
import { GovernanceStatusStrip, compactTenant, errorText, noopStatus } from "./enterprise-admin/shared.tsx";
import {
  ConnectorCredentialRefForm,
  ConnectorForm,
  OrgUnitAccessGrantForm,
  OrgUnitForm,
  OrgUnitMembershipForm,
  SourceBindingForm,
} from "./enterprise-admin/forms.tsx";
import {
  ConnectorImpactPanel,
  ConnectorsPanel,
  GoogleDriveOperationsPanel,
  IngestionJobsPanel,
  IngestionQuarantinesPanel,
  OrgUnitAccessGrantsPanel,
  OrgUnitMembershipsPanel,
  OrgUnitsPanel,
  SourceBindingsPanel,
  SourceObjectLifecyclePanel,
} from "./enterprise-admin/panels.tsx";
import { Icon } from "../ui/Icon";

export function EnterpriseAdminPage({ api, navigate, toast }: AppPageProps) {
  const orgUnits = useEnterpriseOrgUnits();
  const orgUnitMemberships = useEnterpriseOrgUnitMemberships();
  const orgUnitAccessGrants = useEnterpriseOrgUnitAccessGrants();
  const [effectiveMemberId, setEffectiveMemberId] = useState("");
  const effectiveOrgUnitGrants = useEnterpriseOrgUnitEffectiveGrants(
    effectiveMemberId.trim() || null
  );
  const connectors = useEnterpriseConnectors();
  const sourceBindings = useEnterpriseSourceBindings();
  const [selectedBindingId, setSelectedBindingId] = useState<string | null>(null);
  const [selectedConnectorId, setSelectedConnectorId] = useState<string | null>(null);
  // Which creation drawer is open, if any. Only one form is visible at a time,
  // reachable via each monitoring panel's "+ New" action (see TAN-589) instead
  // of all six creation forms being permanently stacked on the page.
  const [activeForm, setActiveForm] = useState<
    | "org-unit"
    | "membership"
    | "access-grant"
    | "connector"
    | "credential-ref"
    | "source-binding"
    | null
  >(null);
  const closeForm = () => setActiveForm(null);
  const createOrgUnit = useCreateEnterpriseOrgUnit();
  const createOrgUnitMembership = useCreateEnterpriseOrgUnitMembership();
  const createOrgUnitAccessGrant = useCreateEnterpriseOrgUnitAccessGrant();
  const updateOrgUnitMembership = useUpdateEnterpriseOrgUnitMembership();
  const updateOrgUnitAccessGrant = useUpdateEnterpriseOrgUnitAccessGrant();
  const createConnector = useCreateEnterpriseConnector();
  const createConnectorCredentialRef = useCreateEnterpriseConnectorCredentialRef();
  const createSourceBinding = useCreateEnterpriseSourceBinding();
  const updateConnector = useUpdateEnterpriseConnector();
  const rotateConnectorCredentialRef = useRotateEnterpriseConnectorCredentialRef();
  const updateSourceBinding = useUpdateEnterpriseSourceBinding();
  const sourceObjects = useEnterpriseSourceObjects(selectedBindingId);
  const connectorImpact = useEnterpriseConnectorImpact(selectedConnectorId);
  const ingestionJobs = useEnterpriseIngestionJobs(selectedBindingId);
  const ingestionQuarantines = useEnterpriseIngestionQuarantines(selectedBindingId);
  const preflightGoogleDrive = usePreflightEnterpriseGoogleDriveBinding();
  const importGoogleDrive = useImportEnterpriseGoogleDriveBinding();
  const reindexGoogleDrive = useReindexEnterpriseGoogleDriveBinding();
  const reindexSourceObject = useReindexEnterpriseSourceObject();
  const reviewIngestionQuarantine = useReviewEnterpriseIngestionQuarantine();
  const deleteSourceObject = useDeleteEnterpriseSourceObject();
  const rescopeSourceObject = useRescopeEnterpriseSourceObject();
  const orgRows = useMemo(() => orgUnits.data?.org_units || [], [orgUnits.data]);
  const membershipRows = useMemo(
    () => orgUnitMemberships.data?.memberships || [],
    [orgUnitMemberships.data]
  );
  const accessGrantRows = useMemo(
    () => orgUnitAccessGrants.data?.access_grants || [],
    [orgUnitAccessGrants.data]
  );
  const effectiveGrantRows = useMemo(
    () => effectiveOrgUnitGrants.data?.grants || [],
    [effectiveOrgUnitGrants.data]
  );
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
  const quarantineRows = useMemo(
    () => ingestionQuarantines.data?.quarantines || [],
    [ingestionQuarantines.data]
  );
  const selectedBinding =
    bindingRows.find((binding) => binding.binding_id === selectedBindingId) || null;
  const drivePreflightPayload =
    preflightGoogleDrive.data?.preflight?.binding_id === selectedBindingId
      ? preflightGoogleDrive.data
      : null;
  const driveImportPayload =
    importGoogleDrive.data?.binding_id === selectedBindingId ? importGoogleDrive.data : null;
  const driveReindexPayload =
    reindexGoogleDrive.data?.binding_id === selectedBindingId ? reindexGoogleDrive.data : null;
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
    orgUnitMemberships.refetch();
    orgUnitAccessGrants.refetch();
    effectiveOrgUnitGrants.refetch();
    connectors.refetch();
    sourceBindings.refetch();
    if (selectedConnectorId) {
      connectorImpact.refetch();
    }
    if (selectedBindingId) {
      sourceObjects.refetch();
    }
    ingestionJobs.refetch();
    ingestionQuarantines.refetch();
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
              <Icon name="refresh-cw" />
              Refresh
            </button>
            <button className="tcp-btn" type="button" onClick={() => navigate("settings")}>
              <Icon name="settings" />
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

        <EnterpriseScopeExplorer
          api={api}
          navigate={navigate}
          orgUnits={orgRows}
          memberships={membershipRows}
          accessGrants={accessGrantRows}
          effectiveGrants={effectiveGrantRows}
          connectors={connectorRows}
          sourceBindings={bindingRows}
          sourceObjects={objectRows}
          loading={orgUnits.isLoading || orgUnitAccessGrants.isLoading || sourceBindings.isLoading}
          error={orgUnits.error || orgUnitAccessGrants.error || sourceBindings.error}
        />

        <DetailDrawer open={activeForm === "org-unit"} title="New org unit" onClose={closeForm}>
          <OrgUnitForm
            busy={createOrgUnit.isPending}
            onCreate={async (input) => {
              try {
                await createOrgUnit.mutateAsync(input);
                toast("ok", "Organization unit created.");
                closeForm();
              } catch (error) {
                toast("err", errorText(error, "Organization unit could not be created."));
              }
            }}
          />
        </DetailDrawer>

        <DetailDrawer
          open={activeForm === "membership"}
          title="Assign membership"
          onClose={closeForm}
        >
          <OrgUnitMembershipForm
            orgUnits={orgRows}
            busy={createOrgUnitMembership.isPending}
            onCreate={async (input) => {
              try {
                await createOrgUnitMembership.mutateAsync(input);
                toast("ok", "Organization membership assigned.");
                closeForm();
              } catch (error) {
                toast("err", errorText(error, "Organization membership could not be assigned."));
              }
            }}
          />
        </DetailDrawer>

        <DetailDrawer
          open={activeForm === "access-grant"}
          title="Grant unit access"
          onClose={closeForm}
        >
          <OrgUnitAccessGrantForm
            orgUnits={orgRows}
            busy={createOrgUnitAccessGrant.isPending}
            onCreate={async (input) => {
              try {
                await createOrgUnitAccessGrant.mutateAsync(input);
                toast("ok", "Organization unit access granted.");
                closeForm();
              } catch (error) {
                toast("err", errorText(error, "Organization unit access could not be granted."));
              }
            }}
          />
        </DetailDrawer>

        <DetailDrawer open={activeForm === "connector"} title="New connector" onClose={closeForm}>
          <ConnectorForm
            busy={createConnector.isPending}
            onCreate={async (input) => {
              try {
                await createConnector.mutateAsync(input);
                toast("ok", "Connector created.");
                closeForm();
              } catch (error) {
                toast("err", errorText(error, "Connector could not be created."));
              }
            }}
          />
        </DetailDrawer>

        <DetailDrawer
          open={activeForm === "credential-ref"}
          title="Attach credential reference"
          onClose={closeForm}
        >
          <ConnectorCredentialRefForm
            tenantPayload={payload}
            connectors={connectorRows}
            busy={createConnectorCredentialRef.isPending || rotateConnectorCredentialRef.isPending}
            onCreate={async (input) => {
              try {
                await createConnectorCredentialRef.mutateAsync(input);
                toast("ok", "Credential reference attached.");
                closeForm();
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
        </DetailDrawer>

        <DetailDrawer
          open={activeForm === "source-binding"}
          title="New source binding"
          onClose={closeForm}
        >
          <SourceBindingForm
            tenantPayload={payload}
            busy={createSourceBinding.isPending}
            onCreate={async (input) => {
              try {
                await createSourceBinding.mutateAsync(input);
                toast("ok", "Source binding created.");
                closeForm();
              } catch (error) {
                toast("err", errorText(error, "Source binding could not be created."));
              }
            }}
          />
        </DetailDrawer>

        <div className="grid gap-4 xl:grid-cols-4">
          <OrgUnitsPanel
            rows={orgRows}
            loading={orgUnits.isLoading}
            error={orgUnits.error}
            onCreateNew={() => setActiveForm("org-unit")}
          />
          <OrgUnitMembershipsPanel
            rows={membershipRows}
            loading={orgUnitMemberships.isLoading}
            error={orgUnitMemberships.error}
            busyMembershipId={
              updateOrgUnitMembership.isPending
                ? updateOrgUnitMembership.variables?.membership_id || null
                : null
            }
            onSetState={(membershipId, state) => {
              updateOrgUnitMembership
                .mutateAsync({ membership_id: membershipId, state })
                .then(() => toast("ok", `Membership ${state}.`))
                .catch((error) =>
                  toast("err", errorText(error, "Organization membership could not be updated."))
                );
            }}
            onCreateNew={() => setActiveForm("membership")}
          />
          <OrgUnitAccessGrantsPanel
            rows={accessGrantRows}
            effectiveRows={effectiveGrantRows}
            loading={orgUnitAccessGrants.isLoading}
            error={orgUnitAccessGrants.error}
            effectiveMemberId={effectiveMemberId}
            onEffectiveMemberId={setEffectiveMemberId}
            busyGrantId={
              updateOrgUnitAccessGrant.isPending
                ? updateOrgUnitAccessGrant.variables?.grant_id || null
                : null
            }
            onSetState={(grantId, state) => {
              updateOrgUnitAccessGrant
                .mutateAsync({ grant_id: grantId, state })
                .then(() => toast("ok", `Access grant ${state}.`))
                .catch((error) =>
                  toast("err", errorText(error, "Organization unit access could not be updated."))
                );
            }}
            onCreateNew={() => setActiveForm("access-grant")}
          />
          <ConnectorsPanel
            rows={connectorRows}
            loading={connectors.isLoading}
            error={connectors.error}
            selectedConnectorId={selectedConnectorId}
            onSelectImpact={setSelectedConnectorId}
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
            onCreateNew={() => setActiveForm("connector")}
            onCreateCredentialRef={() => setActiveForm("credential-ref")}
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
            onCreateNew={() => setActiveForm("source-binding")}
          />
        </div>

        <ConnectorImpactPanel
          connectorId={selectedConnectorId}
          payload={connectorImpact.data}
          loading={connectorImpact.isLoading}
          error={connectorImpact.error}
        />

        <GoogleDriveOperationsPanel
          binding={selectedBinding}
          preflightPayload={drivePreflightPayload}
          importPayload={driveImportPayload}
          reindexPayload={driveReindexPayload}
          preflightBusy={preflightGoogleDrive.isPending}
          importBusy={importGoogleDrive.isPending}
          reindexBusy={reindexGoogleDrive.isPending}
          preflightError={preflightGoogleDrive.error}
          importError={importGoogleDrive.error}
          reindexError={reindexGoogleDrive.error}
          onPreflight={() => {
            if (!selectedBindingId) return;
            preflightGoogleDrive
              .mutateAsync(selectedBindingId)
              .then((payload) =>
                toast(
                  "ok",
                  `Google Drive preflight found ${payload.preflight?.file_count || 0} files.`
                )
              )
              .catch((error) => toast("err", errorText(error, "Google Drive preflight failed.")));
          }}
          onImport={(input) => {
            if (!selectedBindingId) return;
            importGoogleDrive
              .mutateAsync({ binding_id: selectedBindingId, ...input })
              .then((payload) => {
                sourceObjects.refetch();
                ingestionJobs.refetch();
                ingestionQuarantines.refetch();
                if (selectedConnectorId) connectorImpact.refetch();
                toast("ok", `Google Drive import ${payload.ingestion_job?.state || "queued"}.`);
              })
              .catch((error) => toast("err", errorText(error, "Google Drive import failed.")));
          }}
          onReindexBinding={(input) => {
            if (!selectedBindingId) return;
            reindexGoogleDrive
              .mutateAsync({ binding_id: selectedBindingId, ...input })
              .then((payload) => {
                sourceObjects.refetch();
                ingestionJobs.refetch();
                ingestionQuarantines.refetch();
                if (selectedConnectorId) connectorImpact.refetch();
                toast("ok", `Google Drive reindex ${payload.ingestion_job?.state || "queued"}.`);
              })
              .catch((error) => toast("err", errorText(error, "Google Drive reindex failed.")));
          }}
        />

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

        <IngestionQuarantinesPanel
          binding={selectedBinding}
          rows={quarantineRows}
          loading={ingestionQuarantines.isLoading}
          error={ingestionQuarantines.error}
          busyQuarantineId={
            reviewIngestionQuarantine.isPending
              ? reviewIngestionQuarantine.variables?.quarantine_id || null
              : null
          }
          onReview={(quarantineId, disposition) => {
            reviewIngestionQuarantine
              .mutateAsync({ quarantine_id: quarantineId, disposition })
              .then(() => toast("ok", `Quarantine marked ${disposition}.`))
              .catch((error) =>
                toast("err", errorText(error, "Quarantine could not be reviewed."))
              );
          }}
        />
      </StaggerGroup>
    </AnimatedPage>
  );
}
