use std::collections::{HashMap, HashSet};

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_enterprise_contract::{
    enterprise_scope_ids_match, starter_policy_template, starter_policy_templates,
    EffectivePolicySnapshot, EnterprisePolicyInput, EnterprisePolicyRule,
    EnterprisePolicyRuleState, EnterprisePolicyScopeLevel, PolicyStarterTemplate,
    PolicyTemplateInstantiation, PolicyTemplateRuleOverride, RequestPrincipal, TenantContext,
    VerifiedTenantContext,
};
use tandem_server::{now_ms, AppState};

use super::routes_enterprise::{
    enterprise_global_admin_allowed_for_mutation, require_enterprise_admin, EnterpriseResult,
};

#[derive(Debug, Serialize)]
struct PolicyRulesResponse {
    policy_rules: Vec<EnterprisePolicyRule>,
    count: usize,
}

#[derive(Debug, Serialize)]
struct PolicyMutationResponse {
    action: &'static str,
    policy_id: String,
    rules: Vec<EnterprisePolicyRule>,
    receipt_event: &'static str,
}

#[derive(Debug, Serialize)]
struct PolicyValidationResponse {
    valid: bool,
    errors: Vec<PolicyValidationMessage>,
    warnings: Vec<PolicyValidationMessage>,
}

#[derive(Debug, Serialize)]
struct PolicyValidationMessage {
    path: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct PreviewPolicyRequest {
    input: EnterprisePolicyInput,
}

#[derive(Debug, Serialize)]
struct PreviewPolicyResponse {
    snapshot: EffectivePolicySnapshot,
    default_denied: bool,
    winning_rule_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SupersedePolicyRequest {
    rules: Vec<EnterprisePolicyRule>,
}

#[derive(Debug, Deserialize)]
struct InstantiateTemplateRequest {
    instance_id: String,
    #[serde(default)]
    version: Option<u64>,
    #[serde(default)]
    overrides: Vec<PolicyTemplateRuleOverride>,
}

#[derive(Debug, Serialize)]
struct TemplateCatalogResponse {
    templates: Vec<PolicyStarterTemplate>,
}

#[derive(Debug, Serialize)]
struct TemplateInstantiationResponse {
    instantiation: PolicyTemplateInstantiation,
    template: PolicyStarterTemplate,
    effective_preview: Vec<EffectivePolicySnapshot>,
}

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            "/enterprise/policies",
            get(list_policy_rules).post(create_policy_rule),
        )
        .route("/enterprise/policies/validate", post(validate_policy_rule))
        .route("/enterprise/policies/preview", post(preview_policy))
        .route("/enterprise/policies/{rule_id}", patch(update_policy_rule))
        .route(
            "/enterprise/policies/{policy_id}/publish",
            post(publish_policy),
        )
        .route(
            "/enterprise/policies/{policy_id}/disable",
            post(disable_policy),
        )
        .route(
            "/enterprise/policies/{policy_id}/supersede",
            post(supersede_policy),
        )
        .route("/enterprise/policy-templates", get(list_policy_templates))
        .route(
            "/enterprise/policy-templates/{template_id}/instantiate",
            post(instantiate_policy_template),
        )
        .route(
            "/enterprise/policy-templates/{template_id}/rollback",
            post(rollback_policy_template),
        )
        .route(
            "/enterprise/policy-templates/{template_id}/upgrade",
            post(upgrade_policy_template),
        )
}

async fn list_policy_rules(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
) -> EnterpriseResult<PolicyRulesResponse> {
    require_enterprise_admin(&request_principal, verified.as_deref())?;
    let may_manage_global =
        enterprise_global_admin_allowed_for_mutation(&request_principal, verified.as_deref());
    ensure_policy_rules_loaded(&state).await?;
    let mut rules = state
        .enterprise
        .policy_rules
        .read()
        .await
        .values()
        .filter(|rule| admin_scope_matches(rule, &tenant_context, may_manage_global))
        .cloned()
        .collect::<Vec<_>>();
    rules.sort_by(|a, b| {
        a.policy_id
            .cmp(&b.policy_id)
            .then(a.rule_id.cmp(&b.rule_id))
    });
    Ok(Json(PolicyRulesResponse {
        count: rules.len(),
        policy_rules: rules,
    }))
}

async fn create_policy_rule(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Json(mut rule): Json<EnterprisePolicyRule>,
) -> EnterpriseResult<PolicyMutationResponse> {
    require_enterprise_admin(&request_principal, verified.as_deref())?;
    let requested_global = rule.scope_level
        == tandem_enterprise_contract::EnterprisePolicyScopeLevel::Enterprise
        && rule.tenant_context.is_none();
    if requested_global
        && !enterprise_global_admin_allowed_for_mutation(&request_principal, verified.as_deref())
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "code": "ENTERPRISE_GLOBAL_ADMIN_REQUIRED",
                "message": "enterprise global admin access is required to create a tenantless policy"
            })),
        ));
    }
    ensure_policy_rules_loaded(&state).await?;
    if !requested_global {
        rule.tenant_context = Some(tenant_context.clone());
    }
    rule.state = EnterprisePolicyRuleState::Draft;
    rule.updated_at_ms = now_ms();
    let validation = validate_rule(&rule, false);
    if !validation.errors.is_empty() {
        return validation_error(validation);
    }
    let previous = state.enterprise.policy_rules.read().await.clone();
    {
        let mut rules = state.enterprise.policy_rules.write().await;
        if rules.contains_key(&rule.rule_id) {
            return Err(conflict("ENTERPRISE_POLICY_RULE_EXISTS"));
        }
        rules.insert(rule.rule_id.clone(), rule.clone());
    }
    commit_and_audit(
        &state,
        previous,
        &tenant_context,
        &request_principal,
        "enterprise.policy.created",
        json!({
            "policy_id": rule.policy_id,
            "rule_id": rule.rule_id,
            "state": rule.state,
            "policy_scope": if requested_global { "enterprise_global" } else { "tenant" },
            "rule_tenant_context": rule.tenant_context,
        }),
    )
    .await?;
    Ok(Json(PolicyMutationResponse {
        action: "created",
        policy_id: rule.policy_id.clone(),
        rules: vec![rule],
        receipt_event: "enterprise.policy.created",
    }))
}

async fn update_policy_rule(
    State(state): State<AppState>,
    Path(rule_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Json(mut replacement): Json<EnterprisePolicyRule>,
) -> EnterpriseResult<PolicyMutationResponse> {
    require_enterprise_admin(&request_principal, verified.as_deref())?;
    let may_manage_global =
        enterprise_global_admin_allowed_for_mutation(&request_principal, verified.as_deref());
    ensure_policy_rules_loaded(&state).await?;
    let previous = state.enterprise.policy_rules.read().await.clone();
    let existing = previous
        .get(&rule_id)
        .filter(|rule| admin_scope_matches(rule, &tenant_context, may_manage_global))
        .cloned()
        .ok_or_else(|| not_found("ENTERPRISE_POLICY_RULE_NOT_FOUND"))?;
    if existing.state != EnterprisePolicyRuleState::Draft {
        return Err(conflict("ENTERPRISE_POLICY_RULE_NOT_EDITABLE"));
    }
    replacement.rule_id = rule_id.clone();
    replacement
        .tenant_context
        .clone_from(&existing.tenant_context);
    replacement.version = existing.version;
    replacement.template_id.clone_from(&existing.template_id);
    replacement.template_version = existing.template_version;
    if existing.template_id.is_some() {
        replacement.policy_id.clone_from(&existing.policy_id);
    }
    replacement.state = EnterprisePolicyRuleState::Draft;
    replacement.updated_at_ms = now_ms();
    let validation = validate_rule(&replacement, false);
    if !validation.errors.is_empty() {
        return validation_error(validation);
    }
    state
        .enterprise
        .policy_rules
        .write()
        .await
        .insert(rule_id.clone(), replacement.clone());
    commit_and_audit(
        &state,
        previous,
        &tenant_context,
        &request_principal,
        "enterprise.policy.updated",
        json!({"policy_id": replacement.policy_id, "rule_id": rule_id}),
    )
    .await?;
    Ok(Json(PolicyMutationResponse {
        action: "updated",
        policy_id: replacement.policy_id.clone(),
        rules: vec![replacement],
        receipt_event: "enterprise.policy.updated",
    }))
}

async fn validate_policy_rule(
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Json(mut rule): Json<EnterprisePolicyRule>,
) -> EnterpriseResult<PolicyValidationResponse> {
    require_enterprise_admin(&request_principal, verified.as_deref())?;
    let requested_global =
        rule.scope_level == EnterprisePolicyScopeLevel::Enterprise && rule.tenant_context.is_none();
    if !requested_global {
        rule.tenant_context = Some(tenant_context);
    }
    Ok(Json(validate_rule(&rule, true)))
}

async fn preview_policy(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Json(mut request): Json<PreviewPolicyRequest>,
) -> EnterpriseResult<PreviewPolicyResponse> {
    require_enterprise_admin(&request_principal, verified.as_deref())?;
    request.input.tenant_context = tenant_context;
    let snapshot = state
        .resolve_enterprise_policy_input(&request.input, now_ms())
        .await
        .map_err(|_| internal_error("ENTERPRISE_POLICY_PREVIEW_FAILED"))?;
    Ok(Json(PreviewPolicyResponse {
        default_denied: snapshot.decision_source.is_none(),
        winning_rule_id: snapshot
            .decision_source
            .as_ref()
            .map(|source| source.rule_id.clone()),
        snapshot,
    }))
}

async fn publish_policy(
    State(state): State<AppState>,
    Path(policy_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
) -> EnterpriseResult<PolicyMutationResponse> {
    require_enterprise_admin(&request_principal, verified.as_deref())?;
    let may_manage_global =
        enterprise_global_admin_allowed_for_mutation(&request_principal, verified.as_deref());
    ensure_policy_rules_loaded(&state).await?;
    mutate_policy_state(
        &state,
        &tenant_context,
        &request_principal,
        &policy_id,
        may_manage_global,
        PolicyStateTransition {
            target_state: EnterprisePolicyRuleState::Published,
            action: "published",
            event_type: "enterprise.policy.published",
            validate_for_publish: true,
        },
    )
    .await
}

async fn disable_policy(
    State(state): State<AppState>,
    Path(policy_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
) -> EnterpriseResult<PolicyMutationResponse> {
    require_enterprise_admin(&request_principal, verified.as_deref())?;
    let may_manage_global =
        enterprise_global_admin_allowed_for_mutation(&request_principal, verified.as_deref());
    ensure_policy_rules_loaded(&state).await?;
    mutate_policy_state(
        &state,
        &tenant_context,
        &request_principal,
        &policy_id,
        may_manage_global,
        PolicyStateTransition {
            target_state: EnterprisePolicyRuleState::Disabled,
            action: "disabled",
            event_type: "enterprise.policy.disabled",
            validate_for_publish: false,
        },
    )
    .await
}

async fn supersede_policy(
    State(state): State<AppState>,
    Path(policy_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Json(request): Json<SupersedePolicyRequest>,
) -> EnterpriseResult<PolicyMutationResponse> {
    require_enterprise_admin(&request_principal, verified.as_deref())?;
    let may_manage_global =
        enterprise_global_admin_allowed_for_mutation(&request_principal, verified.as_deref());
    ensure_policy_rules_loaded(&state).await?;
    if request.rules.is_empty() {
        return Err(bad_request("ENTERPRISE_POLICY_REPLACEMENT_REQUIRED"));
    }
    let target_tenant_context = {
        let registry = state.enterprise.policy_rules.read().await;
        let tenant_owned = registry.values().any(|rule| {
            rule.policy_id == policy_id
                && rule.tenant_context.is_some()
                && tenant_matches(rule, &tenant_context)
                && rule.state != EnterprisePolicyRuleState::Superseded
        });
        let enterprise_global = may_manage_global
            && registry.values().any(|rule| {
                rule.policy_id == policy_id
                    && rule.tenant_context.is_none()
                    && rule.state != EnterprisePolicyRuleState::Superseded
            });
        match (tenant_owned, enterprise_global) {
            (true, false) => Some(tenant_context.clone()),
            (false, true) => None,
            (false, false) => return Err(not_found("ENTERPRISE_POLICY_NOT_FOUND")),
            (true, true) => return Err(conflict("ENTERPRISE_POLICY_SCOPE_AMBIGUOUS")),
        }
    };
    let mut replacements = Vec::new();
    let mut replacement_ids = HashSet::new();
    for mut rule in request.rules {
        if !replacement_ids.insert(rule.rule_id.clone()) {
            return Err(conflict("ENTERPRISE_POLICY_RULE_ID_CONFLICT"));
        }
        rule.policy_id = policy_id.clone();
        rule.tenant_context.clone_from(&target_tenant_context);
        rule.state = EnterprisePolicyRuleState::Published;
        rule.updated_at_ms = now_ms();
        let validation = validate_rule(&rule, true);
        if !validation.errors.is_empty() {
            return validation_error(validation);
        }
        replacements.push(rule);
    }
    let previous;
    {
        let mut registry = state.enterprise.policy_rules.write().await;
        if replacements
            .iter()
            .any(|rule| registry.contains_key(&rule.rule_id))
        {
            return Err(conflict("ENTERPRISE_POLICY_RULE_ID_CONFLICT"));
        }
        previous = registry.clone();
        for rule in registry.values_mut().filter(|rule| {
            rule.policy_id == policy_id
                && target_tenant_context_matches(rule, target_tenant_context.as_ref())
        }) {
            rule.state = EnterprisePolicyRuleState::Superseded;
            rule.updated_at_ms = now_ms();
        }
        for rule in &replacements {
            registry.insert(rule.rule_id.clone(), rule.clone());
        }
    }
    commit_and_audit(
        &state,
        previous,
        &tenant_context,
        &request_principal,
        "enterprise.policy.superseded",
        json!({"policy_id": policy_id, "replacement_rules": replacements.len()}),
    )
    .await?;
    Ok(Json(PolicyMutationResponse {
        action: "superseded",
        policy_id,
        rules: replacements,
        receipt_event: "enterprise.policy.superseded",
    }))
}

async fn list_policy_templates(
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
) -> EnterpriseResult<TemplateCatalogResponse> {
    require_enterprise_admin(&request_principal, verified.as_deref())?;
    Ok(Json(TemplateCatalogResponse {
        templates: starter_policy_templates(),
    }))
}

async fn instantiate_policy_template(
    State(state): State<AppState>,
    Path(template_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Json(request): Json<InstantiateTemplateRequest>,
) -> EnterpriseResult<TemplateInstantiationResponse> {
    require_enterprise_admin(&request_principal, verified.as_deref())?;
    ensure_policy_rules_loaded(&state).await?;
    let template = starter_policy_template(&template_id, request.version)
        .ok_or_else(|| not_found("ENTERPRISE_POLICY_TEMPLATE_NOT_FOUND"))?;
    let instantiation = template
        .instantiate(
            &request.instance_id,
            tenant_context.clone(),
            &request.overrides,
            now_ms(),
        )
        .map_err(template_validation_error)?;
    let previous;
    {
        let mut registry = state.enterprise.policy_rules.write().await;
        if registry.values().any(|rule| {
            rule.policy_id == request.instance_id && tenant_matches(rule, &tenant_context)
        }) || instantiation
            .rules
            .iter()
            .any(|rule| registry.contains_key(&rule.rule_id))
        {
            return Err(conflict("ENTERPRISE_POLICY_TEMPLATE_INSTANCE_EXISTS"));
        }
        previous = registry.clone();
        for rule in &instantiation.rules {
            registry.insert(rule.rule_id.clone(), rule.clone());
        }
    }
    commit_and_audit(
        &state,
        previous,
        &tenant_context,
        &request_principal,
        "enterprise.policy_template.instantiated",
        json!({
            "template_id": template_id,
            "template_version": template.version,
            "instance_id": request.instance_id,
            "overrides": instantiation.overrides_applied,
        }),
    )
    .await?;
    let effective_preview = preview_instantiation(&instantiation, &tenant_context);
    Ok(Json(TemplateInstantiationResponse {
        instantiation,
        template,
        effective_preview,
    }))
}

async fn rollback_policy_template(
    State(state): State<AppState>,
    Path(template_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Json(request): Json<InstantiateTemplateRequest>,
) -> EnterpriseResult<TemplateInstantiationResponse> {
    require_enterprise_admin(&request_principal, verified.as_deref())?;
    ensure_policy_rules_loaded(&state).await?;
    transition_policy_template(
        &state,
        &tenant_context,
        &request_principal,
        &template_id,
        request,
        TemplateTransition::Rollback,
    )
    .await
}

async fn upgrade_policy_template(
    State(state): State<AppState>,
    Path(template_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Json(request): Json<InstantiateTemplateRequest>,
) -> EnterpriseResult<TemplateInstantiationResponse> {
    require_enterprise_admin(&request_principal, verified.as_deref())?;
    ensure_policy_rules_loaded(&state).await?;
    transition_policy_template(
        &state,
        &tenant_context,
        &request_principal,
        &template_id,
        request,
        TemplateTransition::Upgrade,
    )
    .await
}

#[derive(Clone, Copy)]
enum TemplateTransition {
    Upgrade,
    Rollback,
}

async fn transition_policy_template(
    state: &AppState,
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    template_id: &str,
    request: InstantiateTemplateRequest,
    transition: TemplateTransition,
) -> EnterpriseResult<TemplateInstantiationResponse> {
    let template = starter_policy_template(template_id, request.version)
        .ok_or_else(|| not_found("ENTERPRISE_POLICY_TEMPLATE_VERSION_NOT_FOUND"))?;
    let revision = now_ms();
    let mut instantiation = template
        .instantiate(
            &request.instance_id,
            tenant_context.clone(),
            &request.overrides,
            revision,
        )
        .map_err(template_validation_error)?;
    for rule in &mut instantiation.rules {
        rule.state = EnterprisePolicyRuleState::Published;
        rule.rule_id = format!(
            "{}:template-v{}:revision-{revision}",
            rule.rule_id, template.version
        );
        let validation = validate_rule(rule, true);
        if !validation.errors.is_empty() {
            return validation_error(validation);
        }
    }
    let current_version;
    let previous;
    {
        let mut registry = state.enterprise.policy_rules.write().await;
        current_version = registry
            .values()
            .filter(|rule| {
                rule.policy_id == request.instance_id
                    && rule.template_id.as_deref() == Some(template_id)
                    && tenant_matches(rule, tenant_context)
                    && rule.state != EnterprisePolicyRuleState::Superseded
            })
            .filter_map(|rule| rule.template_version)
            .max()
            .ok_or_else(|| not_found("ENTERPRISE_POLICY_TEMPLATE_INSTANCE_NOT_FOUND"))?;
        match transition {
            TemplateTransition::Upgrade if template.version <= current_version => {
                return Err(conflict(
                    "ENTERPRISE_POLICY_TEMPLATE_UPGRADE_VERSION_REQUIRED",
                ));
            }
            TemplateTransition::Rollback if template.version >= current_version => {
                return Err(conflict(
                    "ENTERPRISE_POLICY_TEMPLATE_ROLLBACK_VERSION_REQUIRED",
                ));
            }
            _ => {}
        }
        previous = registry.clone();
        for rule in registry.values_mut().filter(|rule| {
            rule.policy_id == request.instance_id
                && rule.template_id.as_deref() == Some(template_id)
                && tenant_matches(rule, tenant_context)
                && rule.state != EnterprisePolicyRuleState::Superseded
        }) {
            rule.state = EnterprisePolicyRuleState::Superseded;
            rule.updated_at_ms = revision;
        }
        for rule in &instantiation.rules {
            registry.insert(rule.rule_id.clone(), rule.clone());
        }
    }
    let (event_type, action) = match transition {
        TemplateTransition::Upgrade => ("enterprise.policy_template.upgraded", "upgraded"),
        TemplateTransition::Rollback => ("enterprise.policy_template.rolled_back", "rolled_back"),
    };
    commit_and_audit(
        state,
        previous,
        tenant_context,
        request_principal,
        event_type,
        json!({
            "action": action,
            "template_id": template_id,
            "from_version": current_version,
            "to_version": template.version,
            "instance_id": request.instance_id
        }),
    )
    .await?;
    let effective_preview = preview_instantiation(&instantiation, tenant_context);
    Ok(Json(TemplateInstantiationResponse {
        instantiation,
        template,
        effective_preview,
    }))
}

#[derive(Clone, Copy)]
struct PolicyStateTransition {
    target_state: EnterprisePolicyRuleState,
    action: &'static str,
    event_type: &'static str,
    validate_for_publish: bool,
}

async fn mutate_policy_state(
    state: &AppState,
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    policy_id: &str,
    may_manage_global: bool,
    transition: PolicyStateTransition,
) -> EnterpriseResult<PolicyMutationResponse> {
    let previous = state.enterprise.policy_rules.read().await.clone();
    let mut changed = Vec::new();
    {
        let mut registry = state.enterprise.policy_rules.write().await;
        let candidates = registry
            .values()
            .filter(|rule| {
                policy_state_transition_matches(
                    rule,
                    tenant_context,
                    policy_id,
                    may_manage_global,
                    transition,
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Err(not_found("ENTERPRISE_POLICY_NOT_FOUND"));
        }
        if candidates.iter().any(|rule| rule.tenant_context.is_none())
            && candidates.iter().any(|rule| rule.tenant_context.is_some())
        {
            return Err(conflict("ENTERPRISE_POLICY_SCOPE_AMBIGUOUS"));
        }
        if transition.validate_for_publish {
            for rule in &candidates {
                let validation = validate_rule(rule, true);
                if !validation.errors.is_empty() {
                    return validation_error(validation);
                }
            }
        }
        for rule in registry.values_mut().filter(|rule| {
            policy_state_transition_matches(
                rule,
                tenant_context,
                policy_id,
                may_manage_global,
                transition,
            )
        }) {
            rule.state = transition.target_state;
            rule.updated_at_ms = now_ms();
            changed.push(rule.clone());
        }
    }
    commit_and_audit(
        state,
        previous,
        tenant_context,
        request_principal,
        transition.event_type,
        json!({"policy_id": policy_id, "rule_count": changed.len(), "state": transition.target_state}),
    )
    .await?;
    Ok(Json(PolicyMutationResponse {
        action: transition.action,
        policy_id: policy_id.to_string(),
        rules: changed,
        receipt_event: transition.event_type,
    }))
}

fn policy_state_transition_matches(
    rule: &EnterprisePolicyRule,
    tenant_context: &TenantContext,
    policy_id: &str,
    may_manage_global: bool,
    transition: PolicyStateTransition,
) -> bool {
    rule.policy_id == policy_id
        && admin_scope_matches(rule, tenant_context, may_manage_global)
        && rule.state != EnterprisePolicyRuleState::Superseded
        && (transition.target_state != EnterprisePolicyRuleState::Published
            || rule.state == EnterprisePolicyRuleState::Draft)
}

fn validate_rule(rule: &EnterprisePolicyRule, publishing: bool) -> PolicyValidationResponse {
    let mut errors = rule
        .validation_errors()
        .into_iter()
        .map(|message| PolicyValidationMessage {
            path: "rule".to_string(),
            message,
        })
        .collect::<Vec<_>>();
    if rule.tenant_context.is_none() && rule.scope_level != EnterprisePolicyScopeLevel::Enterprise {
        errors.push(PolicyValidationMessage {
            path: "scope_level".to_string(),
            message: "tenantless policy rules must use enterprise scope".to_string(),
        });
    }
    if publishing && rule.expires_at_ms.is_some_and(|expiry| expiry <= now_ms()) {
        errors.push(PolicyValidationMessage {
            path: "expires_at_ms".to_string(),
            message: "published policy expiry must be in the future".to_string(),
        });
    }
    let mut warnings = Vec::new();
    if rule.tool_patterns.is_empty() {
        warnings.push(PolicyValidationMessage {
            path: "tool_patterns".to_string(),
            message: "empty tool scope applies to every tool in the selected policy scope"
                .to_string(),
        });
    }
    PolicyValidationResponse {
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}

async fn ensure_policy_rules_loaded(state: &AppState) -> Result<(), (StatusCode, Json<Value>)> {
    state
        .ensure_enterprise_policy_rules_loaded()
        .await
        .map_err(|_| internal_error("ENTERPRISE_POLICY_LOAD_FAILED"))
}

fn preview_instantiation(
    instantiation: &PolicyTemplateInstantiation,
    tenant_context: &TenantContext,
) -> Vec<EffectivePolicySnapshot> {
    use tandem_enterprise_contract::EnterprisePolicyResolver;
    let resolver = EnterprisePolicyResolver::new(
        instantiation
            .rules
            .iter()
            .cloned()
            .map(|mut rule| {
                rule.state = EnterprisePolicyRuleState::Published;
                rule
            })
            .collect(),
    );
    instantiation
        .rules
        .iter()
        .filter_map(|rule| rule.tool_patterns.first())
        .map(|tool| {
            resolver.resolve(
                &EnterprisePolicyInput::new(tenant_context.clone()).with_tool(tool),
                now_ms(),
            )
        })
        .collect()
}

async fn commit_and_audit(
    state: &AppState,
    previous: HashMap<String, EnterprisePolicyRule>,
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    event_type: &'static str,
    payload: Value,
) -> Result<(), (StatusCode, Json<Value>)> {
    if state.persist_enterprise_policy_rules().await.is_err() {
        *state.enterprise.policy_rules.write().await = previous;
        return Err(internal_error("ENTERPRISE_POLICY_PERSIST_FAILED"));
    }
    if tandem_server::audit::append_protected_audit_event(
        state,
        event_type,
        tenant_context,
        request_principal
            .actor_id
            .clone()
            .or_else(|| Some(request_principal.source.clone())),
        payload,
    )
    .await
    .is_err()
    {
        *state.enterprise.policy_rules.write().await = previous;
        state
            .persist_enterprise_policy_rules()
            .await
            .map_err(|_| internal_error("ENTERPRISE_POLICY_ROLLBACK_FAILED"))?;
        return Err(internal_error("ENTERPRISE_POLICY_AUDIT_FAILED"));
    }
    Ok(())
}

fn tenant_matches(rule: &EnterprisePolicyRule, tenant: &TenantContext) -> bool {
    rule.tenant_context.as_ref().is_none_or(|rule_tenant| {
        enterprise_scope_ids_match(&rule_tenant.org_id, &tenant.org_id)
            && enterprise_scope_ids_match(&rule_tenant.workspace_id, &tenant.workspace_id)
            && match (
                rule_tenant.deployment_id.as_deref(),
                tenant.deployment_id.as_deref(),
            ) {
                (Some(left), Some(right)) => enterprise_scope_ids_match(left, right),
                (None, None) => true,
                _ => false,
            }
    })
}

fn target_tenant_context_matches(
    rule: &EnterprisePolicyRule,
    target_tenant: Option<&TenantContext>,
) -> bool {
    match target_tenant {
        Some(tenant) => rule.tenant_context.is_some() && tenant_matches(rule, tenant),
        None => rule.tenant_context.is_none(),
    }
}

fn admin_scope_matches(
    rule: &EnterprisePolicyRule,
    tenant: &TenantContext,
    may_manage_global: bool,
) -> bool {
    match rule.tenant_context.as_ref() {
        Some(_) => tenant_matches(rule, tenant),
        None => may_manage_global,
    }
}

fn validation_error<T>(
    validation: PolicyValidationResponse,
) -> Result<T, (StatusCode, Json<Value>)> {
    Err((
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({
            "code": "ENTERPRISE_POLICY_VALIDATION_FAILED",
            "valid": false,
            "errors": validation.errors,
            "warnings": validation.warnings,
        })),
    ))
}

fn template_validation_error(errors: Vec<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({"code":"ENTERPRISE_POLICY_TEMPLATE_VALIDATION_FAILED", "errors":errors})),
    )
}

fn bad_request(code: &'static str) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({"code":code})))
}

fn conflict(code: &'static str) -> (StatusCode, Json<Value>) {
    (StatusCode::CONFLICT, Json(json!({"code":code})))
}

fn not_found(code: &'static str) -> (StatusCode, Json<Value>) {
    (StatusCode::NOT_FOUND, Json(json!({"code":code})))
}

fn internal_error(code: &'static str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"code":code})),
    )
}

#[cfg(test)]
mod admin_scope_tests {
    use super::*;
    use tandem_enterprise_contract::{EnterprisePolicyEffect, EnterprisePolicyScopeLevel};

    #[test]
    fn workspace_admin_scope_excludes_tenantless_enterprise_rules() {
        let tenant = TenantContext::explicit("acme", "engineering", None);
        let global = EnterprisePolicyRule::new(
            "global-rule",
            "global-policy",
            EnterprisePolicyScopeLevel::Enterprise,
            EnterprisePolicyEffect::Deny,
        );
        let tenant_rule = EnterprisePolicyRule::new(
            "tenant-rule",
            "tenant-policy",
            EnterprisePolicyScopeLevel::Tenant,
            EnterprisePolicyEffect::Deny,
        )
        .with_tenant_context(tenant.clone());

        assert!(!admin_scope_matches(&global, &tenant, false));
        assert!(admin_scope_matches(&global, &tenant, true));
        assert!(admin_scope_matches(&tenant_rule, &tenant, false));
    }

    #[test]
    fn workspace_admin_scope_normalizes_tenant_ids() {
        let stored_tenant = TenantContext::explicit_user_workspace(
            " Acme ",
            " ENGINEERING ",
            Some(" Production ".to_string()),
            "admin",
        );
        let request_tenant = TenantContext::explicit_user_workspace(
            "acme",
            "engineering",
            Some("production".to_string()),
            "admin",
        );
        let other_deployment = TenantContext::explicit_user_workspace(
            "acme",
            "engineering",
            Some("staging".to_string()),
            "admin",
        );
        let missing_deployment =
            TenantContext::explicit_user_workspace("acme", "engineering", None, "admin");
        let tenant_rule = EnterprisePolicyRule::new(
            "tenant-rule",
            "tenant-policy",
            EnterprisePolicyScopeLevel::Tenant,
            EnterprisePolicyEffect::Deny,
        )
        .with_tenant_context(stored_tenant);
        let global_rule = EnterprisePolicyRule::new(
            "global-rule",
            "tenant-policy",
            EnterprisePolicyScopeLevel::Enterprise,
            EnterprisePolicyEffect::Deny,
        );

        assert!(admin_scope_matches(&tenant_rule, &request_tenant, false));
        assert!(!admin_scope_matches(&tenant_rule, &other_deployment, false));
        assert!(target_tenant_context_matches(
            &tenant_rule,
            Some(&request_tenant)
        ));
        assert!(!target_tenant_context_matches(
            &tenant_rule,
            Some(&other_deployment)
        ));
        assert!(!target_tenant_context_matches(
            &tenant_rule,
            Some(&missing_deployment)
        ));
        assert!(!target_tenant_context_matches(&tenant_rule, None));
        assert!(target_tenant_context_matches(&global_rule, None));
        assert!(!target_tenant_context_matches(
            &global_rule,
            Some(&request_tenant)
        ));
    }
}
