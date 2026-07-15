// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// Enterprise policy authoring persistence and preview helpers.

fn policy_decision_input_base(decision: &PolicyDecisionRecord) -> EnterprisePolicyInput {
    let mut input = EnterprisePolicyInput::new(decision.tenant_context.clone());
    if let Some(org_unit_id) = policy_decision_org_unit_id(decision) {
        input = input.with_org_unit_id(org_unit_id);
    }
    if let Some(resource) = decision.resource.clone() {
        input = input.with_resource(resource);
    }
    if let Some(workflow_id) = policy_decision_workflow_id(decision) {
        input = input.with_workflow_id(workflow_id);
    }
    if let Some(workflow_phase) = policy_decision_workflow_phase(decision) {
        input = input.with_workflow_phase(workflow_phase);
    }
    if let Some(permission) = policy_decision_permission(decision) {
        input = input.with_permission(permission);
    }
    if let Some(tool) = decision.tool.clone() {
        input = input.with_tool(tool);
    }
    if let Some(arguments) = ["/tool_arguments", "/arguments", "/tool/arguments"]
        .iter()
        .find_map(|pointer| decision.metadata.pointer(pointer).cloned())
    {
        input = input.with_arguments(arguments);
    }
    input
}

fn policy_decision_inputs(decision: &PolicyDecisionRecord) -> Vec<EnterprisePolicyInput> {
    let input = policy_decision_input_base(decision);
    if decision.data_classes.is_empty() {
        return vec![input];
    }
    decision
        .data_classes
        .iter()
        .copied()
        .map(|data_class| input.clone().with_data_class(data_class))
        .collect()
}

impl AppState {
    pub async fn persist_enterprise_policy_rules(&self) -> anyhow::Result<()> {
        let records = {
            let guard = self.enterprise.policy_rules.read().await;
            guard
                .iter()
                .map(|(rule_id, rule)| {
                    let tenant_context = rule
                        .tenant_context
                        .clone()
                        .unwrap_or_else(tandem_enterprise_contract::TenantContext::local_implicit);
                    crate::governance_store::GovernanceStoreFile::PolicyRules.json_record(
                        rule_id,
                        rule,
                        &tenant_context,
                        rule.org_unit_id.as_deref(),
                    )
                })
                .collect::<anyhow::Result<Vec<_>>>()?
        };
        crate::governance_store::for_state(self)
            .write_json_records(
                crate::governance_store::GovernanceStoreFile::PolicyRules,
                &records,
            )
            .await
    }

    pub async fn resolve_enterprise_policy_input(
        &self,
        input: &EnterprisePolicyInput,
        now_ms: u64,
    ) -> anyhow::Result<tandem_enterprise_contract::EffectivePolicySnapshot> {
        self.load_enterprise_policy_rules_if_needed().await?;
        let rules = self
            .enterprise
            .policy_rules
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        Ok(EnterprisePolicyResolver::new(rules).resolve(input, now_ms))
    }

    pub async fn ensure_enterprise_policy_rules_loaded(&self) -> anyhow::Result<()> {
        self.load_enterprise_policy_rules_if_needed().await
    }
}
