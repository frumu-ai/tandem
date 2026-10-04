/// Locks the connector representations without modifying them. Dropping this
/// preparation is harmless; callers must authorize at `commit_checked`.
pub struct McpRemovalPlan<'a> {
    registry: &'a McpRegistry,
    name: String,
    tenant: TenantContext,
    credential_guard: tokio::sync::OwnedMutexGuard<()>,
    servers: tokio::sync::RwLockWriteGuard<'a, HashMap<String, McpServer>>,
    connections: tokio::sync::RwLockWriteGuard<'a, HashMap<String, McpConnection>>,
    processes: tokio::sync::MutexGuard<'a, HashMap<String, Child>>,
}

#[must_use = "await finish to observe completion of connector cleanup"]
pub struct McpRemovalCompletion {
    removed: bool,
    cleanup: Option<tokio::task::JoinHandle<()>>,
}

impl McpRemovalCompletion {
    pub async fn finish(self) -> bool {
        if let Some(cleanup) = self.cleanup {
            let _ = cleanup.await;
        }
        self.removed
    }
}

impl McpRegistry {
    pub async fn prepare_remove_for_tenant<'a>(
        &'a self,
        name: &str,
        tenant: &TenantContext,
    ) -> McpRemovalPlan<'a> {
        let credential_guard = self.credential_mutation_lock.clone().lock_owned().await;
        let servers = self.servers.write().await;
        let connections = self.connections.write().await;
        let processes = self.processes.lock().await;
        McpRemovalPlan {
            registry: self,
            name: name.to_string(),
            tenant: tenant.clone(),
            credential_guard,
            servers,
            connections,
            processes,
        }
    }
}

impl McpRemovalPlan<'_> {
    /// The callback runs after all runtime lock waits. No state is removed on
    /// denial. It must not await or reenter either locked registry.
    pub fn commit_checked<E>(
        self,
        authorize: impl FnOnce() -> Result<(), E>,
    ) -> Result<McpRemovalCompletion, E> {
        let Self {
            registry,
            name,
            tenant,
            credential_guard,
            mut servers,
            mut connections,
            mut processes,
        } = self;
        authorize()?;
        let Some(server) = servers.remove(&name) else {
            return Ok(McpRemovalCompletion {
                removed: false,
                cleanup: None,
            });
        };
        connections.retain(|id, connection| {
            if connection.server_id == name {
                registry.invalidate_oauth_refresh_connection(id);
                false
            } else {
                true
            }
        });
        delete_secret_header_refs(&server.secret_headers, &tenant);
        delete_oauth_secret_ref(server.oauth.as_ref(), &tenant);
        delete_oauth_credential(
            &name,
            server.oauth.as_ref(),
            &tenant,
            &registry.oauth_security_dir,
        );
        let child = processes.remove(&name);
        drop(processes);
        drop(connections);
        drop(servers);
        let registry = registry.clone();
        // Cleanup completes an already committed mutation. Caller cancellation
        // must not abandon the detached child or release credential ordering.
        let cleanup = tokio::spawn(async move {
            let _credential_guard = credential_guard;
            if let Some(mut child) = child {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
            registry.persist_state().await;
        });
        Ok(McpRemovalCompletion {
            removed: true,
            cleanup: Some(cleanup),
        })
    }
}

#[cfg(test)]
mod removal_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn mcp_removal_revalidates_after_each_runtime_lock_wait() {
        let _auth_guard = super::tests::provider_auth_test_guard().await;
        for blocked in 0..4 {
            let file = PathBuf::from(std::env::var_os("TANDEM_HOME").unwrap())
                .join(format!("removal-{}.json", uuid::Uuid::new_v4()));
            let registry = McpRegistry::new_with_state_file(file.clone());
            let tenant = TenantContext::local_implicit();
            registry
                .add_or_update(
                    "remove-test".into(),
                    "https://example.invalid/mcp".into(),
                    HashMap::new(),
                    true,
                )
                .await;
            registry
                .set_bearer_token("remove-test", "test-only-bearer")
                .await
                .unwrap();
            let saved = std::fs::read(&file).unwrap();
            let held_credential = if blocked == 0 {
                Some(registry.credential_mutation_lock.lock().await)
            } else {
                None
            };
            let held_server = if blocked == 1 {
                Some(registry.servers.read().await)
            } else {
                None
            };
            let held_connection = if blocked == 2 {
                Some(registry.connections.read().await)
            } else {
                None
            };
            let held_process = if blocked == 3 {
                Some(registry.processes.lock().await)
            } else {
                None
            };
            let allowed = AtomicBool::new(true);
            let preparation = registry.prepare_remove_for_tenant("remove-test", &tenant);
            tokio::pin!(preparation);
            let first_poll = std::future::poll_fn(|cx| {
                std::task::Poll::Ready(std::future::Future::poll(preparation.as_mut(), cx))
            })
            .await;
            assert!(first_poll.is_pending());
            allowed.store(false, Ordering::SeqCst);
            drop(held_process);
            drop(held_connection);
            drop(held_server);
            drop(held_credential);
            let plan = preparation.await;
            let denied = plan.commit_checked(|| {
                if allowed.load(Ordering::SeqCst) {
                    Ok(())
                } else {
                    Err("revoked")
                }
            });
            assert!(matches!(denied, Err("revoked")));
            assert!(registry.servers.read().await.contains_key("remove-test"));
            let connection = registry
                .connection_for_tenant("remove-test", &tenant)
                .await
                .unwrap();
            assert_eq!(
                resolve_secret_ref_value(&connection.secret_headers["Authorization"], &tenant)
                    .as_deref(),
                Some("Bearer test-only-bearer")
            );
            assert_eq!(std::fs::read(&file).unwrap(), saved);
            assert!(registry.remove_for_tenant("remove-test", &tenant).await);
            assert!(!registry.servers.read().await.contains_key("remove-test"));
            assert!(registry
                .connection_for_tenant("remove-test", &tenant)
                .await
                .is_none());
            let reloaded = McpRegistry::new_with_state_file(file);
            assert!(!reloaded.servers.read().await.contains_key("remove-test"));
        }
    }
}
