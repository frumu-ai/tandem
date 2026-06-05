use crate::app::state::automation::rate_limit::RateLimitManager;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tandem_types::TenantContext;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationSchedulerMetrics {
    pub active_runs: usize,
    pub queued_runs_by_reason: HashMap<String, usize>,
    pub admitted_total: u64,
    pub completed_total: u64,
    pub avg_wait_ms: u64,
    pub p95_wait_ms: u64,
}

// ──────────────────────────────────────────────────────────────
// Queue metadata
// ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum QueueReason {
    Capacity,
    WorkspaceLock,
    RateLimit,
}

impl QueueReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Capacity => "capacity",
            Self::WorkspaceLock => "workspace_lock",
            Self::RateLimit => "rate_limit",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchedulerMetadata {
    #[serde(default = "default_tenant_context")]
    pub tenant_context: TenantContext,
    pub queue_reason: Option<QueueReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limited_provider: Option<String>,
    #[serde(default)]
    pub queued_at_ms: u64,
}

fn default_tenant_context() -> TenantContext {
    TenantContext::local_implicit()
}

// ──────────────────────────────────────────────────────────────
// Preexisting Artifact Registry (MWF-300)
// ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatedArtifact {
    pub path: String,
    pub content_digest: String,
}

#[derive(Debug, Default)]
pub struct PreexistingArtifactRegistry {
    /// run_id -> node_id -> artifact
    pub artifacts: HashMap<String, HashMap<String, ValidatedArtifact>>,
}

impl PreexistingArtifactRegistry {
    pub fn new() -> Self {
        Self {
            artifacts: HashMap::new(),
        }
    }

    /// Record a validated artifact for a node in a run.
    pub fn register_validated(&mut self, run_id: &str, node_id: &str, artifact: ValidatedArtifact) {
        self.artifacts
            .entry(run_id.to_string())
            .or_default()
            .insert(node_id.to_string(), artifact);
    }

    /// Returns true if a valid artifact is registered for this (run, node) pair.
    pub fn is_artifact_prevalidated(&self, run_id: &str, node_id: &str) -> bool {
        self.artifacts
            .get(run_id)
            .and_then(|nodes| nodes.get(node_id))
            .is_some()
    }

    /// Returns the file path of the prevalidated artifact, if any.
    pub fn get_prevalidated_path(&self, run_id: &str, node_id: &str) -> Option<&str> {
        self.artifacts
            .get(run_id)
            .and_then(|nodes| nodes.get(node_id))
            .map(|a| a.path.as_str())
    }

    /// Remove all artifacts for a run (call on run completion/failure).
    pub fn clear_run(&mut self, run_id: &str) {
        self.artifacts.remove(run_id);
    }
}

// ──────────────────────────────────────────────────────────────
// Multi-run scheduler
// ──────────────────────────────────────────────────────────────

pub struct AutomationScheduler {
    pub max_concurrent_runs: usize,
    /// run_id → workspace_root (empty string if no workspace root)
    pub active_runs: HashMap<String, String>,
    /// workspace_root → run_id
    pub locked_workspaces: HashMap<String, String>,
    pub rate_limits: RateLimitManager,
    pub preexisting_registry: PreexistingArtifactRegistry,
    pub admitted_total: u64,
    pub completed_total: u64,
    /// run_id -> metadata
    pub queue_state: HashMap<String, SchedulerMetadata>,
    /// Wait times in ms (last 1000 runs)
    pub wait_times: std::collections::VecDeque<u64>,
}

impl AutomationScheduler {
    pub fn new(max_concurrent_runs: usize) -> Self {
        Self {
            max_concurrent_runs,
            active_runs: HashMap::new(),
            locked_workspaces: HashMap::new(),
            rate_limits: RateLimitManager::new(),
            preexisting_registry: PreexistingArtifactRegistry::new(),
            admitted_total: 0,
            completed_total: 0,
            queue_state: HashMap::new(),
            wait_times: std::collections::VecDeque::with_capacity(1000),
        }
    }

    /// Returns Ok(()) if the run can be admitted right now.
    /// Returns Err(SchedulerMetadata) describing why the run must wait.
    pub fn can_admit(
        &self,
        run_id: &str,
        workspace_root: Option<&str>,
        required_providers: &[String],
    ) -> Result<(), SchedulerMetadata> {
        self.can_admit_for_tenant(
            run_id,
            workspace_root,
            required_providers,
            &TenantContext::local_implicit(),
        )
    }

    pub fn can_admit_for_tenant(
        &self,
        run_id: &str,
        workspace_root: Option<&str>,
        required_providers: &[String],
        tenant_context: &TenantContext,
    ) -> Result<(), SchedulerMetadata> {
        // 1. Check Rate Limits
        for provider in required_providers {
            if self
                .rate_limits
                .is_provider_throttled_for_tenant(tenant_context, provider)
            {
                return Err(SchedulerMetadata {
                    tenant_context: tenant_context.clone(),
                    queue_reason: Some(QueueReason::RateLimit),
                    resource_key: None,
                    rate_limited_provider: Some(provider.clone()),
                    queued_at_ms: self.get_queued_at(run_id),
                });
            }
        }

        // 2. Check workspace lock (prevent priority inversion)
        if let Some(root) = workspace_root {
            if self.locked_workspaces.contains_key(root) {
                return Err(SchedulerMetadata {
                    tenant_context: tenant_context.clone(),
                    queue_reason: Some(QueueReason::WorkspaceLock),
                    resource_key: Some(root.to_string()),
                    rate_limited_provider: None,
                    queued_at_ms: self.get_queued_at(run_id),
                });
            }
        }

        // 3. Check global capacity
        if self.active_runs.len() >= self.max_concurrent_runs {
            return Err(SchedulerMetadata {
                tenant_context: tenant_context.clone(),
                queue_reason: Some(QueueReason::Capacity),
                resource_key: None,
                rate_limited_provider: None,
                queued_at_ms: self.get_queued_at(run_id),
            });
        }

        Ok(())
    }

    fn get_queued_at(&self, run_id: &str) -> u64 {
        self.queue_state
            .get(run_id)
            .map(|m| m.queued_at_ms)
            .unwrap_or_else(crate::util::time::now_ms)
    }

    pub fn track_queue_state(&mut self, run_id: &str, metadata: SchedulerMetadata) {
        self.queue_state.insert(run_id.to_string(), metadata);
    }

    /// Admit a run — records the active slot and workspace lock.
    pub fn admit_run(&mut self, run_id: &str, workspace_root: Option<&str>) {
        let root = workspace_root.unwrap_or("").to_string();
        if !root.is_empty() {
            self.locked_workspaces
                .insert(root.clone(), run_id.to_string());
        }
        self.active_runs.insert(run_id.to_string(), root);
        self.admitted_total += 1;

        if let Some(meta) = self.queue_state.remove(run_id) {
            let wait_ms = crate::util::time::now_ms().saturating_sub(meta.queued_at_ms);
            if self.wait_times.len() >= 1000 {
                self.wait_times.pop_front();
            }
            self.wait_times.push_back(wait_ms);
        }
    }

    pub fn reserve_workspace(&mut self, run_id: &str, workspace_root: Option<&str>) {
        let root = workspace_root.unwrap_or("").to_string();
        if root.is_empty() {
            return;
        }
        self.locked_workspaces.insert(root, run_id.to_string());
    }

    pub fn release_capacity(&mut self, run_id: &str) {
        if self.active_runs.remove(run_id).is_some() {
            self.completed_total += 1;
        }
    }

    pub fn release_workspace(&mut self, run_id: &str) {
        self.locked_workspaces.retain(|_, holder| holder != run_id);
        self.preexisting_registry.clear_run(run_id);
        self.queue_state.remove(run_id);
    }

    /// Release a run — frees capacity and workspace lock.
    pub fn release_run(&mut self, run_id: &str) {
        self.release_capacity(run_id);
        self.release_workspace(run_id);
    }

    pub fn metrics(&self) -> AutomationSchedulerMetrics {
        let mut reasons = HashMap::new();
        for meta in self.queue_state.values() {
            if let Some(reason) = meta.queue_reason {
                *reasons.entry(reason.as_str().to_string()).or_default() += 1;
            }
        }

        let mut wait_times: Vec<u64> = self.wait_times.iter().cloned().collect();
        wait_times.sort_unstable();

        let avg_wait = if wait_times.is_empty() {
            0
        } else {
            wait_times.iter().sum::<u64>() / wait_times.len() as u64
        };

        let p95_wait = if wait_times.is_empty() {
            0
        } else {
            let idx = (wait_times.len() as f64 * 0.95).round() as usize;
            wait_times
                .get(idx.min(wait_times.len() - 1))
                .cloned()
                .unwrap_or(0)
        };

        AutomationSchedulerMetrics {
            active_runs: self.active_runs.len(),
            queued_runs_by_reason: reasons,
            admitted_total: self.admitted_total,
            completed_total: self.completed_total,
            avg_wait_ms: avg_wait,
            p95_wait_ms: p95_wait,
        }
    }

    pub fn active_count(&self) -> usize {
        self.active_runs.len()
    }

    pub fn is_at_capacity(&self) -> bool {
        self.active_runs.len() >= self.max_concurrent_runs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_rate_limit_admission_is_tenant_scoped() {
        let mut scheduler = AutomationScheduler::new(4);
        let tenant_a = TenantContext::explicit("org-a", "workspace", Some("user-a".to_string()));
        let tenant_b = TenantContext::explicit("org-b", "workspace", Some("user-b".to_string()));
        let providers = vec!["openai".to_string()];

        scheduler
            .rate_limits
            .record_throttle_for_tenant(&tenant_a, "openai", 60_000);

        let tenant_a_meta = scheduler
            .can_admit_for_tenant("run-a", None, &providers, &tenant_a)
            .expect_err("tenant A should be throttled");
        assert_eq!(tenant_a_meta.queue_reason, Some(QueueReason::RateLimit));
        assert_eq!(tenant_a_meta.tenant_context, tenant_a);
        assert!(
            scheduler
                .can_admit_for_tenant("run-b", None, &providers, &tenant_b)
                .is_ok(),
            "tenant B should not inherit tenant A provider throttle"
        );
    }
}
