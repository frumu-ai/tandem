use async_trait::async_trait;
use base64::Engine;
use ed25519_dalek::Signer;
use serde_json::{json, Value};
use tandem_tools::Tool;
use tandem_types::{
    AccessDecision, AccessPermission, AssertionMetadata, AuthorityChain, CrossTenantGrant,
    CrossTenantGrantClaims, CrossTenantGrantHeader, CrossTenantGrantParty, CrossTenantGrantRecord,
    DataBoundary, DataClass, HumanActor, PrincipalRef, RequestPrincipal, ResourceKind, ResourceRef,
    ResourceScope, StrictTenantContext, TenantContext, ToolResult, ToolSchema,
    VerifiedTenantContext,
};

use crate::app::state::AppState;

pub(crate) struct EvalCrossTenantGrantProbeTool {
    state: AppState,
}

impl EvalCrossTenantGrantProbeTool {
    pub(crate) const NAME: &'static str = "eval.cross_tenant_grant_probe";
    const KEY_ID: &'static str = "eval-cross-tenant-grant-key";

    pub(crate) fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Tool for EvalCrossTenantGrantProbeTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            Self::NAME,
            "Eval-only positive cross-tenant grant probe for active sharing, bounded denial, revocation, and audit attribution.",
            json!({
                "type": "object",
                "properties": {
                    "scenario": {
                        "type": "string",
                        "description": "Grant scenario to probe: active_share or revoked_share."
                    },
                    "issuer_tenant_id": {
                        "type": "string",
                        "description": "Issuer tenant/org id that owns the shared resource."
                    },
                    "audience_tenant_id": {
                        "type": "string",
                        "description": "Audience tenant/org id executing the read."
                    },
                    "grant_id": {
                        "type": "string",
                        "description": "Deterministic grant id for this eval probe."
                    }
                },
                "required": ["scenario"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.execute_for_tenant(args, TenantContext::local_implicit())
            .await
    }

    async fn execute_for_tenant(
        &self,
        args: Value,
        tenant_context: TenantContext,
    ) -> anyhow::Result<ToolResult> {
        let scenario = args
            .get("scenario")
            .and_then(Value::as_str)
            .unwrap_or("active_share");
        let issuer_tenant_id = args
            .get("issuer_tenant_id")
            .and_then(Value::as_str)
            .unwrap_or("tenant-a");
        let audience_tenant_id = args
            .get("audience_tenant_id")
            .and_then(Value::as_str)
            .unwrap_or("tenant-b");
        let grant_id = args
            .get("grant_id")
            .and_then(Value::as_str)
            .unwrap_or("ct14-finance-share");

        if tenant_context.org_id != audience_tenant_id {
            anyhow::bail!(
                "cross-tenant grant probe must execute as audience tenant `{}`, got `{}`",
                audience_tenant_id,
                tenant_context.org_id
            );
        }

        let issuer = eval_tenant_context(issuer_tenant_id);
        let audience = eval_tenant_context(audience_tenant_id);
        let subject = PrincipalRef::human_user(format!("{audience_tenant_id}-eval-actor"));
        let shared_resource = ResourceRef::new(
            issuer.org_id.clone(),
            issuer.workspace_id.clone(),
            ResourceKind::DocumentCollection,
            format!("finance-drive-{grant_id}"),
        );
        let out_of_scope_resource = ResourceRef::new(
            issuer.org_id.clone(),
            issuer.workspace_id.clone(),
            ResourceKind::DocumentCollection,
            "legal-drive",
        );
        let now_ms = crate::now_ms();
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[19u8; 32]);
        let _keyring_guard = PublicKeyringEnvGuard::install(
            Self::KEY_ID,
            &base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(signing_key.verifying_key().to_bytes()),
        );

        let mut record = signed_record(
            grant_id,
            &issuer,
            &audience,
            &subject,
            &shared_resource,
            now_ms,
            &signing_key,
        )?;
        if scenario == "revoked_share" {
            record.revoke(
                now_ms + 1,
                PrincipalRef::human_user(format!("{issuer_tenant_id}-eval-actor")),
                Some("ct-14 revocation eval".to_string()),
                Some("ct14-policy-revocation".to_string()),
                Some("ct14-audit-revocation".to_string()),
            );
        } else if scenario != "active_share" {
            anyhow::bail!("unsupported cross-tenant grant probe scenario `{scenario}`");
        }

        self.state
            .enterprise_cross_tenant_grants
            .write()
            .await
            .insert(format!("eval::{grant_id}"), record.clone());

        append_grant_audit(
            &self.state,
            "eval.cross_tenant_grant.issued",
            &issuer,
            &record,
        )
        .await?;
        if record.revocation.is_some() {
            append_grant_audit(
                &self.state,
                "eval.cross_tenant_grant.revoked",
                &issuer,
                &record,
            )
            .await?;
        }

        let mut verified = verified_audience_context(
            audience.clone(),
            subject.clone(),
            now_ms,
            vec![DataClass::FinancialRecord],
        );
        crate::http::cross_tenant_grants::enrich_verified_context_with_inbound_cross_tenant_grants(
            &self.state,
            &mut verified,
        )
        .await;
        let strict = verified
            .strict_projection
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("audience strict context missing"))?;

        let financial = strict.evaluate_access(
            &shared_resource,
            AccessPermission::Read,
            DataClass::FinancialRecord,
            now_ms + 2,
        );
        let restricted = strict.evaluate_access(
            &shared_resource,
            AccessPermission::Read,
            DataClass::Restricted,
            now_ms + 2,
        );
        let out_of_scope = strict.evaluate_access(
            &out_of_scope_resource,
            AccessPermission::Read,
            DataClass::FinancialRecord,
            now_ms + 2,
        );

        let expected_financial_decision = if scenario == "active_share" {
            AccessDecision::Allow
        } else {
            AccessDecision::NotApplicable
        };
        if financial.decision != expected_financial_decision {
            anyhow::bail!(
                "expected {:?} for granted financial resource in {}, got {:?} ({})",
                expected_financial_decision,
                scenario,
                financial.decision,
                financial.reason
            );
        }
        if restricted.decision == AccessDecision::Allow {
            anyhow::bail!("restricted data class was allowed by a financial-only grant");
        }
        if out_of_scope.decision == AccessDecision::Allow {
            anyhow::bail!("out-of-scope resource was allowed by the cross-tenant grant");
        }

        crate::audit::append_protected_audit_event(
            &self.state,
            "eval.cross_tenant_grant.access_evaluated",
            &audience,
            Some(subject.id.clone()),
            json!({
                "grant_id": grant_id,
                "scenario": scenario,
                "issuer_tenant": CrossTenantGrantParty::from_tenant_context(&issuer),
                "audience_tenant": CrossTenantGrantParty::from_tenant_context(&audience),
                "financial_decision": financial,
                "restricted_decision": restricted,
                "out_of_scope_decision": out_of_scope,
            }),
        )
        .await?;
        assert_dual_tenant_audit(&self.state, &issuer, &audience, grant_id).await?;

        Ok(ToolResult {
            output: format!("cross-tenant grant probe `{scenario}` passed"),
            metadata: json!({
                "scenario": scenario,
                "grant_id": grant_id,
                "issuer_tenant": issuer,
                "audience_tenant": audience,
                "financial_decision": financial,
                "restricted_decision": restricted,
                "out_of_scope_decision": out_of_scope,
                "audit_event": "dual_tenant_protected_audit_observed",
                "revoked": record.revocation.is_some(),
            }),
        })
    }
}

fn signed_record(
    grant_id: &str,
    issuer: &TenantContext,
    audience: &TenantContext,
    subject: &PrincipalRef,
    resource: &ResourceRef,
    now_ms: u64,
    signing_key: &ed25519_dalek::SigningKey,
) -> anyhow::Result<CrossTenantGrantRecord> {
    let mut claims = CrossTenantGrantClaims::new_v1(
        grant_id,
        CrossTenantGrantParty::from_tenant_context(issuer),
        CrossTenantGrantParty::from_tenant_context(audience),
        subject.clone(),
        ResourceScope::root(resource.clone()),
        vec![AccessPermission::Read],
        vec![DataClass::FinancialRecord],
        now_ms,
        now_ms + 86_400_000,
        PrincipalRef::human_user(format!("{}-eval-actor", issuer.org_id)),
    );
    claims.source_policy_decision_id = Some("ct14-policy-issuance".to_string());
    claims.source_audit_event_id = Some("ct14-audit-issuance".to_string());
    claims.approval_id = Some("ct14-approval-issuance".to_string());

    let header = CrossTenantGrantHeader::ed25519(EvalCrossTenantGrantProbeTool::KEY_ID);
    let encoded_header = encode_json_base64url(&header)?;
    let encoded_claims = encode_json_base64url(&claims)?;
    let signing_input = format!("{encoded_header}.{encoded_claims}");
    let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(signing_key.sign(signing_input.as_bytes()).to_bytes());
    Ok(CrossTenantGrantRecord::active(
        CrossTenantGrant::new(header, claims, signature),
        now_ms,
    ))
}

fn verified_audience_context(
    audience: TenantContext,
    subject: PrincipalRef,
    now_ms: u64,
    data_classes: Vec<DataClass>,
) -> VerifiedTenantContext {
    let request_principal = RequestPrincipal::authenticated_user(subject.id.clone(), "eval");
    let strict = StrictTenantContext::new(
        audience.clone(),
        subject,
        AuthorityChain::from_request(request_principal.clone()),
        ResourceScope::root(ResourceRef::new(
            audience.org_id.clone(),
            audience.workspace_id.clone(),
            ResourceKind::Workspace,
            audience.workspace_id.clone(),
        )),
        AssertionMetadata::new(
            "eval-issuer",
            "eval-runtime",
            now_ms,
            now_ms + 86_400_000,
            "ct14-assertion",
        ),
    )
    .with_data_boundary(DataBoundary::allow(data_classes));

    VerifiedTenantContext {
        tenant_context: audience,
        human_actor: HumanActor::tandem_user(
            request_principal.actor_id.clone().unwrap_or_default(),
        ),
        authority_chain: AuthorityChain::from_request(request_principal),
        roles: Vec::new(),
        org_units: Vec::new(),
        capabilities: Vec::new(),
        policy_version: None,
        strict_projection: Some(strict),
        issuer: "eval-issuer".to_string(),
        audience: "eval-runtime".to_string(),
        issued_at_ms: now_ms,
        expires_at_ms: now_ms + 86_400_000,
        assertion_id: "ct14-assertion".to_string(),
    }
}

async fn append_grant_audit(
    state: &AppState,
    event_type: &'static str,
    tenant_context: &TenantContext,
    record: &CrossTenantGrantRecord,
) -> anyhow::Result<()> {
    crate::audit::append_protected_audit_event(
        state,
        event_type,
        tenant_context,
        tenant_context.actor_id.clone(),
        json!({
            "grant_id": record.grant.claims.grant_id,
            "issuer_tenant": record.grant.claims.issuer,
            "audience_tenant": record.grant.claims.audience,
            "subject": record.grant.claims.subject,
            "resource_scope": record.grant.claims.resource_scope,
            "permissions": record.grant.claims.permissions,
            "data_classes": record.grant.claims.data_classes,
            "state": record.state,
            "revocation": record.revocation,
            "source_policy_decision_id": record.grant.claims.source_policy_decision_id,
            "source_audit_event_id": record.grant.claims.source_audit_event_id,
            "approval_id": record.grant.claims.approval_id,
        }),
    )
    .await
}

async fn assert_dual_tenant_audit(
    state: &AppState,
    issuer: &TenantContext,
    audience: &TenantContext,
    grant_id: &str,
) -> anyhow::Result<()> {
    let raw = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .unwrap_or_default();
    if !raw.contains(grant_id)
        || !raw.contains(&issuer.org_id)
        || !raw.contains(&audience.org_id)
        || !raw.contains("eval.cross_tenant_grant.issued")
        || !raw.contains("eval.cross_tenant_grant.access_evaluated")
    {
        anyhow::bail!(
            "cross-tenant grant probe did not observe protected audit rows for both tenants"
        );
    }
    Ok(())
}

fn eval_tenant_context(tenant_id: &str) -> TenantContext {
    TenantContext::explicit_user_workspace(
        tenant_id,
        "eval-workspace",
        Some("eval-deployment".to_string()),
        format!("{tenant_id}-eval-actor"),
    )
}

fn encode_json_base64url<T: serde::Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_vec(value)
        .map(|bytes| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

struct PublicKeyringEnvGuard {
    previous: Option<String>,
}

impl PublicKeyringEnvGuard {
    fn install(kid: &str, public_key: &str) -> Self {
        let previous = std::env::var("TANDEM_CROSS_TENANT_GRANT_PUBLIC_KEYS").ok();
        std::env::set_var(
            "TANDEM_CROSS_TENANT_GRANT_PUBLIC_KEYS",
            format!("{kid}={public_key}"),
        );
        Self { previous }
    }
}

impl Drop for PublicKeyringEnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var("TANDEM_CROSS_TENANT_GRANT_PUBLIC_KEYS", previous);
        } else {
            std::env::remove_var("TANDEM_CROSS_TENANT_GRANT_PUBLIC_KEYS");
        }
    }
}
