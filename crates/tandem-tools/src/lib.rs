#[path = "approval_classifier.rs"]
pub mod approval_classifier;
#[path = "builtin_tools.rs"]
mod builtin_tools;
#[path = "tool_metadata.rs"]
mod tool_metadata;
use builtin_tools::*;
use tool_metadata::*;

include!("lib_parts/part01.rs");
include!("lib_parts/part02.rs");
include!("lib_parts/part03.rs");
include!("lib_parts/part04.rs");
include!("lib_parts/part05.rs");
include!("lib_parts/part06.rs");

#[cfg(test)]
mod strict_tenant_tests {
    use super::*;

    fn guard_denial(err: &anyhow::Error) -> bool {
        err.to_string()
            .contains("ToolDenied { reason: TenantScope }")
    }

    #[tokio::test]
    async fn strict_mode_denies_external_effect_tools_for_local_implicit_tenant() {
        let registry = ToolRegistry::new();
        registry.set_strict_tenant_enforcement(true);

        for tool in ["webfetch", "websearch", "memory_search", "memory_store"] {
            let err = registry
                .execute_for_tenant(tool, serde_json::json!({}), TenantContext::local_implicit())
                .await
                .expect_err("external-effect tool must be denied for local-implicit tenant");
            assert!(
                guard_denial(&err),
                "expected TenantScope denial for `{tool}`, got: {err}"
            );
        }
    }

    #[tokio::test]
    async fn strict_mode_allows_workspace_tools_for_local_implicit_tenant() {
        let registry = ToolRegistry::new();
        registry.set_strict_tenant_enforcement(true);

        let workspace = tempfile::tempdir().expect("tempdir");
        let result = registry
            .execute_for_tenant(
                "glob",
                serde_json::json!({
                    "pattern": "*.rs",
                    "__workspace_root": workspace.path().to_string_lossy(),
                }),
                TenantContext::local_implicit(),
            )
            .await;
        match result {
            Ok(_) => {}
            Err(err) => assert!(
                !guard_denial(&err),
                "workspace tool must not hit the tenant guard: {err}"
            ),
        }
    }

    #[tokio::test]
    async fn strict_mode_passes_explicit_tenants_through_the_guard() {
        let registry = ToolRegistry::new();
        registry.set_strict_tenant_enforcement(true);

        let result = registry
            .execute_for_tenant(
                "memory_list",
                serde_json::json!({}),
                TenantContext::explicit("org-a", "workspace-a", None),
            )
            .await;
        if let Err(err) = result {
            assert!(
                !guard_denial(&err),
                "explicit tenant must pass the strict guard: {err}"
            );
        }
    }

    #[tokio::test]
    async fn default_mode_does_not_apply_the_tenant_guard() {
        let registry = ToolRegistry::new();

        let result = registry
            .execute_for_tenant(
                "websearch",
                serde_json::json!({}),
                TenantContext::local_implicit(),
            )
            .await;
        match result {
            Ok(_) => {}
            Err(err) => assert!(
                !guard_denial(&err),
                "non-strict registries must not deny local-implicit context: {err}"
            ),
        }
    }

    #[test]
    fn external_effect_classification_matches_capability_metadata() {
        assert!(tool_requires_explicit_tenant(&web_fetch_capabilities()));
        assert!(tool_requires_explicit_tenant(&memory_search_capabilities()));
        assert!(tool_requires_explicit_tenant(&memory_write_capabilities()));
        // bash: network_access via shell capabilities
        assert!(tool_requires_explicit_tenant(&shell_execution_capabilities()));
        assert!(!tool_requires_explicit_tenant(&workspace_read_capabilities()));
        assert!(!tool_requires_explicit_tenant(&workspace_write_capabilities()));
        assert!(!tool_requires_explicit_tenant(&planning_write_capabilities()));
    }
}
