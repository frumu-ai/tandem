use tandem_types::{PrincipalKind, TenantContext, VerifiedTenantContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorySubjectPolicyMode {
    LocalFallback,
    LocalTenantActor,
    EnterpriseVerifiedActor,
    EnterpriseStrictPrincipal,
    EnterpriseBlocked,
}

impl MemorySubjectPolicyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalFallback => "local_fallback",
            Self::LocalTenantActor => "local_tenant_actor",
            Self::EnterpriseVerifiedActor => "enterprise_verified_actor",
            Self::EnterpriseStrictPrincipal => "enterprise_strict_principal",
            Self::EnterpriseBlocked => "enterprise_blocked",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySubjectAudit {
    pub selected_subject: Option<String>,
    pub policy_mode: MemorySubjectPolicyMode,
    pub requested_client_id: Option<String>,
    pub verified_actor: Option<String>,
    pub delegated_subject: Option<String>,
    pub tenant_scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySubjectResolution {
    pub subject: String,
    pub audit: MemorySubjectAudit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorySubjectResolutionError {
    MissingVerifiedActor,
}

impl MemorySubjectResolutionError {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingVerifiedActor => "missing verified memory subject",
        }
    }
}

pub fn normalize_memory_subject(subject_hint: Option<&str>) -> String {
    normalized(subject_hint).unwrap_or_else(|| "default".to_string())
}

pub fn local_memory_subject(subject_hint: Option<&str>) -> MemorySubjectResolution {
    let subject = normalize_memory_subject(subject_hint);
    MemorySubjectResolution {
        subject: subject.clone(),
        audit: MemorySubjectAudit {
            selected_subject: Some(subject),
            policy_mode: MemorySubjectPolicyMode::LocalFallback,
            requested_client_id: normalized(subject_hint),
            verified_actor: None,
            delegated_subject: None,
            tenant_scope: None,
        },
    }
}

pub fn request_memory_subject(
    tenant_context: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    local_subject_hint: Option<&str>,
) -> Result<MemorySubjectResolution, MemorySubjectResolutionError> {
    if let Some(verified) = verified {
        return verified_memory_subject(verified, local_subject_hint);
    }
    if let Some(subject) = normalized(local_subject_hint) {
        return Ok(local_memory_subject(Some(subject.as_str())));
    }
    if let Some(subject) = normalized(tenant_context.actor_id.as_deref()) {
        return Ok(MemorySubjectResolution {
            subject: subject.clone(),
            audit: MemorySubjectAudit {
                selected_subject: Some(subject),
                policy_mode: MemorySubjectPolicyMode::LocalTenantActor,
                requested_client_id: None,
                verified_actor: None,
                delegated_subject: None,
                tenant_scope: Some(tenant_scope(tenant_context)),
            },
        });
    }
    Ok(local_memory_subject(None))
}

pub fn verified_memory_subject(
    verified: &VerifiedTenantContext,
    requested_client_id: Option<&str>,
) -> Result<MemorySubjectResolution, MemorySubjectResolutionError> {
    let strict_principal = verified
        .strict_projection
        .as_ref()
        .map(|projection| &projection.principal);
    let strict_tenant_actor =
        strict_principal.and_then(|principal| normalized(principal.tenant_actor_id.as_deref()));
    let strict_subject = strict_principal.and_then(|principal| normalized(Some(&principal.id)));
    let verified_actor = normalized(verified.tenant_context.actor_id.as_deref())
        .or_else(|| normalized(Some(&verified.human_actor.actor_id)));

    let (subject, policy_mode) = if let Some(subject) = strict_tenant_actor {
        (subject, MemorySubjectPolicyMode::EnterpriseStrictPrincipal)
    } else if let Some(subject) = strict_subject {
        (subject, MemorySubjectPolicyMode::EnterpriseStrictPrincipal)
    } else if let Some(subject) = verified_actor.clone() {
        (subject, MemorySubjectPolicyMode::EnterpriseVerifiedActor)
    } else {
        return Err(MemorySubjectResolutionError::MissingVerifiedActor);
    };

    let delegated_subject = strict_principal.and_then(|principal| {
        let principal_id = normalized(Some(&principal.id))?;
        let is_delegated = principal.kind != PrincipalKind::HumanUser || principal_id != subject;
        is_delegated.then_some(principal_id)
    });

    Ok(MemorySubjectResolution {
        subject: subject.clone(),
        audit: MemorySubjectAudit {
            selected_subject: Some(subject),
            policy_mode,
            requested_client_id: normalized(requested_client_id),
            verified_actor,
            delegated_subject,
            tenant_scope: Some(tenant_scope(&verified.tenant_context)),
        },
    })
}

pub fn blocked_memory_subject_audit(
    tenant_context: Option<&TenantContext>,
    verified: Option<&VerifiedTenantContext>,
    requested_client_id: Option<&str>,
) -> MemorySubjectAudit {
    let resolved =
        verified.and_then(|context| verified_memory_subject(context, requested_client_id).ok());
    MemorySubjectAudit {
        selected_subject: resolved
            .as_ref()
            .and_then(|resolution| resolution.audit.selected_subject.clone()),
        policy_mode: MemorySubjectPolicyMode::EnterpriseBlocked,
        requested_client_id: normalized(requested_client_id),
        verified_actor: verified
            .and_then(verified_actor)
            .or_else(|| tenant_context.and_then(|tenant| normalized(tenant.actor_id.as_deref()))),
        delegated_subject: resolved
            .as_ref()
            .and_then(|resolution| resolution.audit.delegated_subject.clone()),
        tenant_scope: verified
            .map(|context| tenant_scope(&context.tenant_context))
            .or_else(|| tenant_context.map(tenant_scope)),
    }
}

pub fn local_memory_subjects_are_unrestricted(
    tenant_context: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
) -> bool {
    verified.is_none() && normalized(tenant_context.actor_id.as_deref()).is_none()
}

fn verified_actor(verified: &VerifiedTenantContext) -> Option<String> {
    normalized(verified.tenant_context.actor_id.as_deref())
        .or_else(|| normalized(Some(&verified.human_actor.actor_id)))
}

fn tenant_scope(tenant_context: &TenantContext) -> String {
    match tenant_context.deployment_id.as_deref() {
        Some(deployment_id) if !deployment_id.trim().is_empty() => format!(
            "{}/{}/{}",
            tenant_context.org_id, tenant_context.workspace_id, deployment_id
        ),
        _ => format!("{}/{}", tenant_context.org_id, tenant_context.workspace_id),
    }
}

fn normalized(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_types::{
        AssertionMetadata, AuthorityChain, DataBoundary, HumanActor, PrincipalRef,
        RequestPrincipal, ResourceKind, ResourceRef, ResourceScope, StrictTenantContext,
    };

    fn verified_context(
        actor_id: &str,
        strict_principal: Option<PrincipalRef>,
    ) -> VerifiedTenantContext {
        let tenant_context = TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("dep-a".to_string()),
            actor_id,
        );
        let principal = RequestPrincipal::authenticated_user(actor_id, "tandem-web");
        let authority_chain = AuthorityChain::from_request(principal);
        let strict_projection = strict_principal.map(|principal| {
            StrictTenantContext::new(
                tenant_context.clone(),
                principal,
                authority_chain.clone(),
                ResourceScope::root(ResourceRef::new(
                    "org-a",
                    "workspace-a",
                    ResourceKind::Workspace,
                    "workspace-a",
                )),
                AssertionMetadata::new(
                    "tandem-web",
                    "tandem-runtime",
                    1_000,
                    10_000,
                    "assertion-a",
                ),
            )
            .with_data_boundary(DataBoundary::allow(vec![]))
        });
        VerifiedTenantContext {
            tenant_context,
            human_actor: HumanActor::tandem_user(actor_id),
            authority_chain,
            roles: Vec::new(),
            org_units: Vec::new(),
            capabilities: Vec::new(),
            policy_version: None,
            strict_projection,
            issuer: "tandem-web".to_string(),
            audience: "tandem-runtime".to_string(),
            issued_at_ms: 1_000,
            expires_at_ms: 10_000,
            assertion_id: "assertion-a".to_string(),
            assertion_key_id: None,
        }
    }

    #[test]
    fn verified_subject_ignores_client_subject() {
        let verified = verified_context("user-a", None);
        let resolution =
            verified_memory_subject(&verified, Some("forged-client")).expect("verified subject");

        assert_eq!(resolution.subject, "user-a");
        assert_eq!(
            resolution.audit.requested_client_id.as_deref(),
            Some("forged-client")
        );
        assert_eq!(resolution.audit.verified_actor.as_deref(), Some("user-a"));
        assert_eq!(
            resolution.audit.policy_mode,
            MemorySubjectPolicyMode::EnterpriseVerifiedActor
        );
    }

    #[test]
    fn strict_agent_subject_uses_tenant_actor_and_audits_delegate() {
        let verified = verified_context(
            "user-a",
            Some(PrincipalRef::agent_worker("agent-platform").with_tenant_actor_id("user-a")),
        );
        let resolution = verified_memory_subject(&verified, Some("forged-client"))
            .expect("strict agent subject");

        assert_eq!(resolution.subject, "user-a");
        assert_eq!(resolution.audit.verified_actor.as_deref(), Some("user-a"));
        assert_eq!(
            resolution.audit.delegated_subject.as_deref(),
            Some("agent-platform")
        );
        assert_eq!(
            resolution.audit.policy_mode,
            MemorySubjectPolicyMode::EnterpriseStrictPrincipal
        );
    }

    #[test]
    fn strict_external_delegate_subject_uses_delegate_id() {
        let verified = verified_context(
            "user-a",
            Some(PrincipalRef::new(
                PrincipalKind::ExternalDelegate,
                "a2a-worker-1",
            )),
        );
        let resolution = verified_memory_subject(&verified, None).expect("strict delegate subject");

        assert_eq!(resolution.subject, "a2a-worker-1");
        assert_eq!(
            resolution.audit.delegated_subject.as_deref(),
            Some("a2a-worker-1")
        );
    }

    #[test]
    fn local_subject_preserves_local_hint() {
        let tenant_context = TenantContext::local_implicit();
        let resolution = request_memory_subject(&tenant_context, None, Some("local-client"))
            .expect("local subject");

        assert_eq!(resolution.subject, "local-client");
        assert_eq!(
            resolution.audit.policy_mode,
            MemorySubjectPolicyMode::LocalFallback
        );
    }
}
