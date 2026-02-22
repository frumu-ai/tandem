//! Middleware hook pipeline for the Tandem engine.
//!
//! `HookHandler` implementations can intercept key lifecycle events (LLM calls,
//! tool calls, session start/end) with zero changes to core engine code.
//!
//! Hooks run in priority order (lowest priority value first). Modifying hooks
//! return `HookResult::Continue(modified_value)` to proceed or
//! `HookResult::Cancel(reason)` to abort the action.
//!
//! # Example
//! ```rust,ignore
//! struct RateLimitHook;
//!
//! #[async_trait::async_trait]
//! impl HookHandler for RateLimitHook {
//!     fn name(&self) -> &str { "rate_limit" }
//!     fn priority(&self) -> i32 { -100 }  // run first
//!
//!     async fn before_llm_call(
//!         &self, messages: Vec<serde_json::Value>
//!     ) -> HookResult<Vec<serde_json::Value>> {
//!         if quota_exceeded() {
//!             HookResult::Cancel("rate limit exceeded".into())
//!         } else {
//!             HookResult::Continue(messages)
//!         }
//!     }
//! }
//! ```

use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

// ---------------------------------------------------------------------------
// HookResult
// ---------------------------------------------------------------------------

/// Result of a modifying hook — either continue with (possibly modified) data,
/// or cancel the action with a reason string.
#[derive(Debug, Clone)]
pub enum HookResult<T> {
    Continue(T),
    Cancel(String),
}

impl<T> HookResult<T> {
    pub fn is_cancel(&self) -> bool {
        matches!(self, HookResult::Cancel(_))
    }
}

// ---------------------------------------------------------------------------
// HookHandler trait
// ---------------------------------------------------------------------------

/// Trait for hook handlers. All methods have default no-op implementations;
/// only implement the events you care about.
#[async_trait::async_trait]
pub trait HookHandler: Send + Sync {
    /// Unique name for logging and diagnostics.
    fn name(&self) -> &str;

    /// Execution priority — lower values run first. Default is 0.
    fn priority(&self) -> i32 {
        0
    }

    // ------------------------------------------------------------------
    // Modifying hooks (sequential by priority, can cancel the action)
    // ------------------------------------------------------------------

    /// Called before each LLM API call. May modify the message list or cancel.
    async fn before_llm_call(
        &self,
        messages: Vec<Value>,
        model: String,
    ) -> HookResult<(Vec<Value>, String)> {
        HookResult::Continue((messages, model))
    }

    /// Called before each tool is executed. May modify args or cancel the tool call.
    async fn before_tool_call(
        &self,
        tool_name: String,
        args: Value,
    ) -> HookResult<(String, Value)> {
        HookResult::Continue((tool_name, args))
    }

    // ------------------------------------------------------------------
    // Observable hooks (fire-and-forget, run in parallel)
    // ------------------------------------------------------------------

    /// Called after a session is created.
    async fn on_session_start(&self, _session_id: &str, _channel: &str) {}

    /// Called when a session ends or is deleted.
    async fn on_session_end(&self, _session_id: &str) {}

    /// Called after a tool call completes (success or error).
    async fn on_after_tool_call(&self, _tool: &str, _success: bool, _duration: Duration) {}

    /// Called after the LLM returns a response.
    async fn on_llm_response(&self, _model: &str, _token_count: u32, _duration: Duration) {}
}

// ---------------------------------------------------------------------------
// HookRegistry
// ---------------------------------------------------------------------------

/// Registry that manages and runs hooks in priority order.
pub struct HookRegistry {
    hooks: Vec<Arc<dyn HookHandler>>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    /// Register a hook handler. Hooks are automatically sorted by priority.
    pub fn register(&mut self, handler: Arc<dyn HookHandler>) {
        self.hooks.push(handler);
        self.hooks.sort_by_key(|h| h.priority());
        tracing::debug!(
            "hook registered (priority {})",
            self.hooks.last().map(|h| h.priority()).unwrap_or(0)
        );
    }

    /// Run all `before_llm_call` hooks in priority order.
    /// Returns `None` if any hook cancels the call.
    pub async fn run_before_llm_call(
        &self,
        messages: Vec<Value>,
        model: String,
    ) -> Option<(Vec<Value>, String)> {
        let mut current = (messages, model);
        for hook in &self.hooks {
            match hook
                .before_llm_call(current.0.clone(), current.1.clone())
                .await
            {
                HookResult::Continue(next) => current = next,
                HookResult::Cancel(reason) => {
                    tracing::info!(hook = hook.name(), "before_llm_call cancelled: {}", reason);
                    return None;
                }
            }
        }
        Some(current)
    }

    /// Run all `before_tool_call` hooks in priority order.
    /// Returns `None` if any hook cancels the tool call.
    pub async fn run_before_tool_call(
        &self,
        tool_name: String,
        args: Value,
    ) -> Option<(String, Value)> {
        let mut current = (tool_name, args);
        for hook in &self.hooks {
            match hook
                .before_tool_call(current.0.clone(), current.1.clone())
                .await
            {
                HookResult::Continue(next) => current = next,
                HookResult::Cancel(reason) => {
                    tracing::info!(hook = hook.name(), "before_tool_call cancelled: {}", reason);
                    return None;
                }
            }
        }
        Some(current)
    }

    /// Fire all `on_session_start` observable hooks (parallel, best-effort).
    pub async fn fire_session_start(&self, session_id: &str, channel: &str) {
        for hook in &self.hooks {
            hook.on_session_start(session_id, channel).await;
        }
    }

    /// Fire all `on_session_end` observable hooks (parallel, best-effort).
    pub async fn fire_session_end(&self, session_id: &str) {
        for hook in &self.hooks {
            hook.on_session_end(session_id).await;
        }
    }

    /// Fire all `on_after_tool_call` observable hooks (parallel, best-effort).
    pub async fn fire_after_tool_call(&self, tool: &str, success: bool, duration: Duration) {
        for hook in &self.hooks {
            hook.on_after_tool_call(tool, success, duration).await;
        }
    }

    /// Fire all `on_llm_response` observable hooks (parallel, best-effort).
    pub async fn fire_llm_response(&self, model: &str, token_count: u32, duration: Duration) {
        for hook in &self.hooks {
            hook.on_llm_response(model, token_count, duration).await;
        }
    }

    /// Number of registered hooks.
    pub fn len(&self) -> usize {
        self.hooks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe shared hook registry.
pub type SharedHookRegistry = Arc<RwLock<HookRegistry>>;

/// Create a new empty shared hook registry.
pub fn new_hook_registry() -> SharedHookRegistry {
    Arc::new(RwLock::new(HookRegistry::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopHook {
        name: String,
        priority: i32,
    }

    #[async_trait::async_trait]
    impl HookHandler for NoopHook {
        fn name(&self) -> &str {
            &self.name
        }
        fn priority(&self) -> i32 {
            self.priority
        }
    }

    #[tokio::test]
    async fn empty_registry_passes_through() {
        let reg = HookRegistry::new();
        let result = reg
            .run_before_tool_call("shell".into(), serde_json::json!({}))
            .await;
        assert!(result.is_some());
        let (name, _) = result.unwrap();
        assert_eq!(name, "shell");
    }

    #[tokio::test]
    async fn hook_result_cancel_short_circuits() {
        struct CancelHook;
        #[async_trait::async_trait]
        impl HookHandler for CancelHook {
            fn name(&self) -> &str {
                "cancel"
            }
            async fn before_tool_call(
                &self,
                _name: String,
                _args: Value,
            ) -> HookResult<(String, Value)> {
                HookResult::Cancel("blocked".into())
            }
        }

        let mut reg = HookRegistry::new();
        reg.register(Arc::new(CancelHook) as Arc<dyn HookHandler>);
        let result = reg
            .run_before_tool_call("shell".into(), serde_json::json!({}))
            .await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn hooks_sorted_by_priority() {
        let mut reg = HookRegistry::new();
        reg.register(Arc::new(NoopHook {
            name: "low".into(),
            priority: 10,
        }) as Arc<dyn HookHandler>);
        reg.register(Arc::new(NoopHook {
            name: "high".into(),
            priority: -10,
        }) as Arc<dyn HookHandler>);
        assert_eq!(reg.hooks[0].priority(), -10);
        assert_eq!(reg.hooks[1].priority(), 10);
    }
}
