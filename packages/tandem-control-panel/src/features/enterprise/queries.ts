import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { api, isTransientEngineError } from "../../lib/api";

export type EnterpriseTenantContext = {
  org_id?: string;
  workspace_id?: string;
  deployment_id?: string | null;
  actor_id?: string | null;
  source?: string;
};

export type EnterpriseRequestPrincipal = {
  actor_id?: string | null;
  source?: string;
};

export type EnterpriseNoopBase = {
  tenant_context?: EnterpriseTenantContext;
  request_principal?: EnterpriseRequestPrincipal;
  bridge_state?: string;
  status?: string;
  message?: string;
};

export type EnterpriseOrganizationUnit = {
  unit_id: string;
  taxonomy_id?: string;
  display_name: string;
  kind?: string;
  parent_unit_id?: string | null;
  state?: string;
  description?: string | null;
  labels?: string[];
};

export type EnterpriseResourceRef = {
  organization_id: string;
  workspace_id: string;
  project_id?: string | null;
  resource_kind: string;
  resource_id: string;
  parent_path?: unknown[];
  branch_id?: string | null;
  path_prefix?: string | null;
};

export type EnterpriseSecretRef = {
  org_id: string;
  workspace_id: string;
  provider: string;
  secret_id: string;
  name: string;
};

export type EnterpriseIngestionPolicy = {
  allow_indexing?: boolean;
  allow_prompt_context?: boolean;
  require_review?: boolean;
  max_depth?: number | null;
};

export type EnterpriseSourceBinding = {
  binding_id: string;
  connector_id: string;
  source_type: string;
  native_source_id: string;
  source_root_label?: string | null;
  resource_ref: EnterpriseResourceRef;
  data_class: string;
  state?: string;
  credential_ref_id?: string | null;
  ingestion_policy?: EnterpriseIngestionPolicy;
};

export type EnterpriseConnectorInstance = {
  connector_id: string;
  tenant_context?: EnterpriseTenantContext;
  provider: string;
  display_name?: string | null;
  state?: string;
  credential_refs?: EnterpriseConnectorCredentialRef[];
  created_at_ms?: number;
  updated_at_ms?: number;
};

export type EnterpriseConnectorCredentialRef = {
  org_id: string;
  workspace_id: string;
  connector_id: string;
  credential_id: string;
  credential_class?: string;
  secret_ref: EnterpriseSecretRef;
  source_bound_resource?: EnterpriseResourceRef | null;
  created_at_ms?: number;
  rotated_at_ms?: number | null;
  expires_at_ms?: number | null;
};

export type EnterpriseOrgUnitsResponse = EnterpriseNoopBase & {
  org_units?: EnterpriseOrganizationUnit[];
  count?: number;
};

export type EnterpriseConnectorsResponse = EnterpriseNoopBase & {
  connectors?: EnterpriseConnectorInstance[];
  count?: number;
};

export type EnterpriseSourceBindingsResponse = EnterpriseNoopBase & {
  source_bindings?: EnterpriseSourceBinding[];
  count?: number;
};

export type EnterpriseSourceObjectLifecycle = {
  source_object_id: string;
  source_binding_id: string;
  connector_id: string;
  state: string;
  tier: string;
  session_id?: string | null;
  project_id?: string | null;
  import_namespace: string;
  indexed_path: string;
  native_object_id: string;
  resource_ref: EnterpriseResourceRef;
  data_class: string;
  content_hash?: string | null;
  source_hash?: string | null;
  first_seen_at_ms: number;
  last_seen_at_ms: number;
  tombstoned_at_ms?: number | null;
  metadata?: unknown;
};

export type EnterpriseIngestionJob = {
  job_id: string;
  tenant_context?: EnterpriseTenantContext;
  connector_id: string;
  binding_id: string;
  state?: string;
  source_object_ids?: string[];
  started_at_ms?: number | null;
  finished_at_ms?: number | null;
  quarantine_id?: string | null;
};

export type EnterpriseSourceObjectsResponse = EnterpriseNoopBase & {
  source_objects?: EnterpriseSourceObjectLifecycle[];
  count?: number;
};

export type EnterpriseIngestionJobsResponse = EnterpriseNoopBase & {
  ingestion_jobs?: EnterpriseIngestionJob[];
  count?: number;
};

export type EnterpriseSourceObjectActionResponse = EnterpriseNoopBase & {
  action?: string;
  source_object?: EnterpriseSourceObjectLifecycle | null;
  chunks_deleted?: number;
  bytes_estimated?: number;
  import_index_deleted?: boolean;
};

export type CreateEnterpriseOrganizationUnitInput = {
  unit_id: string;
  display_name: string;
  taxonomy_id?: string;
  kind?: string;
  parent_unit_id?: string;
  description?: string;
  labels?: string[];
};

export type CreateEnterpriseSourceBindingInput = {
  binding_id: string;
  connector_id: string;
  source_type: string;
  native_source_id: string;
  source_root_label?: string;
  resource_ref: EnterpriseResourceRef;
  data_class: string;
  credential_ref_id?: string;
  ingestion_policy?: EnterpriseIngestionPolicy;
};

export type CreateEnterpriseConnectorInput = {
  connector_id: string;
  provider: string;
  display_name?: string;
  state?: string;
};

export type UpdateEnterpriseConnectorInput = {
  connector_id: string;
  display_name?: string;
  state?: string;
};

export type CreateEnterpriseConnectorCredentialRefInput = {
  connector_id: string;
  credential_id: string;
  credential_class?: string;
  secret_ref: EnterpriseSecretRef;
  source_bound_resource?: EnterpriseResourceRef;
  expires_at_ms?: number;
};

export type RotateEnterpriseConnectorCredentialRefInput = {
  connector_id: string;
  credential_id: string;
  secret_ref: EnterpriseSecretRef;
  expires_at_ms?: number;
};

export type UpdateEnterpriseSourceBindingInput = {
  binding_id: string;
  state?: string;
  source_root_label?: string;
  credential_ref_id?: string;
  ingestion_policy?: EnterpriseIngestionPolicy;
};

export type EnterpriseSourceObjectActionInput = {
  binding_id: string;
  source_object_id: string;
};

export type RescopeEnterpriseSourceObjectInput = EnterpriseSourceObjectActionInput & {
  resource_ref: EnterpriseResourceRef;
  data_class: string;
};

const retryEnterpriseQuery = (failureCount: number, error: unknown) =>
  isTransientEngineError(error) ? failureCount < 6 : failureCount < 2;

export function useEnterpriseOrgUnits(enabled = true) {
  return useQuery({
    queryKey: ["enterprise", "org-units"],
    queryFn: () =>
      api("/api/engine/enterprise/org-units", {
        method: "GET",
      }) as Promise<EnterpriseOrgUnitsResponse>,
    enabled,
    staleTime: 15000,
    retry: retryEnterpriseQuery,
  });
}

export function useEnterpriseSourceBindings(enabled = true) {
  return useQuery({
    queryKey: ["enterprise", "source-bindings"],
    queryFn: () =>
      api("/api/engine/enterprise/source-bindings", {
        method: "GET",
      }) as Promise<EnterpriseSourceBindingsResponse>,
    enabled,
    staleTime: 15000,
    retry: retryEnterpriseQuery,
  });
}

export function useEnterpriseConnectors(enabled = true) {
  return useQuery({
    queryKey: ["enterprise", "connectors"],
    queryFn: () =>
      api("/api/engine/enterprise/connectors", {
        method: "GET",
      }) as Promise<EnterpriseConnectorsResponse>,
    enabled,
    staleTime: 15000,
    retry: retryEnterpriseQuery,
  });
}

export function useEnterpriseSourceObjects(bindingId?: string | null, enabled = true) {
  return useQuery({
    queryKey: ["enterprise", "source-objects", bindingId || ""],
    queryFn: () =>
      api(
        `/api/engine/enterprise/source-bindings/${encodeURIComponent(
          bindingId || ""
        )}/source-objects`,
        {
          method: "GET",
        }
      ) as Promise<EnterpriseSourceObjectsResponse>,
    enabled: enabled && Boolean(bindingId),
    staleTime: 15000,
    retry: retryEnterpriseQuery,
  });
}

export function useEnterpriseIngestionJobs(bindingId?: string | null, enabled = true) {
  const params = bindingId ? `?binding_id=${encodeURIComponent(bindingId)}` : "";
  return useQuery({
    queryKey: ["enterprise", "ingestion-jobs", bindingId || ""],
    queryFn: () =>
      api(`/api/engine/enterprise/ingestion-jobs${params}`, {
        method: "GET",
      }) as Promise<EnterpriseIngestionJobsResponse>,
    enabled,
    staleTime: 15000,
    retry: retryEnterpriseQuery,
  });
}

export function useCreateEnterpriseOrgUnit() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: CreateEnterpriseOrganizationUnitInput) =>
      api("/api/engine/enterprise/org-units", {
        method: "POST",
        body: JSON.stringify(input),
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["enterprise", "org-units"] });
    },
  });
}

function invalidateConnectorQueries(queryClient: QueryClient) {
  queryClient.invalidateQueries({ queryKey: ["enterprise", "connectors"] });
  queryClient.invalidateQueries({ queryKey: ["enterprise", "source-bindings"] });
  queryClient.invalidateQueries({ queryKey: ["enterprise", "source-objects"] });
}

export function useCreateEnterpriseConnector() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: CreateEnterpriseConnectorInput) =>
      api("/api/engine/enterprise/connectors", {
        method: "POST",
        body: JSON.stringify(input),
      }) as Promise<EnterpriseConnectorsResponse>,
    onSuccess: () => {
      invalidateConnectorQueries(queryClient);
    },
  });
}

export function useUpdateEnterpriseConnector() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ connector_id, ...input }: UpdateEnterpriseConnectorInput) =>
      api(`/api/engine/enterprise/connectors/${encodeURIComponent(connector_id)}`, {
        method: "PATCH",
        body: JSON.stringify(input),
      }) as Promise<EnterpriseConnectorsResponse>,
    onSuccess: () => {
      invalidateConnectorQueries(queryClient);
    },
  });
}

export function useCreateEnterpriseConnectorCredentialRef() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ connector_id, ...input }: CreateEnterpriseConnectorCredentialRefInput) =>
      api(`/api/engine/enterprise/connectors/${encodeURIComponent(connector_id)}/credential-refs`, {
        method: "POST",
        body: JSON.stringify(input),
      }) as Promise<EnterpriseConnectorsResponse>,
    onSuccess: () => {
      invalidateConnectorQueries(queryClient);
    },
  });
}

export function useRotateEnterpriseConnectorCredentialRef() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      connector_id,
      credential_id,
      ...input
    }: RotateEnterpriseConnectorCredentialRefInput) =>
      api(
        `/api/engine/enterprise/connectors/${encodeURIComponent(
          connector_id
        )}/credential-refs/${encodeURIComponent(credential_id)}/rotate`,
        {
          method: "PATCH",
          body: JSON.stringify(input),
        }
      ) as Promise<EnterpriseConnectorsResponse>,
    onSuccess: () => {
      invalidateConnectorQueries(queryClient);
    },
  });
}

function invalidateSourceObjectQueries(queryClient: QueryClient, bindingId: string) {
  queryClient.invalidateQueries({ queryKey: ["enterprise", "source-objects", bindingId] });
  queryClient.invalidateQueries({ queryKey: ["enterprise", "source-bindings"] });
}

export function useCreateEnterpriseSourceBinding() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: CreateEnterpriseSourceBindingInput) =>
      api("/api/engine/enterprise/source-bindings", {
        method: "POST",
        body: JSON.stringify(input),
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["enterprise", "source-bindings"] });
    },
  });
}

export function useUpdateEnterpriseSourceBinding() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ binding_id, ...input }: UpdateEnterpriseSourceBindingInput) =>
      api(`/api/engine/enterprise/source-bindings/${encodeURIComponent(binding_id)}`, {
        method: "PATCH",
        body: JSON.stringify(input),
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["enterprise", "source-bindings"] });
    },
  });
}

export function useReindexEnterpriseSourceObject() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ binding_id, source_object_id }: EnterpriseSourceObjectActionInput) =>
      api(
        `/api/engine/enterprise/source-bindings/${encodeURIComponent(
          binding_id
        )}/source-objects/${encodeURIComponent(source_object_id)}/reindex`,
        {
          method: "POST",
        }
      ) as Promise<EnterpriseSourceObjectActionResponse>,
    onSuccess: (_data, variables) => {
      invalidateSourceObjectQueries(queryClient, variables.binding_id);
    },
  });
}

export function useDeleteEnterpriseSourceObject() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ binding_id, source_object_id }: EnterpriseSourceObjectActionInput) =>
      api(
        `/api/engine/enterprise/source-bindings/${encodeURIComponent(
          binding_id
        )}/source-objects/${encodeURIComponent(source_object_id)}`,
        {
          method: "DELETE",
        }
      ) as Promise<EnterpriseSourceObjectActionResponse>,
    onSuccess: (_data, variables) => {
      invalidateSourceObjectQueries(queryClient, variables.binding_id);
    },
  });
}

export function useRescopeEnterpriseSourceObject() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      binding_id,
      source_object_id,
      resource_ref,
      data_class,
    }: RescopeEnterpriseSourceObjectInput) =>
      api(
        `/api/engine/enterprise/source-bindings/${encodeURIComponent(
          binding_id
        )}/source-objects/${encodeURIComponent(source_object_id)}/scope`,
        {
          method: "PATCH",
          body: JSON.stringify({ resource_ref, data_class }),
        }
      ) as Promise<EnterpriseSourceObjectActionResponse>,
    onSuccess: (_data, variables) => {
      invalidateSourceObjectQueries(queryClient, variables.binding_id);
    },
  });
}
