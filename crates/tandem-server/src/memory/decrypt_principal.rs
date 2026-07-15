// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Build a memory decrypt principal from a verified request context (TAN-672).
//!
//! TAN-668 sealed hosted-KMS memory rows behind an authorized
//! [`MemoryDecryptPrincipal`], threaded into `tandem-memory` via the task-local
//! `with_decrypt_principal`. This module is the missing server-side half: it
//! projects a request's [`VerifiedTenantContext`] into that principal so a hosted
//! read decrypts only the scope the caller is actually authorized for, and mirrors
//! the governed-read data boundary so the DEK scope and the access scope agree.
//!
//! Fail-closed by construction: returns `None` for a local/single-tenant request
//! (no strict projection) or one with no readable data classes. With `None` no
//! principal is scoped, so local rows (NULL `crypto_envelope`) read normally and
//! genuinely hosted-sealed rows fail closed.

use tandem_memory::types::{effective_data_boundary_for_governed_read, MemoryTenantScope};
use tandem_memory::MemoryDecryptPrincipal;
use tandem_types::ResourceKind;
use tandem_types::{
    AccessEffect, AccessPermission, ResourceRef, StrictTenantContext, TenantContext,
    VerifiedTenantContext,
};

/// Project a verified request context into a memory decrypt principal.
///
/// `None` (fail-closed) when there is no strict projection (local/single-tenant),
/// no explicit actor, or no readable data classes to grant.
pub fn memory_decrypt_principal_from_verified_context(
    verified: &VerifiedTenantContext,
    now_ms: u64,
) -> Option<MemoryDecryptPrincipal> {
    let strict = verified.strict_projection.as_ref()?;
    let principal_id = verified_actor_id(verified)?;
    let tenant_scope = MemoryTenantScope {
        org_id: verified.tenant_context.org_id.clone(),
        workspace_id: verified.tenant_context.workspace_id.clone(),
        deployment_id: verified.tenant_context.deployment_id.clone(),
    };
    // The effective allow-list of data classes the caller may read — grants ∪
    // boundary — computed exactly as the governed-read filter does, so the DEK
    // scope a row is sealed under and the class the caller is authorized for
    // agree. An empty allow-list (a pure deny-list boundary) cannot be expressed
    // as an explicit principal grant, so fail closed rather than guess "all".
    let allowed_data_classes =
        effective_data_boundary_for_governed_read(strict, now_ms).allowed_data_classes;
    if allowed_data_classes.is_empty() {
        return None;
    }
    let allowed_source_binding_ids = source_binding_grants(strict, now_ms);
    let owner_subject = super::subject::verified_memory_subject(verified, None)
        .ok()?
        .subject;
    Some(
        MemoryDecryptPrincipal::retrieval_gateway(
            principal_id,
            tenant_scope,
            allowed_data_classes,
            allowed_source_binding_ids,
        )
        .with_owner_subjects(vec![owner_subject]),
    )
}

/// The workspace-level memory-space resource that context (tree/layer) reads
/// operate on. Context nodes and their layers are tenant/workspace-scoped and
/// seal under the tenant `Internal` key scope — not a per-resource scope — so
/// the crypto boundary cannot distinguish two same-tenant nodes. A caller must
/// therefore hold a resource scope that covers this workspace memory space
/// before we grant a scoped decrypt (TAN-672 review).
pub fn workspace_memory_space_resource(tenant_context: &TenantContext) -> ResourceRef {
    ResourceRef::new(
        tenant_context.org_id.clone(),
        tenant_context.workspace_id.clone(),
        ResourceKind::MemorySpace,
        tenant_context.workspace_id.clone(),
    )
}

/// True when the verified request's strict resource scope covers `resource`.
/// Fail-closed: `false` when there is no strict projection. Used to gate a
/// scoped decrypt so a caller whose projection is narrower than the tenant (e.g.
/// scoped to one project or data room) cannot decrypt out-of-scope same-tenant
/// content that merely shares the tenant `Internal` key scope.
pub fn verified_resource_scope_covers(
    verified: &VerifiedTenantContext,
    resource: &ResourceRef,
) -> bool {
    verified
        .strict_projection
        .as_ref()
        .map(|strict| strict.resource_scope.contains(resource))
        .unwrap_or(false)
}

/// The verified actor id (never a client-supplied value): the tenant-context
/// actor, else the human actor. `None` when neither is explicit.
fn verified_actor_id(verified: &VerifiedTenantContext) -> Option<String> {
    normalized(verified.tenant_context.actor_id.as_deref())
        .or_else(|| normalized(Some(&verified.human_actor.actor_id)))
}

/// Source-binding ids the caller holds an active read grant for. Only these let a
/// source-bound (connector-ingested) row's envelope be unwrapped; a row with no
/// source binding needs no such grant.
fn source_binding_grants(strict: &StrictTenantContext, now_ms: u64) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for grant in strict.grants.iter().filter(|grant| {
        grant.effect == AccessEffect::Allow
            && !grant.is_expired_at(now_ms)
            && grant.has_permission(AccessPermission::Read)
    }) {
        for id in source_binding_ids_in_resource(&grant.resource) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

fn source_binding_ids_in_resource(resource: &ResourceRef) -> Vec<String> {
    let mut ids = Vec::new();
    if resource.resource_kind == ResourceKind::SourceBinding {
        if let Some(id) = normalized(Some(&resource.resource_id)) {
            ids.push(id);
        }
    }
    for segment in &resource.parent_path {
        if segment.kind == ResourceKind::SourceBinding {
            if let Some(id) = normalized(Some(&segment.id)) {
                ids.push(id);
            }
        }
    }
    ids
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
    use tandem_enterprise_contract::DataClass;
    use tandem_types::{
        AssertionMetadata, AuthorityChain, DataBoundary, GrantSource, HumanActor, PrincipalRef,
        RequestPrincipal, ResourcePathSegment, ResourceScope, ScopedGrant, TenantContext,
    };

    fn base_verified(strict: Option<StrictTenantContext>) -> VerifiedTenantContext {
        let tenant_context = TenantContext::explicit_user_workspace(
            "acme",
            "hq",
            Some("prod".to_string()),
            "user-finance",
        );
        let principal = RequestPrincipal::authenticated_user("user-finance", "tandem-web");
        let authority_chain = AuthorityChain::from_request(principal);
        VerifiedTenantContext {
            tenant_context,
            human_actor: HumanActor::tandem_user("user-finance"),
            authority_chain,
            roles: Vec::new(),
            org_units: Vec::new(),
            capabilities: Vec::new(),
            policy_version: None,
            strict_projection: strict,
            issuer: "tandem-web".to_string(),
            audience: "tandem-runtime".to_string(),
            issued_at_ms: 1_000,
            expires_at_ms: 10_000,
            assertion_id: "assertion-a".to_string(),
            assertion_key_id: None,
        }
    }

    fn strict_with(boundary: DataBoundary, grants: Vec<ScopedGrant>) -> StrictTenantContext {
        let tenant_context = TenantContext::explicit_user_workspace(
            "acme",
            "hq",
            Some("prod".to_string()),
            "user-finance",
        );
        let principal = PrincipalRef::human_user("user-finance");
        let authority_chain = AuthorityChain::from_request(RequestPrincipal::authenticated_user(
            "user-finance",
            "web",
        ));
        let mut strict = StrictTenantContext::new(
            tenant_context.clone(),
            principal,
            authority_chain,
            ResourceScope::root(ResourceRef::new(
                "acme",
                "hq",
                ResourceKind::Workspace,
                "hq",
            )),
            AssertionMetadata::new("web", "runtime", 1_000, 10_000, "assertion-a"),
        )
        .with_data_boundary(boundary);
        strict.grants = grants;
        strict
    }

    fn read_grant(resource: ResourceRef, data_classes: Vec<DataClass>) -> ScopedGrant {
        ScopedGrant {
            grant_id: "grant-1".to_string(),
            principal: PrincipalRef::human_user("user-finance"),
            resource,
            effect: AccessEffect::Allow,
            permissions: vec![AccessPermission::Read],
            data_classes,
            tool_patterns: Vec::new(),
            grant_source: GrantSource::Direct,
            source_principal: None,
            expires_at_ms: None,
            delegation_id: None,
        }
    }

    #[test]
    fn no_strict_projection_is_fail_closed_none() {
        assert!(
            memory_decrypt_principal_from_verified_context(&base_verified(None), 2_000).is_none()
        );
    }

    #[test]
    fn explicit_boundary_becomes_allowed_data_classes() {
        let strict = strict_with(
            DataBoundary::allow(vec![DataClass::FinancialRecord]),
            Vec::new(),
        );
        let principal =
            memory_decrypt_principal_from_verified_context(&base_verified(Some(strict)), 2_000)
                .expect("hosted principal");
        assert_eq!(principal.tenant_scope.org_id, "acme");
        assert_eq!(principal.allowed_owner_subjects, vec!["user-finance"]);
        assert_eq!(
            principal.tenant_scope.deployment_id.as_deref(),
            Some("prod")
        );
        assert_eq!(
            principal.allowed_data_classes,
            vec![DataClass::FinancialRecord]
        );
        assert_eq!(principal.principal_id, "user-finance");
    }

    #[test]
    fn unrestricted_boundary_derives_classes_from_read_grants() {
        let grant = read_grant(
            ResourceRef::new("acme", "hq", ResourceKind::MemorySpace, "mem"),
            vec![DataClass::Confidential],
        );
        let strict = strict_with(DataBoundary::unrestricted(), vec![grant]);
        let principal =
            memory_decrypt_principal_from_verified_context(&base_verified(Some(strict)), 2_000)
                .expect("hosted principal");
        assert_eq!(
            principal.allowed_data_classes,
            vec![DataClass::Confidential]
        );
    }

    #[test]
    fn source_binding_read_grant_is_extracted() {
        let mut resource = ResourceRef::new(
            "acme",
            "hq",
            ResourceKind::SourceBinding,
            "notion-finance-db",
        );
        resource.parent_path = vec![ResourcePathSegment::new(
            ResourceKind::ConnectorInstance,
            "notion",
        )];
        let strict = strict_with(
            DataBoundary::allow(vec![DataClass::FinancialRecord]),
            vec![read_grant(resource, vec![DataClass::FinancialRecord])],
        );
        let principal =
            memory_decrypt_principal_from_verified_context(&base_verified(Some(strict)), 2_000)
                .expect("hosted principal");
        assert_eq!(
            principal.allowed_source_binding_ids,
            vec!["notion-finance-db".to_string()]
        );
    }

    #[test]
    fn deny_only_boundary_is_fail_closed_none() {
        // allowed empty + denied non-empty is not unrestricted, so the effective
        // boundary keeps an empty allow-list — inexpressible as a principal grant.
        let boundary = DataBoundary {
            allowed_data_classes: Vec::new(),
            denied_data_classes: vec![DataClass::Restricted],
        };
        let strict = strict_with(boundary, Vec::new());
        assert!(memory_decrypt_principal_from_verified_context(
            &base_verified(Some(strict)),
            2_000
        )
        .is_none());
    }

    fn project_scoped_strict(project_id: &str) -> StrictTenantContext {
        let tenant_context = TenantContext::explicit_user_workspace(
            "acme",
            "hq",
            Some("prod".to_string()),
            "user-finance",
        );
        let authority_chain = AuthorityChain::from_request(RequestPrincipal::authenticated_user(
            "user-finance",
            "web",
        ));
        let mut project_ref = ResourceRef::new("acme", "hq", ResourceKind::Project, project_id);
        project_ref.project_id = Some(project_id.to_string());
        StrictTenantContext::new(
            tenant_context,
            PrincipalRef::human_user("user-finance"),
            authority_chain,
            ResourceScope::root(project_ref),
            AssertionMetadata::new("web", "runtime", 1_000, 10_000, "assertion-a"),
        )
        .with_data_boundary(DataBoundary::allow(vec![DataClass::Internal]))
    }

    #[test]
    fn workspace_scope_covers_workspace_memory() {
        let verified = base_verified(Some(strict_with(
            DataBoundary::allow(vec![DataClass::Internal]),
            Vec::new(),
        )));
        let workspace_memory = workspace_memory_space_resource(&verified.tenant_context);
        assert!(verified_resource_scope_covers(&verified, &workspace_memory));
    }

    #[test]
    fn project_scope_does_not_cover_workspace_memory() {
        // A projection scoped to one project is narrower than the workspace memory
        // space context layers seal under, so a scoped decrypt is denied.
        let verified = base_verified(Some(project_scoped_strict("project-x")));
        let workspace_memory = workspace_memory_space_resource(&verified.tenant_context);
        assert!(!verified_resource_scope_covers(
            &verified,
            &workspace_memory
        ));
    }

    /// End-to-end server-boundary test: the principal this module derives from a
    /// verified context actually authorizes (or denies) a real hosted-KMS sealed
    /// memory read via `with_decrypt_principal` — the exact composition the
    /// `/memory/context/*` handlers perform.
    mod hosted {
        use super::*;
        use tandem_memory::db::MemoryDatabase;
        use tandem_memory::decrypt_context::with_decrypt_principal;
        use tandem_memory::dek_cache::MemoryDekCache;
        use tandem_memory::envelope_crypto::HostedMemoryEnvelopeCrypto;
        use tandem_memory::types::{LayerType, MemoryResult, MemoryTenantScope, NodeType};
        use tandem_memory::{
            GoogleCloudKmsDecryptClient, GoogleCloudKmsDecryptRequest,
            GoogleCloudKmsDekUnwrapProvider, GoogleCloudKmsDekWrapProvider,
            GoogleCloudKmsEncryptClient, GoogleCloudKmsEncryptRequest, MemoryCryptoProvider,
            MemoryDecryptBroker, MemoryDecryptBrokerConfig,
        };

        const RUNTIME_PRINCIPAL: &str = "runtime-memory-decryptor";
        const PROVIDER_ID: &str = "google_cloud_kms";
        const KEK_ID: &str = "projects/acme/locations/global/keyRings/memory/cryptoKeys/finance";

        /// Reversible keyed-XOR KMS so a DEK round-trips in-process.
        #[derive(Clone)]
        struct XorFixtureKms {
            fingerprint: u8,
        }
        impl GoogleCloudKmsEncryptClient for XorFixtureKms {
            fn encrypt(&self, request: &GoogleCloudKmsEncryptRequest) -> MemoryResult<Vec<u8>> {
                Ok(request
                    .plaintext
                    .iter()
                    .map(|byte| byte ^ self.fingerprint)
                    .collect())
            }
        }
        impl GoogleCloudKmsDecryptClient for XorFixtureKms {
            fn decrypt(&self, request: &GoogleCloudKmsDecryptRequest) -> MemoryResult<Vec<u8>> {
                Ok(request
                    .ciphertext
                    .iter()
                    .map(|byte| byte ^ self.fingerprint)
                    .collect())
            }
        }

        fn hosted_provider() -> MemoryCryptoProvider {
            let config = MemoryDecryptBrokerConfig::hosted(PROVIDER_ID, RUNTIME_PRINCIPAL).unwrap();
            let broker = MemoryDecryptBroker::new(config).unwrap();
            let kms = XorFixtureKms { fingerprint: 0x5A };
            let wrap = GoogleCloudKmsDekWrapProvider::new(kms.clone(), RUNTIME_PRINCIPAL).unwrap();
            let unwrap = GoogleCloudKmsDekUnwrapProvider::new(kms, RUNTIME_PRINCIPAL).unwrap();
            let hosted = HostedMemoryEnvelopeCrypto::new(
                broker,
                Box::new(wrap),
                Box::new(unwrap),
                MemoryDekCache::new(64),
                PROVIDER_ID,
                RUNTIME_PRINCIPAL,
                KEK_ID,
                "1",
                0,
            );
            MemoryCryptoProvider::hosted(hosted)
        }

        /// A verified context for `org` with an Internal-class data boundary — the
        /// class layers seal under.
        fn verified_internal(org: &str) -> VerifiedTenantContext {
            let tenant_context = TenantContext::explicit_user_workspace(
                org,
                "hq",
                Some("prod".to_string()),
                "user-finance",
            );
            let authority_chain = AuthorityChain::from_request(
                RequestPrincipal::authenticated_user("user-finance", "web"),
            );
            let strict = StrictTenantContext::new(
                tenant_context.clone(),
                PrincipalRef::human_user("user-finance"),
                authority_chain.clone(),
                ResourceScope::root(ResourceRef::new(org, "hq", ResourceKind::Workspace, "hq")),
                AssertionMetadata::new("web", "runtime", 1_000, 10_000, "assertion-a"),
            )
            .with_data_boundary(DataBoundary::allow(vec![DataClass::Internal]));
            VerifiedTenantContext {
                tenant_context,
                human_actor: HumanActor::tandem_user("user-finance"),
                authority_chain,
                roles: Vec::new(),
                org_units: Vec::new(),
                capabilities: Vec::new(),
                policy_version: None,
                strict_projection: Some(strict),
                issuer: "web".to_string(),
                audience: "runtime".to_string(),
                issued_at_ms: 1_000,
                expires_at_ms: 10_000,
                assertion_id: "assertion-a".to_string(),
                assertion_key_id: None,
            }
        }

        async fn hosted_db_with_layer(
        ) -> (tempfile::TempDir, MemoryDatabase, String, MemoryTenantScope) {
            let temp = tempfile::TempDir::new().unwrap();
            let db = MemoryDatabase::new(&temp.path().join("hosted.db"))
                .await
                .unwrap()
                .with_crypto_provider(hosted_provider());
            let tenant = MemoryTenantScope {
                org_id: "acme".to_string(),
                workspace_id: "hq".to_string(),
                deployment_id: Some("prod".to_string()),
            };
            let node = db
                .create_node(
                    "memory://acme/hq/summary.md",
                    None,
                    NodeType::File,
                    None,
                    &tenant,
                )
                .await
                .unwrap();
            db.create_layer(
                &node,
                LayerType::L2,
                "Summary: ACME owes $120k on invoice INV-2043",
                8,
                None,
                &tenant,
            )
            .await
            .unwrap();
            (temp, db, node, tenant)
        }

        #[tokio::test]
        async fn verified_principal_decrypts_a_hosted_sealed_layer() {
            let (_temp, db, node, tenant) = hosted_db_with_layer().await;

            // No principal → the sealed layer fails closed.
            assert!(db.get_layer(&node, LayerType::L2, &tenant).await.is_err());

            // The principal derived from the ACME verified context decrypts it.
            let principal =
                memory_decrypt_principal_from_verified_context(&verified_internal("acme"), 2_000)
                    .expect("hosted principal");
            let layer =
                with_decrypt_principal(principal, db.get_layer(&node, LayerType::L2, &tenant))
                    .await
                    .unwrap()
                    .expect("layer present");
            assert!(layer.content.contains("120k"));
        }

        #[tokio::test]
        async fn cross_tenant_verified_principal_is_denied() {
            let (_temp, db, node, tenant) = hosted_db_with_layer().await;

            // A principal built from a different tenant's verified context cannot
            // decrypt ACME's sealed layer — denied at the broker.
            let other =
                memory_decrypt_principal_from_verified_context(&verified_internal("hooli"), 2_000)
                    .expect("hosted principal");
            let result =
                with_decrypt_principal(other, db.get_layer(&node, LayerType::L2, &tenant)).await;
            assert!(result.is_err(), "cross-tenant hosted read must be denied");
        }

        #[tokio::test]
        async fn project_scoped_caller_cannot_decrypt_workspace_layer() {
            let (_temp, db, node, tenant) = hosted_db_with_layer().await;

            // A same-tenant caller whose projection is scoped to one project (not
            // the workspace) holds Internal access, but the handler gate refuses to
            // scope a decrypt principal for the workspace memory space it doesn't
            // cover — so the sealed layer stays fail-closed.
            let verified = base_verified(Some(project_scoped_strict("project-x")));
            let workspace_memory = workspace_memory_space_resource(&verified.tenant_context);
            let principal = Some(&verified)
                .filter(|verified| verified_resource_scope_covers(verified, &workspace_memory))
                .and_then(|verified| {
                    memory_decrypt_principal_from_verified_context(verified, 2_000)
                });
            assert!(
                principal.is_none(),
                "a project-scoped projection must not yield a workspace decrypt principal"
            );
            // With no principal scoped, the sealed workspace layer fails closed.
            assert!(db.get_layer(&node, LayerType::L2, &tenant).await.is_err());
        }
    }
}
