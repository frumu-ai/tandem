// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn custom_bindings() -> CapabilityBindingsFile {
    let mut file = CapabilityBindingsFile::default();
    file.bindings.clear();
    file.bindings.push(CapabilityBinding {
        capability_id: "test.shared_capability".into(),
        provider: "custom".into(),
        tool_name: "original_tool".into(),
        tool_name_aliases: Vec::new(),
        request_transform: None,
        response_transform: None,
        metadata: Value::Null,
    });
    file
}

async fn checked_mutation(
    resolver: &CapabilityResolver,
    operation: &str,
    authorize: impl Fn() -> anyhow::Result<()> + Send + Sync + 'static,
) -> anyhow::Result<()> {
    match operation {
        "put" => {
            let mut replacement = custom_bindings();
            replacement.bindings[0].tool_name = "replacement_tool".into();
            resolver.set_bindings_checked(replacement, authorize).await
        }
        "refresh" => resolver
            .refresh_builtin_bindings_checked(authorize)
            .await
            .map(|_| ()),
        _ => resolver
            .reset_to_builtin_bindings_checked(authorize)
            .await
            .map(|_| ()),
    }
}

#[tokio::test]
async fn checked_binding_mutations_recheck_after_waiting_for_lock() {
    for operation in ["put", "refresh", "reset"] {
        let temp = tempfile::tempdir().unwrap();
        let resolver = Arc::new(CapabilityResolver::new(temp.path().to_path_buf()));
        resolver.set_bindings(custom_bindings()).await.unwrap();
        let path = temp.path().join("bindings/capability_bindings.json");
        let before = std::fs::read(&path).unwrap();
        let lock = resolver.lock.lock().await;
        let allowed = Arc::new(AtomicBool::new(true));
        let checks = Arc::new(AtomicUsize::new(0));
        let allowed_for_check = allowed.clone();
        let checks_for_check = checks.clone();
        let authorize = move || {
            checks_for_check.fetch_add(1, Ordering::SeqCst);
            if allowed_for_check.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(anyhow!("authorization revoked"))
            }
        };
        let pending_resolver = resolver.clone();
        let pending =
            tokio::spawn(
                async move { checked_mutation(&pending_resolver, operation, authorize).await },
            );
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while checks.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("mutation did not check authority before waiting for the lock");
        allowed.store(false, Ordering::SeqCst);
        drop(lock);
        let error = pending.await.unwrap().unwrap_err();
        assert!(error.to_string().contains("authorization revoked"));
        assert_eq!(std::fs::read(&path).unwrap(), before, "{operation}");
    }
}

#[tokio::test]
async fn checked_binding_mutations_recheck_immediately_before_file_write() {
    for operation in ["put", "refresh", "reset"] {
        let temp = tempfile::tempdir().unwrap();
        let resolver = CapabilityResolver::new(temp.path().to_path_buf());
        resolver.set_bindings(custom_bindings()).await.unwrap();
        let path = temp.path().join("bindings/capability_bindings.json");
        let before = std::fs::read(&path).unwrap();
        let checks = Arc::new(AtomicUsize::new(0));
        let checks_for_check = checks.clone();
        let error = checked_mutation(&resolver, operation, move || {
            if checks_for_check.fetch_add(1, Ordering::SeqCst) < 2 {
                Ok(())
            } else {
                Err(anyhow!("authorization revoked at write"))
            }
        })
        .await
        .unwrap_err();
        assert!(error.to_string().contains("authorization revoked at write"));
        assert_eq!(checks.load(Ordering::SeqCst), 3, "{operation}");
        assert_eq!(std::fs::read(&path).unwrap(), before, "{operation}");
    }
}

#[tokio::test]
async fn binding_reads_do_not_create_or_rewrite_shared_file() {
    let temp = tempfile::tempdir().unwrap();
    let resolver = CapabilityResolver::new(temp.path().to_path_buf());
    let path = temp.path().join("bindings/capability_bindings.json");

    let defaults = resolver.list_bindings().await.unwrap();
    assert!(!defaults.bindings.is_empty());
    assert!(
        !path.exists(),
        "reading missing bindings created a shared file"
    );

    resolver.set_bindings(custom_bindings()).await.unwrap();
    let before = std::fs::read(&path).unwrap();
    let merged = resolver.list_bindings().await.unwrap();
    assert!(
        merged.bindings.len() > 1,
        "read should still expose built-ins"
    );
    resolver
        .resolve(
            CapabilityResolveInput {
                workflow_id: None,
                required_capabilities: Vec::new(),
                optional_capabilities: Vec::new(),
                provider_preference: Vec::new(),
                available_tools: Vec::new(),
            },
            Vec::new(),
        )
        .await
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);

    resolver.refresh_builtin_bindings().await.unwrap();
    assert_ne!(std::fs::read(&path).unwrap(), before);
}
