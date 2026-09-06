use super::*;
use tempfile::tempdir;

fn tenant() -> TenantContext {
    TenantContext::explicit(uuid::Uuid::new_v4().to_string(), "workspace", None)
}

fn oauth(token: &str) -> OAuthProviderCredential {
    OAuthProviderCredential {
        provider_id: "openai-codex".into(),
        access_token: token.into(),
        refresh_token: format!("refresh-{token}"),
        expires_at_ms: 2_000_000_000_000,
        account_id: Some("synthetic-account".into()),
        email: None,
        display_name: None,
        managed_by: "tandem".into(),
        api_key: None,
    }
}

fn revision(
    dir: &Path,
    tenant: &TenantContext,
    kind: ProviderCredentialKind,
) -> ProviderCredentialRevision {
    provider_credential_revision_for_tenant_in_dir(dir, tenant, kind, "openai-codex").unwrap()
}

fn key(dir: &Path, tenant: &TenantContext, token: &str) {
    set_provider_auth_for_tenant_in_dir(dir, tenant, "openai-codex", token).unwrap();
}

#[test]
fn reconnect_delete_and_readd_never_reuse_authorization_revision() {
    let dir = tempdir().unwrap();
    let tenant = tenant();
    let kind = ProviderCredentialKind::ApiKey;
    let mut revisions = HashSet::new();
    for token in ["synthetic-a", "synthetic-b", "synthetic-a", "synthetic-a"] {
        key(dir.path(), &tenant, token);
        let current = revision(dir.path(), &tenant, kind);
        assert!(revisions.insert(current.authorization_revision.clone()));
        // A fresh read from the persisted files has the same revision.
        assert_eq!(revision(dir.path(), &tenant, kind), current);
        let public = serde_json::to_string(&current).unwrap();
        assert!(!public.contains(token));
    }
    assert!(delete_provider_auth_for_tenant_in_dir(dir.path(), &tenant, "openai-codex").unwrap());
    assert!(provider_credential_revision_for_tenant_in_dir(
        dir.path(),
        &tenant,
        kind,
        "openai-codex"
    )
    .is_err());
    let id = tenant_scoped_provider_id(&tenant, "openai-codex");
    assert_eq!(
        record(&strict_json(&kind.index(dir.path())).unwrap(), &id)
            .unwrap()
            .unwrap()
            .state,
        State::Absent
    );
    key(dir.path(), &tenant, "synthetic-a");
    assert!(revisions.insert(revision(dir.path(), &tenant, kind).authorization_revision));
}

#[tokio::test]
async fn same_account_refresh_advances_material_but_compensation_invalidates_authorization() {
    let dir = tempdir().unwrap();
    let tenant = tenant();
    let kind = ProviderCredentialKind::Credential;
    let old = oauth("synthetic-old");
    set_provider_oauth_credential_for_tenant_in_dir(
        dir.path(),
        &tenant,
        "openai-codex",
        old.clone(),
    )
    .unwrap();
    let first = revision(dir.path(), &tenant, kind);
    let new = oauth("synthetic-new");
    assert!(
        refresh_provider_oauth_credential_for_tenant_in_dir_serialized(
            dir.path(),
            &tenant,
            "openai-codex",
            &old,
            new.clone()
        )
        .await
        .unwrap()
    );
    let second = revision(dir.path(), &tenant, kind);
    assert_eq!(first.authorization_revision, second.authorization_revision);
    assert_ne!(first.material_revision, second.material_revision);
    // A stale refresh neither writes material nor advances a revision.
    assert!(
        !refresh_provider_oauth_credential_for_tenant_in_dir_serialized(
            dir.path(),
            &tenant,
            "openai-codex",
            &old,
            oauth("stale")
        )
        .await
        .unwrap()
    );
    assert_eq!(revision(dir.path(), &tenant, kind), second);
    assert!(compare_and_set_provider_oauth_credential_for_tenant_in_dir(
        dir.path(),
        &tenant,
        "openai-codex",
        &new,
        Some(old)
    )
    .unwrap());
    let restored = revision(dir.path(), &tenant, kind);
    assert_ne!(
        restored.authorization_revision,
        first.authorization_revision
    );
    assert_ne!(restored.material_revision, second.material_revision);
}

#[tokio::test]
async fn changed_unknown_or_untracked_account_refresh_requires_new_authorization() {
    for account in [Some("different-account"), None, Some("")] {
        let dir = tempdir().unwrap();
        let tenant = tenant();
        let old = oauth("synthetic-old");
        set_provider_oauth_credential_for_tenant_in_dir(
            dir.path(),
            &tenant,
            "openai-codex",
            old.clone(),
        )
        .unwrap();
        let first = revision(dir.path(), &tenant, ProviderCredentialKind::Credential);
        let mut new = oauth("synthetic-new");
        new.account_id = account.map(str::to_string);
        assert!(
            refresh_provider_oauth_credential_for_tenant_in_dir_serialized(
                dir.path(),
                &tenant,
                "openai-codex",
                &old,
                new
            )
            .await
            .unwrap()
        );
        assert_ne!(
            first.authorization_revision,
            revision(dir.path(), &tenant, ProviderCredentialKind::Credential)
                .authorization_revision
        );
    }
    let dir = tempdir().unwrap();
    let tenant = tenant();
    let old = oauth("untracked-old");
    set_provider_oauth_credential_for_tenant_in_dir(
        dir.path(),
        &tenant,
        "openai-codex",
        old.clone(),
    )
    .unwrap();
    let kind = ProviderCredentialKind::Credential;
    let first = revision(dir.path(), &tenant, kind);
    let mut index = strict_json(&kind.index(dir.path())).unwrap();
    index.as_object_mut().unwrap().remove("credential_bindings");
    write_secure_json(&kind.index(dir.path()), &index).unwrap();
    assert!(
        refresh_provider_oauth_credential_for_tenant_in_dir_serialized(
            dir.path(),
            &tenant,
            "openai-codex",
            &old,
            oauth("new")
        )
        .await
        .unwrap()
    );
    assert_ne!(
        first.authorization_revision,
        revision(dir.path(), &tenant, kind).authorization_revision
    );
}

#[test]
fn missing_corrupt_or_out_of_band_material_cannot_satisfy_revision_lookup() {
    for fault in [
        "missing",
        "index-corrupt",
        "fallback-corrupt",
        "material",
        "schema",
    ] {
        let dir = tempdir().unwrap();
        let tenant = tenant();
        let kind = ProviderCredentialKind::ApiKey;
        key(dir.path(), &tenant, "synthetic-a");
        let index_path = kind.index(dir.path());
        let fallback_path = kind.fallback(dir.path());
        let id = tenant_scoped_provider_id(&tenant, "openai-codex");
        match fault {
            "missing" => {
                std::fs::remove_file(index_path).unwrap();
            }
            "index-corrupt" => std::fs::write(index_path, b"{").unwrap(),
            "fallback-corrupt" => std::fs::write(fallback_path, b"{").unwrap(),
            "material" => {
                let mut value = strict_json(&fallback_path).unwrap();
                value[&id] = json!("out-of-band-replacement");
                write_secure_json(&fallback_path, &value).unwrap();
            }
            "schema" => {
                let mut value = strict_json(&index_path).unwrap();
                value["credential_bindings"][&id]["schema_version"] = json!(2);
                write_secure_json(&index_path, &value).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            provider_credential_revision_for_tenant_in_dir(
                dir.path(),
                &tenant,
                kind,
                "openai-codex"
            )
            .is_err(),
            "{fault}"
        );
        if fault.ends_with("corrupt") {
            assert!(
                set_provider_auth_for_tenant_in_dir(dir.path(), &tenant, "openai-codex", "repair")
                    .is_err(),
                "corrupt files must not erase unrelated accounts"
            );
        }
    }
}

#[test]
fn interrupted_or_failed_write_stays_pending_until_explicit_replacement() {
    for write_material in [false, true] {
        let dir = tempdir().unwrap();
        let tenant = tenant();
        let kind = ProviderCredentialKind::ApiKey;
        key(dir.path(), &tenant, "synthetic-a");
        let first = revision(dir.path(), &tenant, kind);
        let id = tenant_scoped_provider_id(&tenant, "openai-codex");
        {
            let _lock = ProviderCredentialMutationFileLock::acquire_blocking(dir.path()).unwrap();
            let mutation = Mutation::begin(
                dir.path(),
                kind,
                &id,
                Some(json!("synthetic-b")),
                false,
                false,
            )
            .unwrap();
            if write_material {
                let mut map = strict_json(&kind.fallback(dir.path())).unwrap();
                map[&id] = json!("synthetic-b");
                write_secure_json(&kind.fallback(dir.path()), &map).unwrap();
                // Simulate termination after publishing the secret, before commit.
                drop(mutation);
            } else {
                assert!(mutation.finish(Some(ProviderAuthBackend::File)).is_err());
            }
        }
        assert!(provider_credential_revision_for_tenant_in_dir(
            dir.path(),
            &tenant,
            kind,
            "openai-codex"
        )
        .is_err());
        key(dir.path(), &tenant, "synthetic-a");
        assert_ne!(
            first.authorization_revision,
            revision(dir.path(), &tenant, kind).authorization_revision
        );
    }
}

#[test]
fn lifecycle_worker() {
    let Ok(dir) = std::env::var("TANDEM_TEST_LIFECYCLE_DIR") else {
        return;
    };
    let label = std::env::var("TANDEM_TEST_LIFECYCLE_LABEL").unwrap();
    let tenant = TenantContext::explicit(label, "workspace", None);
    key(Path::new(&dir), &tenant, "synthetic-process-key");
    if let Ok(phase) = std::env::var("TANDEM_TEST_LIFECYCLE_CRASH_PHASE") {
        let dir = Path::new(&dir);
        let _lock = ProviderCredentialMutationFileLock::acquire_blocking(dir).unwrap();
        let id = tenant_scoped_provider_id(&tenant, "openai-codex");
        let kind = ProviderCredentialKind::ApiKey;
        let _mutation = Mutation::begin(
            dir,
            kind,
            &id,
            Some(json!("synthetic-crash-key")),
            false,
            false,
        )
        .unwrap();
        if phase == "after-material" {
            let mut material = strict_json(&kind.fallback(dir)).unwrap();
            material[&id] = json!("synthetic-crash-key");
            write_secure_json(&kind.fallback(dir), &material).unwrap();
        }
        // Deliberately bypass destructors, as a terminated process would. The
        // parent must reacquire the OS lock and still reject this pending write.
        std::process::exit(0);
    }
}

#[test]
fn process_exit_before_commit_never_exposes_an_active_revision() {
    for phase in ["before-material", "after-material"] {
        let dir = tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "provider_auth_store::credential_lifecycle::tests::lifecycle_worker",
            ])
            .env("TANDEM_TEST_LIFECYCLE_DIR", dir.path())
            .env("TANDEM_TEST_LIFECYCLE_LABEL", "crash-worker")
            .env("TANDEM_TEST_LIFECYCLE_CRASH_PHASE", phase)
            .status()
            .unwrap();
        assert!(status.success());
        let tenant = TenantContext::explicit("crash-worker", "workspace", None);
        assert!(provider_credential_revision_for_tenant_in_dir(
            dir.path(),
            &tenant,
            ProviderCredentialKind::ApiKey,
            "openai-codex",
        )
        .is_err());
        let actual = load_provider_auth_for_tenant_in_dir(dir.path(), &tenant);
        assert_eq!(
            actual["openai-codex"],
            if phase == "after-material" {
                "synthetic-crash-key"
            } else {
                "synthetic-process-key"
            }
        );
        key(dir.path(), &tenant, "synthetic-explicit-reconnect");
        revision(dir.path(), &tenant, ProviderCredentialKind::ApiKey);
    }
}

#[test]
fn separate_process_api_key_writers_preserve_all_credentials_and_revisions() {
    let dir = tempdir().unwrap();
    let mut children = Vec::new();
    for index in 0..8 {
        children.push(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "provider_auth_store::credential_lifecycle::tests::lifecycle_worker",
                ])
                .env("TANDEM_TEST_LIFECYCLE_DIR", dir.path())
                .env("TANDEM_TEST_LIFECYCLE_LABEL", format!("process-{index}"))
                .spawn()
                .unwrap(),
        );
    }
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    let mut revisions = HashSet::new();
    for index in 0..8 {
        let tenant = TenantContext::explicit(format!("process-{index}"), "workspace", None);
        assert_eq!(
            load_provider_auth_for_tenant_in_dir(dir.path(), &tenant)["openai-codex"],
            "synthetic-process-key"
        );
        assert!(revisions.insert(
            revision(dir.path(), &tenant, ProviderCredentialKind::ApiKey).authorization_revision
        ));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn api_key_file_lock_wait_yields_the_runtime_and_shares_oauth_serialization() {
    let dir = tempdir().unwrap();
    let tenant = tenant();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let path = dir.path().to_path_buf();
    let blocker = std::thread::spawn(move || {
        let _guard = ProviderCredentialMutationFileLock::acquire_blocking(&path).unwrap();
        ready_tx.send(()).unwrap();
        // A broken blocking acquisition must fail this test rather than hang
        // the test process indefinitely waiting for its own async timer.
        let _ = release_rx.recv_timeout(std::time::Duration::from_secs(3));
    });
    ready_rx.recv().unwrap();
    let acquisition = provider_auth_mutation_in_dir(dir.path());
    tokio::pin!(acquisition);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(25), &mut acquisition)
            .await
            .is_err()
    );
    release_tx.send(()).unwrap();
    let mut mutation = acquisition.await.unwrap();
    mutation
        .set_for_tenant(&tenant, "openai-codex", "synthetic-async-key")
        .unwrap();
    let oauth_write = set_provider_oauth_credential_for_tenant_in_dir_serialized(
        dir.path(),
        &tenant,
        "openai-codex",
        oauth("synthetic-oauth"),
    );
    tokio::pin!(oauth_write);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(25), &mut oauth_write)
            .await
            .is_err()
    );
    drop(mutation);
    oauth_write.await.unwrap();
    blocker.join().unwrap();
    revision(dir.path(), &tenant, ProviderCredentialKind::ApiKey);
    revision(dir.path(), &tenant, ProviderCredentialKind::Credential);
}
