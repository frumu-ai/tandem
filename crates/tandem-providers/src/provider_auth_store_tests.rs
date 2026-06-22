use super::*;

use tempfile::tempdir;

#[test]
fn provider_auth_for_tenant_is_isolated_per_tenant_and_from_local() {
    let dir = tempdir().expect("tempdir");
    let tenant_a = TenantContext::explicit("org-a", "workspace-a", None);
    let tenant_b = TenantContext::explicit("org-b", "workspace-b", None);
    let local = TenantContext::local_implicit();

    set_provider_auth_for_tenant_in_dir(dir.path(), &tenant_a, "openrouter", "tenant-a-key")
        .expect("store tenant a credential");
    set_provider_auth_for_tenant_in_dir(dir.path(), &local, "openrouter", "local-key")
        .expect("store local credential");

    let tenant_a_view = load_provider_auth_for_tenant_in_dir(dir.path(), &tenant_a);
    assert_eq!(
        tenant_a_view.get("openrouter").map(String::as_str),
        Some("tenant-a-key")
    );
    assert_eq!(
        tenant_a_view.len(),
        1,
        "tenant A must not see the local credential"
    );

    let tenant_b_view = load_provider_auth_for_tenant_in_dir(dir.path(), &tenant_b);
    assert!(
        tenant_b_view.is_empty(),
        "tenant B must see neither tenant A nor local credentials"
    );

    let local_view = load_provider_auth_for_tenant_in_dir(dir.path(), &local);
    assert_eq!(
        local_view.get("openrouter").map(String::as_str),
        Some("local-key"),
        "local mode sees only the unscoped credential"
    );
    assert_eq!(local_view.len(), 1);
}

#[test]
fn provider_auth_isolates_deployments_within_same_org_workspace() {
    let dir = tempdir().expect("tempdir");
    let mut deployment_one = TenantContext::explicit("org-a", "workspace-a", None);
    deployment_one.deployment_id = Some("deployment-1".to_string());
    let mut deployment_two = deployment_one.clone();
    deployment_two.deployment_id = Some("deployment-2".to_string());

    set_provider_auth_for_tenant_in_dir(
        dir.path(),
        &deployment_one,
        "anthropic",
        "deployment-one-key",
    )
    .expect("store deployment one credential");

    assert_eq!(
        load_provider_auth_for_tenant_in_dir(dir.path(), &deployment_one)
            .get("anthropic")
            .map(String::as_str),
        Some("deployment-one-key")
    );
    assert!(
        load_provider_auth_for_tenant_in_dir(dir.path(), &deployment_two).is_empty(),
        "a different deployment in the same org/workspace must not read the credential"
    );
}

fn make_jwt(payload: serde_json::Value) -> String {
    let header =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","typ":"JWT"}"#);
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_string(&payload).expect("payload json"));
    format!("{header}.{payload}.signature")
}

fn make_unsigned_jwt(payload: serde_json::Value) -> String {
    let header =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"alg":"none","typ":"JWT"}"#);
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_string(&payload).expect("payload json"));
    format!("{header}.{payload}.signature")
}

#[test]
fn decode_codex_jwt_claims_rejects_none_algorithm() {
    let jwt = make_unsigned_jwt(serde_json::json!({
        "exp": 2_000_000_000,
        "sub": "acct_unsigned"
    }));

    assert!(decode_codex_jwt_claims(&jwt).is_none());
}

#[test]
fn load_codex_cli_oauth_credential_reads_auth_file() {
    let dir = tempdir().expect("tempdir");
    let auth_path = dir.path().join("auth.json");
    let jwt = make_jwt(serde_json::json!({
        "exp": 2_000_000_000,
        "email": "user@example.com",
        "https://api.openai.com/auth": {
            "chatgpt_account_user_id": "acct_123"
        }
    }));
    std::fs::write(
        &auth_path,
        serde_json::json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "access_token": jwt,
                "refresh_token": "refresh-token-123",
                "account_id": "acct_123"
            },
            "last_refresh": 123
        })
        .to_string(),
    )
    .expect("write auth");

    let credential = load_codex_cli_oauth_credential_at(&auth_path).expect("credential");
    assert_eq!(credential.provider_id, "openai-codex");
    assert_eq!(credential.managed_by, "codex-cli");
    assert_eq!(credential.refresh_token, "refresh-token-123");
    assert_eq!(credential.account_id.as_deref(), Some("acct_123"));
    assert_eq!(credential.email.as_deref(), Some("user@example.com"));
    assert_eq!(credential.display_name.as_deref(), Some("user@example.com"));
    assert!(credential.expires_at_ms > 0);
}

#[test]
fn write_openai_codex_cli_auth_json_persists_auth_file() {
    let dir = tempdir().expect("tempdir");
    let auth_path = dir.path().join("auth.json");
    let jwt = make_jwt(serde_json::json!({
        "exp": 2_000_000_000,
        "email": "hosted@example.com",
        "https://api.openai.com/auth": {
            "chatgpt_account_user_id": "acct_456"
        }
    }));
    let payload = serde_json::json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "access_token": jwt,
            "refresh_token": "refresh-token-456",
            "account_id": "acct_456"
        },
        "last_refresh": "2026-04-23T08:15:30.000Z"
    });

    write_codex_cli_auth_json_at(&auth_path, &payload).expect("write auth");

    let credential = load_codex_cli_oauth_credential_at(&auth_path).expect("credential");
    assert_eq!(credential.provider_id, "openai-codex");
    assert_eq!(credential.managed_by, "codex-cli");
    assert_eq!(credential.refresh_token, "refresh-token-456");
    assert_eq!(credential.account_id.as_deref(), Some("acct_456"));
    assert_eq!(credential.email.as_deref(), Some("hosted@example.com"));
    assert_eq!(
        credential.display_name.as_deref(),
        Some("hosted@example.com")
    );
}

#[test]
fn load_codex_cli_oauth_credential_reads_flat_auth_file() {
    let dir = tempdir().expect("tempdir");
    let auth_path = dir.path().join("auth.json");
    let jwt = make_jwt(serde_json::json!({
        "exp": 2_000_000_000,
        "email": "flat@example.com",
        "https://api.openai.com/auth": {
            "chatgpt_account_user_id": "acct_flat"
        }
    }));
    std::fs::write(
        &auth_path,
        serde_json::json!({
            "auth_mode": "chatgpt",
            "access_token": jwt,
            "refresh_token": "refresh-token-flat",
            "account_id": "acct_flat",
            "last_refresh": 789
        })
        .to_string(),
    )
    .expect("write auth");

    let credential = load_codex_cli_oauth_credential_at(&auth_path).expect("credential");
    assert_eq!(credential.provider_id, "openai-codex");
    assert_eq!(credential.managed_by, "codex-cli");
    assert_eq!(credential.refresh_token, "refresh-token-flat");
    assert_eq!(credential.account_id.as_deref(), Some("acct_flat"));
    assert_eq!(credential.email.as_deref(), Some("flat@example.com"));
    assert_eq!(credential.display_name.as_deref(), Some("flat@example.com"));
    assert!(credential.expires_at_ms > 0);
}

#[test]
fn load_codex_cli_oauth_credential_tolerates_string_last_refresh() {
    let dir = tempdir().expect("tempdir");
    let auth_path = dir.path().join("auth.json");
    let jwt = make_jwt(serde_json::json!({
        "exp": 2_000_000_000,
        "email": "string-refresh@example.com",
        "https://api.openai.com/auth": {
            "chatgpt_account_user_id": "acct_string_refresh"
        }
    }));
    std::fs::write(
        &auth_path,
        serde_json::json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "access_token": jwt,
                "refresh_token": "refresh-token-string",
                "account_id": "acct_string_refresh",
                "id_token": "id-token-placeholder"
            },
            "last_refresh": "2026-04-23T08:15:30.000Z",
            "OPENAI_API_KEY": null
        })
        .to_string(),
    )
    .expect("write auth");

    let credential = load_codex_cli_oauth_credential_at(&auth_path).expect("credential");
    assert_eq!(credential.provider_id, "openai-codex");
    assert_eq!(credential.managed_by, "codex-cli");
    assert_eq!(credential.refresh_token, "refresh-token-string");
    assert_eq!(
        credential.account_id.as_deref(),
        Some("acct_string_refresh")
    );
    assert_eq!(
        credential.email.as_deref(),
        Some("string-refresh@example.com")
    );
    assert_eq!(
        credential.display_name.as_deref(),
        Some("string-refresh@example.com")
    );
    assert!(credential.expires_at_ms > 0);
}
