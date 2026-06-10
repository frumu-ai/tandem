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
        assert!(tool_requires_explicit_tenant(
            &shell_execution_capabilities()
        ));
        assert!(!tool_requires_explicit_tenant(
            &workspace_read_capabilities()
        ));
        assert!(!tool_requires_explicit_tenant(
            &workspace_write_capabilities()
        ));
        assert!(!tool_requires_explicit_tenant(
            &planning_write_capabilities()
        ));
    }
}

#[cfg(test)]
mod sandbox_and_resolution_tests {
    use super::*;

    fn workspace_args(root: &Path) -> Value {
        serde_json::json!({ "__workspace_root": root.to_string_lossy() })
    }

    async fn resolve_in_registry(registry: &ToolRegistry, name: &str) -> Option<String> {
        let tools = registry.tools.read().await;
        resolve_registered_tool(&tools, name).map(|tool| tool.schema().name)
    }

    #[tokio::test]
    async fn registry_resolution_normalizes_aliases_case_and_namespaces() {
        let registry = ToolRegistry::new();
        // (requested, expected canonical schema name)
        let cases = [
            ("bash", "bash"),
            ("BASH", "bash"),
            ("  bash  ", "bash"),
            ("run_command", "bash"),
            ("shell", "bash"),
            ("powershell", "bash"),
            ("cmd", "bash"),
            ("todowrite", "todo_write"),
            ("update_todo_list", "todo_write"),
            ("update_todos", "todo_write"),
            ("todo-write", "todo_write"),
            ("default_api.bash", "bash"),
            ("default_api:read", "read"),
            ("functions.grep", "grep"),
            ("function.glob", "glob"),
            ("tools.write", "write"),
            ("tool.edit", "edit"),
            ("builtin.read", "read"),
            ("builtin:webfetch", "webfetch"),
        ];
        for (requested, expected) in cases {
            let resolved = resolve_in_registry(&registry, requested).await;
            assert_eq!(
                resolved.as_deref(),
                Some(expected),
                "`{requested}` should resolve to `{expected}`"
            );
        }
    }

    #[tokio::test]
    async fn registry_resolution_rejects_unknown_and_adversarial_names() {
        let registry = ToolRegistry::new();
        // The classifier and the registry must agree that these do NOT
        // resolve: a name that resolved here but classified differently
        // would be a policy bypass.
        let cases = [
            "",
            "   ",
            "definitely_not_a_tool",
            "mcp..bash",
            "mcp.stripe.charge", // MCP tools never resolve via the built-in registry
            "default_api.",
            "default_api.unknown_tool",
            "../bash",
            "bash;rm",
        ];
        for requested in cases {
            assert!(
                resolve_in_registry(&registry, requested).await.is_none(),
                "`{requested}` must not resolve to a built-in tool"
            );
        }
    }

    #[test]
    fn resolve_tool_path_rejects_parent_traversal_and_outside_absolutes() {
        let workspace = tempfile::tempdir().expect("workspace");
        let args = workspace_args(workspace.path());

        assert!(resolve_tool_path("../escape.txt", &args).is_none());
        assert!(resolve_tool_path("nested/../../escape.txt", &args).is_none());
        assert!(resolve_tool_path("/etc/passwd", &args).is_none());
        assert!(
            resolve_tool_path(&format!("{}/file.txt", workspace.path().display()), &args).is_some(),
            "absolute path inside the workspace resolves"
        );
        assert!(resolve_tool_path("inside.txt", &args).is_some());
    }

    #[test]
    fn resolve_tool_path_without_workspace_root_rejects_absolutes() {
        // Policy (TAN-216 decision): with no `__workspace_root`, absolute
        // paths fail closed; relative paths resolve against the effective
        // cwd only. This pins the existing fail-closed behavior so a
        // regression to "absolute allowed" is caught.
        let args = serde_json::json!({});
        assert!(resolve_tool_path("/etc/passwd", &args).is_none());
        assert!(resolve_tool_path("relative.txt", &args).is_some());
    }

    #[test]
    fn resolve_tool_path_rejects_malformed_tokens() {
        let workspace = tempfile::tempdir().expect("workspace");
        let args = workspace_args(workspace.path());
        for token in [
            "*",
            "*.rs",
            "src/*.rs",
            "?",
            "file?.txt",
            "a\u{0007}b.txt",
            "a\nb.txt",
        ] {
            assert!(
                resolve_tool_path(token, &args).is_none(),
                "malformed token `{token:?}` must not resolve"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn resolve_tool_path_rejects_symlink_escapes() {
        let workspace = tempfile::tempdir().expect("workspace");
        let outside = tempfile::tempdir().expect("outside");
        let secret = outside.path().join("secret.txt");
        std::fs::write(&secret, "outside-the-workspace").expect("write secret");

        // workspace/link -> outside dir; workspace/file_link -> outside file
        std::os::unix::fs::symlink(outside.path(), workspace.path().join("link"))
            .expect("dir symlink");
        std::os::unix::fs::symlink(&secret, workspace.path().join("file_link"))
            .expect("file symlink");

        let args = serde_json::json!({
            "__workspace_root": workspace.path().to_string_lossy(),
            "__effective_cwd": workspace.path().to_string_lossy(),
        });

        assert!(
            resolve_tool_path("link/secret.txt", &args).is_none(),
            "path through an escaping directory symlink must not resolve"
        );
        assert!(
            resolve_tool_path("file_link", &args).is_none(),
            "escaping file symlink must not resolve"
        );
    }

    #[test]
    fn sensitive_paths_are_flagged_for_fallback_protection() {
        for path in [
            ".aws/credentials",
            ".docker/config.json",
            "home/user/.aws/credentials",
        ] {
            assert!(
                tandem_types::is_sensitive_path(Path::new(path)),
                "`{path}` must be flagged sensitive"
            );
        }
        assert!(!tandem_types::is_sensitive_path(Path::new("src/main.rs")));
        assert!(!tandem_types::is_sensitive_path(Path::new("config.json")));
    }
}
