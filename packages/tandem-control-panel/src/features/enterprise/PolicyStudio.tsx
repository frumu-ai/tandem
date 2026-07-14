import { useMemo, useState } from "react";
import { Badge, EmptyState, PanelCard, Toolbar } from "../../ui/index.tsx";
import { Field } from "../../pages/enterprise-admin/shared.tsx";
import {
  activePolicyRulesForSupersede,
  buildPolicyPreviewArguments,
  buildTemplatePredicateOverrides,
  parsePolicyOperand,
  preservedPolicyRuleMetadata,
  splitPolicyList,
} from "../../../lib/enterprise/policy-authoring.js";
import type { EnterpriseTenantContext } from "./queries";
import {
  type EnterprisePolicyRule,
  type PolicyEffect,
  type PolicyStarterTemplate,
  useDisableEnterprisePolicy,
  useEnterprisePolicies,
  useEnterprisePolicyTemplates,
  useInstantiateEnterprisePolicyTemplate,
  usePreviewEnterprisePolicy,
  usePublishEnterprisePolicy,
  useRollbackEnterprisePolicyTemplate,
  useSaveEnterprisePolicy,
  useSupersedeEnterprisePolicy,
  useUpgradeEnterprisePolicyTemplate,
  useValidateEnterprisePolicy,
} from "./policyQueries";

const valueTypes = [
  "string",
  "boolean",
  "integer",
  "decimal",
  "currency_code",
  "host",
  "email_domain",
  "path",
  "repository",
  "array_length",
  "exists",
];

const operators = [
  "equals",
  "not_equals",
  "in",
  "not_in",
  "starts_with",
  "ends_with",
  "less_than",
  "less_than_or_equal",
  "greater_than",
  "greater_than_or_equal",
  "is_subdomain_of",
  "not_subdomain_of",
  "within",
  "not_within",
  "owner_equals",
  "name_equals",
  "exists",
  "not_exists",
];

const dataClasses = [
  "public",
  "internal",
  "confidential",
  "restricted",
  "customer_data",
  "source_code",
  "financial_record",
  "credential",
  "regulated",
  "executive",
];

type RuleDraft = {
  ruleId: string;
  policyId: string;
  scopeLevel: string;
  orgUnitId: string;
  resourceKind: string;
  resourceId: string;
  workflowId: string;
  workflowPhase: string;
  effect: PolicyEffect;
  toolPatterns: string;
  selectedDataClasses: string[];
  overridable: boolean;
  expiresAt: string;
  approvalId: string;
  reasonCode: string;
  reason: string;
  predicateEnabled: boolean;
  conditionId: string;
  selector: string;
  valueType: string;
  operator: string;
  operand: string;
};

const emptyDraft = (): RuleDraft => ({
  ruleId: "",
  policyId: "",
  scopeLevel: "tenant",
  orgUnitId: "",
  resourceKind: "repository",
  resourceId: "",
  workflowId: "",
  workflowPhase: "",
  effect: "deny",
  toolPatterns: "",
  selectedDataClasses: [],
  overridable: false,
  expiresAt: "",
  approvalId: "",
  reasonCode: "",
  reason: "",
  predicateEnabled: false,
  conditionId: "condition-1",
  selector: "/target",
  valueType: "string",
  operator: "equals",
  operand: "",
});

function buildRule(
  draft: RuleDraft,
  tenant?: EnterpriseTenantContext,
  existing?: EnterprisePolicyRule
): EnterprisePolicyRule {
  const preserved = preservedPolicyRuleMetadata(existing);
  const rule: EnterprisePolicyRule = {
    rule_id: draft.ruleId.trim(),
    policy_id: preserved.policy_id || draft.policyId.trim(),
    version: preserved.version ?? 1,
    scope_level: draft.scopeLevel,
    effect: draft.effect,
    tool_patterns: splitPolicyList(draft.toolPatterns),
    data_classes: draft.selectedDataClasses,
    overridable: draft.overridable,
    reason_code: draft.reasonCode.trim() || `policy_${draft.effect}`,
    reason: draft.reason.trim() || `Policy resolves to ${draft.effect}`,
    updated_at_ms: Date.now(),
    ...(preserved.template_id ? { template_id: preserved.template_id } : {}),
    ...(preserved.template_version == null
      ? {}
      : { template_version: preserved.template_version }),
  };
  if (draft.scopeLevel === "org_unit") rule.org_unit_id = draft.orgUnitId.trim();
  if (draft.scopeLevel === "resource") {
    rule.resource = {
      organization_id: tenant?.org_id || "local",
      workspace_id: tenant?.workspace_id || "local",
      resource_kind: draft.resourceKind,
      resource_id: draft.resourceId.trim(),
    };
  }
  if (["workflow", "phase"].includes(draft.scopeLevel)) {
    rule.workflow_id = draft.workflowId.trim();
  }
  if (draft.scopeLevel === "phase") rule.workflow_phase = draft.workflowPhase.trim();
  if (draft.approvalId.trim()) rule.approval_id = draft.approvalId.trim();
  if (draft.expiresAt) rule.expires_at_ms = new Date(draft.expiresAt).getTime();
  if (draft.predicateEnabled) {
    rule.predicate = {
      expression_version: "permission_predicates/v1",
      condition: {
        condition_id: draft.conditionId.trim() || undefined,
        selector: draft.selector.trim(),
        value_type: draft.valueType,
        operator: draft.operator,
        operand: parsePolicyOperand(draft.operand, draft.operator, draft.valueType),
      },
    };
  }
  return rule;
}

function draftFromRule(rule: EnterprisePolicyRule): RuleDraft {
  const condition = rule.predicate?.condition;
  const resource = (rule.resource || {}) as Record<string, unknown>;
  return {
    ruleId: rule.rule_id,
    policyId: rule.policy_id,
    scopeLevel: rule.scope_level,
    orgUnitId: rule.org_unit_id || "",
    resourceKind: String(resource.resource_kind || "repository"),
    resourceId: String(resource.resource_id || ""),
    workflowId: rule.workflow_id || "",
    workflowPhase: rule.workflow_phase || "",
    effect: rule.effect,
    toolPatterns: (rule.tool_patterns || []).join(", "),
    selectedDataClasses: rule.data_classes || [],
    overridable: Boolean(rule.overridable),
    expiresAt: rule.expires_at_ms ? new Date(rule.expires_at_ms).toISOString().slice(0, 16) : "",
    approvalId: rule.approval_id || "",
    reasonCode: rule.reason_code,
    reason: rule.reason,
    predicateEnabled: Boolean(condition),
    conditionId: condition?.condition_id || "condition-1",
    selector: condition?.selector || "/target",
    valueType: condition?.value_type || "string",
    operator: condition?.operator || "equals",
    operand: Array.isArray(condition?.operand)
      ? condition.operand.join(", ")
      : condition?.operand == null
        ? ""
        : String(condition.operand),
  };
}

function tone(effect: string) {
  if (effect === "allow" || effect === "published") return "ok" as const;
  if (effect === "deny" || effect === "disabled") return "err" as const;
  return "warn" as const;
}

function policyErrorMessages(error: unknown): string[] {
  if (!error) return [];
  const details = (error as { details?: { errors?: unknown[] } }).details;
  const messages = details?.errors
    ?.map((item) => {
      if (typeof item === "string") return item;
      if (item && typeof item === "object" && "message" in item) {
        return String((item as { message: unknown }).message);
      }
      return "";
    })
    .filter(Boolean);
  if (messages?.length) return messages;
  return [error instanceof Error ? error.message : "Policy operation failed"];
}

export function PolicyStudio({ tenant }: { tenant?: EnterpriseTenantContext }) {
  const policies = useEnterprisePolicies();
  const templates = useEnterprisePolicyTemplates();
  const validatePolicy = useValidateEnterprisePolicy();
  const previewPolicy = usePreviewEnterprisePolicy();
  const savePolicy = useSaveEnterprisePolicy();
  const publishPolicy = usePublishEnterprisePolicy();
  const disablePolicy = useDisableEnterprisePolicy();
  const supersedePolicy = useSupersedeEnterprisePolicy();
  const instantiateTemplate = useInstantiateEnterprisePolicyTemplate();
  const rollbackTemplate = useRollbackEnterprisePolicyTemplate();
  const upgradeTemplate = useUpgradeEnterprisePolicyTemplate();
  const [draft, setDraft] = useState<RuleDraft>(emptyDraft);
  const [editing, setEditing] = useState(false);
  const [previewTool, setPreviewTool] = useState("");
  const [previewValue, setPreviewValue] = useState("");
  const [templateId, setTemplateId] = useState("crm-agent");
  const [instanceId, setInstanceId] = useState("");
  const [overrideRuleId, setOverrideRuleId] = useState("");
  const [overrideConditionId, setOverrideConditionId] = useState("");
  const [overrideOperand, setOverrideOperand] = useState("");
  const [rollbackVersion, setRollbackVersion] = useState("1");
  const rules = useMemo(() => policies.data?.policy_rules || [], [policies.data]);
  const templateRows = useMemo(() => templates.data?.templates || [], [templates.data]);
  const selectedTemplate = templateRows.find((template) => template.template_id === templateId);
  const selectedOverrideCondition = selectedTemplate?.rules.find(
    (rule) => rule.rule_id === overrideRuleId.trim()
  )?.predicate?.condition;
  const selectedRule = rules.find((rule) => rule.rule_id === draft.ruleId);
  const currentRule = buildRule(draft, tenant, selectedRule);
  const busy =
    validatePolicy.isPending ||
    previewPolicy.isPending ||
    savePolicy.isPending ||
    publishPolicy.isPending ||
    disablePolicy.isPending ||
    supersedePolicy.isPending;
  const operationErrors = [
    policies.error,
    templates.error,
    validatePolicy.error,
    previewPolicy.error,
    savePolicy.error,
    publishPolicy.error,
    disablePolicy.error,
    supersedePolicy.error,
    instantiateTemplate.error,
    upgradeTemplate.error,
    rollbackTemplate.error,
  ].flatMap(policyErrorMessages);

  const set = <K extends keyof RuleDraft>(key: K, value: RuleDraft[K]) =>
    setDraft((current) => ({ ...current, [key]: value }));

  return (
    <PanelCard
      title="Policy studio"
      subtitle="Author inherited, parameter-aware runtime policy without editing files"
      actions={
        <Toolbar>
          <Badge tone="info">{rules.length} rules</Badge>
          <Badge tone="ghost">deny by default</Badge>
        </Toolbar>
      }
    >
      <div className="grid gap-5 xl:grid-cols-[minmax(16rem,0.8fr)_minmax(28rem,1.8fr)]">
        <section className="grid content-start gap-3" aria-label="Authored policies">
          <div className="flex items-center justify-between">
            <h3 className="tcp-text-section">Authored rules</h3>
            <button
              type="button"
              className="tcp-btn"
              onClick={() => {
                setDraft(emptyDraft());
                setEditing(false);
              }}
            >
              New rule
            </button>
          </div>
          {rules.length === 0 ? (
            <EmptyState
              title="No authored policy"
              text="Create a draft or start from a template."
            />
          ) : (
            <div className="grid gap-2">
              {rules.map((rule) => (
                <button
                  type="button"
                  key={rule.rule_id}
                  className="rounded-md border border-white/10 bg-white/[0.03] p-3 text-left hover:border-white/20"
                  onClick={() => {
                    setDraft(draftFromRule(rule));
                    setEditing(rule.state === "draft");
                    setPreviewTool(rule.tool_patterns?.[0] || "");
                  }}
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="font-medium text-tcp-text-primary">{rule.rule_id}</span>
                    <Badge tone={tone(rule.state || "draft")}>{rule.state || "draft"}</Badge>
                  </div>
                  <div className="mt-1 flex flex-wrap gap-1">
                    <Badge tone={tone(rule.effect)}>{rule.effect}</Badge>
                    <Badge tone="ghost">{rule.scope_level}</Badge>
                    {rule.template_id ? (
                      <Badge tone="info">
                        {rule.template_id} v{rule.template_version}
                      </Badge>
                    ) : null}
                  </div>
                  <div className="mt-2 truncate tcp-text-caption text-tcp-text-muted">
                    {(rule.tool_patterns || ["all tools"]).join(", ")}
                  </div>
                </button>
              ))}
            </div>
          )}
        </section>

        <section className="grid gap-4" aria-label="Policy rule editor">
          <div className="grid gap-3 md:grid-cols-2">
            <Field label="Policy ID">
              <input
                className="tcp-input"
                value={draft.policyId}
                onInput={(event) => set("policyId", event.currentTarget.value)}
                placeholder="finance-production"
              />
            </Field>
            <Field label="Rule ID">
              <input
                className="tcp-input"
                value={draft.ruleId}
                disabled={editing}
                onInput={(event) => set("ruleId", event.currentTarget.value)}
                placeholder="large-payment-approval"
              />
            </Field>
            <Field label="Scope">
              <select
                className="tcp-select"
                value={draft.scopeLevel}
                onChange={(event) => set("scopeLevel", event.currentTarget.value)}
              >
                {[
                  "enterprise",
                  "tenant",
                  "org_unit",
                  "workspace",
                  "resource",
                  "workflow",
                  "phase",
                ].map((value) => (
                  <option key={value}>{value}</option>
                ))}
              </select>
            </Field>
            <Field label="Effect">
              <select
                className="tcp-select"
                value={draft.effect}
                onChange={(event) => set("effect", event.currentTarget.value as PolicyEffect)}
              >
                <option value="allow">allow</option>
                <option value="approval_required">approval required</option>
                <option value="deny">deny</option>
              </select>
            </Field>
            {draft.scopeLevel === "org_unit" ? (
              <Field label="Organization unit ID">
                <input
                  className="tcp-input"
                  value={draft.orgUnitId}
                  onInput={(event) => set("orgUnitId", event.currentTarget.value)}
                />
              </Field>
            ) : null}
            {draft.scopeLevel === "resource" ? (
              <>
                <Field label="Resource kind">
                  <input
                    className="tcp-input"
                    value={draft.resourceKind}
                    onInput={(event) => set("resourceKind", event.currentTarget.value)}
                  />
                </Field>
                <Field label="Resource ID">
                  <input
                    className="tcp-input"
                    value={draft.resourceId}
                    onInput={(event) => set("resourceId", event.currentTarget.value)}
                  />
                </Field>
              </>
            ) : null}
            {["workflow", "phase"].includes(draft.scopeLevel) ? (
              <Field label="Workflow ID">
                <input
                  className="tcp-input"
                  value={draft.workflowId}
                  onInput={(event) => set("workflowId", event.currentTarget.value)}
                />
              </Field>
            ) : null}
            {draft.scopeLevel === "phase" ? (
              <Field label="Workflow phase">
                <input
                  className="tcp-input"
                  value={draft.workflowPhase}
                  onInput={(event) => set("workflowPhase", event.currentTarget.value)}
                />
              </Field>
            ) : null}
            <Field label="Tool patterns (comma separated)">
              <input
                className="tcp-input"
                value={draft.toolPatterns}
                onInput={(event) => set("toolPatterns", event.currentTarget.value)}
                placeholder="mcp.github.*"
              />
            </Field>
            <Field label="Approval class">
              <input
                className="tcp-input"
                value={draft.approvalId}
                onInput={(event) => set("approvalId", event.currentTarget.value)}
                disabled={draft.effect !== "approval_required"}
                placeholder="finance-large-payment"
              />
            </Field>
            <Field label="Expiry">
              <input
                className="tcp-input"
                type="datetime-local"
                value={draft.expiresAt}
                onInput={(event) => set("expiresAt", event.currentTarget.value)}
              />
            </Field>
            <Field label="Override behavior">
              <label className="flex min-h-10 items-center gap-2">
                <input
                  type="checkbox"
                  checked={draft.overridable}
                  onChange={(event) => set("overridable", event.currentTarget.checked)}
                />
                <span>More-specific scopes may override</span>
              </label>
            </Field>
          </div>

          <fieldset className="grid gap-3 rounded-md border border-white/10 p-3">
            <legend className="px-1 tcp-text-label">Data classes</legend>
            <div className="flex flex-wrap gap-3">
              {dataClasses.map((value) => (
                <label key={value} className="flex items-center gap-1.5 text-sm">
                  <input
                    type="checkbox"
                    checked={draft.selectedDataClasses.includes(value)}
                    onChange={(event) =>
                      set(
                        "selectedDataClasses",
                        event.currentTarget.checked
                          ? [...draft.selectedDataClasses, value]
                          : draft.selectedDataClasses.filter((item) => item !== value)
                      )
                    }
                  />
                  {value}
                </label>
              ))}
            </div>
          </fieldset>

          <fieldset className="grid gap-3 rounded-md border border-white/10 p-3">
            <legend className="px-1 tcp-text-label">Parameter predicate</legend>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={draft.predicateEnabled}
                onChange={(event) => set("predicateEnabled", event.currentTarget.checked)}
              />
              <span>Match trusted tool arguments</span>
            </label>
            {draft.predicateEnabled ? (
              <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
                <Field label="Condition ID">
                  <input
                    className="tcp-input"
                    value={draft.conditionId}
                    onInput={(event) => set("conditionId", event.currentTarget.value)}
                  />
                </Field>
                <Field label="Argument selector">
                  <input
                    className="tcp-input"
                    value={draft.selector}
                    onInput={(event) => set("selector", event.currentTarget.value)}
                    placeholder="/recipient/email"
                  />
                </Field>
                <Field label="Value type">
                  <select
                    className="tcp-select"
                    value={draft.valueType}
                    onChange={(event) => set("valueType", event.currentTarget.value)}
                  >
                    {valueTypes.map((value) => (
                      <option key={value}>{value}</option>
                    ))}
                  </select>
                </Field>
                <Field label="Operator">
                  <select
                    className="tcp-select"
                    value={draft.operator}
                    onChange={(event) => set("operator", event.currentTarget.value)}
                  >
                    {operators.map((value) => (
                      <option key={value}>{value}</option>
                    ))}
                  </select>
                </Field>
                <Field label="Operand">
                  <input
                    className="tcp-input"
                    value={draft.operand}
                    onInput={(event) => set("operand", event.currentTarget.value)}
                    placeholder={
                      draft.operator.includes("in") ? "example.com, example.org" : "example.com"
                    }
                  />
                </Field>
              </div>
            ) : null}
          </fieldset>

          <div className="grid gap-3 md:grid-cols-2">
            <Field label="Reason code">
              <input
                className="tcp-input"
                value={draft.reasonCode}
                onInput={(event) => set("reasonCode", event.currentTarget.value)}
              />
            </Field>
            <Field label="Operator-facing reason">
              <input
                className="tcp-input"
                value={draft.reason}
                onInput={(event) => set("reason", event.currentTarget.value)}
              />
            </Field>
          </div>

          <Toolbar>
            <button
              type="button"
              className="tcp-btn"
              disabled={busy}
              onClick={() => validatePolicy.mutate(currentRule)}
            >
              Validate
            </button>
            <button
              type="button"
              className="tcp-btn tcp-btn-primary"
              disabled={busy || !draft.ruleId.trim() || !draft.policyId.trim()}
              onClick={() => savePolicy.mutate({ rule: currentRule, update: editing })}
            >
              {editing ? "Save draft" : "Create draft"}
            </button>
            <button
              type="button"
              className="tcp-btn"
              disabled={busy || !draft.policyId.trim()}
              onClick={() => publishPolicy.mutate(draft.policyId.trim())}
            >
              Publish policy
            </button>
            <button
              type="button"
              className="tcp-btn tcp-btn-danger"
              disabled={busy || !draft.policyId.trim()}
              onClick={() => disablePolicy.mutate(draft.policyId.trim())}
            >
              Disable policy
            </button>
            <button
              type="button"
              className="tcp-btn"
              disabled={busy || !selectedRule || selectedRule.state !== "published"}
              onClick={() => {
                const policyRules = activePolicyRulesForSupersede(rules, draft.policyId);
                const nextVersion = Math.max(...policyRules.map((rule) => rule.version), 0) + 1;
                supersedePolicy.mutate({
                  policyId: draft.policyId,
                  rules: policyRules.map((rule) => {
                    const source = rule.rule_id === selectedRule?.rule_id ? currentRule : rule;
                    return {
                      ...source,
                      rule_id: `${source.rule_id}:v${nextVersion}`,
                      version: nextVersion,
                      state: "published",
                      updated_at_ms: Date.now(),
                    };
                  }),
                });
              }}
            >
              Supersede with edits
            </button>
          </Toolbar>

          {validatePolicy.data ? (
            <div
              className={`rounded-md border p-3 ${validatePolicy.data.valid ? "border-emerald-500/30" : "border-red-500/30"}`}
            >
              <div className="font-medium">
                {validatePolicy.data.valid ? "Policy is valid" : "Policy needs changes"}
              </div>
              {[...validatePolicy.data.errors, ...validatePolicy.data.warnings].map((message) => (
                <div
                  key={`${message.path}:${message.message}`}
                  className="mt-1 text-sm text-tcp-text-muted"
                >
                  <code>{message.path}</code>: {message.message}
                </div>
              ))}
            </div>
          ) : null}

          <fieldset className="grid gap-3 rounded-md border border-white/10 p-3">
            <legend className="px-1 tcp-text-label">Effective policy preview</legend>
            <div className="grid gap-3 md:grid-cols-2">
              <Field label="Tool">
                <input
                  className="tcp-input"
                  value={previewTool}
                  onInput={(event) => setPreviewTool(event.currentTarget.value)}
                  placeholder="mcp.payments.create_payment"
                />
              </Field>
              <Field label="Selected argument value">
                <input
                  className="tcp-input"
                  value={previewValue}
                  onInput={(event) => setPreviewValue(event.currentTarget.value)}
                  placeholder="15000.00"
                />
              </Field>
            </div>
            <button
              type="button"
              className="tcp-btn w-fit"
              disabled={previewPolicy.isPending || !previewTool.trim()}
              onClick={() =>
                previewPolicy.mutate({
                  tenant_context: tenant || {},
                  tool: previewTool.trim(),
                  arguments: draft.predicateEnabled
                    ? buildPolicyPreviewArguments(
                        draft.selector,
                        parsePolicyOperand(previewValue, "equals", draft.valueType)
                      )
                    : {},
                })
              }
            >
              Preview inherited winner
            </button>
            {previewPolicy.data ? (
              <div className="rounded-md bg-white/[0.03] p-3">
                <div className="flex items-center gap-2">
                  <Badge tone={tone(previewPolicy.data.snapshot.effect)}>
                    {previewPolicy.data.snapshot.effect}
                  </Badge>
                  <span className="font-medium">
                    {previewPolicy.data.winning_rule_id || "Default deny"}
                  </span>
                </div>
                <p className="mt-2 text-sm text-tcp-text-muted">
                  {previewPolicy.data.snapshot.reason}
                </p>
                <div className="mt-2 tcp-text-caption text-tcp-text-muted">
                  {(previewPolicy.data.snapshot.inherited_sources || [])
                    .map((source) => `${source.rule_id} (${source.effect})`)
                    .join(" → ") || "No inherited candidates"}
                </div>
              </div>
            ) : null}
          </fieldset>
        </section>
      </div>

      {operationErrors.length ? (
        <div className="mt-4 rounded-md border border-red-500/30 bg-red-500/5 p-3" role="alert">
          <div className="font-medium">Policy operation needs attention</div>
          {operationErrors.map((message, index) => (
            <div key={`${index}:${message}`} className="mt-1 text-sm text-tcp-text-muted">
              {message}
            </div>
          ))}
        </div>
      ) : null}

      <section
        className="mt-6 grid gap-3 border-t border-white/10 pt-5"
        aria-label="Starter policy templates"
      >
        <div>
          <h3 className="tcp-text-section">Starter policies</h3>
          <p className="tcp-text-caption text-tcp-text-muted">
            Versioned CRM, finance, and coding rule shapes with bounded deployment overrides.
          </p>
        </div>
        <div className="grid gap-3 lg:grid-cols-3">
          {templateRows.map((template) => (
            <TemplateCard
              key={template.template_id}
              template={template}
              selected={template.template_id === templateId}
              onSelect={() => setTemplateId(template.template_id)}
            />
          ))}
        </div>
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-6">
          <Field label="Deployment policy ID">
            <input
              className="tcp-input"
              value={instanceId}
              onInput={(event) => setInstanceId(event.currentTarget.value)}
              placeholder="finance-production"
            />
          </Field>
          <Field label="Override rule (optional)">
            <input
              className="tcp-input"
              value={overrideRuleId}
              onInput={(event) => setOverrideRuleId(event.currentTarget.value)}
              placeholder="large-payments"
            />
          </Field>
          <Field label="Condition ID">
            <input
              className="tcp-input"
              value={overrideConditionId}
              onInput={(event) => setOverrideConditionId(event.currentTarget.value)}
              placeholder="approval-threshold"
            />
          </Field>
          <Field label="Replacement operand">
            <input
              className="tcp-input"
              value={overrideOperand}
              onInput={(event) => setOverrideOperand(event.currentTarget.value)}
              placeholder="5000.00"
            />
          </Field>
          <Field label="Rollback version">
            <input
              className="tcp-input"
              type="number"
              min="1"
              value={rollbackVersion}
              onInput={(event) => setRollbackVersion(event.currentTarget.value)}
            />
          </Field>
          <div className="flex items-end">
            <div className="grid w-full gap-2">
              <button
                type="button"
                className="tcp-btn tcp-btn-primary w-full"
                disabled={!selectedTemplate || !instanceId.trim() || instantiateTemplate.isPending}
                onClick={() =>
                  instantiateTemplate.mutate({
                    templateId: selectedTemplate!.template_id,
                    version: selectedTemplate!.version,
                    instanceId: instanceId.trim(),
                    overrides: buildTemplatePredicateOverrides(
                      overrideRuleId,
                      overrideConditionId,
                      overrideOperand,
                      selectedOverrideCondition?.operator,
                      selectedOverrideCondition?.value_type
                    ),
                  })
                }
              >
                Create template draft
              </button>
              <button
                type="button"
                className="tcp-btn w-full"
                disabled={!selectedTemplate || !instanceId.trim() || upgradeTemplate.isPending}
                onClick={() =>
                  upgradeTemplate.mutate({
                    templateId: selectedTemplate!.template_id,
                    version: selectedTemplate!.version,
                    instanceId: instanceId.trim(),
                  })
                }
              >
                Upgrade to v{selectedTemplate?.version || 1}
              </button>
              <button
                type="button"
                className="tcp-btn w-full"
                disabled={!selectedTemplate || !instanceId.trim() || rollbackTemplate.isPending}
                onClick={() =>
                  rollbackTemplate.mutate({
                    templateId: selectedTemplate!.template_id,
                    version: Number(rollbackVersion),
                    instanceId: instanceId.trim(),
                  })
                }
              >
                Roll back to v{rollbackVersion || "?"}
              </button>
            </div>
          </div>
        </div>
        {instantiateTemplate.data ? (
          <div className="rounded-md border border-cyan-500/30 bg-cyan-500/5 p-3">
            <div className="font-medium">
              Created {instantiateTemplate.data.instantiation.instance_id} from{" "}
              {instantiateTemplate.data.template.display_name} v
              {instantiateTemplate.data.instantiation.template_version}
            </div>
            <div className="mt-1 text-sm text-tcp-text-muted">
              Template diff:{" "}
              {instantiateTemplate.data.instantiation.overrides_applied.join(", ") ||
                "no deployment overrides"}
            </div>
            <div className="mt-1 text-sm text-tcp-text-muted">
              {instantiateTemplate.data.instantiation.rules.length} draft rules · previewed{" "}
              {instantiateTemplate.data.effective_preview.length} effective outcomes
            </div>
          </div>
        ) : null}
      </section>
    </PanelCard>
  );
}

function TemplateCard({
  template,
  selected,
  onSelect,
}: {
  template: PolicyStarterTemplate;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onSelect}
      className={`rounded-md border p-3 text-left ${selected ? "border-cyan-400/60 bg-cyan-500/10" : "border-white/10 bg-white/[0.03]"}`}
    >
      <div className="flex items-center justify-between">
        <span className="font-medium">{template.display_name}</span>
        <Badge tone="info">v{template.version}</Badge>
      </div>
      <p className="mt-2 text-sm text-tcp-text-muted">{template.description}</p>
      <div className="mt-3 flex flex-wrap gap-1">
        {template.default_tool_scope.map((tool) => (
          <Badge key={tool} tone="ghost">
            {tool}
          </Badge>
        ))}
      </div>
      <div className="mt-2 tcp-text-caption text-tcp-text-muted">
        Overrides: {template.allowed_override_fields.join(", ")}
      </div>
    </button>
  );
}
