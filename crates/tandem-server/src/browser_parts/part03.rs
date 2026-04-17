#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::BrowserConfig;
    use tandem_tools::ToolRegistry;

    #[test]
    fn local_and_private_hosts_are_detected() {
        assert!(is_local_or_private_host("localhost"));
        assert!(is_local_or_private_host("127.0.0.1"));
        assert!(is_local_or_private_host("10.1.2.3"));
        assert!(is_local_or_private_host("192.168.0.10"));
        assert!(!is_local_or_private_host("example.com"));
        assert!(!is_local_or_private_host("8.8.8.8"));
    }

    #[test]
    fn allow_host_check_accepts_subdomains() {
        let allow_hosts = vec!["example.com".to_string()];
        ensure_allowed_browser_url("https://example.com/path", &allow_hosts).expect("root host");
        ensure_allowed_browser_url("https://app.example.com/path", &allow_hosts)
            .expect("subdomain host");
        let err =
            ensure_allowed_browser_url("https://example.org/path", &allow_hosts).expect_err("deny");
        assert!(err.to_string().contains("allowlist"));
    }

    #[test]
    fn browser_release_asset_name_matches_platform() {
        let asset = browser_release_asset_name().expect("asset name");
        assert!(asset.starts_with("tandem-browser-"));
        if cfg!(target_os = "windows") {
            assert!(asset.ends_with(".zip"));
            assert!(asset.contains("-windows-"));
        } else if cfg!(target_os = "macos") {
            assert!(asset.ends_with(".zip"));
            assert!(asset.contains("-darwin-"));
        } else if cfg!(target_os = "linux") {
            assert!(asset.ends_with(".tar.gz"));
            assert!(asset.contains("-linux-"));
        }
    }

    #[test]
    fn managed_sidecar_path_uses_shared_binaries_dir() {
        let temp_root =
            std::env::temp_dir().join(format!("tandem-browser-test-{}", Uuid::new_v4()));
        std::env::set_var("TANDEM_HOME", &temp_root);

        let path = managed_sidecar_install_path().expect("managed path");

        assert!(path.starts_with(temp_root.join("binaries")));
        assert_eq!(
            path.file_name().and_then(|value| value.to_str()),
            Some(sidecar_binary_name())
        );

        std::env::remove_var("TANDEM_HOME");
    }

    #[test]
    fn bool_env_value_uses_clap_friendly_literals() {
        assert_eq!(bool_env_value(true), "true");
        assert_eq!(bool_env_value(false), "false");
    }

    #[test]
    fn normalize_browser_open_request_drops_empty_profile_id() {
        let mut request = BrowserOpenRequest {
            url: "https://example.com".to_string(),
            profile_id: Some("   ".to_string()),
            headless: None,
            viewport: None,
            wait_until: None,
            executable_path: None,
            user_data_root: None,
            allow_no_sandbox: false,
            headless_default: true,
        };

        normalize_browser_open_request(&mut request);

        assert_eq!(request.profile_id, None);
    }

    #[test]
    fn parse_browser_wait_args_accepts_canonical_condition_shape() {
        let parsed = parse_browser_wait_args(&json!({
            "session_id": "browser-1",
            "condition": { "kind": "selector", "value": "#login" },
            "timeout_ms": 5000
        }))
        .expect("canonical browser_wait args");

        assert_eq!(parsed.session_id, "browser-1");
        assert_eq!(parsed.condition.kind, "selector");
        assert_eq!(parsed.condition.value.as_deref(), Some("#login"));
        assert_eq!(parsed.timeout_ms, Some(5000));
    }

    #[test]
    fn parse_browser_wait_args_accepts_wait_for_alias_and_camel_case() {
        let parsed = parse_browser_wait_args(&json!({
            "sessionId": "browser-1",
            "waitFor": { "type": "text", "value": "Dashboard" },
            "timeoutMs": 1500
        }))
        .expect("aliased browser_wait args");

        assert_eq!(parsed.session_id, "browser-1");
        assert_eq!(parsed.condition.kind, "text");
        assert_eq!(parsed.condition.value.as_deref(), Some("Dashboard"));
        assert_eq!(parsed.timeout_ms, Some(1500));
    }

    #[test]
    fn parse_browser_wait_args_accepts_top_level_condition_fields() {
        let parsed = parse_browser_wait_args(&json!({
            "session_id": "browser-1",
            "kind": "url",
            "value": "/settings"
        }))
        .expect("top-level browser_wait args");

        assert_eq!(parsed.condition.kind, "url");
        assert_eq!(parsed.condition.value.as_deref(), Some("/settings"));
    }

    #[test]
    fn parse_browser_wait_args_infers_selector_alias_without_explicit_kind() {
        let parsed = parse_browser_wait_args(&json!({
            "session_id": "browser-1",
            "condition": { "selector": "[data-testid='save']" }
        }))
        .expect("selector alias browser_wait args");

        assert_eq!(parsed.condition.kind, "selector");
        assert_eq!(
            parsed.condition.value.as_deref(),
            Some("[data-testid='save']")
        );
    }

    #[tokio::test]
    async fn register_tools_keeps_browser_status_available_when_disabled() {
        let tools = ToolRegistry::new();
        let browser = BrowserSubsystem::new(BrowserConfig::default());

        browser
            .register_tools(&tools, None)
            .await
            .expect("register browser tools");

        let names = tools
            .list()
            .await
            .into_iter()
            .map(|schema| schema.name)
            .collect::<Vec<_>>();
        assert!(names.iter().any(|name| name == "browser_status"));
        assert!(!names.iter().any(|name| name == "browser_open"));
        assert!(!browser.health_summary().await.tools_registered);
    }

    #[tokio::test]
    async fn close_sessions_for_owner_removes_matching_sessions() {
        let browser = BrowserSubsystem::new(BrowserConfig::default());
        browser
            .insert_session(
                "session-1".to_string(),
                Some("owner-1".to_string()),
                "https://example.com".to_string(),
            )
            .await;
        browser
            .insert_session(
                "session-2".to_string(),
                Some("owner-2".to_string()),
                "https://example.org".to_string(),
            )
            .await;

        let closed = browser.close_sessions_for_owner("owner-1").await;

        assert_eq!(closed, 1);
        assert!(browser.session("session-1").await.is_none());
        assert!(browser.session("session-2").await.is_some());
    }
}
