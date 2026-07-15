import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../../lib/api";

export type PolicyEffect = "allow" | "deny" | "approval_required";
export type PolicyRuleState = "draft" | "published" | "disabled" | "superseded";

export type PolicyCondition = {
  condition_id?: string;
  selector: string;
  value_type: string;
  operator: string;
  operand?: unknown;
};

export type PermissionPredicate = {
  expression_version: "permission_predicates/v1";
  condition?: PolicyCondition;
  all?: PermissionPredicate[];
  any?: PermissionPredicate[];
  not?: PermissionPredicate;
};

export type EnterprisePolicyRule = {
  rule_id: string;
  policy_id: string;
  version: number;
  scope_level: string;
  tenant_context?: unknown;
  org_unit_id?: string;
  resource?: unknown;
  workflow_id?: string;
  workflow_phase?: string;
  permissions?: string[];
  data_classes?: string[];
  tool_patterns?: string[];
  predicate?: PermissionPredicate;
  effect: PolicyEffect;
  state?: PolicyRuleState;
  overridable?: boolean;
  expires_at_ms?: number;
  template_id?: string;
  template_version?: number;
  reason_code: string;
  reason: string;
  approval_id?: string;
  updated_at_ms: number;
};

export type PolicyStarterTemplate = {
  template_id: string;
  version: number;
  display_name: string;
  domain: string;
  description: string;
  default_tool_scope: string[];
  data_constraints: string[];
  receipt_expectations: string[];
  allowed_override_fields: string[];
  rules: EnterprisePolicyRule[];
};

export type PolicyValidationMessage = { path: string; message: string };
export type PolicyValidationResponse = {
  valid: boolean;
  errors: PolicyValidationMessage[];
  warnings: PolicyValidationMessage[];
};

export type PolicyPreviewResponse = {
  snapshot: {
    effect: PolicyEffect;
    reason_code: string;
    reason: string;
    policy_version_id: string;
    decision_source?: { rule_id: string; policy_id: string };
    inherited_sources?: Array<{ rule_id: string; policy_id: string; effect: PolicyEffect }>;
  };
  default_denied: boolean;
  winning_rule_id?: string;
};

export type TemplateRuleOverride = {
  rule_id: string;
  tool_patterns?: string[];
  approval_id?: string;
  expires_at_ms?: number;
  predicate_operands?: Record<string, unknown>;
};

export type TemplateInstantiationResponse = {
  instantiation: {
    instance_id: string;
    template_id: string;
    template_version: number;
    rules: EnterprisePolicyRule[];
    overrides_applied: string[];
  };
  template: PolicyStarterTemplate;
  effective_preview: PolicyPreviewResponse["snapshot"][];
};

const policyKey = ["enterprise", "policies"] as const;

export function useEnterprisePolicies() {
  return useQuery({
    queryKey: policyKey,
    queryFn: () =>
      api("/api/engine/enterprise/policies", { method: "GET" }) as Promise<{
        policy_rules: EnterprisePolicyRule[];
        count: number;
      }>,
    staleTime: 10_000,
  });
}

export function useEnterprisePolicyTemplates() {
  return useQuery({
    queryKey: ["enterprise", "policy-templates"],
    queryFn: () =>
      api("/api/engine/enterprise/policy-templates", { method: "GET" }) as Promise<{
        templates: PolicyStarterTemplate[];
      }>,
    staleTime: 60_000,
  });
}

export function useValidateEnterprisePolicy() {
  return useMutation({
    mutationFn: (rule: EnterprisePolicyRule) =>
      api("/api/engine/enterprise/policies/validate", {
        method: "POST",
        body: JSON.stringify(rule),
      }) as Promise<PolicyValidationResponse>,
  });
}

export function usePreviewEnterprisePolicy() {
  return useMutation({
    mutationFn: (input: Record<string, unknown>) =>
      api("/api/engine/enterprise/policies/preview", {
        method: "POST",
        body: JSON.stringify({ input }),
      }) as Promise<PolicyPreviewResponse>,
  });
}

export function useSaveEnterprisePolicy() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ rule, update }: { rule: EnterprisePolicyRule; update: boolean }) =>
      api(
        update
          ? `/api/engine/enterprise/policies/${encodeURIComponent(rule.rule_id)}`
          : "/api/engine/enterprise/policies",
        {
          method: update ? "PATCH" : "POST",
          body: JSON.stringify(rule),
        }
      ),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: policyKey }),
  });
}

function usePolicyStateMutation(action: "publish" | "disable") {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (policyId: string) =>
      api(`/api/engine/enterprise/policies/${encodeURIComponent(policyId)}/${action}`, {
        method: "POST",
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: policyKey }),
  });
}

export function usePublishEnterprisePolicy() {
  return usePolicyStateMutation("publish");
}

export function useDisableEnterprisePolicy() {
  return usePolicyStateMutation("disable");
}

export function useSupersedeEnterprisePolicy() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ policyId, rules }: { policyId: string; rules: EnterprisePolicyRule[] }) =>
      api(`/api/engine/enterprise/policies/${encodeURIComponent(policyId)}/supersede`, {
        method: "POST",
        body: JSON.stringify({ rules }),
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: policyKey }),
  });
}

export function useInstantiateEnterprisePolicyTemplate() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      templateId,
      instanceId,
      version,
      overrides,
    }: {
      templateId: string;
      instanceId: string;
      version: number;
      overrides: TemplateRuleOverride[];
    }) =>
      api(`/api/engine/enterprise/policy-templates/${encodeURIComponent(templateId)}/instantiate`, {
        method: "POST",
        body: JSON.stringify({ instance_id: instanceId, version, overrides }),
      }) as Promise<TemplateInstantiationResponse>,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: policyKey }),
  });
}

function useTransitionEnterprisePolicyTemplate(action: "upgrade" | "rollback") {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      templateId,
      instanceId,
      version,
    }: {
      templateId: string;
      instanceId: string;
      version: number;
    }) =>
      api(`/api/engine/enterprise/policy-templates/${encodeURIComponent(templateId)}/${action}`, {
        method: "POST",
        body: JSON.stringify({ instance_id: instanceId, version, overrides: [] }),
      }) as Promise<TemplateInstantiationResponse>,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: policyKey }),
  });
}

export function useUpgradeEnterprisePolicyTemplate() {
  return useTransitionEnterprisePolicyTemplate("upgrade");
}

export function useRollbackEnterprisePolicyTemplate() {
  return useTransitionEnterprisePolicyTemplate("rollback");
}
