// Orchestrator Budget Tracker
// Cost tracking and enforcement with hard limits
// See: docs/orchestration_plan.md

use crate::error::{Result, TandemError};
use crate::orchestrator::types::{Budget, OrchestratorConfig};
use serde::{Deserialize, Serialize};
use std::time::Instant;

// ============================================================================
// Budget Tracker
// ============================================================================

/// Tracks and enforces budget limits for an orchestration run
pub struct BudgetTracker {
    /// Current budget state
    budget: Budget,
    /// Wall clock start time
    start_time: Instant,
    /// Time elapsed before this session (for resumption)
    previous_elapsed_secs: u64,
    is_active: bool,
    /// Estimated tokens per character (for estimation fallback)
    tokens_per_char: f64,
}

/// Result of a budget check
#[derive(Debug, Clone, Serialize)]
pub enum BudgetCheckResult {
    /// Budget is OK, can proceed
    Ok,
    /// Approaching limit (80%+)
    Warning { dimension: String, percentage: f64 },
    /// Budget exceeded
    Exceeded { dimension: String, reason: String },
}

impl BudgetTracker {
    /// Create a new budget tracker from config
    pub fn new(config: &OrchestratorConfig) -> Self {
        Self {
            budget: Budget::from_config(config),
            start_time: Instant::now(),
            previous_elapsed_secs: 0,
            is_active: true,
            tokens_per_char: 0.25, // ~4 chars per token (conservative estimate)
        }
    }

    /// Create from existing budget state (for resumption)
    pub fn from_budget(budget: Budget) -> Self {
        let previous_elapsed_secs = budget.wall_time_secs;
        Self {
            budget,
            start_time: Instant::now(),
            previous_elapsed_secs,
            is_active: false,
            tokens_per_char: 0.25,
        }
    }

    /// Update budget limits from a new config (e.g. after resumption with updated defaults)
    pub fn update_limits(&mut self, config: &OrchestratorConfig) {
        let new_budget = Budget::from_config(config);
        self.budget.max_iterations = new_budget.max_iterations;
        self.budget.max_tokens = new_budget.max_tokens;
        self.budget.max_wall_time_secs = new_budget.max_wall_time_secs;
        self.budget.max_subagent_runs = new_budget.max_subagent_runs;

        // If we increased limits, we may no longer be exceeded.
        if self.budget.exceeded {
            let still_exceeded = self.budget.iterations_used >= self.budget.max_iterations
                || self.budget.tokens_used >= self.budget.max_tokens
                || self.budget.wall_time_secs >= self.budget.max_wall_time_secs
                || self.budget.subagent_runs_used >= self.budget.max_subagent_runs;

            if !still_exceeded {
                self.budget.exceeded = false;
                self.budget.exceeded_reason = None;
            }
        }
    }

    pub fn set_active(&mut self, active: bool) {
        if self.is_active == active {
            return;
        }

        if self.is_active {
            self.previous_elapsed_secs += self.start_time.elapsed().as_secs();
        }

        self.start_time = Instant::now();
        self.is_active = active;
        self.update_wall_time();
    }

    /// Update wall time from actual elapsed time
    pub fn update_wall_time(&mut self) {
        let current = if self.is_active {
            self.start_time.elapsed().as_secs()
        } else {
            0
        };
        self.budget.wall_time_secs = self.previous_elapsed_secs + current;
    }

    /// Record an iteration
    pub fn record_iteration(&mut self) {
        self.budget.iterations_used += 1;
        self.update_wall_time();
    }

    /// Record a sub-agent run
    pub fn record_subagent_run(&mut self) {
        self.budget.subagent_runs_used += 1;
        self.update_wall_time();
    }

    /// Record token usage (actual if available, otherwise estimate from text length)
    pub fn record_tokens(&mut self, tokens: Option<u64>, text_length: Option<usize>) {
        let token_count = tokens.unwrap_or_else(|| {
            text_length
                .map(|len| (len as f64 * self.tokens_per_char) as u64)
                .unwrap_or(0)
        });
        self.budget.tokens_used += token_count;
        self.update_wall_time();
    }

    /// Check if budget allows proceeding
    pub fn check(&mut self) -> BudgetCheckResult {
        self.update_wall_time();

        // Check each dimension
        let checks = [
            (
                "iterations",
                self.budget.iterations_used as f64,
                self.budget.max_iterations as f64,
            ),
            (
                "tokens",
                self.budget.tokens_used as f64,
                self.budget.max_tokens as f64,
            ),
            (
                "wall_time",
                self.budget.wall_time_secs as f64,
                self.budget.max_wall_time_secs as f64,
            ),
            (
                "subagent_runs",
                // Ensure we don't count completed runs against the limit if we wanted to reset (optional policy)
                // But typically subagent runs is a hard cost limit.
                // For now, we keep it cumulative but user raised issue about it not resetting.
                // If the user implies "active" subagents, that's different.
                // But budget is usually cumulative.
                // We will stick to cumulative for now but maybe increase the default limit as done in types.rs
                self.budget.subagent_runs_used as f64,
                self.budget.max_subagent_runs as f64,
            ),
        ];

        for (dimension, used, max) in checks {
            if max > 0.0 {
                let percentage = used / max;

                if percentage >= 1.0 {
                    self.budget.exceeded = true;
                    self.budget.exceeded_reason = Some(format!(
                        "{} limit exceeded: {} / {}",
                        dimension, used as u64, max as u64
                    ));
                    return BudgetCheckResult::Exceeded {
                        dimension: dimension.to_string(),
                        reason: format!(
                            "{} limit exceeded ({} / {})",
                            dimension, used as u64, max as u64
                        ),
                    };
                }

                if percentage >= 0.8 {
                    return BudgetCheckResult::Warning {
                        dimension: dimension.to_string(),
                        percentage,
                    };
                }
            }
        }

        BudgetCheckResult::Ok
    }

    /// Check if a proposed action would exceed budget
    pub fn would_exceed(&self, tokens_estimate: u64) -> bool {
        self.budget.tokens_used + tokens_estimate > self.budget.max_tokens
            || self.budget.subagent_runs_used + 1 > self.budget.max_subagent_runs
            || self.budget.iterations_used + 1 > self.budget.max_iterations
    }

    /// Get current budget state
    pub fn get_budget(&self) -> &Budget {
        &self.budget
    }

    /// Get mutable budget state
    pub fn get_budget_mut(&mut self) -> &mut Budget {
        &mut self.budget
    }

    /// Get a snapshot of current budget for UI
    pub fn snapshot(&mut self) -> Budget {
        self.update_wall_time();
        self.budget.clone()
    }

    /// Reset the start time (for pause/resume)
    pub fn reset_start_time(&mut self, previous_elapsed: u64) {
        self.previous_elapsed_secs = previous_elapsed;
        self.start_time = Instant::now();
        self.update_wall_time();
    }

    /// Get remaining budget as percentages
    pub fn remaining_percentages(&mut self) -> BudgetPercentages {
        self.update_wall_time();

        let safe_div = |used: u64, max: u64| -> f64 {
            if max == 0 {
                0.0
            } else {
                1.0 - (used as f64 / max as f64).min(1.0)
            }
        };

        BudgetPercentages {
            iterations: safe_div(
                self.budget.iterations_used as u64,
                self.budget.max_iterations as u64,
            ),
            tokens: safe_div(self.budget.tokens_used, self.budget.max_tokens),
            wall_time: safe_div(self.budget.wall_time_secs, self.budget.max_wall_time_secs),
            subagent_runs: safe_div(
                self.budget.subagent_runs_used as u64,
                self.budget.max_subagent_runs as u64,
            ),
        }
    }
}

/// Budget remaining percentages for UI display
#[derive(Debug, Clone, Serialize)]
pub struct BudgetPercentages {
    pub iterations: f64,
    pub tokens: f64,
    pub wall_time: f64,
    pub subagent_runs: f64,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> OrchestratorConfig {
        OrchestratorConfig {
            max_iterations: 10,
            max_total_tokens: 100_000,
            max_wall_time_secs: 60,
            max_subagent_runs: 20,
            ..Default::default()
        }
    }

    #[test]
    fn test_budget_tracking() {
        let mut tracker = BudgetTracker::new(&test_config());

        tracker.record_iteration();
        assert_eq!(tracker.budget.iterations_used, 1);

        tracker.record_subagent_run();
        assert_eq!(tracker.budget.subagent_runs_used, 1);

        tracker.record_tokens(Some(5000), None);
        assert_eq!(tracker.budget.tokens_used, 5000);
    }

    #[test]
    fn test_budget_check_ok() {
        let mut tracker = BudgetTracker::new(&test_config());

        tracker.record_iteration();
        tracker.record_tokens(Some(10_000), None);

        match tracker.check() {
            BudgetCheckResult::Ok => {}
            _ => panic!("Expected Ok"),
        }
    }

    #[test]
    fn test_budget_check_warning() {
        let mut tracker = BudgetTracker::new(&test_config());

        // Use 85% of tokens
        tracker.record_tokens(Some(85_000), None);

        match tracker.check() {
            BudgetCheckResult::Warning {
                dimension,
                percentage,
            } => {
                assert_eq!(dimension, "tokens");
                assert!(percentage >= 0.8);
            }
            _ => panic!("Expected Warning"),
        }
    }

    #[test]
    fn test_budget_check_exceeded() {
        let mut tracker = BudgetTracker::new(&test_config());

        // Exceed iterations
        for _ in 0..11 {
            tracker.record_iteration();
        }

        match tracker.check() {
            BudgetCheckResult::Exceeded { dimension, .. } => {
                assert_eq!(dimension, "iterations");
            }
            _ => panic!("Expected Exceeded"),
        }
    }

    #[test]
    fn test_token_estimation() {
        let mut tracker = BudgetTracker::new(&test_config());

        // Estimate tokens from 1000 characters
        tracker.record_tokens(None, Some(1000));

        // Should be ~250 tokens (0.25 tokens per char)
        assert_eq!(tracker.budget.tokens_used, 250);
    }
}
