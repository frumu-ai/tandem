// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use hmac::{Hmac, Mac};
use sha2::Sha256;

const PREDICATE_EVIDENCE_CONDITION_LIMIT: usize = 32;

#[derive(Debug, Clone, Serialize)]
struct PredicateDecisionEvidence {
    expression_version: String,
    expression_digest: String,
    result: tandem_enterprise_contract::PredicateResult,
    conditions: Vec<PredicateDecisionConditionEvidence>,
    truncated: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    reason_codes: Vec<&'static str>,
    policy_id: String,
    policy_version_id: String,
    rule_id: String,
    rule_version: u64,
    policy_decision_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    approval_request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    action_binding: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct PredicateDecisionConditionEvidence {
    condition_id: String,
    selector_digest: String,
    value_type: tandem_enterprise_contract::PredicateValueType,
    operator: tandem_enterprise_contract::PredicateOperator,
    result: tandem_enterprise_contract::PredicateResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    value_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason_code: Option<&'static str>,
}

fn predicate_evidence_rule<'a>(
    rules: &'a [tandem_enterprise_contract::EnterprisePolicyRule],
    snapshot: &tandem_enterprise_contract::EffectivePolicySnapshot,
    input: &tandem_enterprise_contract::EnterprisePolicyInput,
    now_ms: u64,
) -> Option<&'a tandem_enterprise_contract::EnterprisePolicyRule> {
    if let Some(source) = snapshot.decision_source.as_ref() {
        return rules.iter().find(|rule| rule.rule_id == source.rule_id);
    }
    rules
        .iter()
        .filter(|rule| {
            rule.predicate.is_some() && rule.matches_scope_without_predicate(input, now_ms)
        })
        .max_by_key(|rule| {
            let effect_priority = match rule.effect {
                tandem_enterprise_contract::EnterprisePolicyEffect::Allow => 0,
                tandem_enterprise_contract::EnterprisePolicyEffect::ApprovalRequired => 1,
                tandem_enterprise_contract::EnterprisePolicyEffect::Deny => 2,
            };
            (
                rule.scope_level.inheritance_rank(),
                effect_priority,
                rule.version,
                rule.updated_at_ms,
                rule.rule_id.clone(),
            )
        })
}

fn predicate_decision_evidence(
    rule: &tandem_enterprise_contract::EnterprisePolicyRule,
    arguments: &Value,
    tenant_context: &tandem_types::TenantContext,
    policy_version_id: &str,
    policy_decision_id: &str,
    approval_request_id: Option<String>,
    action_binding: Option<String>,
) -> anyhow::Result<Option<PredicateDecisionEvidence>> {
    let Some(predicate) = rule.predicate.as_ref() else {
        return Ok(None);
    };
    let master_key = predicate_audit_hmac_key()?;
    Ok(Some(build_predicate_decision_evidence(
        rule,
        predicate,
        arguments,
        tenant_context,
        policy_version_id,
        policy_decision_id,
        approval_request_id,
        action_binding,
        &master_key,
    )?))
}

fn governed_exact_action_binding(
    tool: &str,
    arguments: &Value,
    tenant_context: &tandem_types::TenantContext,
    connector_generations: &[String],
) -> anyhow::Result<String> {
    let master_key = predicate_audit_hmac_key()?;
    let deployment_key = deployment_audit_hmac_key(&master_key, tenant_context)?;
    let canonical_action_digest = tandem_core::fintech_protected_action_hash(
        tool,
        &governed_user_action_arguments(arguments),
    );
    let binding = serde_json::to_vec(&serde_json::json!({
        "canonical_action_digest": canonical_action_digest,
        "connector_generations": connector_generations,
    }))?;
    audit_hmac(&deployment_key, b"exact-action", &binding)
}

fn governed_user_action_arguments(arguments: &Value) -> Value {
    let mut sanitized = arguments.clone();
    if let Some(object) = sanitized.as_object_mut() {
        object.retain(|key, _| !key.starts_with("__"));
    }
    sanitized
}

async fn governed_connector_generation_bindings(
    state: &AppState,
    tool: &str,
    tenant_context: &tandem_types::TenantContext,
) -> Vec<String> {
    let Some((server_namespace, _)) = tool
        .strip_prefix("mcp.")
        .and_then(|tool| tool.split_once('.'))
    else {
        return Vec::new();
    };
    let mut bindings = state
        .mcp
        .list_connections()
        .await
        .into_values()
        .filter(|connection| {
            crate::http::mcp::mcp_namespace_segment(&connection.server_id) == server_namespace
                && tandem_enterprise_contract::enterprise_scope_ids_match(
                    &connection.tenant_context.org_id,
                    &tenant_context.org_id,
                )
                && tandem_enterprise_contract::enterprise_scope_ids_match(
                    &connection.tenant_context.workspace_id,
                    &tenant_context.workspace_id,
                )
                && match (
                    connection.tenant_context.deployment_id.as_deref(),
                    tenant_context.deployment_id.as_deref(),
                ) {
                    (Some(left), Some(right)) => {
                        tandem_enterprise_contract::enterprise_scope_ids_match(left, right)
                    }
                    (None, None) => true,
                    _ => false,
                }
        })
        .map(|connection| {
            format!(
                "{}:{}",
                connection.connection_id, connection.connection_generation
            )
        })
        .collect::<Vec<_>>();
    bindings.sort();
    bindings
}

#[allow(clippy::too_many_arguments)]
fn build_predicate_decision_evidence(
    rule: &tandem_enterprise_contract::EnterprisePolicyRule,
    predicate: &tandem_enterprise_contract::PermissionPredicate,
    arguments: &Value,
    tenant_context: &tandem_types::TenantContext,
    policy_version_id: &str,
    policy_decision_id: &str,
    approval_request_id: Option<String>,
    action_binding: Option<String>,
    master_key: &[u8],
) -> anyhow::Result<PredicateDecisionEvidence> {
    let deployment_key = deployment_audit_hmac_key(master_key, tenant_context)?;
    let expression = serde_json::to_vec(predicate)?;
    let trace = predicate.evaluate_with_trace(arguments);
    let condition_count = trace.conditions.len();
    let conditions = trace
        .conditions
        .into_iter()
        .take(PREDICATE_EVIDENCE_CONDITION_LIMIT)
        .enumerate()
        .map(|(index, condition)| {
            let value_digest = condition
                .normalized_value
                .as_ref()
                .filter(|_| predicate_value_digest_allowed(condition.value_type))
                .map(serde_json::to_vec)
                .transpose()?
                .map(|value| audit_hmac(&deployment_key, b"selected-value", &value))
                .transpose()?;
            Ok(PredicateDecisionConditionEvidence {
                condition_id: condition
                    .condition_id
                    .unwrap_or_else(|| format!("condition-{}", index + 1)),
                selector_digest: audit_hmac(
                    &deployment_key,
                    b"selector",
                    condition.selector.as_bytes(),
                )?,
                value_type: condition.value_type,
                operator: condition.operator,
                result: condition.result,
                value_digest,
                reason_code: condition.reason_code,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(PredicateDecisionEvidence {
        expression_version: predicate.expression_version.clone(),
        expression_digest: audit_hmac(&deployment_key, b"expression", &expression)?,
        result: trace.result,
        conditions,
        truncated: trace.truncated
            || condition_count > PREDICATE_EVIDENCE_CONDITION_LIMIT,
        reason_codes: trace.reason_codes,
        policy_id: rule.policy_id.clone(),
        policy_version_id: policy_version_id.to_string(),
        rule_id: rule.rule_id.clone(),
        rule_version: rule.version,
        policy_decision_id: policy_decision_id.to_string(),
        approval_request_id,
        action_binding,
    })
}

fn predicate_value_digest_allowed(
    value_type: tandem_enterprise_contract::PredicateValueType,
) -> bool {
    !matches!(
        value_type,
        tandem_enterprise_contract::PredicateValueType::Boolean
            | tandem_enterprise_contract::PredicateValueType::CurrencyCode
            | tandem_enterprise_contract::PredicateValueType::Exists
            | tandem_enterprise_contract::PredicateValueType::ArrayLength
    )
}

fn predicate_audit_hmac_key() -> anyhow::Result<Vec<u8>> {
    let production_posture = crate::config::env::resolve_runtime_auth_mode()
        != tandem_types::RuntimeAuthMode::LocalSingleTenant
        || crate::config::env::hosted_control_plane_configured();
    predicate_audit_hmac_key_for_posture(
        configured_predicate_audit_hmac_key()?,
        production_posture,
    )
}

fn predicate_audit_hmac_key_for_posture(
    configured_key: Option<Vec<u8>>,
    production_posture: bool,
) -> anyhow::Result<Vec<u8>> {
    if let Some(key) = configured_key {
        return Ok(key);
    }
    if production_posture {
        anyhow::bail!(
            "predicate-governed decisions require TANDEM_AUDIT_HMAC_KEY or TANDEM_AUDIT_HMAC_KEY_FILE in hosted/enterprise mode"
        );
    }
    static LOCAL_EPHEMERAL_KEY: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    Ok(LOCAL_EPHEMERAL_KEY
        .get_or_init(|| uuid::Uuid::new_v4().as_bytes().to_vec())
        .clone())
}

fn configured_predicate_audit_hmac_key() -> anyhow::Result<Option<Vec<u8>>> {
    let value = match std::env::var("TANDEM_AUDIT_HMAC_KEY") {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => match std::env::var("TANDEM_AUDIT_HMAC_KEY_FILE") {
            Ok(path) if !path.trim().is_empty() => Some(std::fs::read_to_string(path.trim())?),
            _ => None,
        },
    };
    Ok(value
        .map(|value| value.trim().as_bytes().to_vec())
        .filter(|value| !value.is_empty()))
}

fn deployment_audit_hmac_key(
    master_key: &[u8],
    tenant_context: &tandem_types::TenantContext,
) -> anyhow::Result<Vec<u8>> {
    let scope = tenant_context
        .deployment_id
        .as_deref()
        .and_then(tandem_enterprise_contract::canonical_enterprise_scope_id)
        .map(|deployment| format!("deployment:{deployment}"))
        .map(Ok)
        .unwrap_or_else(|| {
            let org_id = tandem_enterprise_contract::canonical_enterprise_scope_id(
                &tenant_context.org_id,
            )
            .ok_or_else(|| anyhow::anyhow!("tenant organization ID is empty"))?;
            let workspace_id = tandem_enterprise_contract::canonical_enterprise_scope_id(
                &tenant_context.workspace_id,
            )
            .ok_or_else(|| anyhow::anyhow!("tenant workspace ID is empty"))?;
            Ok::<_, anyhow::Error>(format!("workspace:{org_id}:{workspace_id}"))
        })?;
    audit_hmac_bytes(master_key, b"deployment-scope", scope.as_bytes())
}

fn audit_hmac(key: &[u8], domain: &[u8], value: &[u8]) -> anyhow::Result<String> {
    let digest = audit_hmac_bytes(key, domain, value)?;
    Ok(format!("hmac-sha256:{}", hex_bytes(&digest)))
}

fn audit_hmac_bytes(key: &[u8], domain: &[u8], value: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key)
        .map_err(|_| anyhow::anyhow!("audit HMAC key is invalid"))?;
    mac.update(b"tandem-predicate-evidence/v1\0");
    mac.update(domain);
    mac.update(b"\0");
    mac.update(value);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod predicate_evidence_tests {
    use super::*;
    use serde_json::json;
    use tandem_enterprise_contract::{
        EnterprisePolicyEffect, EnterprisePolicyRule, EnterprisePolicyScopeLevel,
        PermissionPredicate, PredicateCondition, PredicateExpression, PredicateOperator,
        PredicateResult, PredicateValueType,
    };

    fn evidence_rule(predicate: PermissionPredicate) -> EnterprisePolicyRule {
        EnterprisePolicyRule::new(
            "evidence-rule",
            "evidence-policy",
            EnterprisePolicyScopeLevel::Tenant,
            EnterprisePolicyEffect::Deny,
        )
        .with_version(4)
        .with_predicate(predicate)
    }

    fn email_predicate() -> PermissionPredicate {
        PermissionPredicate {
            expression_version: "permission_predicates/v1".to_string(),
            expression: PredicateExpression::Condition {
                condition: PredicateCondition {
                    condition_id: Some("recipient-domain".to_string()),
                    selector: "/recipient".to_string(),
                    value_type: PredicateValueType::EmailDomain,
                    operator: PredicateOperator::IsSubdomainOf,
                    operand: json!("example.com"),
                },
            },
        }
    }

    fn build_email_evidence(
        deployment: &str,
        arguments: Value,
    ) -> PredicateDecisionEvidence {
        let predicate = email_predicate();
        let rule = evidence_rule(predicate.clone());
        build_predicate_decision_evidence(
            &rule,
            &predicate,
            &arguments,
            &tandem_types::TenantContext::explicit_user_workspace(
                "org-a",
                "workspace-a",
                Some(deployment.to_string()),
                "actor-a",
            ),
            "policy-version-4",
            "decision-1",
            Some("approval-1".to_string()),
            Some("hmac-sha256:action".to_string()),
            b"deployment-master-secret",
        )
        .expect("evidence")
    }

    #[test]
    fn evidence_covers_match_no_match_and_indeterminate_without_disclosure() {
        let matched = build_email_evidence(
            "deployment-a",
            json!({"recipient":"private-local-part@team.example.com"}),
        );
        let no_match = build_email_evidence(
            "deployment-a",
            json!({"recipient":"other-private@outside.test"}),
        );
        let indeterminate = build_email_evidence("deployment-a", json!({}));
        assert_eq!(matched.result, PredicateResult::Match);
        assert_eq!(no_match.result, PredicateResult::NoMatch);
        assert_eq!(indeterminate.result, PredicateResult::Indeterminate);
        assert_eq!(
            indeterminate.conditions[0].reason_code,
            Some("selector_missing")
        );

        let serialized = serde_json::to_string(&matched).expect("serialize evidence");
        for secret in [
            "private-local-part",
            "team.example.com",
            "example.com",
            "/recipient",
        ] {
            assert!(!serialized.contains(secret), "leaked `{secret}`");
        }
    }

    #[test]
    fn evidence_is_correlatable_only_inside_one_deployment() {
        let first = build_email_evidence(
            "deployment-a",
            json!({"recipient":"person@example.com"}),
        );
        let repeated = build_email_evidence(
            "deployment-a",
            json!({"recipient":"person@example.com"}),
        );
        let other_deployment = build_email_evidence(
            "deployment-b",
            json!({"recipient":"person@example.com"}),
        );
        assert_eq!(first.expression_digest, repeated.expression_digest);
        assert_eq!(
            first.conditions[0].value_digest,
            repeated.conditions[0].value_digest
        );
        assert_ne!(first.expression_digest, other_deployment.expression_digest);
        assert_ne!(
            first.conditions[0].selector_digest,
            other_deployment.conditions[0].selector_digest
        );
        assert_ne!(
            first.conditions[0].value_digest,
            other_deployment.conditions[0].value_digest
        );
    }

    #[test]
    fn deployment_hmac_scope_canonicalizes_tenant_ids() {
        let master_key = b"deployment-master-secret";
        let canonical = tandem_types::TenantContext::explicit(
            "org-a",
            "workspace-a",
            Some("deployment-a".to_string()),
        );
        let alternate = tandem_types::TenantContext::explicit(
            " Org-A ",
            " Workspace-A ",
            Some(" Deployment-A ".to_string()),
        );
        assert_eq!(
            deployment_audit_hmac_key(master_key, &canonical).expect("canonical key"),
            deployment_audit_hmac_key(master_key, &alternate).expect("alternate key")
        );

        let canonical_workspace =
            tandem_types::TenantContext::explicit("org-a", "workspace-a", None);
        let alternate_workspace =
            tandem_types::TenantContext::explicit(" Org-A ", " Workspace-A ", None);
        assert_eq!(
            deployment_audit_hmac_key(master_key, &canonical_workspace)
                .expect("canonical workspace key"),
            deployment_audit_hmac_key(master_key, &alternate_workspace)
                .expect("alternate workspace key")
        );
    }

    #[test]
    fn low_cardinality_values_are_omitted_and_oversized_traces_are_bounded() {
        let boolean_predicate = PermissionPredicate {
            expression_version: "permission_predicates/v1".to_string(),
            expression: PredicateExpression::Condition {
                condition: PredicateCondition {
                    condition_id: Some("confirmed".to_string()),
                    selector: "/confirmed".to_string(),
                    value_type: PredicateValueType::Boolean,
                    operator: PredicateOperator::Equals,
                    operand: json!(true),
                },
            },
        };
        let boolean_rule = evidence_rule(boolean_predicate.clone());
        let boolean = build_predicate_decision_evidence(
            &boolean_rule,
            &boolean_predicate,
            &json!({"confirmed":true}),
            &tandem_types::TenantContext::explicit("org-a", "workspace-a", None),
            "policy-version-4",
            "decision-boolean",
            None,
            None,
            b"master-secret",
        )
        .expect("boolean evidence");
        assert!(boolean.conditions[0].value_digest.is_none());

        let repeated = PredicateExpression::Condition {
            condition: PredicateCondition {
                condition_id: None,
                selector: "/secret".to_string(),
                value_type: PredicateValueType::String,
                operator: PredicateOperator::Equals,
                operand: json!("private-operand"),
            },
        };
        let oversized_predicate = PermissionPredicate {
            expression_version: "permission_predicates/v1".to_string(),
            expression: PredicateExpression::All {
                all: vec![repeated; 40],
            },
        };
        let oversized_rule = evidence_rule(oversized_predicate.clone());
        let oversized = build_predicate_decision_evidence(
            &oversized_rule,
            &oversized_predicate,
            &json!({"secret":"private-selected-value"}),
            &tandem_types::TenantContext::explicit("org-a", "workspace-a", None),
            "policy-version-4",
            "decision-oversized",
            None,
            None,
            b"master-secret",
        )
        .expect("oversized evidence");
        assert!(oversized.truncated);
        assert_eq!(oversized.conditions.len(), PREDICATE_EVIDENCE_CONDITION_LIMIT);
        let serialized = serde_json::to_string(&oversized).expect("serialize evidence");
        assert!(!serialized.contains("private-operand"));
        assert!(!serialized.contains("private-selected-value"));
    }

    #[test]
    fn hosted_predicate_evidence_fails_closed_without_hmac_authority() {
        let error = predicate_audit_hmac_key_for_posture(None, true)
            .expect_err("hosted mode must require an audit HMAC key");
        assert!(error.to_string().contains("TANDEM_AUDIT_HMAC_KEY"));
        assert_eq!(
            predicate_audit_hmac_key_for_posture(Some(b"configured".to_vec()), true)
                .expect("configured hosted key"),
            b"configured"
        );
    }

    #[test]
    fn exact_action_binding_excludes_runtime_only_arguments() {
        let tenant = tandem_types::TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("deployment-a".to_string()),
            "actor-a",
        );
        let connector_generations = vec!["connection-a:4".to_string()];
        let visible_arguments = json!({
            "recipient": "customer@example.com",
            "subject": "Quarterly update"
        });
        let mut injected_arguments = visible_arguments.clone();
        let object = injected_arguments
            .as_object_mut()
            .expect("visible arguments are an object");
        object.insert(
            "__verified_tenant_context".to_string(),
            json!({"assertion_id":"rotating-assertion","issued_at_ms":1234}),
        );
        object.insert("__session_id".to_string(), json!("runtime-session"));

        let visible_binding = governed_exact_action_binding(
            "mcp.email.send",
            &visible_arguments,
            &tenant,
            &connector_generations,
        )
        .expect("visible binding");
        let injected_binding = governed_exact_action_binding(
            "mcp.email.send",
            &injected_arguments,
            &tenant,
            &connector_generations,
        )
        .expect("injected binding");
        let changed_binding = governed_exact_action_binding(
            "mcp.email.send",
            &json!({
                "recipient": "different@example.com",
                "subject": "Quarterly update"
            }),
            &tenant,
            &connector_generations,
        )
        .expect("changed binding");

        assert_eq!(visible_binding, injected_binding);
        assert_ne!(visible_binding, changed_binding);
    }
}
