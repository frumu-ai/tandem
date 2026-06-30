use tandem_memory::types::{GovernedReadMode, MemoryAccessFilter};
use tandem_types::{RuntimeAuthMode, VerifiedTenantContext};

pub fn governed_memory_read_mode(
    runtime_auth_mode: RuntimeAuthMode,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    has_grant_backed_access: bool,
) -> GovernedReadMode {
    let has_strict_projection = verified_tenant_context
        .and_then(|context| context.strict_projection.as_ref())
        .is_some();
    if runtime_auth_mode != RuntimeAuthMode::LocalSingleTenant
        || verified_tenant_context.is_some()
        || has_strict_projection
        || has_grant_backed_access
    {
        GovernedReadMode::GovernedStrict
    } else {
        GovernedReadMode::LocalNoop
    }
}

pub fn governed_memory_read_filter(
    runtime_auth_mode: RuntimeAuthMode,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    has_grant_backed_access: bool,
    now_ms: u64,
) -> Option<MemoryAccessFilter> {
    governed_memory_read_filter_with_workflow_phase(
        runtime_auth_mode,
        verified_tenant_context,
        has_grant_backed_access,
        now_ms,
        None,
    )
}

pub fn governed_memory_read_filter_with_workflow_phase(
    runtime_auth_mode: RuntimeAuthMode,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    has_grant_backed_access: bool,
    now_ms: u64,
    workflow_phase: Option<&str>,
) -> Option<MemoryAccessFilter> {
    match governed_memory_read_mode(
        runtime_auth_mode,
        verified_tenant_context,
        has_grant_backed_access,
    ) {
        GovernedReadMode::LocalNoop => None,
        GovernedReadMode::GovernedStrict => {
            let mut filter = MemoryAccessFilter::governed(
                verified_tenant_context.and_then(|context| context.strict_projection.clone()),
                now_ms,
            );
            if let Some(workflow_phase) = workflow_phase
                .map(str::trim)
                .filter(|workflow_phase| !workflow_phase.is_empty())
            {
                filter = filter.with_workflow_phase(workflow_phase.to_string());
            }
            Some(filter)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_memory::types::GovernedReadMode;

    #[test]
    fn hosted_mode_without_verified_context_builds_fail_closed_filter() {
        let filter =
            governed_memory_read_filter(RuntimeAuthMode::HostedSingleTenant, None, false, 2_000)
                .expect("hosted reads are governed");

        assert_eq!(filter.mode, GovernedReadMode::GovernedStrict);
        assert!(filter.strict_context.is_none());
    }

    #[test]
    fn enterprise_mode_missing_assertion_builds_fail_closed_filter() {
        let filter =
            governed_memory_read_filter(RuntimeAuthMode::EnterpriseRequired, None, false, 2_000)
                .expect("enterprise reads are governed");

        assert_eq!(filter.mode, GovernedReadMode::GovernedStrict);
        assert!(filter.strict_context.is_none());
    }

    #[test]
    fn governed_filter_carries_workflow_phase() {
        let filter = governed_memory_read_filter_with_workflow_phase(
            RuntimeAuthMode::EnterpriseRequired,
            None,
            false,
            2_000,
            Some(" draft "),
        )
        .expect("enterprise reads are governed");

        assert_eq!(filter.workflow_phase.as_deref(), Some("draft"));
    }

    #[test]
    fn local_mode_without_enterprise_context_keeps_noop_filter() {
        let filter =
            governed_memory_read_filter(RuntimeAuthMode::LocalSingleTenant, None, false, 2_000);

        assert!(filter.is_none());
    }
}
