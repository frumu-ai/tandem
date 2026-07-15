use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    DataClass, EnterprisePolicyEffect, EnterprisePolicyRule, EnterprisePolicyRuleState,
    EnterprisePolicyScopeLevel, PermissionPredicate, PredicateCondition, PredicateExpression,
    PredicateOperator, PredicateValueType, TenantContext,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyStarterTemplate {
    pub template_id: String,
    pub version: u64,
    pub display_name: String,
    pub domain: String,
    pub description: String,
    pub default_tool_scope: Vec<String>,
    pub data_constraints: Vec<DataClass>,
    pub receipt_expectations: Vec<String>,
    pub allowed_override_fields: Vec<String>,
    pub rules: Vec<EnterprisePolicyRule>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyTemplateRuleOverride {
    pub rule_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_patterns: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub predicate_operands: HashMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyTemplateInstantiation {
    pub instance_id: String,
    pub template_id: String,
    pub template_version: u64,
    pub rules: Vec<EnterprisePolicyRule>,
    pub overrides_applied: Vec<String>,
}

impl PolicyStarterTemplate {
    pub fn instantiate(
        &self,
        instance_id: &str,
        tenant_context: TenantContext,
        overrides: &[PolicyTemplateRuleOverride],
        now_ms: u64,
    ) -> Result<PolicyTemplateInstantiation, Vec<String>> {
        let mut errors = Vec::new();
        if instance_id.trim().is_empty() {
            errors.push("instance_id is required".to_string());
        }
        let mut rules = self
            .rules
            .iter()
            .cloned()
            .map(|mut rule| {
                let source_rule_id = rule.rule_id.clone();
                rule.rule_id = format!("{instance_id}:{source_rule_id}");
                rule.policy_id = instance_id.to_string();
                rule.tenant_context = Some(tenant_context.clone());
                rule.template_id = Some(self.template_id.clone());
                rule.template_version = Some(self.version);
                rule.version = self.version;
                rule.state = EnterprisePolicyRuleState::Draft;
                rule.updated_at_ms = now_ms;
                rule
            })
            .collect::<Vec<_>>();
        let mut overrides_applied = Vec::new();
        for requested in overrides {
            let full_rule_id = format!("{instance_id}:{}", requested.rule_id);
            let Some(rule) = rules.iter_mut().find(|rule| rule.rule_id == full_rule_id) else {
                errors.push(format!(
                    "override references unknown rule `{}`",
                    requested.rule_id
                ));
                continue;
            };
            if !rule.overridable
                && (requested.tool_patterns.is_some()
                    || requested.approval_id.is_some()
                    || requested.expires_at_ms.is_some()
                    || !requested.predicate_operands.is_empty())
            {
                errors.push(format!("rule `{}` is non-overridable", requested.rule_id));
                continue;
            }
            if let Some(patterns) = &requested.tool_patterns {
                if !self
                    .allowed_override_fields
                    .iter()
                    .any(|field| field == "tool_patterns")
                {
                    errors.push("template does not permit tool_patterns overrides".to_string());
                } else {
                    rule.tool_patterns = patterns.clone();
                    overrides_applied.push(format!("{}.tool_patterns", requested.rule_id));
                }
            }
            if let Some(approval_id) = &requested.approval_id {
                if !self
                    .allowed_override_fields
                    .iter()
                    .any(|field| field == "approval_id")
                {
                    errors.push("template does not permit approval_id overrides".to_string());
                } else {
                    rule.approval_id = Some(approval_id.clone());
                    overrides_applied.push(format!("{}.approval_id", requested.rule_id));
                }
            }
            if let Some(expires_at_ms) = requested.expires_at_ms {
                if !self
                    .allowed_override_fields
                    .iter()
                    .any(|field| field == "expires_at_ms")
                {
                    errors.push("template does not permit expires_at_ms overrides".to_string());
                } else {
                    rule.expires_at_ms = Some(expires_at_ms);
                    overrides_applied.push(format!("{}.expires_at_ms", requested.rule_id));
                }
            }
            if !requested.predicate_operands.is_empty() {
                if !self
                    .allowed_override_fields
                    .iter()
                    .any(|field| field == "predicate_operands")
                {
                    errors.push("template does not permit predicate operand overrides".to_string());
                } else if let Some(predicate) = rule.predicate.as_mut() {
                    for (condition_id, operand) in &requested.predicate_operands {
                        if replace_operand(&mut predicate.expression, condition_id, operand.clone())
                        {
                            overrides_applied.push(format!(
                                "{}.predicate_operands.{condition_id}",
                                requested.rule_id
                            ));
                        } else {
                            errors.push(format!(
                                "rule `{}` has no condition `{condition_id}`",
                                requested.rule_id
                            ));
                        }
                    }
                } else {
                    errors.push(format!("rule `{}` has no predicate", requested.rule_id));
                }
            }
        }
        errors.extend(
            rules
                .iter()
                .flat_map(EnterprisePolicyRule::validation_errors),
        );
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(PolicyTemplateInstantiation {
            instance_id: instance_id.to_string(),
            template_id: self.template_id.clone(),
            template_version: self.version,
            rules,
            overrides_applied,
        })
    }
}

fn replace_operand(
    expression: &mut PredicateExpression,
    condition_id: &str,
    operand: Value,
) -> bool {
    match expression {
        PredicateExpression::All { all } | PredicateExpression::Any { any: all } => all
            .iter_mut()
            .any(|child| replace_operand(child, condition_id, operand.clone())),
        PredicateExpression::Not { not } => replace_operand(not, condition_id, operand),
        PredicateExpression::Condition { condition } => {
            if condition.condition_id.as_deref() == Some(condition_id) {
                condition.operand = operand;
                true
            } else {
                false
            }
        }
    }
}

fn condition(
    id: &str,
    selector: &str,
    value_type: PredicateValueType,
    operator: PredicateOperator,
    operand: Value,
) -> PermissionPredicate {
    PermissionPredicate {
        expression_version: "permission_predicates/v1".to_string(),
        expression: PredicateExpression::Condition {
            condition: PredicateCondition {
                condition_id: Some(id.to_string()),
                selector: selector.to_string(),
                value_type,
                operator,
                operand,
            },
        },
    }
}

fn template_rule(
    rule_id: &str,
    effect: EnterprisePolicyEffect,
    tools: &[&str],
) -> EnterprisePolicyRule {
    EnterprisePolicyRule::new(
        rule_id,
        "template",
        EnterprisePolicyScopeLevel::Tenant,
        effect,
    )
    .with_tool_patterns(tools.iter().map(|tool| (*tool).to_string()).collect())
}

pub fn starter_policy_templates() -> Vec<PolicyStarterTemplate> {
    vec![crm_template(), finance_template_v2(), coding_template()]
}

pub fn starter_policy_template(
    template_id: &str,
    version: Option<u64>,
) -> Option<PolicyStarterTemplate> {
    let mut matching = all_policy_template_versions()
        .into_iter()
        .filter(|template| template.template_id == template_id)
        .collect::<Vec<_>>();
    matching.sort_by_key(|template| template.version);
    match version {
        Some(version) => matching
            .into_iter()
            .find(|template| template.version == version),
        None => matching.pop(),
    }
}

fn all_policy_template_versions() -> Vec<PolicyStarterTemplate> {
    vec![
        crm_template(),
        finance_template_v1(),
        finance_template_v2(),
        coding_template(),
    ]
}

fn crm_template() -> PolicyStarterTemplate {
    let internal = template_rule(
        "internal-drafts",
        EnterprisePolicyEffect::Allow,
        &["mcp.crm.create_email_draft"],
    )
    .with_predicate(condition(
        "company-domains",
        "/recipient/email",
        PredicateValueType::EmailDomain,
        PredicateOperator::In,
        json!(["example.com"]),
    ));
    let external = template_rule(
        "external-drafts",
        EnterprisePolicyEffect::ApprovalRequired,
        &["mcp.crm.create_email_draft"],
    )
    .with_predicate(condition(
        "external-domains",
        "/recipient/email",
        PredicateValueType::EmailDomain,
        PredicateOperator::NotIn,
        json!(["example.com"]),
    ))
    .with_approval_id("crm-external-recipient")
    .with_overridable(true);
    let credentials = template_rule(
        "deny-credential-export",
        EnterprisePolicyEffect::Deny,
        &["mcp.crm.export_credentials"],
    )
    .with_data_classes(vec![DataClass::Credential]);
    PolicyStarterTemplate {
        template_id: "crm-agent".to_string(),
        version: 1,
        display_name: "CRM agent".to_string(),
        domain: "crm".to_string(),
        description: "Draft safely for company domains and approve external recipients."
            .to_string(),
        default_tool_scope: vec!["mcp.crm.*".to_string()],
        data_constraints: vec![DataClass::CustomerData, DataClass::Credential],
        receipt_expectations: vec![
            "policy_decision".to_string(),
            "approval".to_string(),
            "execution".to_string(),
        ],
        allowed_override_fields: vec![
            "approval_id".to_string(),
            "predicate_operands".to_string(),
            "expires_at_ms".to_string(),
        ],
        rules: vec![internal, external, credentials],
    }
}

fn finance_template_v1() -> PolicyStarterTemplate {
    let small = template_rule(
        "small-payments",
        EnterprisePolicyEffect::Allow,
        &["mcp.payments.create_payment"],
    )
    .with_predicate(condition(
        "payment-threshold",
        "/amount/value",
        PredicateValueType::Decimal,
        PredicateOperator::LessThan,
        json!("10000.00"),
    ))
    .with_overridable(true);
    let large = template_rule(
        "large-payments",
        EnterprisePolicyEffect::ApprovalRequired,
        &["mcp.payments.create_payment"],
    )
    .with_predicate(condition(
        "approval-threshold",
        "/amount/value",
        PredicateValueType::Decimal,
        PredicateOperator::GreaterThanOrEqual,
        json!("10000.00"),
    ))
    .with_approval_id("finance-large-payment")
    .with_overridable(true);
    let credential_deny = template_rule(
        "deny-finance-credentials",
        EnterprisePolicyEffect::Deny,
        &["mcp.payments.export_credentials"],
    )
    .with_data_classes(vec![DataClass::Credential, DataClass::FinancialRecord]);
    PolicyStarterTemplate {
        template_id: "finance-agent".to_string(),
        version: 1,
        display_name: "Finance agent".to_string(),
        domain: "finance".to_string(),
        description: "Bound payment authority by amount and protect finance credentials."
            .to_string(),
        default_tool_scope: vec!["mcp.payments.*".to_string()],
        data_constraints: vec![DataClass::FinancialRecord, DataClass::Credential],
        receipt_expectations: vec![
            "policy_decision".to_string(),
            "approval".to_string(),
            "execution".to_string(),
        ],
        allowed_override_fields: vec![
            "approval_id".to_string(),
            "predicate_operands".to_string(),
            "expires_at_ms".to_string(),
        ],
        rules: vec![small, large, credential_deny],
    }
}

fn finance_template_v2() -> PolicyStarterTemplate {
    let mut template = finance_template_v1();
    template.version = 2;
    template.description =
        "Bound payment authority by amount, require approval from 5,000, and protect finance credentials."
            .to_string();
    for rule in &mut template.rules {
        if let Some(predicate) = rule.predicate.as_mut() {
            let condition_id = match rule.rule_id.as_str() {
                "small-payments" => "payment-threshold",
                "large-payments" => "approval-threshold",
                _ => continue,
            };
            replace_operand(&mut predicate.expression, condition_id, json!("5000.00"));
        }
    }
    template
}

fn coding_template() -> PolicyStarterTemplate {
    let repo = template_rule(
        "approved-repository",
        EnterprisePolicyEffect::Allow,
        &["mcp.github.*"],
    )
    .with_predicate(condition(
        "repository",
        "/repository",
        PredicateValueType::Repository,
        PredicateOperator::Equals,
        json!("example/app"),
    ))
    .with_overridable(true);
    let main = template_rule(
        "protect-main",
        EnterprisePolicyEffect::ApprovalRequired,
        &["mcp.github.merge_pull_request"],
    )
    .with_predicate(condition(
        "protected-branch",
        "/target_branch",
        PredicateValueType::String,
        PredicateOperator::Equals,
        json!("main"),
    ))
    .with_approval_id("coding-protected-branch")
    .with_overridable(true);
    let secret = template_rule(
        "deny-secret-export",
        EnterprisePolicyEffect::Deny,
        &["filesystem.read", "mcp.github.create_file"],
    )
    .with_data_classes(vec![DataClass::Credential]);
    PolicyStarterTemplate {
        template_id: "coding-agent".to_string(),
        version: 1,
        display_name: "Coding agent".to_string(),
        domain: "coding".to_string(),
        description: "Constrain repository scope, protect main, and block credential export."
            .to_string(),
        default_tool_scope: vec!["mcp.github.*".to_string(), "filesystem.*".to_string()],
        data_constraints: vec![DataClass::SourceCode, DataClass::Credential],
        receipt_expectations: vec![
            "policy_decision".to_string(),
            "approval".to_string(),
            "execution".to_string(),
        ],
        allowed_override_fields: vec![
            "tool_patterns".to_string(),
            "approval_id".to_string(),
            "predicate_operands".to_string(),
            "expires_at_ms".to_string(),
        ],
        rules: vec![repo, main, secret],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant() -> TenantContext {
        TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "admin-a")
    }

    #[test]
    fn catalog_is_versioned_and_instantiates_drafts() {
        let template = starter_policy_template("finance-agent", Some(1)).unwrap();
        let instance = template
            .instantiate("finance-prod", tenant(), &[], 100)
            .unwrap();
        assert_eq!(instance.template_version, 1);
        assert!(instance
            .rules
            .iter()
            .all(|rule| rule.state == EnterprisePolicyRuleState::Draft));
        assert_eq!(
            starter_policy_template("finance-agent", None)
                .unwrap()
                .version,
            2
        );
        assert_eq!(
            starter_policy_template("finance-agent", Some(2))
                .unwrap()
                .version,
            2
        );
        assert!(starter_policy_template("finance-agent", Some(3)).is_none());
        assert_eq!(starter_policy_templates().len(), 3);
    }

    #[test]
    fn bounded_overrides_apply_without_copying_template() {
        let template = starter_policy_template("finance-agent", None).unwrap();
        let instance = template
            .instantiate(
                "finance-eu",
                tenant(),
                &[PolicyTemplateRuleOverride {
                    rule_id: "large-payments".to_string(),
                    predicate_operands: HashMap::from([(
                        "approval-threshold".to_string(),
                        json!("5000.00"),
                    )]),
                    ..Default::default()
                }],
                100,
            )
            .unwrap();
        assert_eq!(
            instance.overrides_applied,
            vec!["large-payments.predicate_operands.approval-threshold"]
        );
    }

    #[test]
    fn non_overridable_deny_rejects_content_changes() {
        let template = starter_policy_template("coding-agent", None).unwrap();
        let errors = template
            .instantiate(
                "coding-prod",
                tenant(),
                &[PolicyTemplateRuleOverride {
                    rule_id: "deny-secret-export".to_string(),
                    tool_patterns: Some(vec!["*".to_string()]),
                    ..Default::default()
                }],
                100,
            )
            .unwrap_err();
        assert!(errors.iter().any(|error| error.contains("non-overridable")));
    }

    #[test]
    fn allowed_overrides_do_not_displace_non_overridable_denies() {
        let template = starter_policy_template("coding-agent", None).unwrap();
        let mut instance = template
            .instantiate(
                "coding-prod",
                tenant(),
                &[PolicyTemplateRuleOverride {
                    rule_id: "approved-repository".to_string(),
                    predicate_operands: HashMap::from([(
                        "repository".to_string(),
                        json!("frumu-ai/tandem"),
                    )]),
                    ..Default::default()
                }],
                100,
            )
            .unwrap();
        for rule in &mut instance.rules {
            rule.state = EnterprisePolicyRuleState::Published;
        }
        let snapshot = crate::EnterprisePolicyResolver::new(instance.rules).resolve(
            &crate::EnterprisePolicyInput::new(tenant())
                .with_tool("mcp.github.create_file")
                .with_data_class(DataClass::Credential)
                .with_arguments(json!({"repository":"frumu-ai/tandem"})),
            101,
        );
        assert_eq!(snapshot.effect, EnterprisePolicyEffect::Deny);
        assert!(snapshot
            .decision_source
            .is_some_and(|source| source.rule_id.ends_with("deny-secret-export")));
    }
}
