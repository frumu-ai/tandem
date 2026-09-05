//! Mutable request authority is checked after credential resolution and again
//! after authentication recovery. It is never cached in a provider permit.
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
    pub async fn scope<F: Future>(self, future: F) -> F::Output {
        DISPATCH_AUTHORITY.scope(self, future).await
    }
}

pub(crate) async fn revalidate() -> anyhow::Result<()> {
    if let Ok(authority) = DISPATCH_AUTHORITY.try_with(Clone::clone) {
        (authority.check)().await?;
    }
    Ok(())
}
