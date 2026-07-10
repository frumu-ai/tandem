use super::*;

use tempfile::tempdir;
use tokio::sync::Barrier;

#[derive(Debug)]
struct SharedTestCredential {
    key: String,
}

fn shared_test_keyring() -> &'static std::sync::Mutex<HashMap<String, String>> {
    static STORE: OnceLock<std::sync::Mutex<HashMap<String, String>>> = OnceLock::new();
    STORE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

impl keyring::credential::CredentialApi for SharedTestCredential {
    fn set_password(&self, password: &str) -> keyring::Result<()> {
        shared_test_keyring()
            .lock()
            .expect("shared keyring lock")
            .insert(self.key.clone(), password.to_string());
        Ok(())
    }

    fn get_password(&self) -> keyring::Result<String> {
        shared_test_keyring()
            .lock()
            .expect("shared keyring lock")
            .get(&self.key)
            .cloned()
            .ok_or(keyring::Error::NoEntry)
    }

    fn delete_password(&self) -> keyring::Result<()> {
        shared_test_keyring()
            .lock()
            .expect("shared keyring lock")
            .remove(&self.key)
            .map(|_| ())
            .ok_or(keyring::Error::NoEntry)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug)]
struct SharedTestCredentialBuilder;

impl keyring::credential::CredentialBuilderApi for SharedTestCredentialBuilder {
    fn build(
        &self,
        target: Option<&str>,
        service: &str,
        user: &str,
    ) -> keyring::Result<Box<keyring::credential::Credential>> {
        Ok(Box::new(SharedTestCredential {
            key: format!("{}::{service}::{user}", target.unwrap_or_default()),
        }))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn persistence(&self) -> keyring::credential::CredentialPersistence {
        keyring::credential::CredentialPersistence::ProcessOnly
    }
}

const CROSS_PROCESS_WORKER_TEST: &str =
    "provider_auth_store::provider_auth_store_tests::cross_process_credential_mutation_worker";

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

fn oauth_credential(label: &str) -> OAuthProviderCredential {
    OAuthProviderCredential {
        provider_id: "openai-codex".to_string(),
        access_token: format!("access-{label}"),
        refresh_token: format!("refresh-{label}"),
        expires_at_ms: 2_000_000_000_000,
        account_id: Some(format!("account-{label}")),
        email: None,
        display_name: None,
        managed_by: "tandem".to_string(),
        api_key: Some(format!("api-{label}")),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_serialized_provider_credential_mutations_preserve_every_tenant() {
    const MUTATION_COUNT: usize = 32;

    let dir = tempdir().expect("tempdir");
    let security_dir = dir.path().to_path_buf();
    for index in (1..MUTATION_COUNT).step_by(2) {
        let tenant =
            TenantContext::explicit(format!("org-{index}"), format!("workspace-{index}"), None);
        set_provider_oauth_credential_for_tenant_in_dir_serialized(
            &security_dir,
            &tenant,
            "openai-codex",
            oauth_credential(&format!("stale-{index}")),
        )
        .await
        .expect("seed credential to delete");
    }

    let barrier = std::sync::Arc::new(Barrier::new(MUTATION_COUNT));
    let mut tasks = Vec::with_capacity(MUTATION_COUNT);

    for index in 0..MUTATION_COUNT {
        let security_dir = security_dir.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            let tenant =
                TenantContext::explicit(format!("org-{index}"), format!("workspace-{index}"), None);
            barrier.wait().await;
            if index % 2 == 0 {
                set_provider_oauth_credential_for_tenant_in_dir_serialized(
                    &security_dir,
                    &tenant,
                    "openai-codex",
                    oauth_credential(&index.to_string()),
                )
                .await
                .expect("store concurrent credential");
            } else {
                assert!(delete_provider_credential_for_tenant_in_dir_serialized(
                    &security_dir,
                    &tenant,
                    "openai-codex",
                )
                .await
                .expect("delete concurrent credential"));
            }
        }));
    }

    for task in tasks {
        task.await.expect("credential mutation task");
    }

    for index in 0..MUTATION_COUNT {
        let tenant =
            TenantContext::explicit(format!("org-{index}"), format!("workspace-{index}"), None);
        let stored = load_provider_oauth_credential_for_tenant_in_dir(
            &security_dir,
            &tenant,
            "openai-codex",
        );
        if index % 2 == 0 {
            assert_eq!(
                stored
                    .unwrap_or_else(|| panic!("credential for tenant {index}"))
                    .access_token,
                format!("access-{index}")
            );
        } else {
            assert!(stored.is_none(), "deleted credential for tenant {index}");
        }
    }
}

#[test]
fn cross_process_credential_mutation_worker() {
    let Ok(action) = std::env::var("TANDEM_TEST_PROVIDER_CREDENTIAL_WORKER") else {
        return;
    };
    let security_dir = PathBuf::from(
        std::env::var_os("TANDEM_TEST_PROVIDER_CREDENTIAL_DIR")
            .expect("worker credential directory"),
    );
    let label =
        std::env::var("TANDEM_TEST_PROVIDER_CREDENTIAL_LABEL").expect("worker credential label");
    let tenant = TenantContext::explicit(format!("org-{label}"), "workspace", None);

    match action.as_str() {
        "set" => {
            set_provider_oauth_credential_for_tenant_in_dir(
                &security_dir,
                &tenant,
                "openai-codex",
                oauth_credential(&label),
            )
            .expect("worker stores credential");
        }
        "stale-refresh" => {
            let expected = load_provider_oauth_credential_for_tenant_in_dir(
                &security_dir,
                &tenant,
                "openai-codex",
            )
            .expect("worker loads original credential");
            let ready_path = PathBuf::from(
                std::env::var_os("TANDEM_TEST_PROVIDER_CREDENTIAL_READY")
                    .expect("worker ready path"),
            );
            let release_path = PathBuf::from(
                std::env::var_os("TANDEM_TEST_PROVIDER_CREDENTIAL_RELEASE")
                    .expect("worker release path"),
            );
            let result_path = PathBuf::from(
                std::env::var_os("TANDEM_TEST_PROVIDER_CREDENTIAL_RESULT")
                    .expect("worker result path"),
            );
            std::fs::write(&ready_path, b"ready").expect("signal stale refresh read");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !release_path.exists() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out waiting to release stale refresh"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let swapped = compare_and_set_provider_oauth_credential_for_tenant_in_dir(
                &security_dir,
                &tenant,
                "openai-codex",
                &expected,
                Some(oauth_credential("stale-replacement")),
            )
            .expect("worker compare-and-set");
            std::fs::write(result_path, if swapped { "swapped" } else { "stale" })
                .expect("write stale refresh result");
        }
        "keyring-cas" => {
            keyring::set_default_credential_builder(Box::new(SharedTestCredentialBuilder));
            let provider_id = "openai-codex";
            let scoped_provider_id = tenant_scoped_provider_id(&tenant, provider_id);
            let expected = oauth_credential("keyring-original");
            let persisted = credential_with_provider_id(
                ProviderCredential::OAuth(expected.clone()),
                scoped_provider_id.clone(),
            );
            credential_keyring_entry(&scoped_provider_id)
                .expect("test keyring entry")
                .set_password(&serde_json::to_string(&persisted).expect("serialize credential"))
                .expect("seed keyring credential");
            save_provider_credentials_index_to_dir(
                &security_dir,
                &HashSet::from([scoped_provider_id.clone()]),
            )
            .expect("seed credential index");

            let swapped = compare_and_set_provider_oauth_credential_for_tenant_in_dir(
                &security_dir,
                &tenant,
                provider_id,
                &expected,
                Some(oauth_credential("keyring-refreshed")),
            )
            .expect("compare and set keyring credential");
            assert!(swapped, "keyring-backed credential must match CAS");
            assert!(
                !load_credential_fallback_map_from_dir(&security_dir)
                    .contains_key(&scoped_provider_id),
                "refresh must preserve keyring storage"
            );
            let refreshed = load_provider_oauth_credential_for_tenant_in_dir(
                &security_dir,
                &tenant,
                provider_id,
            )
            .expect("refreshed keyring credential");
            assert_eq!(refreshed.access_token, "access-keyring-refreshed");
        }
        other => panic!("unknown credential worker action {other}"),
    }
}

fn credential_worker_command(
    security_dir: &Path,
    action: &str,
    label: &str,
) -> std::process::Command {
    let mut command =
        std::process::Command::new(std::env::current_exe().expect("current test exe"));
    command
        .arg("--exact")
        .arg(CROSS_PROCESS_WORKER_TEST)
        .arg("--nocapture")
        .env("TANDEM_TEST_PROVIDER_CREDENTIAL_WORKER", action)
        .env("TANDEM_TEST_PROVIDER_CREDENTIAL_DIR", security_dir)
        .env("TANDEM_TEST_PROVIDER_CREDENTIAL_LABEL", label);
    command
}

#[test]
fn keyring_backed_oauth_credential_can_be_refreshed_with_compare_and_set() {
    let dir = tempdir().expect("tempdir");
    let status = credential_worker_command(dir.path(), "keyring-cas", "keyring-cas")
        .status()
        .expect("run isolated keyring CAS worker");
    assert!(status.success(), "keyring CAS worker must succeed");
}

#[test]
fn cross_process_tenant_writes_preserve_the_whole_credential_map() {
    const PROCESS_COUNT: usize = 12;

    let dir = tempdir().expect("tempdir");
    let mut children = Vec::with_capacity(PROCESS_COUNT);
    for index in 0..PROCESS_COUNT {
        let child = credential_worker_command(dir.path(), "set", &index.to_string())
            .env("TANDEM_TEST_PROVIDER_CREDENTIAL_RMW_DELAY_MS", "50")
            .spawn()
            .expect("spawn credential writer");
        children.push(child);
    }
    for mut child in children {
        assert!(child.wait().expect("wait for credential writer").success());
    }

    for index in 0..PROCESS_COUNT {
        let tenant = TenantContext::explicit(format!("org-{index}"), "workspace", None);
        let stored =
            load_provider_oauth_credential_for_tenant_in_dir(dir.path(), &tenant, "openai-codex")
                .unwrap_or_else(|| panic!("credential for process {index}"));
        assert_eq!(stored.access_token, format!("access-{index}"));
    }
}

#[test]
fn cross_process_disconnect_cannot_be_resurrected_by_stale_refresh() {
    let dir = tempdir().expect("tempdir");
    let tenant = TenantContext::explicit("org-race", "workspace", None);
    set_provider_oauth_credential_for_tenant_in_dir(
        dir.path(),
        &tenant,
        "openai-codex",
        oauth_credential("original"),
    )
    .expect("seed original credential");

    let ready_path = dir.path().join("refresh.ready");
    let release_path = dir.path().join("refresh.release");
    let result_path = dir.path().join("refresh.result");
    let mut child = credential_worker_command(dir.path(), "stale-refresh", "race")
        .env("TANDEM_TEST_PROVIDER_CREDENTIAL_READY", &ready_path)
        .env("TANDEM_TEST_PROVIDER_CREDENTIAL_RELEASE", &release_path)
        .env("TANDEM_TEST_PROVIDER_CREDENTIAL_RESULT", &result_path)
        .spawn()
        .expect("spawn stale refresh worker");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !ready_path.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for stale refresh read"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        delete_provider_credential_for_tenant_in_dir(dir.path(), &tenant, "openai-codex",)
            .expect("disconnect credential")
    );
    std::fs::write(&release_path, b"release").expect("release stale refresh");
    assert!(child.wait().expect("wait for stale refresh").success());
    assert_eq!(
        std::fs::read_to_string(result_path).expect("stale refresh result"),
        "stale"
    );
    assert!(
        load_provider_oauth_credential_for_tenant_in_dir(dir.path(), &tenant, "openai-codex",)
            .is_none(),
        "the stale refresh must not recreate the disconnected credential"
    );
}

#[test]
fn oauth_tenant_enumeration_recovers_local_and_hosted_scopes_without_crossing_tenants() {
    let dir = tempdir().expect("tempdir");
    let local = TenantContext::local_implicit();
    let tenant_a = TenantContext::explicit("org-a", "workspace-a", Some("actor-a".to_string()));
    let mut tenant_b = TenantContext::explicit("org-b", "workspace-b", None);
    tenant_b.deployment_id = Some("deployment-b".to_string());

    for (tenant, label) in [(&local, "local"), (&tenant_a, "a"), (&tenant_b, "b")] {
        set_provider_oauth_credential_for_tenant_in_dir(
            dir.path(),
            tenant,
            "openai-codex",
            oauth_credential(label),
        )
        .expect("store scoped OAuth credential");
    }
    set_provider_oauth_credential_for_tenant_in_dir(
        dir.path(),
        &tenant_a,
        "another-provider",
        OAuthProviderCredential {
            provider_id: "another-provider".to_string(),
            ..oauth_credential("other")
        },
    )
    .expect("store unrelated OAuth credential");

    let contexts = list_provider_oauth_tenant_contexts_in_dir(dir.path(), "openai-codex");
    assert_eq!(contexts.len(), 3);
    assert!(contexts.iter().any(TenantContext::is_local_implicit));
    let enumerated_a = contexts
        .iter()
        .find(|tenant| tenant.org_id == "org-a")
        .expect("tenant A");
    assert_eq!(enumerated_a.workspace_id, "workspace-a");
    assert_eq!(enumerated_a.deployment_id, None);
    assert_eq!(enumerated_a.actor_id, None);
    let enumerated_b = contexts
        .iter()
        .find(|tenant| tenant.org_id == "org-b")
        .expect("tenant B");
    assert_eq!(enumerated_b.workspace_id, "workspace-b");
    assert_eq!(enumerated_b.deployment_id.as_deref(), Some("deployment-b"));
    assert_eq!(enumerated_b.actor_id, None);

    assert!(list_provider_oauth_tenant_contexts_in_dir(dir.path(), "missing").is_empty());
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
