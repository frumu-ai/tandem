//! Authority retained across connector readiness, OAuth recovery and DNS waits.
use std::sync::Arc;

#[derive(Clone)]
pub struct McpRequestAuthority {
    check: Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
}

impl McpRequestAuthority {
    pub fn new(check: impl Fn() -> Result<(), String> + Send + Sync + 'static) -> Self {
        Self {
            check: Arc::new(check),
        }
    }

    pub(crate) fn revalidate(&self) -> Result<(), String> {
        (self.check)()
    }
}
