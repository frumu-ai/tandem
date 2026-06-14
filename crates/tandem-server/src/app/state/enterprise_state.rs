//! EAA-12 (TAN-37): enterprise state ownership extracted out of core `AppState`.
//!
//! The enterprise registries (org units, memberships, access grants,
//! cross-tenant grants, source bindings, connectors, ingestion jobs and
//! quarantines) and their on-disk persistence paths are bundled here into one
//! cohesive [`EnterpriseState`] wrapper, rather than being carried as ~16 loose
//! fields on `AppState`. This is the state seam the enterprise extension
//! (`tandem-enterprise-server`) owns: it can construct and initialize its own
//! wrapper, and core `AppState` no longer scatters enterprise-only registries
//! through its body.
//!
//! Behavior is unchanged — public/local builds get the same empty registries
//! and default paths they had before; this is pure encapsulation.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tandem_enterprise_contract::{
    ConnectorInstance as EnterpriseConnectorInstance,
    CrossTenantGrantRecord as EnterpriseCrossTenantGrantRecord,
    IngestionJob as EnterpriseIngestionJob, IngestionQuarantine as EnterpriseIngestionQuarantine,
    OrganizationUnit as EnterpriseOrganizationUnit,
    OrganizationUnitAccessGrant as EnterpriseOrganizationUnitAccessGrant,
    OrganizationUnitMembership as EnterpriseOrganizationUnitMembership,
    SourceBinding as EnterpriseSourceBinding,
};
use tokio::sync::RwLock;

use crate::config;

/// Enterprise registries and their persistence paths, owned as one unit.
///
/// All registries are cheap `Arc<RwLock<_>>` handles, so `EnterpriseState` is
/// `Clone` and is cloned alongside `AppState`.
#[derive(Clone)]
pub struct EnterpriseState {
    pub org_units: Arc<RwLock<HashMap<String, EnterpriseOrganizationUnit>>>,
    pub org_units_path: PathBuf,
    pub org_unit_memberships: Arc<RwLock<HashMap<String, EnterpriseOrganizationUnitMembership>>>,
    pub org_unit_memberships_path: PathBuf,
    pub org_unit_access_grants: Arc<RwLock<HashMap<String, EnterpriseOrganizationUnitAccessGrant>>>,
    pub org_unit_access_grants_path: PathBuf,
    pub cross_tenant_grants: Arc<RwLock<HashMap<String, EnterpriseCrossTenantGrantRecord>>>,
    pub cross_tenant_grants_path: PathBuf,
    pub source_bindings: Arc<RwLock<HashMap<String, EnterpriseSourceBinding>>>,
    pub source_bindings_path: PathBuf,
    pub connectors: Arc<RwLock<HashMap<String, EnterpriseConnectorInstance>>>,
    pub connectors_path: PathBuf,
    pub ingestion_jobs: Arc<RwLock<HashMap<String, EnterpriseIngestionJob>>>,
    pub ingestion_jobs_path: PathBuf,
    pub ingestion_quarantines: Arc<RwLock<HashMap<String, EnterpriseIngestionQuarantine>>>,
    pub ingestion_quarantines_path: PathBuf,
}

impl EnterpriseState {
    /// Build empty registries with persistence paths resolved from config —
    /// the same defaults `AppState::new_starting` previously inlined.
    pub fn new() -> Self {
        Self {
            org_units: Arc::new(RwLock::new(HashMap::new())),
            org_units_path: config::paths::resolve_enterprise_org_units_path(),
            org_unit_memberships: Arc::new(RwLock::new(HashMap::new())),
            org_unit_memberships_path: config::paths::resolve_enterprise_org_unit_memberships_path(
            ),
            org_unit_access_grants: Arc::new(RwLock::new(HashMap::new())),
            org_unit_access_grants_path:
                config::paths::resolve_enterprise_org_unit_access_grants_path(),
            cross_tenant_grants: Arc::new(RwLock::new(HashMap::new())),
            cross_tenant_grants_path: config::paths::resolve_enterprise_cross_tenant_grants_path(),
            source_bindings: Arc::new(RwLock::new(HashMap::new())),
            source_bindings_path: config::paths::resolve_enterprise_source_bindings_path(),
            connectors: Arc::new(RwLock::new(HashMap::new())),
            connectors_path: config::paths::resolve_enterprise_connectors_path(),
            ingestion_jobs: Arc::new(RwLock::new(HashMap::new())),
            ingestion_jobs_path: config::paths::resolve_enterprise_ingestion_jobs_path(),
            ingestion_quarantines: Arc::new(RwLock::new(HashMap::new())),
            ingestion_quarantines_path:
                config::paths::resolve_enterprise_ingestion_quarantines_path(),
        }
    }
}

impl Default for EnterpriseState {
    fn default() -> Self {
        Self::new()
    }
}
