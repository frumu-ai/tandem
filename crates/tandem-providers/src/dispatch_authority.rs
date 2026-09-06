//! Mutable request authority is checked after credential resolution and again
//! after authentication recovery and before each adapter send, including retries.
//! It is never cached in a provider permit.
use std::future::Future;
use std::sync::Arc;

use futures::future::BoxFuture;

#[derive(Clone)]
pub struct ProviderDispatchAuthority {
    check: Arc<dyn Fn() -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
}

tokio::task_local! {
    static DISPATCH_AUTHORITY: ProviderDispatchAuthority;
}

impl ProviderDispatchAuthority {
    pub fn new<F, Fut>(check: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        Self {
            check: Arc::new(move || Box::pin(check())),
        }
    }

    /// Scope the guard to this future, like the registry's tenant credentials.
    /// Spawned tasks must explicitly carry their own authority scope.
    pub fn scope<F: Future>(self, future: F) -> impl Future<Output = F::Output> {
        // Engine prompts can have large futures. Keep them off the nested
        // task-local wrapper's stack while preserving the same polling scope.
        DISPATCH_AUTHORITY.scope(self, Box::pin(future))
    }
}

pub(crate) async fn revalidate() -> anyhow::Result<()> {
    if let Ok(authority) = DISPATCH_AUTHORITY.try_with(Clone::clone) {
        (authority.check)().await?;
    }
    Ok(())
}

/// Redirects would dispatch another request inside reqwest without an async
/// authority check. Provider endpoints must be configured to their final URL.
pub(crate) fn provider_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("provider HTTP client initialization failed")
}
