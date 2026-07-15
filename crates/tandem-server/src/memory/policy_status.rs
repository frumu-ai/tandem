// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::Serialize;
use tandem_types::RuntimeAuthMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPolicyMode {
    LocalGlobal,
    EnterpriseScoped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemoryContextPolicyStatus {
    pub runtime_auth_mode: RuntimeAuthMode,
    pub effective_memory_auth_mode: RuntimeAuthMode,
    pub memory_policy_mode: MemoryPolicyMode,
    pub strict_required: bool,
    pub context_assertion_verifier_configured: bool,
    pub hosted_control_plane_configured: bool,
    pub cross_tenant_grant_signing_key_configured: bool,
    pub strict_memory_enforcement: bool,
    pub strict_mcp_enforcement: bool,
    pub strict_tool_enforcement: bool,
    pub startup_ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<&'static str>,
}

impl MemoryContextPolicyStatus {
    pub fn from_parts(
        runtime_auth_mode: RuntimeAuthMode,
        context_assertion_verifier_configured: bool,
        hosted_control_plane_configured: bool,
        cross_tenant_grant_signing_key_configured: bool,
        strict_memory_enforcement: bool,
        strict_mcp_enforcement: bool,
        strict_tool_enforcement: bool,
    ) -> Self {
        let strict_required = enterprise_memory_policy_required(
            runtime_auth_mode,
            context_assertion_verifier_configured,
            hosted_control_plane_configured,
        );
        let strict_active =
            strict_memory_enforcement && strict_mcp_enforcement && strict_tool_enforcement;
        let failure_reason = if strict_required && !strict_active {
            Some("strict_tenant_enforcement_unavailable")
        } else {
            None
        };
        let effective_memory_auth_mode =
            if strict_required && runtime_auth_mode == RuntimeAuthMode::LocalSingleTenant {
                RuntimeAuthMode::EnterpriseRequired
            } else {
                runtime_auth_mode
            };
        Self {
            runtime_auth_mode,
            effective_memory_auth_mode,
            memory_policy_mode: if strict_required {
                MemoryPolicyMode::EnterpriseScoped
            } else {
                MemoryPolicyMode::LocalGlobal
            },
            strict_required,
            context_assertion_verifier_configured,
            hosted_control_plane_configured,
            cross_tenant_grant_signing_key_configured,
            strict_memory_enforcement,
            strict_mcp_enforcement,
            strict_tool_enforcement,
            startup_ready: failure_reason.is_none(),
            failure_reason,
        }
    }

    pub fn ensure_startup_ready(&self) -> anyhow::Result<()> {
        if let Some(reason) = self.failure_reason {
            anyhow::bail!(
                "enterprise memory/context policy is not ready: {reason}; runtime_auth_mode={}, memory_policy_mode={:?}, strict_memory={}, strict_mcp={}, strict_tools={}",
                self.runtime_auth_mode.as_str(),
                self.memory_policy_mode,
                self.strict_memory_enforcement,
                self.strict_mcp_enforcement,
                self.strict_tool_enforcement
            );
        }
        Ok(())
    }
}

pub fn enterprise_memory_policy_required(
    runtime_auth_mode: RuntimeAuthMode,
    context_assertion_verifier_configured: bool,
    hosted_control_plane_configured: bool,
) -> bool {
    runtime_auth_mode != RuntimeAuthMode::LocalSingleTenant
        || context_assertion_verifier_configured
        || hosted_control_plane_configured
}

pub fn resolve_memory_context_runtime_auth_mode() -> RuntimeAuthMode {
    let runtime_auth_mode = crate::config::env::resolve_runtime_auth_mode();
    if enterprise_memory_policy_required(
        runtime_auth_mode,
        crate::config::env::context_assertion_verifier_configured(),
        crate::config::env::hosted_control_plane_configured(),
    ) {
        match runtime_auth_mode {
            RuntimeAuthMode::LocalSingleTenant => RuntimeAuthMode::EnterpriseRequired,
            mode => mode,
        }
    } else {
        runtime_auth_mode
    }
}

pub fn current_memory_context_policy_status() -> MemoryContextPolicyStatus {
    MemoryContextPolicyStatus::from_parts(
        crate::config::env::resolve_runtime_auth_mode(),
        crate::config::env::context_assertion_verifier_configured(),
        crate::config::env::hosted_control_plane_configured(),
        crate::config::env::cross_tenant_grant_signing_key_configured(),
        tandem_memory::db::strict_tenant_enforcement_default(),
        tandem_runtime::mcp::strict_tenant_enforcement_default(),
        tandem_tools::strict_tenant_enforcement_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enterprise_configured_with_strict_flag_disabled_fails_closed() {
        let status = MemoryContextPolicyStatus::from_parts(
            RuntimeAuthMode::EnterpriseRequired,
            true,
            false,
            false,
            false,
            true,
            true,
        );

        assert!(!status.startup_ready);
        assert_eq!(
            status.failure_reason,
            Some("strict_tenant_enforcement_unavailable")
        );
        assert!(status.ensure_startup_ready().is_err());
    }

    #[test]
    fn verifier_key_configured_requires_enterprise_scoped_memory() {
        let status = MemoryContextPolicyStatus::from_parts(
            RuntimeAuthMode::LocalSingleTenant,
            true,
            false,
            false,
            true,
            true,
            true,
        );

        assert!(status.startup_ready);
        assert!(status.strict_required);
        assert_eq!(
            status.memory_policy_mode,
            MemoryPolicyMode::EnterpriseScoped
        );
        assert_eq!(
            status.effective_memory_auth_mode,
            RuntimeAuthMode::EnterpriseRequired
        );
    }

    #[test]
    fn local_configured_without_signing_key_keeps_local_global_memory() {
        let status = MemoryContextPolicyStatus::from_parts(
            RuntimeAuthMode::LocalSingleTenant,
            false,
            false,
            false,
            false,
            false,
            false,
        );

        assert!(status.startup_ready);
        assert!(!status.strict_required);
        assert_eq!(status.memory_policy_mode, MemoryPolicyMode::LocalGlobal);
        assert_eq!(
            status.effective_memory_auth_mode,
            RuntimeAuthMode::LocalSingleTenant
        );
    }

    #[test]
    fn prompt_context_provider_path_keeps_legacy_global_search_local_only() {
        let source = include_str!("../app/state/prompt_context_hook.rs");
        assert!(
            source.contains(
                "crate::memory::policy_status::resolve_memory_context_runtime_auth_mode()"
            ),
            "prompt memory access must resolve the enterprise-aware memory policy mode"
        );

        assert!(source.contains("async fn search_prompt_memory_with_scope"));
        assert_eq!(source.matches(".search_global_memory(").count(), 0);

        let function_source = source
            .split("pub(super) async fn search_prompt_global_memory")
            .nth(1)
            .and_then(|tail| tail.split("fn extract_run_client_id").next())
            .expect("prompt global memory search helper");
        let local_arm = function_source
            .split("PromptMemoryAccess::Local { user_id, .. } =>")
            .nth(1)
            .and_then(|tail| tail.split("PromptMemoryAccess::Governed {").next())
            .expect("local prompt memory arm");
        let governed_and_blocked_arms = function_source
            .split("PromptMemoryAccess::Governed {")
            .nth(1)
            .expect("governed prompt memory arm");

        assert_eq!(local_arm.matches(".search_global_memory(").count(), 0);
        assert!(local_arm.contains("search_prompt_memory_with_scope"));
        assert_eq!(
            governed_and_blocked_arms
                .matches(".search_global_memory(")
                .count(),
            0,
            "provider-bound governed prompt context must use tenant-scoped memory search"
        );
        assert!(governed_and_blocked_arms.contains("search_prompt_memory_with_scope"));
    }

    #[test]
    fn enterprise_aware_policy_is_used_by_ingress_and_live_registries() {
        let middleware = include_str!("../http/middleware.rs");
        assert!(
            middleware.contains("resolve_memory_context_runtime_auth_mode()"),
            "HTTP ingress must verify assertions when verifier/control-plane config promotes local mode"
        );

        let mcp_bootstrap = include_str!("../http/mcp.rs");
        assert!(
            mcp_bootstrap.contains("current_memory_context_policy_status()"),
            "live MCP/tool registries must use the same strict policy as startup/readiness"
        );
        assert!(
            mcp_bootstrap.contains("memory_context_policy.strict_required"),
            "live MCP/tool registries must flip strict mode for config-promoted enterprise policy"
        );
    }
}
