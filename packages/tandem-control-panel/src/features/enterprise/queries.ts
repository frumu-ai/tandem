import { useQuery } from "@tanstack/react-query";
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

export type EnterpriseOrgUnitsResponse = EnterpriseNoopBase & {
  org_units?: any[];
  count?: number;
};

export type EnterpriseSourceBindingsResponse = EnterpriseNoopBase & {
  source_bindings?: any[];
  count?: number;
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
