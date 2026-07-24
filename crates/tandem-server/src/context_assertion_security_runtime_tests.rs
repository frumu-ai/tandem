// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use ed25519_dalek::{Signer, SigningKey};
use tandem_enterprise_contract::{
    AuthorityChain, HumanActor, RequestPrincipal, TenantContext, TenantContextAssertionClaims,
    TenantContextAssertionHeader,
};

const ASSERTION_ENV: &[&str] = &[
    "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS",
    "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS_FILE",
    "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY",
    "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY_FILE",
    "TANDEM_CONTEXT_ASSERTION_ISSUER",
    "TANDEM_CONTEXT_ASSERTION_AUDIENCE",
    "TANDEM_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS",
    "TANDEM_CONTEXT_ASSERTION_MAX_LIFETIME_MS",
    "TANDEM_CONTEXT_ASSERTION_REPLAY_MODE",
    "TANDEM_CONTEXT_ASSERTION_REPLAY_STORE_FILE",
    "TANDEM_HOSTED_CONTROL_PLANE_URL",
    "TANDEM_ENTERPRISE_CONTROL_PLANE_URL",
];

struct AssertionEnvGuard(Vec<(String, Option<String>)>);

impl AssertionEnvGuard {
    fn cleared() -> Self {
        let saved = ASSERTION_ENV
            .iter()
            .map(|name| ((*name).to_string(), std::env::var(name).ok()))
            .collect::<Vec<_>>();
        for name in ASSERTION_ENV {
            std::env::remove_var(name);
        }
        Self(saved)
    }

    fn set(&self, name: &str, value: impl AsRef<std::ffi::OsStr>) {
        std::env::set_var(name, value);
    }
}

impl Drop for AssertionEnvGuard {
    fn drop(&mut self) {
        for (name, value) in self.0.drain(..) {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }
}

fn signing_key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn metadata_keyring(key: &SigningKey, kid: &str) -> String {
    let mut entries = BTreeMap::new();
    entries.insert(
        kid.to_string(),
        serde_json::json!({
            "purpose": "context_assertion",
            "public_key": base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(key.verifying_key().to_bytes()),
            "organization_id": "org-a",
            "deployment_id": "dep-a",
            "allowed_audiences": ["tandem-runtime"],
            "status": "active"
        }),
    );
    serde_json::to_string(&entries).expect("keyring")
}

fn claims(assertion_id: &str) -> TenantContextAssertionClaims {
    let principal = RequestPrincipal::authenticated_user("user-a", "tandem-web");
    TenantContextAssertionClaims::new_v1(
        "tandem-web",
        "tandem-runtime",
        1_800_000_000_000,
        1_800_000_060_000,
        assertion_id,
        TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("dep-a".to_string()),
            "user-a",
        ),
        HumanActor::tandem_user("user-a"),
        AuthorityChain::from_request(principal),
        vec!["workspace:user".to_string()],
    )
}

fn sign(key: &SigningKey, kid: &str, claims: &TenantContextAssertionClaims) -> String {
    let header = TenantContextAssertionHeader::ed25519(kid);
    let encoded_header = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&header).expect("header"));
    let encoded_claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(claims).expect("claims"));
    let signing_input = format!("{encoded_header}.{encoded_claims}");
    let signature = key.sign(signing_input.as_bytes());
    format!(
        "{signing_input}.{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.to_bytes())
    )
}

fn configure_hosted(guard: &AssertionEnvGuard, keyring: &str, replay_path: &Path) {
    guard.set("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS", keyring);
    guard.set("TANDEM_CONTEXT_ASSERTION_REPLAY_STORE_FILE", replay_path);
}

#[test]
#[serial_test::serial(context_assertion_env)]
fn hosted_loader_rejects_legacy_keys_missing_replay_and_replay_off() {
    let guard = AssertionEnvGuard::cleared();
    let key = signing_key(21);
    guard.set(
        "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key.verifying_key().to_bytes()),
    );
    let legacy =
        RuntimeContextAssertionSecurity::load_from_env(RuntimeAuthMode::HostedSingleTenant)
            .expect_err("legacy hosted key must fail");
    assert!(legacy.contains("rejects legacy"));

    std::env::remove_var("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY");
    guard.set(
        "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS",
        metadata_keyring(&key, "key-a"),
    );
    let missing_replay =
        RuntimeContextAssertionSecurity::load_from_env(RuntimeAuthMode::HostedSingleTenant)
            .expect_err("missing replay store must fail");
    assert!(missing_replay.contains("REPLAY_STORE_FILE"));

    let temp = tempfile::tempdir().expect("tempdir");
    guard.set(
        "TANDEM_CONTEXT_ASSERTION_REPLAY_STORE_FILE",
        temp.path().join("replay.json"),
    );
    guard.set("TANDEM_CONTEXT_ASSERTION_REPLAY_MODE", "off");
    let disabled =
        RuntimeContextAssertionSecurity::load_from_env(RuntimeAuthMode::HostedSingleTenant)
            .expect_err("hosted replay off must fail");
    assert!(disabled.contains("cannot disable"));
}

#[test]
#[serial_test::serial(context_assertion_env)]
fn configured_hosted_control_plane_uses_hosted_assertion_rules() {
    let guard = AssertionEnvGuard::cleared();
    let key = signing_key(29);
    guard.set(
        "TANDEM_HOSTED_CONTROL_PLANE_URL",
        "https://control.example.test",
    );
    guard.set(
        "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key.verifying_key().to_bytes()),
    );
    let error = RuntimeContextAssertionSecurity::load_from_env(RuntimeAuthMode::LocalSingleTenant)
        .expect_err("configured hosted control plane must reject local legacy compatibility");
    assert!(error.contains("rejects legacy"));
}

#[cfg(unix)]
#[test]
#[serial_test::serial(context_assertion_env)]
fn hosted_keyring_file_requires_owner_only_regular_file() {
    use std::os::unix::fs::PermissionsExt;

    let guard = AssertionEnvGuard::cleared();
    let temp = tempfile::tempdir().expect("tempdir");
    let keyring_path = temp.path().join("keyring.json");
    std::fs::write(&keyring_path, metadata_keyring(&signing_key(22), "key-a"))
        .expect("keyring file");
    std::fs::set_permissions(&keyring_path, std::fs::Permissions::from_mode(0o644))
        .expect("permissions");
    guard.set("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS_FILE", &keyring_path);
    guard.set(
        "TANDEM_CONTEXT_ASSERTION_REPLAY_STORE_FILE",
        temp.path().join("replay.json"),
    );
    let insecure =
        RuntimeContextAssertionSecurity::load_from_env(RuntimeAuthMode::EnterpriseRequired)
            .expect_err("insecure keyring permissions must fail");
    assert!(insecure.contains("insecure mode"));

    std::fs::set_permissions(&keyring_path, std::fs::Permissions::from_mode(0o600))
        .expect("permissions");
    assert!(
        RuntimeContextAssertionSecurity::load_from_env(RuntimeAuthMode::EnterpriseRequired)
            .expect("secure keyring")
            .is_some()
    );
    let keyring_link = temp.path().join("keyring-link.json");
    std::os::unix::fs::symlink(&keyring_path, &keyring_link).expect("keyring symlink");
    guard.set("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS_FILE", &keyring_link);
    let symlinked =
        RuntimeContextAssertionSecurity::load_from_env(RuntimeAuthMode::EnterpriseRequired)
            .expect_err("symlinked keyring must fail");
    assert!(symlinked.contains("failed to open keyring file"));
}

#[test]
#[serial_test::serial(context_assertion_env)]
fn failed_reload_retains_last_known_good_and_requests_never_reread_env() {
    let guard = AssertionEnvGuard::cleared();
    let temp = tempfile::tempdir().expect("tempdir");
    let replay_path = temp.path().join("replay.json");
    let first_key = signing_key(23);
    let second_key = signing_key(24);
    configure_hosted(&guard, &metadata_keyring(&first_key, "key-a"), &replay_path);
    let state = crate::AppState::new_starting("assertion-reload-test".to_string(), true);

    let first_reload = state
        .reload_context_assertion_security(RuntimeAuthMode::HostedSingleTenant)
        .expect("first load");
    let first = first_reload.current.expect("first snapshot");
    assert!(first_reload.previous.is_none());

    guard.set("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS", "{invalid");
    assert!(state
        .reload_context_assertion_security(RuntimeAuthMode::HostedSingleTenant)
        .is_err());
    let retained = state
        .context_assertion_security_snapshot()
        .expect("retained snapshot");
    assert!(std::sync::Arc::ptr_eq(&first, &retained));

    guard.set(
        "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS",
        metadata_keyring(&second_key, "key-b"),
    );
    let second_reload = state
        .reload_context_assertion_security(RuntimeAuthMode::HostedSingleTenant)
        .expect("rotation");
    assert!(std::sync::Arc::ptr_eq(
        second_reload.previous.as_ref().expect("previous"),
        &first
    ));
    let second = second_reload.current.expect("second snapshot");
    assert_ne!(first.keyring_fingerprint(), second.keyring_fingerprint());
    assert!(std::sync::Arc::ptr_eq(
        &second,
        &state
            .context_assertion_security_snapshot()
            .expect("published rotation")
    ));

    guard.set("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS", "{invalid-again");
    assert!(first
        .verify_at(
            &sign(&first_key, "key-a", &claims("old-generation")),
            1_800_000_000_000,
        )
        .is_ok());
    assert!(second
        .verify_at(
            &sign(&second_key, "key-b", &claims("new-generation")),
            1_800_000_000_000,
        )
        .is_ok());
    assert_eq!(
        second.verify_at(
            &sign(&first_key, "key-a", &claims("wrong-generation")),
            1_800_000_000_000,
        ),
        Err(ContextAssertionError::UnknownKey)
    );
}
